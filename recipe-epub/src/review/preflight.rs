//! Read-only planning shared by both native entry points.
use super::{ReviewRun, RunOptions, error};
use crate::{Chunk, EpubError, Usage};
use serde::{Deserialize, Serialize};
/// A complete USD range, or unknown; endpoints cannot be independently absent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct CostEstimate {
    pub usd: Option<(f64, f64)>,
    pub basis: String,
    pub uncalibrated: bool,
}
impl CostEstimate {
    fn new(basis: &str, uncalibrated: bool) -> Self {
        Self {
            usd: Some((0.0, 0.0)),
            basis: basis.into(),
            uncalibrated,
        }
    }

    fn add(&mut self, usd: Option<(f64, f64)>) {
        self.usd = sum_ranges(self.usd, usd);
    }
}

fn sum_ranges(a: Option<(f64, f64)>, b: Option<(f64, f64)>) -> Option<(f64, f64)> {
    a.zip(b)
        .map(|((a_low, a_high), (b_low, b_high))| (a_low + b_low, a_high + b_high))
}

#[derive(Debug, Clone, Serialize)]
pub struct Preflight {
    pub recovery: Option<RecoveryPreflight>,
    pub total: usize,
    pub cached: usize,
    pub pending: usize,
    pub estimated_low_usd: Option<f64>,
    pub estimated_high_usd: Option<f64>,
    pub extraction: CostEstimate,
    pub verification: CostEstimate,
    pub reservation_usd: Option<f64>,
    pub basis: &'static str,
}
impl Preflight {
    // Keep the established public total fields as a compatibility projection.
    fn update_total(&mut self) {
        let total = sum_ranges(self.extraction.usd, self.verification.usd);
        self.estimated_low_usd = total.map(|range| range.0);
        self.estimated_high_usd = total.map(|range| range.1);
    }
}
#[derive(Debug, Clone, Serialize)]
pub struct RecoveryPreflight {
    pub models: Vec<String>,
    pub extraction_remaining_usd: f64,
    pub verification_remaining_usd: f64,
    pub verification_pending: usize,
    pub verification_cached: usize,
}
pub fn reservation(model: &str, source: &Chunk) -> Result<f64, EpubError> {
    let bytes = serde_json::to_vec(&crate::indexed::build_indexed_chunk_request(source))?.len()
        as u64
        + 4096;
    crate::accounting::cost_for_usage(
        model,
        &Usage {
            input_tokens: bytes * 2,
            // The legacy driver may make one payload repair retry. Keep this
            // helper conservative for that path; recovery actions reserve each
            // attempt separately from their exact output limit below.
            output_tokens: 32_000,
            ..Usage::default()
        },
    )
    .ok_or_else(|| error("cannot reserve budget for an unpriced model"))
}
pub fn plan(run: &ReviewRun, options: &RunOptions) -> Result<Preflight, EpubError> {
    if !options.budget_usd.is_finite() || options.budget_usd < 0.0 {
        return Err(error("budget must be finite and nonnegative"));
    }
    if options.refresh && !options.allow_network {
        return Err(error("refresh requires network access"));
    }
    if run.prompt_version != crate::cache::PROMPT_VERSION {
        return Err(error("prompt changed; create a new extraction run"));
    }
    if run.metadata.as_ref().is_some_and(|m| {
        !m.prompt_fingerprint.is_empty()
            && m.prompt_fingerprint != crate::cache::prompt_fingerprint()
    }) {
        return Err(error(
            "prompt/schema fingerprint changed; create a new extraction run",
        ));
    }
    for id in &options.chunks {
        if !run.chunks.iter().any(|c| &c.id == id) {
            return Err(error(format!("unknown chunk {id}")));
        }
    }
    if let Some(state) = &run.recovery {
        return recovery_plan(run, options, state);
    }
    let directory = options
        .cache_dir
        .clone()
        .unwrap_or_else(crate::cache::default_dir);
    let mut result = Preflight {
        recovery: None,
        total: 0,
        cached: 0,
        pending: 0,
        estimated_low_usd: Some(0.0),
        estimated_high_usd: Some(0.0),
        reservation_usd: Some(0.0),
        basis: "Approximate extraction and verification ranges; token-based estimates, not a billing statement; excludes credit-purchase fees.",
        extraction: CostEstimate::new(
            "Request-size estimate; no compatible completed extraction observations were available.",
            true,
        ),
        verification: CostEstimate::new("No verification work is pending for this run.", false),
    };
    for c in &run.chunks {
        if !options.chunks.is_empty() && !options.chunks.contains(&c.id) {
            continue;
        }
        result.total += 1;
        if !options.refresh
            && (c.output.is_some()
                || crate::cache::read_entry(
                    &directory,
                    &crate::cache::identity(&run.model, &c.source)?,
                )
                .is_some())
        {
            result.cached += 1;
            continue;
        }
        result.pending += 1;
        if !options.allow_network {
            continue;
        }
        let request_bytes =
            serde_json::to_vec(&crate::indexed::build_indexed_chunk_request(&c.source))?.len()
                as u64;
        let lines = c.source.text.lines().count() as u64;
        let cost = |input_tokens, output_tokens| {
            crate::accounting::cost_for_usage(
                &run.model,
                &Usage {
                    input_tokens,
                    output_tokens,
                    ..Usage::default()
                },
            )
        };
        let low = cost(request_bytes / 5, (lines * 5).max(16));
        let high = cost(request_bytes * 2 / 3, (lines * 80).clamp(64, 32000));
        result.extraction.add(low.zip(high));
        result.update_total();
        result.reservation_usd = result
            .reservation_usd
            .zip(reservation(&run.model, &c.source).ok())
            .map(|(a, b)| a + b);
    }
    Ok(result)
}

fn recovery_plan(
    run: &ReviewRun,
    options: &RunOptions,
    initial: &crate::recovery::State,
) -> Result<Preflight, EpubError> {
    // The shared scheduler builds these actions from prepared source and exact
    // candidate revisions on a clone. No cache values are applied, no request
    // is reserved, and no candidate output is fabricated during planning.
    // Planning must use the identical document-bound verifier request as
    // execution, but it must never mutate the saved run/checkpoint. The shared
    // helper keeps one planning clone instead of cloning document-heavy state
    // once for binding and again for action enumeration.
    let (prepared, actions) = initial
        .planned_actions_with_source_evidence(
            &run.documents,
            (!run.source_line_provenance.is_empty())
                .then_some(run.source_line_provenance.as_slice()),
        )
        .map_err(error)?;
    let state = &prepared;
    let directory = options
        .cache_dir
        .clone()
        .unwrap_or_else(crate::cache::default_dir)
        .join("verified-recovery-v2");
    let total = state
        .groups
        .iter()
        .filter(|g| g.enabled)
        .map(|g| g.chunks.len())
        .sum();
    let cached = state
        .groups
        .iter()
        .filter(|g| g.enabled && g.accepted.is_some())
        .map(|g| g.chunks.len())
        .sum();
    let mut result = Preflight {
        recovery: Some(RecoveryPreflight {
            models: state.models.clone(),
            extraction_remaining_usd: (state.budget_usd * 0.8 - state.allocated(false)).max(0.0),
            verification_remaining_usd: (state.budget_usd * 0.2 - state.allocated(true)).max(0.0),
            verification_pending: 0,
            verification_cached: 0,
        }),
        total,
        cached,
        pending: 0,
        estimated_low_usd: Some(0.0),
        estimated_high_usd: Some(0.0),
        reservation_usd: Some(0.0),
        basis: "Approximate extraction and verification ranges; the spending limit reserves 80% for extraction/recovery and 20% for verification. Token-based estimates, not a billing statement.",
        extraction: CostEstimate::new(
            "Request-size estimate; no compatible completed extraction observations were available.",
            false,
        ),
        verification: CostEstimate::new(
            "Request-size estimate; no compatible completed verification observations were available.",
            false,
        ),
    };
    let mut candidate_pending = false;
    for action in actions {
        if action.chunk.is_some() {
            // Every action here is an exact future extraction request. The
            // shared builder has already expanded all missing source slots;
            // there is no need to multiply a first request or fabricate a
            // candidate output to discover later keys.
            candidate_pending = true;
        }
        let cached = !options.refresh
            && super::recovery_cache::read(&directory, &action)
                .is_some_and(|value| cache_can_complete_action(state, &action, &value));
        if cached {
            if action.chunk.is_some() {
                result.cached += 1;
            } else if let Some(recovery) = &mut result.recovery {
                recovery.verification_cached += 1;
            }
            continue;
        }
        if action.chunk.is_some() {
            result.pending += 1;
        } else if let Some(recovery) = &mut result.recovery {
            recovery.verification_pending += 1;
        }
        if options.allow_network {
            if !action.priced {
                result.reservation_usd = None;
            }
            let bytes = serde_json::to_vec(&action.request)?.len() as u64;
            let cost = crate::accounting::cost_for_usage(
                &action.model,
                &Usage {
                    input_tokens: bytes.div_ceil(3),
                    output_tokens: action.output_limit as u64,
                    ..Usage::default()
                },
            );
            let estimate = if action.chunk.is_some() {
                &mut result.extraction
            } else {
                &mut result.verification
            };
            let range = if let Some((range, samples)) = calibrated_cost(state, &action) {
                estimate.basis = format!(
                    "{} compatible completed {} sample{} normalized to this request size.",
                    samples,
                    if action.chunk.is_some() {
                        "extraction"
                    } else {
                        "verification"
                    },
                    if samples == 1 { "" } else { "s" }
                );
                Some(range)
            } else {
                estimate.uncalibrated = true;
                cost.map(|value| (value * 0.5, value * 2.0))
            };
            estimate.add(range);
            let reserved = action.reservation_usd;
            result.reservation_usd = result.reservation_usd.map(|a| a + reserved);
        }
    }
    if candidate_pending {
        result.verification.uncalibrated = true;
        result.verification.usd = if options.allow_network {
            conditional_verification_estimate(state)
        } else {
            Some((0.0, 0.0))
        };
        result.verification.basis =
            "Conditional on candidate extraction; request-size verifier estimate remains uncalibrated until an exact candidate revision is available.".into();
    }
    result.update_total();
    Ok(result)
}

fn conditional_verification_estimate(state: &crate::recovery::State) -> Option<(f64, f64)> {
    let candidate_model = state.models.first().map(String::as_str).unwrap_or_default();
    let verifier_model = if candidate_model.starts_with("gemini-") {
        "@cf/zai-org/glm-5.3"
    } else {
        "gemini-2.5-flash"
    };
    let Ok(output_limit) = crate::recovery::output_limit(verifier_model, true) else {
        return None;
    };
    let mut range = Some((0.0, 0.0));
    for group in state
        .groups
        .iter()
        .filter(|group| group.enabled && group.accepted.is_none())
    {
        for chunk in &group.chunks {
            let Ok(bytes) = serde_json::to_vec(&crate::indexed::build_indexed_chunk_request(
                &state.source[*chunk],
            )) else {
                return None;
            };
            let cost = crate::accounting::cost_for_usage(
                verifier_model,
                &Usage {
                    input_tokens: (bytes.len() as u64).div_ceil(3),
                    output_tokens: output_limit as u64,
                    ..Usage::default()
                },
            );
            range = sum_ranges(range, cost.map(|value| (value * 0.5, value * 2.0)));
        }
    }
    range
}

fn calibrated_cost(
    state: &crate::recovery::State,
    action: &crate::recovery::Action,
) -> Option<((f64, f64), usize)> {
    let operation = if action.chunk.is_some() {
        "extract"
    } else {
        "verify"
    };
    let request_bytes = serde_json::to_vec(&action.request).ok()?.len() as f64;
    let mut estimates = Vec::new();
    for attempt in &state.attempts {
        if attempt.inherited
            || attempt.pending
            || attempt.error.is_some()
            || attempt.telemetry.contract != crate::recovery::VERIFICATION_CONTRACT
            || attempt.telemetry.operation != operation
            || attempt.telemetry.model != action.model
            || attempt.telemetry.output_limit != action.output_limit
        {
            continue;
        }
        let Some(cost) = attempt
            .estimated_usd
            .filter(|value| value.is_finite() && *value >= 0.0)
        else {
            continue;
        };
        if attempt.usage.is_none() {
            continue;
        }
        let observed_bytes = attempt
            .telemetry
            .source_bytes
            .saturating_add(attempt.telemetry.context_bytes)
            .saturating_add(attempt.telemetry.candidate_bytes)
            .saturating_add(attempt.telemetry.schema_bytes) as f64;
        if observed_bytes > 0.0 {
            estimates.push(cost * request_bytes / observed_bytes);
        }
    }
    if estimates.is_empty() {
        return None;
    }
    estimates.sort_by(f64::total_cmp);
    Some(((*estimates.first()?, *estimates.last()?), estimates.len()))
}

/// Validate a recovery cache entry with the same source-indexed contracts used
/// by execution. A JSON object with the expected top-level arrays is not enough:
/// extraction must account for every source line and verification must account
/// for every target line exactly once with a valid role and finding references.
fn valid_cached_payload(
    state: &crate::recovery::State,
    action: &crate::recovery::Action,
    value: &serde_json::Value,
) -> bool {
    if action.chunk.is_some() {
        let Some(chunk) = action.chunk.and_then(|index| state.source.get(index)) else {
            return false;
        };
        let Ok(lowered) = crate::indexed::lower_indexed_payload(chunk, value.clone()) else {
            return false;
        };
        let Ok(recipes) = crate::parse_recipes_payload(lowered) else {
            return false;
        };
        crate::extractor::validate_chunk_recipes(chunk, &recipes).is_ok()
    } else {
        valid_verification_payload(state, action, value)
    }
}

fn clean_verification_payload(value: &serde_json::Value) -> bool {
    value
        .get("findings")
        .and_then(serde_json::Value::as_array)
        .is_some_and(Vec::is_empty)
}

fn has_ambiguous_classification(value: &serde_json::Value) -> bool {
    value
        .get("classifications")
        .and_then(serde_json::Value::as_array)
        .is_some_and(|entries| {
            entries.iter().any(|entry| {
                entry.get("kind").and_then(serde_json::Value::as_str) == Some("ambiguous")
            })
        })
}

/// A cached verifier reply receives preflight credit only when applying it can
/// complete this exact stage. Findings and ambiguity need recovery; a clean
/// stronger response also cannot erase prior deterministic source findings.
fn cache_can_complete_action(
    state: &crate::recovery::State,
    action: &crate::recovery::Action,
    value: &serde_json::Value,
) -> bool {
    valid_cached_payload(state, action, value)
        && (action.chunk.is_some()
            || (clean_verification_payload(value)
                && !has_ambiguous_classification(value)
                && state
                    .prior_deterministic_verification_findings(action)
                    .is_empty()))
}

fn valid_verification_payload(
    state: &crate::recovery::State,
    action: &crate::recovery::Action,
    value: &serde_json::Value,
) -> bool {
    let Some(target) = action.verification_chunk else {
        return false;
    };
    let Some(group) = state.groups.get(action.group) else {
        return false;
    };
    let Some(candidate) = group.candidates.get(action.candidate) else {
        return false;
    };
    if !group.chunks.contains(&target) {
        return false;
    }
    let Some(position) = group.chunks.iter().position(|index| *index == target) else {
        return false;
    };
    let line_count = state.source[target].text.lines().count();
    let Some(classifications) = value.get("classifications").and_then(|v| v.as_array()) else {
        return false;
    };
    let Some(findings) = value.get("findings").and_then(|v| v.as_array()) else {
        return false;
    };
    let mut covered = std::collections::HashSet::new();
    let empty = candidate.outputs.iter().flatten().all(Vec::is_empty);
    for classification in classifications {
        let Some(object) = classification.as_object() else {
            return false;
        };
        let allowed = ["chunk", "line", "lines", "kind"];
        if object.keys().any(|key| !allowed.contains(&key.as_str())) {
            return false;
        }
        if object.contains_key("lines") && object.contains_key("line") {
            return false;
        }
        let Some(chunk) = object.get("chunk").and_then(serde_json::Value::as_u64) else {
            return false;
        };
        let Some(kind) = object.get("kind").and_then(serde_json::Value::as_str) else {
            return false;
        };
        if chunk as usize != target
            || ![
                "title",
                "ingredient",
                "method",
                "metadata",
                "variation",
                "non_recipe",
                "ambiguous",
            ]
            .contains(&kind)
            || (empty && kind != "non_recipe")
        {
            return false;
        }
        let lines: Vec<usize> = if let Some(lines) = object.get("lines") {
            let Some(lines) = lines.as_array() else {
                return false;
            };
            if lines.is_empty() {
                return false;
            }
            let mut values = Vec::with_capacity(lines.len());
            for line in lines {
                let Some(line) = line.as_u64() else {
                    return false;
                };
                values.push(line as usize);
            }
            values
        } else if let Some(line) = object.get("line").and_then(serde_json::Value::as_u64) {
            vec![line as usize]
        } else {
            return false;
        };
        for line in lines {
            if line >= line_count || !covered.insert(line) {
                return false;
            }
            let Some(roles) = candidate.source_roles.get(position) else {
                return false;
            };
            if roles.get(line).is_none_or(|role| {
                !["title", "ingredient", "method", "metadata", "non_recipe"]
                    .contains(&role.as_str())
            }) {
                return false;
            }
        }
    }
    if covered.len() != line_count {
        return false;
    }
    for finding in findings {
        let Some(object) = finding.as_object() else {
            return false;
        };
        if object
            .keys()
            .any(|key| !["category", "message", "chunk", "lines"].contains(&key.as_str()))
        {
            return false;
        }
        let Some(category) = object.get("category").and_then(serde_json::Value::as_str) else {
            return false;
        };
        let Some(message) = object.get("message").and_then(serde_json::Value::as_str) else {
            return false;
        };
        let Some(chunk) = object.get("chunk").and_then(serde_json::Value::as_u64) else {
            return false;
        };
        let Some(lines) = object.get("lines").and_then(|v| v.as_array()) else {
            return false;
        };
        if !["processing", "coverage", "fidelity"].contains(&category)
            || message.trim().is_empty()
            || !group.chunks.contains(&(chunk as usize))
            || lines.is_empty()
            || lines.iter().any(|line| {
                line.as_u64().is_none_or(|line| {
                    state.source[chunk as usize].text.lines().count() <= line as usize
                })
            })
        {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]
    #[rstest::rstest]
    #[case(Some((1.0, 2.0)), Some((3.0, 5.0)), Some((4.0, 7.0)))]
    #[case(Some((0.0, 0.0)), Some((0.0, 0.0)), Some((0.0, 0.0)))]
    #[case(Some((1.0, 2.0)), None, None)]
    #[case(None, Some((1.0, 2.0)), None)]
    fn estimates_keep_unknown_ranges_atomic(
        #[case] extraction: Option<(f64, f64)>,
        #[case] verification: Option<(f64, f64)>,
        #[case] expected: Option<(f64, f64)>,
    ) {
        let mut estimate = super::CostEstimate::new("Test basis", false);
        estimate.add(extraction);
        estimate.add(verification);
        assert_eq!(estimate.usd, expected);
        let wire = serde_json::to_value(&estimate).unwrap();
        assert_eq!(wire["usd"], serde_json::json!(expected));
        assert_eq!(
            serde_json::from_value::<super::CostEstimate>(wire).unwrap(),
            estimate
        );
    }

    use super::*;
    use crate::{Chunk, recovery::State, review::ReviewRun};
    use serde_json::json;

    fn state() -> State {
        State::new(
            vec![Chunk {
                text: "Copyright".into(),
                doc_path: "source.xhtml".into(),
                title_hint: None,
                links: vec![],
                images: vec![],
            }],
            "gemini-2.5-flash",
            10.0,
        )
        .expect("test state should be valid")
    }

    #[test]
    fn extraction_cache_requires_complete_indexed_source_coverage() {
        let state = state();
        let action = state
            .ready_actions()
            .expect("test state should produce a planned action")
            .pop()
            .expect("test state should have one extraction action");
        assert!(valid_cached_payload(
            &state,
            &action,
            &json!({"recipes": [], "ignored": [0]})
        ));
        assert!(!valid_cached_payload(
            &state,
            &action,
            &json!({"recipes": [], "ignored": []})
        ));
    }

    #[test]
    fn verification_cache_requires_each_target_line_once() {
        let mut state = state();
        let extraction = state
            .next_action()
            .expect("test state should produce an extraction action")
            .expect("test state should have an extraction action");
        state
            .apply(&extraction, json!({"recipes": [], "ignored": [0]}))
            .expect("test extraction response should apply");
        let verification = state
            .ready_actions()
            .expect("test state should produce a verification action")
            .pop()
            .expect("test state should have one verification action");
        assert!(valid_cached_payload(
            &state,
            &verification,
            &json!({
                "classifications": [{"chunk": 0, "lines": [0], "kind": "non_recipe"}],
                "findings": []
            })
        ));
        let clean = json!({
            "classifications": [{"chunk": 0, "lines": [0], "kind": "non_recipe"}],
            "findings": []
        });
        assert!(cache_can_complete_action(&state, &verification, &clean));
        let ambiguous = json!({
            "classifications": [{"chunk": 0, "lines": [0], "kind": "ambiguous"}],
            "findings": []
        });
        assert!(has_ambiguous_classification(&ambiguous));
        assert!(!cache_can_complete_action(
            &state,
            &verification,
            &ambiguous
        ));
        let mut stronger = verification.clone();
        stronger.verification_stage = Some(1);
        state.groups[0].candidates[0].verification_stages.push(
            crate::recovery::VerificationStage {
                target: 0,
                stage: 0,
                model: verification.model.clone(),
                status: "failed".into(),
                attempts: vec![],
                evidence: vec![],
                findings: vec![crate::recovery::Finding {
                    category: "fidelity".into(),
                    message: "missing ingredient".into(),
                    chunk: 0,
                    lines: vec![0],
                    resolved: false,
                    model: verification.model.clone(),
                }],
            },
        );
        assert!(!cache_can_complete_action(&state, &stronger, &clean));
        let with_finding = json!({
            "classifications": [{"chunk": 0, "lines": [0], "kind": "non_recipe"}],
            "findings": [{"category":"coverage","message":"needs recovery","chunk":0,"lines":[0]}]
        });
        assert!(valid_cached_payload(&state, &verification, &with_finding));
        assert!(!clean_verification_payload(&with_finding));
        assert!(!valid_cached_payload(
            &state,
            &verification,
            &json!({"classifications": [], "findings": []})
        ));
        assert!(!valid_cached_payload(
            &state,
            &verification,
            &json!({
                "classifications": [
                    {"chunk": 0, "lines": [0], "kind": "non_recipe"},
                    {"chunk": 0, "lines": [0], "kind": "non_recipe"}
                ],
                "findings": []
            })
        ));
    }

    #[test]
    fn calibration_uses_only_matching_completed_current_contract_samples() {
        let mut state = state();
        let action = state
            .planned_actions()
            .expect("test state should produce a calibration action")
            .pop()
            .expect("test state should have one calibration action");
        let attempt = state
            .reserve(&action)
            .expect("test calibration action should reserve");
        state.settle(
            attempt,
            Some(crate::Usage {
                input_tokens: 10,
                output_tokens: 20,
                ..crate::Usage::default()
            }),
            Some(json!({"recipes": [], "ignored": [0]})),
            None,
        );
        assert_eq!(
            calibrated_cost(&state, &action).map(|(_, count)| count),
            Some(1)
        );
        state.attempts[attempt].telemetry.contract = "legacy".into();
        assert!(calibrated_cost(&state, &action).is_none());
    }

    #[test]
    fn recovery_preflight_binds_current_documents_for_the_same_verifier_identity_as_execution() {
        let mut state = state();
        let extraction = state.next_action().unwrap().unwrap();
        state
            .apply(&extraction, json!({"recipes": [], "ignored": [0]}))
            .unwrap();
        let documents = vec![crate::source::SourceDocument {
            path: "source.xhtml".into(),
            anchors: vec![],
            images: vec![],
            blocks: vec![crate::source::SourceBlock {
                id: "source".into(),
                element_index: 0,
                anchor: None,
                tag: "p".into(),
                classes: String::new(),
                text: "Copyright".into(),
                links: vec![],
            }],
        }];
        let provenance = vec![vec![crate::SourceLine {
            anchors: Vec::new(),
            document_line: 0,
            contributors: vec![crate::SourceElement {
                element_index: 0,
                tag: "p".into(),
                classes: String::new(),
                anchor: None,
                ancestors: vec![],
            }],
            links: vec![],
            images: vec![],
            transformed: false,
        }]];
        let mut execution = state.clone();
        assert!(execution.bind_documents(&documents).unwrap());
        assert!(execution.bind_source_line_provenance(&provenance).unwrap());
        let execution_action = execution.planned_actions().unwrap().pop().unwrap();
        let unbound_action = state.planned_actions().unwrap().pop().unwrap();
        assert_ne!(execution_action.key, unbound_action.key);

        let base = std::env::temp_dir().join(format!(
            "document-preflight-{}",
            crate::review::store::new_id()
        ));
        let directory = base.join("verified-recovery-v2");
        std::fs::create_dir_all(&directory).unwrap();
        super::super::recovery_cache::write(
            &directory,
            &execution_action,
            &json!({"classifications":[{"chunk":0,"lines":[0],"kind":"non_recipe"}],"findings":[]}),
        )
        .unwrap();
        let run = ReviewRun {
            recovery: Some(state.clone()),
            execution_status: None,
            metadata: None,
            charges: vec![],
            version: 1,
            epub_sha256: "test".into(),
            source: "test".into(),
            model: "gemini-2.5-flash".into(),
            prompt_version: crate::cache::PROMPT_VERSION.into(),
            parent: None,
            chunks: vec![],
            documents,
            navigation_documents: std::collections::BTreeSet::new(),
            hybrid_audits: vec![],
            source_line_provenance: provenance,
            source_line_provenance_sha256: Some(
                crate::recovery::source_line_provenance_sha256(
                    &state.source,
                    &execution.source_line_provenance,
                )
                .unwrap(),
            ),
            recipes: vec![],
            parsed: json!([]),
            reserved_usd: 0.0,
            image_text: None,
        };
        let options = crate::review::RunOptions {
            cache_dir: Some(base.clone()),
            budget_usd: 10.0,
            ..Default::default()
        };
        let planned = recovery_plan(&run, &options, &state).unwrap();
        assert_eq!(planned.recovery.unwrap().verification_cached, 1);
        let _ = std::fs::remove_dir_all(base);
    }
}
