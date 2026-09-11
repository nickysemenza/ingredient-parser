//! Drive every chunk through the model ladder, then cross-check, second-guess
//! what was flagged, and escalate the whole book when too much was flagged.
//!
//! Concurrency counts chunks, not calls: each chunk job walks its own ladder.
//! Every call, cached or live, becomes a `CallRecord`. Cancellation drops the
//! in-flight stream and returns what was settled.

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use futures::{FutureExt, StreamExt, stream};
use jiff::Timestamp;

use crate::cache::{CachedCall, ChunkCache, cache_key};
use crate::chunk::Chunk;
use crate::contract::{CONTRACT_VERSION, ChunkRequest, Kind, Lowered, build_request, lower};
use crate::cost::{Usage, cost_for_usage};
use crate::crosscheck::{ExtractedTitle, crosscheck, titles_match};
use crate::epub::nav::Nav;
use crate::eta::{EtaTracker, RemainingInput, chunk_tokens};
use crate::gateway::{CallFailure, CallMeta, CallResult, build_http, parse_response};
use crate::lines::BookLines;
use crate::models::Model;
use crate::report::{
    CallOutcome, CallPurpose, CallRecord, Chosen, ChunkReport, ChunkStatus, CrossCheck, Escalation,
    EtaSample, ExtractOptions, Flag, Phase, Progress, SecondOpinion,
};
use crate::transport::{CancelToken, Transport, TransportError};
use crate::validate::{Validation, validate};

/// Models tried per chunk before the book-level policy takes over.
const MODELS_PER_CHUNK: usize = 2;
/// Attempts per model per chunk (the second carries feedback).
const ATTEMPTS_PER_MODEL: usize = 2;
/// Flagged share above which the whole book is re-run with a stronger model.
pub const ESCALATION_FLAG_FRACTION: f32 = 0.25;
/// Nav recall below which the whole book is re-run.
pub const ESCALATION_RECALL_FLOOR: f32 = 0.85;
const TRANSIENT_DEFAULT_MS: u64 = 2_000;

pub struct RunInput<'a> {
    pub book: &'a BookLines,
    pub nav: &'a Nav,
    pub chunks: &'a [Chunk],
    pub ladder: Vec<&'static Model>,
    pub options: &'a ExtractOptions,
    pub started: Timestamp,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ChunkResult {
    pub chunk_index: usize,
    pub lowered: Option<Lowered>,
    pub report: ChunkReport,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RunOutput {
    /// In chunk order.
    pub chunks: Vec<ChunkResult>,
    pub calls: Vec<CallRecord>,
    pub crosscheck: CrossCheck,
    pub escalation: Option<Escalation>,
    pub eta_trace: Vec<EtaSample>,
    /// A chunk failed every model, or escalation ran out of ladder.
    pub incomplete: bool,
    pub cancelled: bool,
}

/// One chunk settled by one ladder walk.
struct Settled {
    lowered: Option<Lowered>,
    validation: Validation,
    model: Option<&'static Model>,
    attempts: usize,
    cached: bool,
}

struct Shared<'a, T, C> {
    input: &'a RunInput<'a>,
    transport: &'a T,
    cache: &'a C,
    cancel: &'a CancelToken,
    seq: AtomicUsize,
    calls: Mutex<Vec<CallRecord>>,
    tracker: Mutex<EtaTracker>,
    in_flight: Mutex<HashMap<String, (u64, String)>>,
    /// Models the gateway refuses for the rest of the run (quota exhausted).
    exhausted: Mutex<HashSet<&'static str>>,
}

impl<T: Transport, C: ChunkCache> Shared<'_, T, C> {
    fn now_ms(&self) -> u64 {
        (Timestamp::now().as_millisecond() - self.input.started.as_millisecond()).max(0) as u64
    }

    fn cost_so_far(&self) -> f64 {
        self.calls
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .filter_map(|c| c.cost_usd)
            .sum()
    }

    /// One call: cache, transport, decode. Records the call and returns the
    /// decoded result or the failure.
    async fn call(
        &self,
        chunk: &Chunk,
        model: &'static Model,
        tier: usize,
        purpose: CallPurpose,
        attempt: usize,
        request: &ChunkRequest,
    ) -> Result<(CallResult, bool), CallError> {
        let meta = CallMeta {
            cookbook: &self.input.options.label,
            chunk: &chunk.id,
            purpose: purpose_str(purpose),
        };
        let http = build_http(model, request, self.input.options.max_output_tokens, &meta);
        let key = cache_key(CONTRACT_VERSION, model.id, model.route.as_str(), &http.body);
        let started_ms = self.now_ms();
        let seq = self.seq.fetch_add(1, Ordering::SeqCst);
        let mut record = CallRecord {
            seq,
            chunk_id: chunk.id.clone(),
            model: model.id.to_string(),
            tier,
            purpose,
            attempt,
            started_ms,
            latency_ms: 0,
            cached: false,
            status: None,
            request_id: None,
            usage: Usage::default(),
            cost_usd: Some(0.0),
            truncated: false,
            outcome: CallOutcome::Ok,
        };
        let cached = self.cache.get(&key);
        let response = match cached {
            Some(hit) => {
                record.cached = true;
                Ok(hit.response)
            }
            None => {
                self.in_flight
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .insert(chunk.id.clone(), (started_ms, model.id.to_string()));
                let sent = self.transport.send(http, self.cancel).await;
                self.in_flight
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .remove(&chunk.id);
                sent
            }
        };
        record.latency_ms = self.now_ms().saturating_sub(started_ms);
        let outcome = match response {
            Err(err) => {
                record.outcome = CallOutcome::Transport {
                    kind: err.kind().to_string(),
                    message: err.to_string(),
                };
                Err(CallError::Transport(err))
            }
            Ok(response) => {
                record.status = Some(response.status);
                match parse_response(model.route, &response) {
                    Ok(result) => {
                        record.request_id = result.request_id.clone();
                        record.truncated = result.truncated;
                        if !record.cached {
                            record.usage = result.usage;
                            record.cost_usd = cost_for_usage(model, &result.usage);
                            if !result.truncated {
                                self.cache.put(
                                    &key,
                                    &CachedCall {
                                        key: key.clone(),
                                        model: model.id.to_string(),
                                        contract: CONTRACT_VERSION.to_string(),
                                        response: response.clone(),
                                        usage: result.usage,
                                        recorded_at: Timestamp::now().to_string(),
                                    },
                                );
                            }
                            self.tracker
                                .lock()
                                .unwrap_or_else(|e| e.into_inner())
                                .record_call(model.id, record.latency_ms);
                        }
                        Ok((result, record.cached))
                    }
                    Err(failure) => {
                        if failure.is_quota_exhausted() {
                            self.exhausted
                                .lock()
                                .unwrap_or_else(|e| e.into_inner())
                                .insert(model.id);
                        }
                        record.request_id = failure.request_id().map(str::to_string);
                        record.outcome = CallOutcome::Transport {
                            kind: match &failure {
                                CallFailure::Http { .. } => "http".into(),
                                CallFailure::Payload { .. } => "payload".into(),
                            },
                            message: failure.to_string(),
                        };
                        Err(CallError::Call(failure))
                    }
                }
            }
        };
        self.calls
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(record);
        outcome
    }

    fn is_exhausted(&self, model: &Model) -> bool {
        self.exhausted
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .contains(model.id)
    }

    /// Mark the last record for this chunk as invalid with the given faults.
    fn note_invalid(&self, chunk: &Chunk, faults: Vec<String>) {
        let mut calls = self.calls.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(record) = calls.iter_mut().rev().find(|c| c.chunk_id == chunk.id) {
            record.outcome = CallOutcome::Invalid { faults };
        }
    }

    /// Walk `models` for one chunk. `purpose` names the first attempt on each
    /// model; the second attempt on the same model is a retry with feedback.
    async fn settle(
        &self,
        chunk: &Chunk,
        models: &[(usize, &'static Model)],
        purpose: CallPurpose,
        base_feedback: Option<&str>,
    ) -> Settled {
        let book = self.input.book;
        let base = build_request(chunk, book);
        let mut attempts = 0;
        for &(tier, model) in models {
            if self.is_exhausted(model) {
                continue;
            }
            let mut feedback: Option<String> = base_feedback.map(str::to_string);
            let mut attempt = 0;
            while attempt < ATTEMPTS_PER_MODEL {
                if self.cancel.is_cancelled() {
                    return Settled {
                        lowered: None,
                        validation: Validation::default(),
                        model: None,
                        attempts,
                        cached: false,
                    };
                }
                attempt += 1;
                attempts += 1;
                let call_purpose = if attempt == 1 {
                    purpose
                } else {
                    CallPurpose::Retry
                };
                let request = with_feedback(&base, feedback.as_deref());
                match self
                    .call(chunk, model, tier, call_purpose, attempt, &request)
                    .await
                {
                    Err(CallError::Transport(TransportError::Cancelled)) => {
                        return Settled {
                            lowered: None,
                            validation: Validation::default(),
                            model: None,
                            attempts,
                            cached: false,
                        };
                    }
                    Err(CallError::Transport(
                        TransportError::Timeout | TransportError::Connect,
                    )) => {
                        self.transport.sleep(TRANSIENT_DEFAULT_MS).await;
                        continue;
                    }
                    Err(CallError::Transport(TransportError::Other(_))) => break,
                    Err(CallError::Call(failure)) => match failure.transient_delay_ms() {
                        // The gateway will refuse this model for the rest of
                        // the run; move down the ladder at once.
                        _ if failure.is_quota_exhausted() => break,
                        Some(delay) => {
                            self.transport.sleep(delay).await;
                            continue;
                        }
                        None if matches!(failure, CallFailure::Payload { .. }) => {
                            feedback = Some("The previous answer was not a valid tool call. Answer only by calling the tool.".into());
                            continue;
                        }
                        None => break,
                    },
                    Ok((result, cached)) => {
                        let Some(input) = result.input else {
                            self.note_invalid(chunk, vec!["no tool call in the answer".into()]);
                            feedback = Some(
                                "Answer only by calling the tool with the line selections.".into(),
                            );
                            continue;
                        };
                        if result.truncated && attempt < ATTEMPTS_PER_MODEL {
                            // Stochastic; the same model often fits on a retry.
                            feedback = Some("The previous answer was cut off at the output limit. Keep every field as short index lists.".into());
                            continue;
                        }
                        let lowered = match lower(chunk, book, input) {
                            Ok(l) => l,
                            Err(invalid) => {
                                self.note_invalid(chunk, vec![invalid.0.clone()]);
                                feedback = Some(invalid.0);
                                continue;
                            }
                        };
                        let mut validation = validate(chunk, book, &lowered);
                        if !validation.is_ok() {
                            let faults: Vec<String> =
                                validation.hard.iter().map(ToString::to_string).collect();
                            self.note_invalid(chunk, faults);
                            feedback = Some(validation.feedback());
                            continue;
                        }
                        if result.truncated {
                            validation.soft.push(Flag::Truncated);
                        }
                        return Settled {
                            lowered: Some(lowered),
                            validation,
                            model: Some(model),
                            attempts,
                            cached,
                        };
                    }
                }
            }
        }
        Settled {
            lowered: None,
            validation: Validation::default(),
            model: None,
            attempts,
            cached: false,
        }
    }

    fn progress(
        &self,
        phase: Phase,
        results: &[Option<ChunkResult>],
        in_flight_count: usize,
    ) -> Progress {
        let settled: Vec<&ChunkResult> = results.iter().flatten().collect();
        let done = settled.len();
        let total = results.len();
        let failed = settled
            .iter()
            .filter(|r| r.report.status == ChunkStatus::Failed)
            .count();
        let cached = settled.iter().filter(|r| r.report.cached).count();
        let recipes_so_far = settled.iter().map(|r| r.report.recipes).sum();
        let cost_so_far = self.cost_so_far();
        let now = self.now_ms();
        let in_flight = self.in_flight.lock().unwrap_or_else(|e| e.into_inner());
        let oldest = in_flight
            .values()
            .map(|(started, _)| now.saturating_sub(*started))
            .max();
        let mut active_models: Vec<String> = in_flight.values().map(|(_, m)| m.clone()).collect();
        drop(in_flight);
        active_models.sort();
        active_models.dedup();
        let typical_output = self
            .input
            .chunks
            .iter()
            .map(|c| chunk_tokens(c).1)
            .sum::<u64>()
            .checked_div(total as u64)
            .unwrap_or(0);
        let eta = self
            .tracker
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remaining(&RemainingInput {
                remaining: total - done,
                oldest_in_flight_ms: oldest,
                concurrency: self.input.options.concurrency,
                ladder: &self.input.ladder,
                typical_output_tokens: typical_output,
                cost_so_far,
                settled_chunks: done,
            });
        Progress {
            phase,
            done,
            total,
            in_flight: in_flight_count,
            failed,
            cached,
            recipes_so_far,
            cost_so_far_usd: cost_so_far,
            elapsed_ms: now,
            eta,
            active_models,
        }
    }
}

enum CallError {
    Transport(TransportError),
    Call(CallFailure),
}

fn purpose_str(p: CallPurpose) -> &'static str {
    match p {
        CallPurpose::Extract => "extract",
        CallPurpose::Retry => "retry",
        CallPurpose::SecondOpinion => "second_opinion",
        CallPurpose::Escalation => "escalation",
        CallPurpose::Classify => "classify",
    }
}

fn with_feedback(base: &ChunkRequest, feedback: Option<&str>) -> ChunkRequest {
    match feedback {
        None => base.clone(),
        Some(fb) => ChunkRequest {
            user: format!(
                "{}\n\nYour previous answer was rejected:\n{fb}\nReturn a corrected selection.",
                base.user
            ),
            ..base.clone()
        },
    }
}

fn to_result(chunk: &Chunk, settled: Settled, extra_flags: Vec<Flag>) -> ChunkResult {
    let mut flags = settled.validation.soft.clone();
    flags.extend(extra_flags);
    let recipes = settled
        .lowered
        .as_ref()
        .map(|l| l.items.iter().filter(|i| i.kind == Kind::Recipe).count())
        .unwrap_or(0);
    ChunkResult {
        chunk_index: chunk.index,
        report: ChunkReport {
            id: chunk.id.clone(),
            start: chunk.start,
            end: chunk.end,
            chars: chunk.chars,
            status: if settled.lowered.is_some() {
                ChunkStatus::Ok
            } else {
                ChunkStatus::Failed
            },
            final_model: settled.model.map(|m| m.id.to_string()),
            attempts: settled.attempts,
            flags,
            second_opinion: None,
            recipes,
            cached: settled.cached,
        },
        lowered: settled.lowered,
    }
}

fn extracted_titles(results: &[ChunkResult]) -> Vec<ExtractedTitle> {
    results
        .iter()
        .filter_map(|r| r.lowered.as_ref().map(|l| (r.chunk_index, l)))
        .flat_map(|(chunk, lowered)| {
            lowered
                .items
                .iter()
                .filter(|i| !i.continues)
                .map(move |item| ExtractedTitle {
                    chunk,
                    title: item.title.clone(),
                    line: item.title_lines.first().copied(),
                    kind: item.kind,
                    ingredients: item.ingredient_count(),
                    steps: item.step_count(),
                })
        })
        .collect()
}

fn flag_feedback(chunk: &Chunk, flags: &[Flag]) -> String {
    flags
        .iter()
        .map(|f| match f {
            Flag::MissingNavTitle { title, line } => format!(
                "Line {} is the recipe title {title:?} according to the book's table of contents; extract that recipe.",
                line.saturating_sub(chunk.start)
            ),
            Flag::PhantomTitle { title } => format!("{title:?} is not a recipe in this book's table of contents; it is probably a caption or heading. Do not emit it as a recipe unless it has its own ingredients and method."),
            Flag::LowAmountParseRate { .. } => "Most selected ingredient lines carry no quantity; check that ingredient lines were not confused with prose or headings.".to_string(),
            Flag::IngredientLikeIgnored { count } => format!("{count} lines that look like ingredient quantities were placed in ignored; assign them to their recipe."),
            Flag::RecipeWithoutSteps { title } => format!("Recipe {title:?} has ingredients but no steps while long paragraphs were ignored; its method is probably among them."),
            Flag::Truncated => "The previous answer was cut off; keep every field as short index lists.".to_string(),
            Flag::CaptionAsTitle { line } => format!("Line {} is a caption, not a title.", line.saturating_sub(chunk.start)),
            Flag::UnassignedLines { count } => {
                format!("{count} lines were left out of the previous answer; assign every line.")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Count of ingredient lines whose parse found an amount: the tiebreaker for
/// competing answers.
fn amount_lines(lowered: &Lowered) -> usize {
    lowered
        .items
        .iter()
        .flat_map(|i| i.sections.iter().flat_map(|s| s.ingredients.iter()))
        .filter(|t| !ingredient::from_str(&t.text).amounts.is_empty())
        .count()
}

fn nav_matches(lowered: &Lowered, missing: &[String]) -> usize {
    missing
        .iter()
        .filter(|m| lowered.items.iter().any(|i| titles_match(&i.title, m)))
        .count()
}

/// Which of two valid answers to keep, and why.
fn choose(first: &ChunkResult, second: &Settled, missing_titles: &[String]) -> (Chosen, String) {
    let Some(second_lowered) = &second.lowered else {
        return (
            Chosen::First,
            "second model produced no valid answer".into(),
        );
    };
    let Some(first_lowered) = &first.lowered else {
        return (Chosen::Second, "first answer was invalid".into());
    };
    let first_flags = first
        .report
        .flags
        .iter()
        .filter(|f| !matches!(f, Flag::MissingNavTitle { .. } | Flag::PhantomTitle { .. }))
        .count();
    let second_flags = second.validation.soft.len();
    if first_flags != second_flags {
        return if second_flags < first_flags {
            (
                Chosen::Second,
                format!("fewer flags ({second_flags} vs {first_flags})"),
            )
        } else {
            (
                Chosen::First,
                format!("fewer flags ({first_flags} vs {second_flags})"),
            )
        };
    }
    let (n1, n2) = (
        nav_matches(first_lowered, missing_titles),
        nav_matches(second_lowered, missing_titles),
    );
    if n1 != n2 {
        return if n2 > n1 {
            (
                Chosen::Second,
                format!("recovers {n2} contents titles vs {n1}"),
            )
        } else {
            (
                Chosen::First,
                format!("recovers {n1} contents titles vs {n2}"),
            )
        };
    }
    let (a1, a2) = (amount_lines(first_lowered), amount_lines(second_lowered));
    if a1 != a2 {
        return if a2 > a1 {
            (
                Chosen::Second,
                format!("more parsable ingredient lines ({a2} vs {a1})"),
            )
        } else {
            (
                Chosen::First,
                format!("more parsable ingredient lines ({a1} vs {a2})"),
            )
        };
    }
    (Chosen::Second, "tie; higher tier wins".into())
}

pub async fn run<T: Transport, C: ChunkCache>(
    input: &RunInput<'_>,
    transport: &T,
    cache: &C,
    cancel: &CancelToken,
    progress: &mut (dyn FnMut(Progress) + Send),
) -> RunOutput {
    let shared = Shared {
        input,
        transport,
        cache,
        cancel,
        seq: AtomicUsize::new(0),
        calls: Mutex::new(Vec::new()),
        tracker: Mutex::new(EtaTracker::new()),
        in_flight: Mutex::new(HashMap::new()),
        exhausted: Mutex::new(HashSet::new()),
    };
    let shared = &shared;
    let ladder: Vec<(usize, &'static Model)> = input.ladder.iter().copied().enumerate().collect();
    let initial: Vec<(usize, &'static Model)> =
        ladder.iter().copied().take(MODELS_PER_CHUNK).collect();
    let initial = &initial;
    let concurrency = input.options.concurrency.max(1);
    let mut results: Vec<Option<ChunkResult>> = vec![None; input.chunks.len()];
    let mut eta_trace = Vec::new();
    let cancelled;

    // Phase 1: every chunk through the first models of the ladder.
    {
        let mut stream = stream::iter(input.chunks.iter())
            .map(move |chunk| async move {
                (
                    chunk,
                    shared
                        .settle(chunk, initial, CallPurpose::Extract, None)
                        .await,
                )
            })
            .buffer_unordered(concurrency);
        progress(shared.progress(Phase::Extract, &results, 0));
        loop {
            let next = futures::select_biased! {
                _ = cancel.cancelled().fuse() => None,
                item = stream.next().fuse() => item,
            };
            let Some((chunk, settled)) = next else {
                cancelled = cancel.is_cancelled() && results.iter().any(Option::is_none);
                break;
            };
            let flagged = !settled.validation.soft.is_empty();
            shared
                .tracker
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .record_chunk(flagged);
            results[chunk.index] = Some(to_result(chunk, settled, Vec::new()));
            let in_flight = shared
                .in_flight
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .len();
            let p = shared.progress(Phase::Extract, &results, in_flight);
            eta_trace.push(EtaSample {
                elapsed_ms: p.elapsed_ms,
                remaining_low_ms: p.eta.remaining_low_ms,
                remaining_high_ms: p.eta.remaining_high_ms,
            });
            progress(p);
        }
    }
    let mut settled_results: Vec<ChunkResult> = results
        .iter()
        .enumerate()
        .map(|(i, r)| {
            r.clone().unwrap_or_else(|| ChunkResult {
                chunk_index: i,
                lowered: None,
                report: ChunkReport {
                    id: input.chunks[i].id.clone(),
                    start: input.chunks[i].start,
                    end: input.chunks[i].end,
                    chars: input.chunks[i].chars,
                    status: ChunkStatus::Failed,
                    final_model: None,
                    attempts: 0,
                    flags: Vec::new(),
                    second_opinion: None,
                    recipes: 0,
                    cached: false,
                },
            })
        })
        .collect();
    let take_calls = |shared: &Shared<'_, T, C>| {
        std::mem::take(&mut *shared.calls.lock().unwrap_or_else(|e| e.into_inner()))
    };
    if cancelled {
        return RunOutput {
            chunks: settled_results,
            calls: take_calls(shared),
            crosscheck: CrossCheck {
                nav_titles: 0,
                matched: 0,
                missing: vec![],
                phantom: vec![],
                recall: None,
            },
            escalation: None,
            eta_trace,
            incomplete: true,
            cancelled: true,
        };
    }

    // Phase 2: book-level cross-check.
    progress(shared.progress(Phase::Crosscheck, &results, 0));
    let (mut check, flags) = crosscheck(
        input.book,
        input.nav,
        input.chunks,
        &extracted_titles(&settled_results),
    );
    for (chunk_index, flag) in flags {
        settled_results[chunk_index].report.flags.push(flag);
    }

    // Phase 3: a different model re-reads flagged chunks.
    if input.options.second_opinion {
        let jobs: Vec<(usize, (usize, &'static Model), String)> = settled_results
            .iter()
            .filter(|r| !r.report.flags.is_empty() && r.lowered.is_some())
            .filter_map(|r| {
                let tier = r
                    .report
                    .final_model
                    .as_deref()
                    .and_then(|id| ladder.iter().find(|(_, m)| m.id == id))
                    .map(|(t, _)| *t)?;
                let next = ladder.get(tier + 1).copied()?;
                Some((
                    r.chunk_index,
                    next,
                    flag_feedback(&input.chunks[r.chunk_index], &r.report.flags),
                ))
            })
            .collect();
        if !jobs.is_empty() {
            progress(shared.progress(Phase::SecondOpinion, &results, jobs.len()));
            let chunks = input.chunks;
            let decisions: Vec<(usize, (usize, &'static Model), Settled)> = {
                let mut stream = stream::iter(jobs)
                    .map(move |(index, next, feedback)| async move {
                        let settled = shared
                            .settle(
                                &chunks[index],
                                &[next],
                                CallPurpose::SecondOpinion,
                                Some(&feedback),
                            )
                            .await;
                        (index, next, settled)
                    })
                    .buffer_unordered(concurrency);
                let mut out = Vec::new();
                while let Some(decision) = stream.next().await {
                    out.push(decision);
                }
                out
            };
            for (index, next, settled) in decisions {
                let (chosen, reason) = choose(&settled_results[index], &settled, &check.missing);
                let opinion = SecondOpinion {
                    model: next.1.id.to_string(),
                    chosen,
                    reason,
                };
                let attempts = settled_results[index].report.attempts + settled.attempts;
                if chosen == Chosen::Second {
                    let mut replaced = to_result(&input.chunks[index], settled, Vec::new());
                    replaced.report.attempts = attempts;
                    settled_results[index] = replaced;
                } else {
                    settled_results[index].report.attempts = attempts;
                }
                settled_results[index].report.second_opinion = Some(opinion);
            }
            let (recheck, flags) = crosscheck(
                input.book,
                input.nav,
                input.chunks,
                &extracted_titles(&settled_results),
            );
            check = recheck;
            for r in &mut settled_results {
                r.report.flags.retain(|f| {
                    !matches!(f, Flag::MissingNavTitle { .. } | Flag::PhantomTitle { .. })
                });
            }
            for (chunk_index, flag) in flags {
                settled_results[chunk_index].report.flags.push(flag);
            }
        }
    }

    // Phase 4: whole-book escalation.
    let mut escalation = None;
    let mut exhausted = false;
    if input.options.whole_book_escalation && !settled_results.is_empty() {
        let flagged = settled_results
            .iter()
            .filter(|r| !r.report.flags.is_empty() || r.lowered.is_none())
            .count();
        let fraction = flagged as f32 / settled_results.len() as f32;
        let low_recall = check.recall.is_some_and(|r| r < ESCALATION_RECALL_FLOOR);
        if fraction > ESCALATION_FLAG_FRACTION || low_recall {
            let max_tier = settled_results
                .iter()
                .filter_map(|r| r.report.final_model.as_deref())
                .filter_map(|id| ladder.iter().find(|(_, m)| m.id == id).map(|(t, _)| *t))
                .max()
                .unwrap_or(0);
            let from = ladder
                .get(max_tier)
                .map(|(_, m)| m.id.to_string())
                .unwrap_or_default();
            let reason = if low_recall {
                format!(
                    "contents recall {:.0}% below {:.0}%",
                    check.recall.unwrap_or(0.0) * 100.0,
                    ESCALATION_RECALL_FLOOR * 100.0
                )
            } else {
                format!("{:.0}% of chunks flagged", fraction * 100.0)
            };
            let next = ladder
                .iter()
                .copied()
                .skip(max_tier + 1)
                .find(|(_, m)| !shared.is_exhausted(m));
            match next {
                Some(next) => {
                    progress(shared.progress(Phase::Escalation, &results, settled_results.len()));
                    let rerun: Vec<(usize, Settled)> = {
                        let mut stream = stream::iter(input.chunks.iter())
                            .map(move |chunk| async move {
                                (
                                    chunk.index,
                                    shared
                                        .settle(chunk, &[next], CallPurpose::Escalation, None)
                                        .await,
                                )
                            })
                            .buffer_unordered(concurrency);
                        let mut out = Vec::new();
                        while let Some(item) = stream.next().await {
                            out.push(item);
                        }
                        out
                    };
                    for (index, settled) in rerun {
                        let attempts = settled_results[index].report.attempts + settled.attempts;
                        if settled.lowered.is_some() {
                            let mut replaced = to_result(&input.chunks[index], settled, Vec::new());
                            replaced.report.attempts = attempts;
                            settled_results[index] = replaced;
                        } else {
                            settled_results[index].report.attempts = attempts;
                        }
                    }
                    let (recheck, flags) = crosscheck(
                        input.book,
                        input.nav,
                        input.chunks,
                        &extracted_titles(&settled_results),
                    );
                    check = recheck;
                    for (chunk_index, flag) in flags {
                        settled_results[chunk_index].report.flags.push(flag);
                    }
                    escalation = Some(Escalation {
                        reason,
                        flagged_fraction: fraction,
                        from_model: from,
                        to_model: next.1.id.to_string(),
                    });
                }
                None => {
                    exhausted = true;
                    escalation = Some(Escalation {
                        reason,
                        flagged_fraction: fraction,
                        from_model: from,
                        to_model: String::new(),
                    });
                }
            }
        }
    }

    let failed = settled_results.iter().any(|r| r.lowered.is_none());
    RunOutput {
        chunks: settled_results,
        calls: take_calls(shared),
        crosscheck: check,
        escalation,
        eta_trace,
        incomplete: failed || exhausted,
        cancelled: false,
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::cache::{MemoryCache, NoCache};
    use crate::chunk::{ChunkOptions, chunk as make_chunks};
    use crate::epub::open::SpineDoc;
    use crate::gateway::Route;
    use crate::models::model;
    use crate::test_support::{
        Oracle, RoleMap, ScriptedTransport, error_response, request_meta, tool_response,
    };
    use serde_json::json;
    use std::sync::Arc;

    fn roles() -> RoleMap {
        RoleMap {
            title: &["t"],
            ingredient: &["i"],
            step: &["s"],
            ..RoleMap::default()
        }
    }

    /// Four recipes, each its own chunk at a small budget.
    fn book() -> (BookLines, Nav, Vec<Chunk>) {
        let mut html = String::new();
        for name in ["Apple Pie", "Bean Soup", "Corn Bread", "Duck Rice"] {
            html.push_str(&format!("<h2 class=\"t\">{name}</h2>"));
            for i in 0..5 {
                html.push_str(&format!(
                    "<p class=\"i\">{} cups ingredient {i} for {name}</p>",
                    i + 1
                ));
            }
            html.push_str(&format!("<p class=\"s\">Cook everything for {name} until done and serve it warm to the table.</p>"));
        }
        let doc = SpineDoc {
            index: 0,
            path: "c.xhtml".into(),
            xhtml: format!("<html><body>{html}</body></html>"),
        };
        let nav = Nav::default();
        let book = BookLines::build(&[doc], &nav);
        let chunks = make_chunks(
            &book,
            &ChunkOptions {
                budget: 200,
                slack: 400,
            },
        );
        assert_eq!(chunks.len(), 4, "{chunks:?}");
        (book, nav, chunks)
    }

    fn ladder(ids: &[&str]) -> Vec<&'static Model> {
        ids.iter().map(|id| model(id).unwrap()).collect()
    }

    fn options(second_opinion: bool, escalation: bool) -> ExtractOptions {
        ExtractOptions {
            label: "Test".into(),
            concurrency: 2,
            second_opinion,
            whole_book_escalation: escalation,
            ..ExtractOptions::default()
        }
    }

    fn input<'a>(
        book: &'a BookLines,
        nav: &'a Nav,
        chunks: &'a [Chunk],
        ladder: Vec<&'static Model>,
        options: &'a ExtractOptions,
    ) -> RunInput<'a> {
        RunInput {
            book,
            nav,
            chunks,
            ladder,
            options,
            started: Timestamp::now(),
        }
    }

    /// The oracle's answer for the chunk a request names.
    fn oracle_answer(
        request: &crate::transport::HttpRequest,
        book: &BookLines,
        chunks: &[Chunk],
    ) -> crate::transport::HttpResponse {
        let (model_id, chunk_id, _) = request_meta(request).unwrap();
        let chunk = chunks.iter().find(|c| c.id == chunk_id).unwrap();
        let payload = Oracle::new(roles()).payload(chunk, book);
        tool_response(
            model(&model_id).unwrap().route,
            &payload,
            Usage {
                input_tokens: 100,
                output_tokens: 20,
                ..Usage::default()
            },
            false,
        )
    }

    async fn drive(
        transport: &ScriptedTransport,
        cache: &impl ChunkCache,
        book: &BookLines,
        nav: &Nav,
        chunks: &[Chunk],
        ladder: Vec<&'static Model>,
        options: &ExtractOptions,
    ) -> (RunOutput, Vec<Progress>) {
        let input = input(book, nav, chunks, ladder, options);
        let mut events = Vec::new();
        let out = run(&input, transport, cache, &CancelToken::new(), &mut |p| {
            events.push(p)
        })
        .await;
        (out, events)
    }

    #[tokio::test]
    async fn happy_path_settles_every_chunk_once() {
        let (book, nav, chunks) = book();
        let (b, c) = (book.clone(), chunks.clone());
        let transport = ScriptedTransport::new(move |req, _| Ok(oracle_answer(req, &b, &c)));
        let (out, events) = drive(
            &transport,
            &NoCache,
            &book,
            &nav,
            &chunks,
            ladder(&["gemini-2.5-flash", "claude-haiku-4-5"]),
            &options(true, true),
        )
        .await;
        assert_eq!(transport.calls(), 4);
        assert!(
            out.chunks
                .iter()
                .all(|r| r.report.status == ChunkStatus::Ok && r.report.flags.is_empty())
        );
        assert!(
            out.chunks
                .iter()
                .all(|r| r.report.final_model.as_deref() == Some("gemini-2.5-flash"))
        );
        assert_eq!(
            out.chunks.iter().map(|r| r.report.recipes).sum::<usize>(),
            4
        );
        assert_eq!(out.calls.len(), 4);
        assert!(
            out.calls
                .iter()
                .all(|c| c.purpose == CallPurpose::Extract && c.attempt == 1 && !c.cached)
        );
        assert!(
            out.calls
                .iter()
                .all(|c| c.cost_usd.is_some_and(|c| c > 0.0))
        );
        assert!(!out.incomplete && !out.cancelled && out.escalation.is_none());
        assert_eq!(out.crosscheck.recall, None, "no nav");
        let last = events.last().unwrap();
        assert_eq!((last.done, last.total, last.failed), (4, 4, 0));
        assert_eq!(last.recipes_so_far, 4);
        assert!(last.cost_so_far_usd > 0.0);
        assert_eq!(out.eta_trace.len(), 4);
    }

    #[tokio::test]
    async fn invalid_answers_retry_with_feedback_then_step_the_ladder() {
        let (book, nav, chunks) = book();
        let (b, c) = (book.clone(), chunks.clone());
        let transport = ScriptedTransport::new(move |req, _| {
            let (model_id, chunk_id, _) = request_meta(req).unwrap();
            // The first model never claims every line for chunk k001.
            if chunk_id == "k001" && model_id == "gemini-2.5-flash" {
                let user = crate::test_support::request_user_text(req);
                if req.body.to_string().contains("rejected") {
                    assert!(
                        user.contains("not assigned"),
                        "feedback is appended: {user}"
                    );
                }
                return Ok(tool_response(
                    Route::CompatChat,
                    &json!({"items": [], "ignored": [0]}),
                    Usage::default(),
                    false,
                ));
            }
            Ok(oracle_answer(req, &b, &c))
        });
        let (out, _) = drive(
            &transport,
            &NoCache,
            &book,
            &nav,
            &chunks,
            ladder(&["gemini-2.5-flash", "claude-haiku-4-5"]),
            &options(false, false),
        )
        .await;
        let k1 = &out.chunks[1];
        assert_eq!(k1.report.status, ChunkStatus::Ok);
        assert_eq!(k1.report.final_model.as_deref(), Some("claude-haiku-4-5"));
        assert_eq!(k1.report.attempts, 3);
        let k1_calls: Vec<&CallRecord> =
            out.calls.iter().filter(|c| c.chunk_id == "k001").collect();
        assert_eq!(
            k1_calls
                .iter()
                .map(|c| (c.model.as_str(), c.purpose, c.attempt))
                .collect::<Vec<_>>(),
            [
                ("gemini-2.5-flash", CallPurpose::Extract, 1),
                ("gemini-2.5-flash", CallPurpose::Retry, 2),
                ("claude-haiku-4-5", CallPurpose::Extract, 1),
            ]
        );
        assert!(matches!(k1_calls[0].outcome, CallOutcome::Invalid { .. }));
        assert_eq!(k1_calls[2].outcome, CallOutcome::Ok);
        assert_eq!(transport.calls(), 6);
    }

    /// A "Wholesale Rate limited" 429 means the gateway refuses the model for
    /// the account: no pause, no retry, and no further chunk tries it.
    #[tokio::test]
    async fn quota_exhausted_models_are_skipped_for_the_rest_of_the_run() {
        let (book, nav, chunks) = book();
        let (b, c) = (book.clone(), chunks.clone());
        let transport = ScriptedTransport::new(move |req, _| {
            let (model_id, _, _) = request_meta(req).unwrap();
            if model_id == "gemini-2.5-flash" {
                Ok(error_response(
                    429,
                    "{\"error\":[{\"code\":2018,\"message\":\"Wholesale Rate limited\"}]}",
                ))
            } else {
                Ok(oracle_answer(req, &b, &c))
            }
        });
        let opts = ExtractOptions {
            concurrency: 1,
            ..options(false, false)
        };
        let (out, _) = drive(
            &transport,
            &NoCache,
            &book,
            &nav,
            &chunks,
            ladder(&["gemini-2.5-flash", "claude-haiku-4-5"]),
            &opts,
        )
        .await;
        assert!(transport.sleeps_ms.lock().unwrap().is_empty());
        assert!(!out.incomplete);
        let refused = out
            .calls
            .iter()
            .filter(|c| c.model == "gemini-2.5-flash")
            .count();
        assert_eq!(refused, 1, "only the first chunk tries the refused model");
        assert!(
            out.chunks
                .iter()
                .all(|c| c.report.final_model.as_deref() == Some("claude-haiku-4-5"))
        );
    }

    #[tokio::test]
    async fn transient_errors_pause_then_retry_and_hard_errors_step_the_ladder() {
        let (book, nav, chunks) = book();
        let (b, c) = (book.clone(), chunks.clone());
        let transport = ScriptedTransport::new(move |req, n| {
            let (model_id, chunk_id, _) = request_meta(req).unwrap();
            match (chunk_id.as_str(), model_id.as_str(), n) {
                ("k000", "gemini-2.5-flash", 0) => Ok(error_response(429, "slow")),
                ("k002", "gemini-2.5-flash", _) => Ok(error_response(400, "bad request")),
                _ => Ok(oracle_answer(req, &b, &c)),
            }
        });
        let opts = ExtractOptions {
            concurrency: 1,
            ..options(false, false)
        };
        let (out, _) = drive(
            &transport,
            &NoCache,
            &book,
            &nav,
            &chunks,
            ladder(&["gemini-2.5-flash", "claude-haiku-4-5"]),
            &opts,
        )
        .await;
        assert_eq!(*transport.sleeps_ms.lock().unwrap(), vec![2000]);
        assert_eq!(
            out.chunks[0].report.final_model.as_deref(),
            Some("gemini-2.5-flash")
        );
        assert_eq!(out.chunks[0].report.attempts, 2);
        assert_eq!(
            out.chunks[2].report.final_model.as_deref(),
            Some("claude-haiku-4-5")
        );
        assert_eq!(
            out.chunks[2].report.attempts, 2,
            "a 400 is not retried on the same model"
        );
        assert!(
            out.calls.iter().any(
                |c| c.status == Some(429) && matches!(c.outcome, CallOutcome::Transport { .. })
            )
        );
    }

    #[tokio::test]
    async fn truncation_is_retried_once_then_kept_with_a_flag() {
        let (book, nav, chunks) = book();
        let (b, c) = (book.clone(), chunks.clone());
        let transport = ScriptedTransport::new(move |req, _| {
            let mut r = oracle_answer(req, &b, &c);
            r.body = r.body.replace(
                "\"finish_reason\":\"tool_calls\"",
                "\"finish_reason\":\"length\"",
            );
            Ok(r)
        });
        let (out, _) = drive(
            &transport,
            &NoCache,
            &book,
            &nav,
            &chunks,
            ladder(&["gemini-2.5-flash"]),
            &options(false, false),
        )
        .await;
        assert!(
            out.chunks
                .iter()
                .all(|r| r.report.status == ChunkStatus::Ok)
        );
        assert!(out.chunks.iter().all(|r| r.report.attempts == 2));
        assert!(
            out.chunks
                .iter()
                .all(|r| r.report.flags == [Flag::Truncated])
        );
        assert!(out.calls.iter().all(|c| c.truncated));
    }

    #[tokio::test]
    async fn every_model_failing_marks_the_chunk_failed_and_the_run_incomplete() {
        let (book, nav, chunks) = book();
        let (b, c) = (book.clone(), chunks.clone());
        let transport = ScriptedTransport::new(move |req, _| {
            let (_, chunk_id, _) = request_meta(req).unwrap();
            if chunk_id == "k003" {
                Err(TransportError::Other("boom".into()))
            } else {
                Ok(oracle_answer(req, &b, &c))
            }
        });
        let (out, events) = drive(
            &transport,
            &NoCache,
            &book,
            &nav,
            &chunks,
            ladder(&["gemini-2.5-flash", "claude-haiku-4-5"]),
            &options(false, false),
        )
        .await;
        assert_eq!(out.chunks[3].report.status, ChunkStatus::Failed);
        assert!(out.chunks[3].lowered.is_none());
        assert_eq!(out.chunks[3].report.attempts, 2);
        assert!(out.incomplete);
        assert_eq!(events.last().unwrap().failed, 1);
    }

    #[tokio::test]
    async fn cache_hits_cost_nothing_and_skip_the_transport() {
        let (book, nav, chunks) = book();
        let (b, c) = (book.clone(), chunks.clone());
        let transport = ScriptedTransport::new(move |req, _| Ok(oracle_answer(req, &b, &c)));
        let cache = MemoryCache::default();
        let l = || ladder(&["gemini-2.5-flash"]);
        let (first, _) = drive(
            &transport,
            &cache,
            &book,
            &nav,
            &chunks,
            l(),
            &options(false, false),
        )
        .await;
        assert_eq!(transport.calls(), 4);
        assert_eq!(cache.len(), 4);
        let (second, events) = drive(
            &transport,
            &cache,
            &book,
            &nav,
            &chunks,
            l(),
            &options(false, false),
        )
        .await;
        assert_eq!(transport.calls(), 4, "no new transport calls");
        assert!(
            second
                .calls
                .iter()
                .all(|c| c.cached && c.cost_usd == Some(0.0) && c.usage.is_zero())
        );
        assert!(second.chunks.iter().all(|r| r.report.cached));
        assert_eq!(events.last().unwrap().cached, 4);
        assert_eq!(
            first
                .chunks
                .iter()
                .map(|r| r.lowered.clone())
                .collect::<Vec<_>>(),
            second
                .chunks
                .iter()
                .map(|r| r.lowered.clone())
                .collect::<Vec<_>>()
        );
    }

    #[tokio::test]
    async fn flagged_chunks_get_a_second_opinion_from_the_next_model() {
        let (book, nav, chunks) = book();
        let (b, c) = (book.clone(), chunks.clone());
        let transport = ScriptedTransport::new(move |req, _| {
            let (model_id, chunk_id, _) = request_meta(req).unwrap();
            if chunk_id == "k002" && model_id == "gemini-2.5-flash" {
                // Valid but suspicious: four quantity lines end up ignored.
                let chunk = c.iter().find(|c| c.id == "k002").unwrap();
                let n = chunk.lines();
                let ignored: Vec<usize> = (1..n).collect();
                return Ok(tool_response(
                    Route::CompatChat,
                    &json!({"items":[{"title":[0],"sections":[{"ingredients":[1]}]}],"ignored":ignored[1..]}),
                    Usage::default(),
                    false,
                ));
            }
            Ok(oracle_answer(req, &b, &c))
        });
        let (out, _) = drive(
            &transport,
            &NoCache,
            &book,
            &nav,
            &chunks,
            ladder(&["gemini-2.5-flash", "claude-haiku-4-5"]),
            &options(true, false),
        )
        .await;
        assert_eq!(transport.calls_with_purpose("second_opinion"), 1);
        let k2 = &out.chunks[2];
        let opinion = k2.report.second_opinion.as_ref().unwrap();
        assert_eq!(opinion.model, "claude-haiku-4-5");
        assert_eq!(opinion.chosen, Chosen::Second);
        assert!(opinion.reason.contains("fewer flags"), "{}", opinion.reason);
        assert_eq!(k2.report.final_model.as_deref(), Some("claude-haiku-4-5"));
        assert!(k2.report.flags.is_empty());
        assert_eq!(k2.report.attempts, 2);
        let second_call = out
            .calls
            .iter()
            .find(|c| c.purpose == CallPurpose::SecondOpinion)
            .unwrap();
        assert_eq!(second_call.chunk_id, "k002");
    }

    #[tokio::test]
    async fn heavy_flagging_escalates_the_whole_book() {
        let (book, nav, chunks) = book();
        let (b, c) = (book.clone(), chunks.clone());
        let transport = ScriptedTransport::new(move |req, _| {
            let (model_id, chunk_id, _) = request_meta(req).unwrap();
            // The first model returns thin stepless recipes for every chunk.
            if model_id == "gemini-2.5-flash" {
                let chunk = c.iter().find(|c| c.id == chunk_id).unwrap();
                let ignored: Vec<usize> = (2..chunk.lines()).collect();
                return Ok(tool_response(
                    Route::CompatChat,
                    &json!({"items":[{"title":[0],"sections":[{"ingredients":[1]}]}],"ignored":ignored}),
                    Usage::default(),
                    false,
                ));
            }
            Ok(oracle_answer(req, &b, &c))
        });
        // Second opinion off so escalation is what fixes it.
        let (out, _) = drive(
            &transport,
            &NoCache,
            &book,
            &nav,
            &chunks,
            ladder(&["gemini-2.5-flash", "claude-haiku-4-5"]),
            &options(false, true),
        )
        .await;
        let esc = out.escalation.as_ref().unwrap();
        assert_eq!(
            (esc.from_model.as_str(), esc.to_model.as_str()),
            ("gemini-2.5-flash", "claude-haiku-4-5")
        );
        assert!(esc.flagged_fraction > ESCALATION_FLAG_FRACTION);
        assert_eq!(transport.calls_with_purpose("escalation"), 4);
        assert!(out.chunks.iter().all(|r| r.report.final_model.as_deref()
            == Some("claude-haiku-4-5")
            && r.report.flags.is_empty()));
        assert!(!out.incomplete);
        // With a single-model ladder there is nowhere to go.
        let (b2, c2) = (book.clone(), chunks.clone());
        let transport = ScriptedTransport::new(move |req, _| {
            let (_, chunk_id, _) = request_meta(req).unwrap();
            let chunk = c2.iter().find(|c| c.id == chunk_id).unwrap();
            let ignored: Vec<usize> = (2..chunk.lines()).collect();
            let _ = &b2;
            Ok(tool_response(
                Route::CompatChat,
                &json!({"items":[{"title":[0],"sections":[{"ingredients":[1]}]}],"ignored":ignored}),
                Usage::default(),
                false,
            ))
        });
        let (out, _) = drive(
            &transport,
            &NoCache,
            &book,
            &nav,
            &chunks,
            ladder(&["gemini-2.5-flash"]),
            &options(false, true),
        )
        .await;
        assert!(out.incomplete);
        assert_eq!(out.escalation.as_ref().unwrap().to_model, "");
    }

    #[tokio::test]
    async fn cancellation_returns_what_settled() {
        let (book, nav, chunks) = book();
        let (b, c) = (book.clone(), chunks.clone());
        let token = CancelToken::new();
        let t2 = token.clone();
        let hits = Arc::new(AtomicUsize::new(0));
        let h2 = hits.clone();
        let transport = ScriptedTransport::new(move |req, _| {
            if h2.fetch_add(1, Ordering::SeqCst) == 1 {
                t2.cancel();
                return Err(TransportError::Cancelled);
            }
            Ok(oracle_answer(req, &b, &c))
        });
        let opts = ExtractOptions {
            concurrency: 1,
            ..options(true, true)
        };
        let input = input(
            &book,
            &nav,
            &chunks,
            ladder(&["gemini-2.5-flash", "claude-haiku-4-5"]),
            &opts,
        );
        let out = run(&input, &transport, &NoCache, &token, &mut |_| {}).await;
        assert!(out.cancelled && out.incomplete);
        assert_eq!(
            out.chunks
                .iter()
                .filter(|r| r.report.status == ChunkStatus::Ok)
                .count(),
            1
        );
        assert!(transport.calls() <= 2);
        assert!(out.escalation.is_none());
    }
}
