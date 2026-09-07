//! Runtime-independent whole-book extraction orchestration.
//!
//! Callers provide the extraction transport; this module owns bounded scheduling,
//! model escalation, failure salvage, stable ordering, progress previews, usage
//! accounting, and final assembly. It deliberately has no HTTP, filesystem,
//! cache, Tokio, or browser dependencies.

use futures::stream::{self, StreamExt};
use serde::Serialize;

use crate::{
    Chunk, ChunkExtractionFailure, ChunkOutcome, CookbookRecipe, ExtractedRecipe, Usage,
    assemble_recipes,
};

/// Which caller-provided extraction transport to use for a chunk.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelTier {
    Primary,
    Fallback,
}

/// Whole-book scheduling and reporting options.
#[derive(Debug, Clone)]
pub struct OrchestrationOptions {
    /// Maximum number of chunks extracting concurrently. Zero is treated as one.
    pub concurrency: usize,
    /// Whether a failed primary extraction should be retried through the fallback tier.
    pub fallback: bool,
    /// Whether progress snapshots should include assembled recipe previews.
    pub previews: bool,
}

impl Default for OrchestrationOptions {
    fn default() -> Self {
        Self {
            concurrency: 8,
            fallback: false,
            previews: false,
        }
    }
}

/// One tier's terminal failure, including all usage and truncation signals known
/// to the transport and parser retry driver.
#[derive(Debug, Clone, Serialize)]
pub struct TierFailure {
    pub message: String,
    pub usage: Usage,
    pub truncated: bool,
    pub attempts: Vec<crate::FailedAttempt>,
}

impl From<ChunkExtractionFailure> for TierFailure {
    fn from(failure: ChunkExtractionFailure) -> Self {
        Self {
            message: failure.error.to_string(),
            usage: failure.usage,
            truncated: failure.truncated,
            attempts: failure.attempts,
        }
    }
}

/// A chunk lost after its primary and optional fallback tiers failed.
#[derive(Debug, Clone, Serialize)]
pub struct ChunkFailure {
    pub index: usize,
    pub doc_path: String,
    pub primary: TierFailure,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fallback: Option<TierFailure>,
}

/// Ordered extraction information for a successful chunk.
#[derive(Debug, Clone, Serialize)]
pub struct ChunkReport {
    pub index: usize,
    pub doc_path: String,
    pub tier: ModelTier,
    pub recipes: Vec<ExtractedRecipe>,
    /// Usage across the successful tier plus a failed primary tier, if escalation occurred.
    pub usage: Usage,
    /// Usage from the tier that produced `recipes` (zero for a cache hit).
    pub tier_usage: Usage,
    pub cached: bool,
    pub truncated: bool,
    /// Present when the fallback tier recovered a primary failure.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub primary_failure: Option<TierFailure>,
}

/// A progress snapshot emitted initially and after each completed chunk.
#[derive(Debug, Clone, Serialize)]
pub struct ExtractionProgress {
    pub done: usize,
    pub total: usize,
    pub cached: usize,
    pub failed: usize,
    /// Recipes assembled from the chunks completed so far, in original book order.
    /// A continuation that completes before its head may appear only in a later preview.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preview: Option<Vec<CookbookRecipe>>,
}

/// Complete whole-book extraction result.
#[derive(Debug, Clone, Serialize)]
pub struct ExtractionReport {
    pub recipes: Vec<CookbookRecipe>,
    /// Successful chunks in original input order.
    pub chunks: Vec<ChunkReport>,
    /// Failed chunks in original input order.
    pub failures: Vec<ChunkFailure>,
    /// Usage from every reported call, including failed attempts and fallback calls.
    pub usage: Usage,
    pub primary_usage: Usage,
    pub fallback_usage: Usage,
    pub chunks_cached: usize,
    /// Successful or failed tiers that reported token-limit truncation.
    pub truncations: Vec<ChunkTruncation>,
}

/// A tier that reported token-limit truncation.
#[derive(Debug, Clone, Serialize)]
pub struct ChunkTruncation {
    pub index: usize,
    pub doc_path: String,
    pub tier: ModelTier,
}

struct CompletedChunk {
    chunk: Chunk,
    report: Result<ChunkReport, ChunkFailure>,
}

/// Extract and assemble a complete book using caller-provided transports.
///
/// `extract(index, chunk, tier)` receives an owned, complete [`Chunk`] on each
/// invocation, which makes ordinary async closures (including browser closures
/// with non-`Send` futures) straightforward. The callback should drive exactly
/// one model tier, normally with [`crate::try_extract_chunk_detailed`] when it
/// starts from raw model responses.
///
/// Chunks execute concurrently and may finish in any order. Reports, previews,
/// and final assembly always use their original input order.
pub async fn extract_chunks_with<F, Fut, P>(
    chunks: Vec<Chunk>,
    source: &str,
    options: &OrchestrationOptions,
    extract: F,
    progress: P,
) -> ExtractionReport
where
    F: Fn(usize, Chunk, ModelTier) -> Fut,
    Fut: core::future::Future<Output = Result<ChunkOutcome, ChunkExtractionFailure>>,
    P: Fn(ExtractionProgress),
{
    let total = chunks.len();
    let links: Vec<crate::Link> = chunks
        .iter()
        .flat_map(|chunk| chunk.links.clone())
        .collect();
    progress(ExtractionProgress {
        done: 0,
        total,
        cached: 0,
        failed: 0,
        preview: options.previews.then(Vec::new),
    });

    let mut stream = stream::iter(chunks.into_iter().enumerate().map(|(index, chunk)| {
        let extract = &extract;
        async move {
            let primary = extract(index, chunk.clone(), ModelTier::Primary).await;
            let report = match primary {
                Ok(outcome) => Ok(success_report(
                    index,
                    &chunk.doc_path,
                    ModelTier::Primary,
                    outcome,
                    None,
                )),
                Err(primary) if options.fallback => {
                    let primary = TierFailure::from(primary);
                    match extract(index, chunk.clone(), ModelTier::Fallback).await {
                        Ok(outcome) => Ok(success_report(
                            index,
                            &chunk.doc_path,
                            ModelTier::Fallback,
                            outcome,
                            Some(primary),
                        )),
                        Err(fallback) => Err(ChunkFailure {
                            index,
                            doc_path: chunk.doc_path.clone(),
                            primary,
                            fallback: Some(fallback.into()),
                        }),
                    }
                }
                Err(primary) => Err(ChunkFailure {
                    index,
                    doc_path: chunk.doc_path.clone(),
                    primary: primary.into(),
                    fallback: None,
                }),
            };
            (index, CompletedChunk { chunk, report })
        }
    }))
    .buffer_unordered(options.concurrency.max(1));

    let mut completed: Vec<Option<CompletedChunk>> =
        core::iter::repeat_with(|| None).take(total).collect();
    let mut done = 0;
    let mut cached = 0;
    let mut failed = 0;
    while let Some((index, result)) = stream.next().await {
        match &result.report {
            Ok(report) => cached += usize::from(report.cached),
            Err(_) => failed += 1,
        }
        completed[index] = Some(result);
        done += 1;
        if done < total {
            let preview = options
                .previews
                .then(|| assemble_completed(&completed, &links, source));
            progress(ExtractionProgress {
                done,
                total,
                cached,
                failed,
                preview,
            });
        }
    }

    let report = build_report(completed, links, source);
    if total > 0 {
        progress(ExtractionProgress {
            done,
            total,
            cached,
            failed,
            preview: options.previews.then(|| report.recipes.clone()),
        });
    }
    report
}

fn success_report(
    index: usize,
    doc_path: &str,
    tier: ModelTier,
    outcome: ChunkOutcome,
    primary_failure: Option<TierFailure>,
) -> ChunkReport {
    let tier_usage = outcome.usage;
    let mut usage = tier_usage.clone();
    if let Some(failure) = &primary_failure {
        usage.add(&failure.usage);
    }
    ChunkReport {
        index,
        doc_path: doc_path.to_string(),
        tier,
        recipes: outcome.recipes,
        usage,
        tier_usage,
        cached: outcome.cached,
        truncated: outcome.truncated,
        primary_failure,
    }
}

fn assemble_completed(
    completed: &[Option<CompletedChunk>],
    links: &[crate::Link],
    source: &str,
) -> Vec<CookbookRecipe> {
    let per_chunk = completed
        .iter()
        .flatten()
        .filter_map(|item| match &item.report {
            Ok(report) => Some((item.chunk.clone(), report.recipes.clone())),
            Err(_) => None,
        })
        .collect();
    assemble_recipes(per_chunk, links.to_vec(), source)
}

fn build_report(
    completed: Vec<Option<CompletedChunk>>,
    links: Vec<crate::Link>,
    source: &str,
) -> ExtractionReport {
    let recipes = assemble_completed(&completed, &links, source);
    let mut chunks = Vec::new();
    let mut failures = Vec::new();
    let mut usage = Usage::default();
    let mut primary_usage = Usage::default();
    let mut fallback_usage = Usage::default();
    let mut chunks_cached = 0;
    let mut truncations = Vec::new();

    for completed in completed.into_iter().flatten() {
        match completed.report {
            Ok(report) => {
                usage.add(&report.usage);
                match report.tier {
                    ModelTier::Primary => primary_usage.add(&report.usage),
                    ModelTier::Fallback => {
                        if let Some(primary) = &report.primary_failure {
                            primary_usage.add(&primary.usage);
                            fallback_usage.add(&report.tier_usage);
                            if primary.truncated {
                                truncations.push(ChunkTruncation {
                                    index: report.index,
                                    doc_path: report.doc_path.clone(),
                                    tier: ModelTier::Primary,
                                });
                            }
                        }
                    }
                }
                if report.truncated {
                    truncations.push(ChunkTruncation {
                        index: report.index,
                        doc_path: report.doc_path.clone(),
                        tier: report.tier,
                    });
                }
                chunks_cached += usize::from(report.cached);
                chunks.push(report);
            }
            Err(failure) => {
                usage.add(&failure.primary.usage);
                primary_usage.add(&failure.primary.usage);
                if failure.primary.truncated {
                    truncations.push(ChunkTruncation {
                        index: failure.index,
                        doc_path: failure.doc_path.clone(),
                        tier: ModelTier::Primary,
                    });
                }
                if let Some(fallback) = &failure.fallback {
                    usage.add(&fallback.usage);
                    fallback_usage.add(&fallback.usage);
                    if fallback.truncated {
                        truncations.push(ChunkTruncation {
                            index: failure.index,
                            doc_path: failure.doc_path.clone(),
                            tier: ModelTier::Fallback,
                        });
                    }
                }
                failures.push(failure);
            }
        }
    }

    ExtractionReport {
        recipes,
        chunks,
        failures,
        usage,
        primary_usage,
        fallback_usage,
        chunks_cached,
        truncations,
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use std::cell::{Cell, RefCell};
    use std::rc::Rc;
    use std::task::Poll;

    use futures::future::poll_fn;
    use recipe_scraper::RecipeSection;

    use super::*;
    use crate::{
        CallFailure, CallResult, EpubError, ImageRef, RecipeMeta, try_extract_chunk_detailed,
    };

    fn chunk(path: &str, text: &str) -> Chunk {
        Chunk {
            title_hint: None,
            text: text.to_string(),
            doc_path: path.to_string(),
            links: Vec::new(),
            images: Vec::new(),
        }
    }

    fn recipe(title: &str, ingredient: Option<&str>) -> ExtractedRecipe {
        ExtractedRecipe {
            meta: RecipeMeta {
                title: title.to_string(),
                ..Default::default()
            },
            sections: vec![RecipeSection {
                name: None,
                ingredients: ingredient.into_iter().map(str::to_string).collect(),
                instructions: vec!["Cook.".to_string()],
            }],
        }
    }

    fn outcome(recipes: Vec<ExtractedRecipe>, tokens: u64) -> ChunkOutcome {
        ChunkOutcome {
            recipes,
            usage: Usage {
                input_tokens: tokens,
                ..Default::default()
            },
            cached: false,
            truncated: false,
        }
    }

    fn failure(message: &str, tokens: u64, truncated: bool) -> ChunkExtractionFailure {
        let usage = Usage {
            input_tokens: tokens,
            ..Default::default()
        };
        ChunkExtractionFailure {
            error: EpubError::Proxy(message.to_string()),
            usage: usage.clone(),
            truncated,
            attempts: vec![crate::FailedAttempt {
                attempt: 0,
                message: message.to_string(),
                usage,
                truncated,
            }],
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn completion_order_does_not_change_reports_or_assembly() {
        let delayed_once = Rc::new(Cell::new(false));
        let progress = Rc::new(RefCell::new(Vec::new()));
        let progress_sink = Rc::clone(&progress);
        let report = extract_chunks_with(
            vec![chunk("a.xhtml", "Alpha"), chunk("b.xhtml", "Beta")],
            "book.epub",
            &OrchestrationOptions {
                concurrency: 2,
                previews: true,
                ..Default::default()
            },
            move |index, _, _| {
                let delayed_once = Rc::clone(&delayed_once);
                async move {
                    if index == 0 {
                        poll_fn(move |cx| {
                            if delayed_once.replace(true) {
                                Poll::Ready(())
                            } else {
                                cx.waker().wake_by_ref();
                                Poll::Pending
                            }
                        })
                        .await;
                    }
                    let title = if index == 0 { "Alpha" } else { "Beta" };
                    Ok(outcome(
                        vec![recipe(title, Some("1 item"))],
                        index as u64 + 1,
                    ))
                }
            },
            move |snapshot| progress_sink.borrow_mut().push(snapshot),
        )
        .await;

        assert_eq!(
            report
                .recipes
                .iter()
                .map(|recipe| recipe.meta.title.as_str())
                .collect::<Vec<_>>(),
            ["Alpha", "Beta"]
        );
        assert_eq!(
            report
                .chunks
                .iter()
                .map(|chunk| chunk.index)
                .collect::<Vec<_>>(),
            [0, 1]
        );
        let snapshots = progress.borrow();
        assert_eq!(snapshots.len(), 3);
        assert_eq!(snapshots[0].done, 0);
        assert_eq!(snapshots[1].done, 1);
        assert_eq!(snapshots[1].preview.as_ref().unwrap()[0].meta.title, "Beta");
        assert_eq!(snapshots[2].preview.as_ref().unwrap(), &report.recipes);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn fallback_salvages_partial_book_and_accounts_for_every_tier() {
        let report = extract_chunks_with(
            vec![chunk("a.xhtml", "A"), chunk("b.xhtml", "B")],
            "book.epub",
            &OrchestrationOptions {
                fallback: true,
                ..Default::default()
            },
            |index, _, tier| async move {
                match (index, tier) {
                    (0, ModelTier::Primary) => Err(failure("primary malformed", 10, true)),
                    (0, ModelTier::Fallback) => Ok(outcome(vec![recipe("A", Some("a"))], 20)),
                    (1, ModelTier::Primary) => Err(failure("primary failed", 30, false)),
                    (1, ModelTier::Fallback) => Err(failure("fallback failed", 40, true)),
                    _ => Ok(outcome(Vec::new(), 0)),
                }
            },
            |_| {},
        )
        .await;

        assert_eq!(report.recipes.len(), 1);
        assert_eq!(report.chunks.len(), 1);
        assert_eq!(report.failures.len(), 1);
        assert_eq!(report.usage.input_tokens, 100);
        assert_eq!(report.primary_usage.input_tokens, 40);
        assert_eq!(report.fallback_usage.input_tokens, 60);
        assert_eq!(report.truncations.len(), 2);
        assert_eq!(report.chunks[0].usage.input_tokens, 30);
        assert!(report.chunks[0].primary_failure.is_some());
        assert_eq!(report.failures[0].index, 1);
        assert_eq!(
            report.failures[0]
                .fallback
                .as_ref()
                .unwrap()
                .usage
                .input_tokens,
            40
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn continuation_finishing_first_is_assembled_after_its_head() {
        let first_poll = Rc::new(Cell::new(false));
        let previews = Rc::new(RefCell::new(Vec::new()));
        let preview_sink = Rc::clone(&previews);
        let head = chunk("a.xhtml", "Cake\nflour");
        let mut tail = chunk("b.xhtml", "Bake until done");
        tail.title_hint = Some("Cake".to_string());
        let report = extract_chunks_with(
            vec![head, tail],
            "book.epub",
            &OrchestrationOptions {
                concurrency: 2,
                previews: true,
                ..Default::default()
            },
            move |index, _, _| {
                let first_poll = Rc::clone(&first_poll);
                async move {
                    if index == 0 {
                        poll_fn(move |cx| {
                            if first_poll.replace(true) {
                                Poll::Ready(())
                            } else {
                                cx.waker().wake_by_ref();
                                Poll::Pending
                            }
                        })
                        .await;
                    }
                    Ok(outcome(
                        vec![recipe("Cake", (index == 0).then_some("flour"))],
                        0,
                    ))
                }
            },
            move |snapshot| preview_sink.borrow_mut().push(snapshot),
        )
        .await;

        assert!(previews.borrow()[1].preview.as_ref().unwrap().is_empty());
        assert_eq!(report.recipes.len(), 1);
        assert_eq!(report.recipes[0].sections.len(), 2);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn complete_chunks_preserve_text_links_and_image_positions() {
        let mut recipes = chunk(
            "recipes.xhtml",
            "Sauce\n1 item\nDish\n1 recipe Sauce (this page)",
        );
        recipes.images.extend([
            (
                0,
                ImageRef {
                    path: "sauce.jpg".to_string(),
                    mime: "image/jpeg".to_string(),
                    alt: None,
                },
            ),
            (
                2,
                ImageRef {
                    path: "dish.jpg".to_string(),
                    mime: "image/jpeg".to_string(),
                    alt: None,
                },
            ),
        ]);
        let mut links = chunk("links.xhtml", "Index");
        links.links.push(crate::Link {
            text: "Sauce".to_string(),
            href: "recipes.xhtml#sauce".to_string(),
        });
        let report = extract_chunks_with(
            vec![recipes, links],
            "book.epub",
            &OrchestrationOptions::default(),
            |index, chunk, _| async move {
                let extracted = if index == 0 {
                    assert_eq!(
                        chunk.text,
                        "Sauce\n1 item\nDish\n1 recipe Sauce (this page)"
                    );
                    assert_eq!(chunk.images[0].0, 0);
                    assert_eq!(chunk.images[1].0, 2);
                    vec![
                        recipe("Sauce", Some("1 item")),
                        recipe("Dish", Some("1 recipe Sauce (this page)")),
                    ]
                } else {
                    assert_eq!(chunk.text, "Index");
                    assert_eq!(chunk.links.len(), 1);
                    Vec::new()
                };
                Ok(outcome(extracted, 0))
            },
            |_| {},
        )
        .await;
        assert_eq!(report.recipes[0].image.as_ref().unwrap().path, "sauce.jpg");
        assert_eq!(report.recipes[1].image.as_ref().unwrap().path, "dish.jpg");
        assert_eq!(report.recipes[1].references.len(), 1);
        assert_eq!(
            report.recipes[1].references[0].confidence,
            crate::RefConfidence::Linked
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn concurrency_is_bounded_and_cached_progress_is_monotonic() {
        let active = Rc::new(Cell::new(0usize));
        let max_active = Rc::new(Cell::new(0usize));
        let observed_max = Rc::clone(&max_active);
        let progress = Rc::new(RefCell::new(Vec::new()));
        let progress_sink = Rc::clone(&progress);
        let chunks = (0..5)
            .map(|index| chunk(&format!("{index}.xhtml"), &format!("Recipe {index}")))
            .collect();
        let report = extract_chunks_with(
            chunks,
            "book.epub",
            &OrchestrationOptions {
                concurrency: 2,
                ..Default::default()
            },
            move |index, _, _| {
                let active = Rc::clone(&active);
                let max_active = Rc::clone(&max_active);
                async move {
                    let mut entered = false;
                    poll_fn(move |cx| {
                        if entered {
                            active.set(active.get() - 1);
                            Poll::Ready(())
                        } else {
                            entered = true;
                            active.set(active.get() + 1);
                            max_active.set(max_active.get().max(active.get()));
                            cx.waker().wake_by_ref();
                            Poll::Pending
                        }
                    })
                    .await;
                    Ok(ChunkOutcome {
                        recipes: vec![recipe(&format!("Recipe {index}"), Some("item"))],
                        usage: Usage::default(),
                        cached: index % 2 == 0,
                        truncated: false,
                    })
                }
            },
            move |snapshot| progress_sink.borrow_mut().push(snapshot),
        )
        .await;

        assert_eq!(report.chunks.len(), 5);
        assert_eq!(report.chunks_cached, 3);
        assert_eq!(observed_max.get(), 2);
        let snapshots = progress.borrow();
        assert_eq!(snapshots.len(), 6);
        assert!(
            snapshots
                .windows(2)
                .all(|pair| pair[0].cached <= pair[1].cached)
        );
        assert_eq!(snapshots.last().unwrap().cached, 3);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn empty_and_zero_concurrency_are_valid() {
        let calls = Cell::new(0);
        let progress = RefCell::new(Vec::new());
        let report = extract_chunks_with(
            Vec::new(),
            "book.epub",
            &OrchestrationOptions {
                concurrency: 0,
                ..Default::default()
            },
            |_, _, _| {
                calls.set(calls.get() + 1);
                async { Ok(outcome(Vec::new(), 0)) }
            },
            |snapshot| progress.borrow_mut().push(snapshot),
        )
        .await;
        assert!(report.recipes.is_empty());
        assert_eq!(calls.get(), 0);
        assert_eq!(progress.borrow().len(), 1);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn successful_truncation_is_reported_without_escalation() {
        let calls = RefCell::new(Vec::new());
        let report = extract_chunks_with(
            vec![chunk("a.xhtml", "A")],
            "book.epub",
            &OrchestrationOptions {
                concurrency: 0,
                fallback: true,
                ..Default::default()
            },
            |_, _, tier| {
                calls.borrow_mut().push(tier);
                async move {
                    Ok(ChunkOutcome {
                        recipes: vec![recipe("A", Some("a"))],
                        usage: Usage::default(),
                        cached: false,
                        truncated: true,
                    })
                }
            },
            |_| {},
        )
        .await;
        assert_eq!(&*calls.borrow(), &[ModelTier::Primary]);
        assert_eq!(report.truncations.len(), 1);
        assert_eq!(report.truncations[0].tier, ModelTier::Primary);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn disabled_fallback_reports_an_all_failed_book() {
        let calls = Cell::new(0);
        let report = extract_chunks_with(
            vec![chunk("a.xhtml", "A")],
            "book.epub",
            &OrchestrationOptions::default(),
            |_, _, tier| {
                calls.set(calls.get() + 1);
                async move {
                    assert_eq!(tier, ModelTier::Primary);
                    Err(failure("no recipe", 5, false))
                }
            },
            |_| {},
        )
        .await;
        assert!(report.recipes.is_empty());
        assert!(report.chunks.is_empty());
        assert_eq!(report.failures.len(), 1);
        assert!(report.failures[0].fallback.is_none());
        assert_eq!(calls.get(), 1);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn detailed_retry_keeps_usage_from_retryable_call_failures() {
        let calls = Cell::new(0);
        let result = try_extract_chunk_detailed("doc", || {
            let attempt = calls.get();
            calls.set(attempt + 1);
            async move {
                if attempt == 0 {
                    Err(CallFailure::retryable_payload(
                        EpubError::Deserialize(
                            serde_json::from_str::<serde_json::Value>("{").unwrap_err(),
                        ),
                        Usage {
                            input_tokens: 7,
                            ..Default::default()
                        },
                        false,
                    ))
                } else {
                    Ok(CallResult {
                        input: Some(serde_json::json!({
                            "recipes": [{
                                "title": "Recovered",
                                "sections": [{ "ingredients": ["one"] }]
                            }]
                        })),
                        usage: Usage {
                            input_tokens: 11,
                            ..Default::default()
                        },
                        truncated: false,
                    })
                }
            }
        })
        .await
        .unwrap();
        assert_eq!(calls.get(), 2);
        assert_eq!(result.usage.input_tokens, 18);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn detailed_retry_failure_keeps_all_attempt_usage() {
        let calls = Cell::new(0);
        let failure = try_extract_chunk_detailed("doc", || {
            let attempt = calls.get();
            calls.set(attempt + 1);
            async move {
                if attempt == 0 {
                    Ok(CallResult {
                        input: Some(serde_json::json!({ "recipes": "[bad" })),
                        usage: Usage {
                            input_tokens: 7,
                            ..Default::default()
                        },
                        truncated: false,
                    })
                } else {
                    Err(CallFailure::transport_with_metadata(
                        EpubError::Proxy("connection reset".to_string()),
                        Usage {
                            input_tokens: 11,
                            ..Default::default()
                        },
                        false,
                    ))
                }
            }
        })
        .await
        .unwrap_err();
        assert_eq!(calls.get(), 2);
        assert_eq!(failure.usage.input_tokens, 18);
        assert_eq!(failure.attempts.len(), 2);
    }
}
