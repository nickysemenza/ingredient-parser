//! Audit-only experiment manifests and ledger-backed child execution.
//!
//! This keeps the ordinary saved-run audit lifecycle authoritative. The
//! experiment layer freezes parent identity and supplies only durable external
//! accounting around that lifecycle.

use super::{
    ExperimentManifest, ExperimentRun, Ledger, Result, durable_json, hash, ledger_status, rejected,
};
use crate::{
    Usage,
    recovery::{Action, State},
    review::{
        AuditAccounting, AuditAccountingHandle, AuditRequest, ExtractionControl, ReviewRun,
        RunOptions, audit_to_run_scoped_controlled,
    },
};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Instant,
};

const AUDIT_EXPERIMENT_KIND: &str = "audit-experiment-prepare-v1";
const AUDIT_OPERATION: &str = "audit_child";
const AUDIT_BUCKET: &str = "verifier";

pub(super) fn is_audit_manifest(manifest: &Value) -> bool {
    manifest["kind"] == AUDIT_EXPERIMENT_KIND && manifest["operation"] == AUDIT_OPERATION
}

#[derive(Debug, Clone)]
pub struct AuditExperimentPrepareRequest {
    pub parent_run: PathBuf,
    pub expectations: PathBuf,
    pub groups: Vec<usize>,
    pub reviewer_model: String,
    pub correct: bool,
    /// An explicit user-run ceiling. Ledger admission remains authoritative.
    pub budget_usd: f64,
    pub output_dir: PathBuf,
}

#[derive(Debug, Clone)]
pub struct AuditExperimentRunRequest {
    pub manifest: PathBuf,
    pub parent_run: PathBuf,
    pub child_run: PathBuf,
    pub output_dir: PathBuf,
    pub ledger: PathBuf,
    pub dispatch: bool,
}

pub fn prepare_audit_experiment(
    request: AuditExperimentPrepareRequest,
) -> Result<ExperimentManifest> {
    if !request.budget_usd.is_finite() || request.budget_usd < 0.0 {
        return Err(rejected("audit budget must be finite and non-negative"));
    }
    let parent_bytes = fs::read(&request.parent_run)?;
    let parent: ReviewRun = serde_json::from_slice(&parent_bytes)?;
    let state = parent
        .recovery
        .as_ref()
        .ok_or_else(|| rejected("audit experiment parent has no recovery state"))?;
    state.validate().map_err(rejected)?;
    if serde_json::to_value(&state.source)?
        != serde_json::to_value(
            parent
                .chunks
                .iter()
                .map(|chunk| &chunk.source)
                .collect::<Vec<_>>(),
        )?
    {
        return Err(rejected(
            "audit experiment recovery source differs from saved run chunks",
        ));
    }
    if state.attempts.iter().any(|attempt| attempt.pending) {
        return Err(rejected(
            "audit experiment parent has pending attempts with unknown completion or billing",
        ));
    }
    if state.strategy != crate::hybrid::HybridStrategy::Hybrid {
        return Err(rejected(
            "audit experiment requires a hybrid parent candidate",
        ));
    }
    let groups = selected_groups(state, &request.groups)?;
    let expectations = fs::read(&request.expectations)?;
    let expectations_value: Value = serde_json::from_slice(&expectations)?;
    let source_sha256 = hash(&serde_json::to_vec(&state.source)?);
    validate_expectations(&expectations_value, &parent, &source_sha256)?;

    super::require_new_directory(&request.output_dir)?;
    let prepared = prepared_audit_child(
        &request.parent_run,
        request.output_dir.join("prepared-child.json"),
        &request.reviewer_model,
        request.correct,
        request.budget_usd,
        &groups,
    )?;
    let planned = prepared
        .recovery
        .as_ref()
        .ok_or_else(|| rejected("prepared audit child has no recovery state"))?;
    let actions = planned.planned_actions().map_err(rejected)?;
    if actions.is_empty() || actions.iter().any(|action| action.chunk.is_some()) {
        return Err(rejected(
            "audit experiment did not produce audit-only actions",
        ));
    }
    // Hash the exact durable representation, rather than a parsed equivalent.
    // `durable_json` uses pretty JSON followed by a newline.
    let frozen_expectations = durable_bytes(&expectations_value)?;
    durable_json(
        &request.output_dir.join("expectations.json"),
        &expectations_value,
    )?;
    durable_json(
        &request.output_dir.join("parent.json"),
        &serde_json::to_value(&parent)?,
    )?;
    durable_json(
        &request.output_dir.join("source.json"),
        &serde_json::to_value(&state.source)?,
    )?;
    let requests = actions
        .iter()
        .enumerate()
        .map(|(index, action)| {
            let filename = format!("audit-request-{index}.json");
            let value = serde_json::to_value(&action.request)?;
            let bytes = durable_bytes(&value)?;
            durable_json(&request.output_dir.join(&filename), &value)?;
            Ok(json!({
                "file": filename,
                "sha256": hash(&bytes),
                "action_key": action.key,
                "group": action.group,
                "candidate": action.candidate,
            }))
        })
        .collect::<Result<Vec<_>>>()?;
    let frozen_groups = groups
        .iter()
        .map(|index| {
            let group = &planned.groups[*index];
            let candidate_index = group.candidates.len() - 1;
            let candidate = &group.candidates[candidate_index];
            Ok(json!({
                "group": index,
                "candidate": candidate_index,
                "chunks": group.chunks,
                "revision": candidate.revision,
                "outputs_sha256": hash(&serde_json::to_vec(&candidate.outputs)?),
                "assignments_sha256": hash(&serde_json::to_vec(&candidate.hybrid_assignments)?),
                "overrides_sha256": hash(&serde_json::to_vec(&candidate.hybrid_text_overrides)?),
                "assignment_issues_sha256": hash(&serde_json::to_vec(&candidate.hybrid_assignment_issues)?),
            }))
        })
        .collect::<Result<Vec<_>>>()?;
    let manifest = json!({
        "kind": AUDIT_EXPERIMENT_KIND,
        "operation": AUDIT_OPERATION,
        "parent_run_sha256": hash(&parent_bytes),
        "parent_epub_sha256": parent.epub_sha256,
        "source_sha256": source_sha256,
        "documents_sha256": hash(&serde_json::to_vec(&parent.documents)?),
        "navigation_documents_sha256": hash(&serde_json::to_vec(&parent.navigation_documents)?),
        "source_line_provenance_sha256": state.source_line_provenance_sha256,
        "expectations_file": "expectations.json",
        "expectations_sha256": hash(&frozen_expectations),
        "parent_file": "parent.json",
        "source_file": "source.json",
        "reviewer_model": request.reviewer_model,
        "correct": request.correct,
        "budget_usd": request.budget_usd,
        "concurrency": super::DEFAULT_CONCURRENCY,
        "groups": frozen_groups,
        "requests": requests,
        "reservation_envelopes": actions.iter().map(|action| reservation_envelope(action, request.correct)).collect::<Result<Vec<_>>>()?,
        "scope": "Frozen hybrid audit child. Initial audit, one context expansion, and one correction re-audit remain bounded by the shared retry limit; this manifest makes no provider call."
    });
    let path = request.output_dir.join("manifest.json");
    durable_json(&path, &manifest)?;
    Ok(ExperimentManifest {
        path,
        value: manifest,
    })
}

pub async fn run_audit_experiment(request: AuditExperimentRunRequest) -> Result<ExperimentRun> {
    let manifest_bytes = fs::read(&request.manifest)?;
    let manifest: Value = serde_json::from_slice(&manifest_bytes)?;
    if !is_audit_manifest(&manifest) {
        return Err(rejected("manifest is not an audit experiment"));
    }
    let parent_bytes = fs::read(&request.parent_run)?;
    validate_parent(&manifest, &parent_bytes)?;
    let frozen_expectations = fs::read(
        request
            .manifest
            .parent()
            .ok_or_else(|| rejected("audit manifest has no parent"))?
            .join(
                manifest["expectations_file"]
                    .as_str()
                    .ok_or_else(|| rejected("audit manifest has no expectations file"))?,
            ),
    )?;
    if manifest["expectations_sha256"] != hash(&frozen_expectations) {
        return Err(rejected("frozen audit expectations differ from manifest"));
    }
    super::require_new_directory(&request.output_dir)?;
    let groups = manifest_groups(&manifest)?;
    let budget = manifest["budget_usd"]
        .as_f64()
        .filter(|value| value.is_finite() && *value >= 0.0)
        .ok_or_else(|| rejected("manifest has no finite frozen audit budget"))?;
    let reviewer_model = manifest["reviewer_model"]
        .as_str()
        .ok_or_else(|| rejected("manifest has no reviewer model"))?;
    let correct = manifest["correct"]
        .as_bool()
        .ok_or_else(|| rejected("manifest has no correction mode"))?;
    let prepared = prepared_audit_child(
        &request.parent_run,
        request.child_run.clone(),
        reviewer_model,
        correct,
        budget,
        &groups,
    )?;
    validate_frozen_prepared(&manifest, &request.manifest, &prepared)?;
    let preparation = json!({
        "kind": "audit-experiment-execution-v1",
        "operation": AUDIT_OPERATION,
        "manifest_sha256": hash(&manifest_bytes),
        "parent_run_sha256": hash(&parent_bytes),
        "dispatch_requested": request.dispatch,
        "groups": groups,
        "child_run": request.child_run,
        "actions": manifest["reservation_envelopes"],
    });
    durable_json(&request.output_dir.join("preparation.json"), &preparation)?;
    if !request.dispatch {
        return Ok(ExperimentRun {
            preparation,
            result: None,
        });
    }

    let trial_id = hash(request.output_dir.to_string_lossy().as_bytes());
    let reservation_caps = manifest["reservation_envelopes"]
        .as_array()
        .ok_or_else(|| rejected("manifest has no audit reservation envelopes"))?
        .iter()
        .map(|entry| {
            let group = entry["group"]
                .as_u64()
                .and_then(|value| usize::try_from(value).ok())
                .ok_or_else(|| rejected("audit reservation group is invalid"))?;
            let cap = entry["maximum_reservation_usd"]
                .as_f64()
                .filter(|value| value.is_finite() && *value >= 0.0)
                .ok_or_else(|| rejected("audit reservation bound is invalid"))?;
            Ok((group, cap))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    let accounting: AuditAccountingHandle = Arc::new(Mutex::new(LedgerAuditAccounting::new(
        Ledger::open(&request.ledger),
        trial_id,
        request.output_dir.join("audit-ledger-map.json"),
        reservation_caps,
    )));
    let audit = AuditRequest {
        run: request.parent_run.clone(),
        out: request.child_run.clone(),
        model: reviewer_model.into(),
        correct,
        options: RunOptions {
            allow_network: true,
            budget_usd: budget,
            concurrency: super::DEFAULT_CONCURRENCY,
            strategy: crate::hybrid::HybridStrategy::Hybrid,
            ..Default::default()
        },
    };
    let started = Instant::now();
    let control = ExtractionControl::default();
    let outcome =
        audit_to_run_scoped_controlled(audit, &control, Some(&groups), Some(accounting), |_| {})
            .await
            .map_err(|error| rejected(error.to_string()))?;
    let result = json!({
        "operation": AUDIT_OPERATION,
        "wall_ms": started.elapsed().as_millis(),
        "child_run": outcome.path,
        "status": outcome.run.status(),
        "incomplete": outcome.run.incomplete(),
    });
    durable_json(&request.output_dir.join("result.json"), &result)?;
    Ok(ExperimentRun {
        preparation,
        result: Some(result),
    })
}

fn selected_groups(state: &State, requested: &[usize]) -> Result<Vec<usize>> {
    let groups = if requested.is_empty() {
        state
            .groups
            .iter()
            .enumerate()
            .filter_map(|(index, group)| group.enabled.then_some(index))
            .collect::<Vec<_>>()
    } else {
        requested.to_vec()
    };
    if groups.is_empty()
        || groups.windows(2).any(|pair| pair[0] >= pair[1])
        || groups.iter().any(|index| *index >= state.groups.len())
    {
        return Err(rejected(
            "audit groups must be unique, ordered saved group indexes",
        ));
    }
    for index in &groups {
        let group = &state.groups[*index];
        let candidate = group
            .candidates
            .last()
            .ok_or_else(|| rejected("audit group has no frozen candidate"))?;
        if !candidate.outputs.iter().all(Option::is_some) {
            return Err(rejected("audit group has no complete frozen candidate"));
        }
    }
    Ok(groups)
}

fn prepared_audit_child(
    parent_run: &Path,
    child_run: PathBuf,
    reviewer_model: &str,
    correct: bool,
    budget_usd: f64,
    groups: &[usize],
) -> Result<ReviewRun> {
    crate::review::audit::prepare_audit_scoped_for_experiment(
        &AuditRequest {
            run: parent_run.into(),
            out: child_run,
            model: reviewer_model.into(),
            correct,
            options: RunOptions {
                allow_network: true,
                budget_usd,
                concurrency: super::DEFAULT_CONCURRENCY,
                strategy: crate::hybrid::HybridStrategy::Hybrid,
                ..Default::default()
            },
        },
        Some(groups),
    )
    .map_err(|error| rejected(error.to_string()))
}

fn validate_expectations(
    expectations: &Value,
    parent: &ReviewRun,
    source_sha256: &str,
) -> Result<()> {
    let binding = expectations
        .get("epub_sha256")
        .or_else(|| expectations.get("source_sha256"))
        .or_else(|| expectations.pointer("/provenance/epub_sha256"))
        .or_else(|| expectations.pointer("/provenance/selected_source_sha256"))
        .and_then(Value::as_str)
        .ok_or_else(|| rejected("expectations must bind epub_sha256 or source_sha256"))?;
    if binding != parent.epub_sha256 && binding != source_sha256 {
        return Err(rejected("expectations do not bind the audit parent source"));
    }
    Ok(())
}

/// The exact bytes written by `durable_json` for a JSON value.
///
/// Frozen evidence is identity-bearing, so its hash must bind the file on disk
/// rather than merely an equivalent parsed value.
fn durable_bytes(value: &Value) -> Result<Vec<u8>> {
    let mut bytes = serde_json::to_vec_pretty(value)?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn reservation_envelope(action: &Action, correct: bool) -> Result<Value> {
    let passes = if correct { 2u64 } else { 1 };
    let attempts = crate::recovery::MAX_ATTEMPTS as u64;
    let logical_calls = passes + 1; // one optional context expansion
    Ok(json!({
        "group": action.group,
        "candidate": action.candidate,
        "action_key": action.key,
        "request_sha256": hash(&durable_bytes(&serde_json::to_value(&action.request)?)?),
        "model": action.model,
        "reservation_usd": action.reservation_usd,
        "max_attempts_per_call": attempts,
        "max_audit_passes": passes,
        "max_context_expansions": 1,
        "maximum_calls": attempts * logical_calls,
        "maximum_reservation_usd": action.reservation_usd * (attempts * logical_calls) as f64,
    }))
}

fn manifest_groups(manifest: &Value) -> Result<Vec<usize>> {
    let groups = manifest["groups"]
        .as_array()
        .ok_or_else(|| rejected("audit manifest has no groups"))?
        .iter()
        .map(|entry| {
            entry["group"]
                .as_u64()
                .and_then(|value| usize::try_from(value).ok())
                .ok_or_else(|| rejected("audit manifest group is invalid"))
        })
        .collect::<Result<Vec<_>>>()?;
    if groups.is_empty() || groups.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err(rejected("audit manifest groups are invalid"));
    }
    Ok(groups)
}

fn validate_parent(manifest: &Value, bytes: &[u8]) -> Result<()> {
    if manifest["parent_run_sha256"] != hash(bytes) {
        return Err(rejected("audit parent differs from frozen manifest"));
    }
    let parent: ReviewRun = serde_json::from_slice(bytes)?;
    let state = parent
        .recovery
        .as_ref()
        .ok_or_else(|| rejected("audit parent has no recovery state"))?;
    state.validate().map_err(rejected)?;
    if parent.epub_sha256 != manifest["parent_epub_sha256"] {
        return Err(rejected(
            "audit parent EPUB identity differs from frozen manifest",
        ));
    }
    if hash(&serde_json::to_vec(&state.source)?) != manifest["source_sha256"] {
        return Err(rejected("audit parent source differs from frozen manifest"));
    }
    if hash(&serde_json::to_vec(&parent.documents)?) != manifest["documents_sha256"]
        || hash(&serde_json::to_vec(&parent.navigation_documents)?)
            != manifest["navigation_documents_sha256"]
        || state.source_line_provenance_sha256.as_deref()
            != manifest["source_line_provenance_sha256"].as_str()
    {
        return Err(rejected(
            "audit parent source evidence differs from frozen manifest",
        ));
    }
    Ok(())
}

fn validate_frozen_prepared(
    manifest: &Value,
    manifest_path: &Path,
    prepared: &ReviewRun,
) -> Result<()> {
    let state = prepared
        .recovery
        .as_ref()
        .ok_or_else(|| rejected("prepared audit child has no recovery state"))?;
    for entry in manifest["groups"]
        .as_array()
        .ok_or_else(|| rejected("audit manifest has no groups"))?
    {
        let group = entry["group"]
            .as_u64()
            .and_then(|value| usize::try_from(value).ok())
            .and_then(|index| state.groups.get(index))
            .ok_or_else(|| rejected("frozen audit group is unavailable"))?;
        let candidate = entry["candidate"]
            .as_u64()
            .and_then(|value| usize::try_from(value).ok())
            .and_then(|index| group.candidates.get(index))
            .ok_or_else(|| rejected("frozen audit candidate is unavailable"))?;
        if candidate.revision != entry["revision"].as_u64().unwrap_or_default()
            || hash(&serde_json::to_vec(&candidate.outputs)?) != entry["outputs_sha256"]
            || hash(&serde_json::to_vec(&candidate.hybrid_assignments)?)
                != entry["assignments_sha256"]
            || hash(&serde_json::to_vec(&candidate.hybrid_text_overrides)?)
                != entry["overrides_sha256"]
            || hash(&serde_json::to_vec(&candidate.hybrid_assignment_issues)?)
                != entry["assignment_issues_sha256"]
        {
            return Err(rejected(
                "prepared audit candidate differs from frozen manifest",
            ));
        }
    }
    let actions = state.planned_actions().map_err(rejected)?;
    let requests = manifest["requests"]
        .as_array()
        .ok_or_else(|| rejected("audit manifest has no frozen requests"))?;
    let envelopes = manifest["reservation_envelopes"]
        .as_array()
        .ok_or_else(|| rejected("audit manifest has no reservation envelopes"))?;
    if actions.len() != requests.len() || actions.len() != envelopes.len() {
        return Err(rejected("frozen audit request count differs"));
    }
    let directory = manifest_path
        .parent()
        .ok_or_else(|| rejected("audit manifest has no parent"))?;
    for ((action, request), envelope) in actions.iter().zip(requests).zip(envelopes) {
        let filename = request["file"]
            .as_str()
            .ok_or_else(|| rejected("frozen audit request has no file"))?;
        let bytes = fs::read(directory.join(filename))?;
        let frozen: Value = serde_json::from_slice(&bytes)?;
        let expected = serde_json::to_value(&action.request)?;
        let expected_bytes = durable_bytes(&expected)?;
        if request["action_key"] != action.key
            || request["group"] != action.group
            || request["candidate"] != action.candidate
            || request["sha256"] != hash(&bytes)
            || frozen != expected
            || envelope["action_key"] != action.key
            || envelope["request_sha256"] != hash(&expected_bytes)
            || envelope["model"] != action.model
            || envelope["reservation_usd"].as_f64() != Some(action.reservation_usd)
        {
            return Err(rejected("audit request differs from frozen manifest"));
        }
    }
    Ok(())
}

pub(super) fn report_audit_experiment(
    manifest_path: &Path,
    manifest_bytes: &[u8],
    manifest: &Value,
    evidence_dir: &Path,
    ledger_path: &Path,
) -> Result<Value> {
    let preparation: Value =
        serde_json::from_slice(&fs::read(evidence_dir.join("preparation.json"))?)?;
    if preparation["manifest_sha256"] != hash(manifest_bytes) {
        return Err(rejected(
            "audit evidence preparation belongs to a different manifest",
        ));
    }
    let status = ledger_status(ledger_path)?;
    let result = evidence_dir
        .join("result.json")
        .exists()
        .then(|| fs::read(evidence_dir.join("result.json")))
        .transpose()?
        .map(|bytes| serde_json::from_slice::<Value>(&bytes))
        .transpose()?;
    let child_path = result
        .as_ref()
        .and_then(|value| value["child_run"].as_str())
        .or_else(|| preparation["child_run"].as_str())
        .map(PathBuf::from);
    let Some(child_path) = child_path else {
        return Ok(json!({
            "kind": "audit-experiment-report-v1",
            "manifest_sha256": hash(manifest_bytes),
            "ledger": status,
            "complete": false,
            "fully_audited_duration_ms": Value::Null,
            "reason": "audit child has not been created",
        }));
    };
    let child = match ReviewRun::read(&child_path) {
        Ok(run) => run,
        Err(_) => {
            return Ok(json!({
                "kind": "audit-experiment-report-v1",
                "manifest_sha256": hash(manifest_bytes),
                "ledger": status,
                "complete": false,
                "fully_audited_duration_ms": Value::Null,
                "reason": "audit child evidence is unavailable",
            }));
        }
    };
    validate_child_identity(manifest, &child)?;
    let state = child.recovery.as_ref();
    let mut usage = Usage::default();
    let mut known_usd = 0.0;
    if let Some(state) = state {
        for attempt in state.attempts.iter().filter(|attempt| !attempt.inherited) {
            if let Some(value) = &attempt.usage {
                usage.input_tokens += value.input_tokens;
                usage.output_tokens += value.output_tokens;
                usage.cache_read_input_tokens += value.cache_read_input_tokens;
                usage.cache_creation_input_tokens += value.cache_creation_input_tokens;
            }
            known_usd += attempt.estimated_usd.unwrap_or(0.0);
        }
    }
    let mapping: Value = fs::read(evidence_dir.join("audit-ledger-map.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_else(|| json!({"entries":{}}));
    let held_usd = mapping["entries"]
        .as_object()
        .into_iter()
        .flat_map(|entries| entries.values())
        .filter_map(Value::as_str)
        .filter_map(|id| status.entries.iter().find(|entry| entry.id == id))
        .filter(|entry| matches!(entry.status.as_str(), "reserved" | "unknown"))
        .map(|entry| entry.reserved_usd)
        .sum::<f64>();
    let groups = audit_group_report(manifest, state)?;
    let complete = !groups.is_empty() && groups.iter().all(|group| group["accepted"] == true);
    let quality = evaluate_frozen_audit_quality(
        manifest,
        manifest_path.parent().unwrap_or_else(|| Path::new(".")),
        &child,
        complete,
    )?;
    let fully_audited_duration_ms = if complete {
        result
            .as_ref()
            .and_then(|value| value["wall_ms"].as_u64())
            .map(Value::from)
            .unwrap_or(Value::Null)
    } else {
        Value::Null
    };
    Ok(json!({
        "kind": "audit-experiment-report-v1",
        "manifest_sha256": hash(manifest_bytes),
        "manifest": manifest_path.display().to_string(),
        "ledger": status,
        "result": result,
        "child_run": child_path,
        "complete": complete,
        "usage": usage,
        "known_usd": known_usd,
        "held_usd": held_usd,
        "groups": groups,
        "quality": quality,
        "quality_accepted": quality["accepted"].as_bool().unwrap_or(false),
        "fully_audited_duration_ms": fully_audited_duration_ms,
        "note": if complete { "All frozen groups have accepted audit evidence. Source quality is reported when the frozen expectation document uses a supported generic format." } else { "One or more frozen groups remain incomplete, have findings, or have no child evidence." },
    }))
}

/// Scores only the two generic expectation documents that the shared review
/// evaluator already owns. Historical private formats intentionally remain
/// unscored here: accepting their familiar identity fields would silently turn
/// a distinct ownership/placement contract into a generic recipe comparison.
fn evaluate_frozen_audit_quality(
    manifest: &Value,
    manifest_directory: &Path,
    child: &ReviewRun,
    complete: bool,
) -> Result<Value> {
    let Some(file) = manifest["expectations_file"].as_str() else {
        return Ok(
            json!({"status":"unscored","reason":"audit manifest has no frozen expectations copy","accepted":false}),
        );
    };
    let bytes = match fs::read(manifest_directory.join(file)) {
        Ok(bytes) => bytes,
        Err(_) => {
            return Ok(
                json!({"status":"unscored","reason":"frozen expectations copy is unavailable","accepted":false}),
            );
        }
    };
    if manifest["expectations_sha256"] != hash(&bytes) {
        return Ok(
            json!({"status":"unscored","reason":"frozen expectations copy hash differs","accepted":false}),
        );
    }
    let expectations: Value = match serde_json::from_slice(&bytes) {
        Ok(value) => value,
        Err(_) => {
            return Ok(
                json!({"status":"unscored","reason":"frozen expectations copy is not valid JSON","accepted":false}),
            );
        }
    };
    if expectations["kind"] == super::frozen::KIND {
        let state = child.recovery.as_ref().ok_or_else(|| {
            rejected("frozen source-role evaluation requires audit child recovery state")
        })?;
        return super::frozen::evaluate(state, &child.epub_sha256, &expectations, complete);
    }
    if let Err(reason) = validate_generic_expectations(&expectations) {
        return Ok(json!({"status":"unscored","reason":reason,"accepted":false}));
    }

    // The terminal child is the authoritative assembled output. Replay its
    // stored slots so exact ingredient expectations see the same continuation
    // barriers and parsed representation as ordinary saved-run evaluation.
    let mut replayed = child.clone();
    if let Err(error) = replayed.replay() {
        return Ok(
            json!({"status":"unscored","reason":format!("offline replay of audit child failed: {error}"),"accepted":false}),
        );
    }
    match crate::review::evaluate_document(&replayed, expectations) {
        Ok(evaluation) => {
            let accepted = complete && !crate::review::evaluation_failed(&evaluation);
            Ok(json!({"status":"scored","evaluation":evaluation,"accepted":accepted}))
        }
        Err(error) => Ok(
            json!({"status":"unscored","reason":format!("existing evaluator rejected frozen expectations: {error}"),"accepted":false}),
        ),
    }
}

/// Reject unknown top-level fields before deserializing. The evaluator's
/// public expectation structs intentionally accept older documents, while an
/// audit experiment must never mistake a richer frozen private format for a
/// generic one merely because it contains `epub_sha256`.
fn validate_generic_expectations(value: &Value) -> std::result::Result<(), &'static str> {
    let Some(object) = value.as_object() else {
        return Err("unsupported frozen expectation shape");
    };
    let is_exact_shape =
        |allowed: &[&str]| object.keys().all(|key| allowed.contains(&key.as_str()));
    match value["kind"].as_str() {
        Some("source_coverage") => {
            if !is_exact_shape(&["kind", "epub_sha256", "recipes", "ingredients"])
                || serde_json::from_value::<crate::review::SourceExpectations>(value.clone())
                    .is_err()
            {
                return Err("malformed source_coverage frozen expectations");
            }
            Ok(())
        }
        Some(_) => Err("unsupported frozen expectation shape"),
        None => {
            if !is_exact_shape(&["epub_sha256", "recipes", "ingredients"])
                || serde_json::from_value::<crate::review::Expectations>(value.clone()).is_err()
            {
                return Err("malformed legacy frozen expectations");
            }
            Ok(())
        }
    }
}

fn validate_child_identity(manifest: &Value, child: &ReviewRun) -> Result<()> {
    let parent_path = child
        .parent
        .as_deref()
        .ok_or_else(|| rejected("audit child has no parent lineage"))?;
    if hash(&fs::read(parent_path)?) != manifest["parent_run_sha256"] {
        return Err(rejected(
            "audit child parent lineage differs from frozen manifest",
        ));
    }
    let state = child
        .recovery
        .as_ref()
        .ok_or_else(|| rejected("audit child has no recovery state"))?;
    state.validate().map_err(rejected)?;
    if child.epub_sha256 != manifest["parent_epub_sha256"]
        || hash(&serde_json::to_vec(&state.source)?) != manifest["source_sha256"]
        || hash(&serde_json::to_vec(&child.documents)?) != manifest["documents_sha256"]
        || hash(&serde_json::to_vec(&child.navigation_documents)?)
            != manifest["navigation_documents_sha256"]
        || state.source_line_provenance_sha256.as_deref()
            != manifest["source_line_provenance_sha256"].as_str()
    {
        return Err(rejected(
            "audit child source identity differs from frozen manifest",
        ));
    }
    Ok(())
}

fn audit_group_report(manifest: &Value, state: Option<&State>) -> Result<Vec<Value>> {
    let Some(state) = state else {
        return Ok(vec![]);
    };
    manifest["groups"]
        .as_array()
        .ok_or_else(|| rejected("audit manifest has no groups"))?
        .iter()
        .map(|entry| {
            let group_index = entry["group"]
                .as_u64()
                .and_then(|value| usize::try_from(value).ok())
                .ok_or_else(|| rejected("audit manifest group is invalid"))?;
            let candidate_index = entry["candidate"]
                .as_u64()
                .and_then(|value| usize::try_from(value).ok())
                .ok_or_else(|| rejected("audit manifest candidate is invalid"))?;
            let group = state
                .groups
                .get(group_index)
                .ok_or_else(|| rejected("audit child group is unavailable"))?;
            let candidate = group
                .candidates
                .get(candidate_index)
                .ok_or_else(|| rejected("audit child candidate is unavailable"))?;
            let audits = candidate
                .hybrid_audits
                .iter()
                .skip(candidate.audit_baseline_count)
                .collect::<Vec<_>>();
            let accepted = group.accepted == Some(candidate_index)
                && audits.last().is_some_and(|audit| audit.accepted);
            Ok(json!({
                "group": group_index,
                "candidate": candidate_index,
                "accepted": accepted,
                "findings": audits.iter().flat_map(|audit| audit.findings.iter()).collect::<Vec<_>>(),
                "audits": audits,
                "correction_history": candidate.hybrid_correction_history,
                "unresolved_assignment_issues": candidate.hybrid_assignment_issues,
            }))
        })
        .collect()
}

struct LedgerAuditAccounting {
    ledger: Ledger,
    trial_id: String,
    mapping_path: PathBuf,
    entries: BTreeMap<String, String>,
    ordinals: BTreeMap<String, usize>,
    settled: BTreeSet<String>,
    reservation_caps: BTreeMap<usize, f64>,
    reserved_by_group: BTreeMap<usize, f64>,
}

impl LedgerAuditAccounting {
    fn new(
        ledger: Ledger,
        trial_id: String,
        mapping_path: PathBuf,
        reservation_caps: BTreeMap<usize, f64>,
    ) -> Self {
        Self {
            ledger,
            trial_id,
            mapping_path,
            entries: BTreeMap::new(),
            ordinals: BTreeMap::new(),
            settled: BTreeSet::new(),
            reservation_caps,
            reserved_by_group: BTreeMap::new(),
        }
    }

    fn persist_mapping(&self) -> Result<()> {
        durable_json(
            &self.mapping_path,
            &json!({
                "kind": "audit-experiment-ledger-map-v1",
                "bucket": AUDIT_BUCKET,
                "entries": self.entries,
            }),
        )
    }

    fn attempt_key(state: &State, index: usize) -> String {
        let attempt = &state.attempts[index];
        let ordinal = state.attempts[..=index]
            .iter()
            .filter(|candidate| !candidate.inherited && candidate.key == attempt.key)
            .count();
        format!("{}#{ordinal}", attempt.key)
    }
}

impl AuditAccounting for LedgerAuditAccounting {
    fn reserve(&mut self, action: &Action) -> std::result::Result<(), String> {
        let cap = self
            .reservation_caps
            .get(&action.group)
            .copied()
            .ok_or_else(|| "audit action is outside the frozen reservation scope".to_owned())?;
        let reserved = self
            .reserved_by_group
            .get(&action.group)
            .copied()
            .unwrap_or(0.0);
        if reserved + action.reservation_usd > cap + 1e-9 {
            return Err("audit action exceeds its frozen reservation envelope".into());
        }
        let ordinal = self.ordinals.entry(action.key.clone()).or_insert(0);
        *ordinal += 1;
        let attempt_key = format!("{}#{ordinal}", action.key);
        let entry = format!("{}:{attempt_key}", self.trial_id);
        self.ledger
            .reserve(
                AUDIT_BUCKET,
                &entry,
                action.reservation_usd,
                "frozen hybrid audit experiment",
            )
            .map_err(|error| error.to_string())?;
        self.entries.insert(attempt_key, entry);
        self.reserved_by_group
            .entry(action.group)
            .and_modify(|value| *value += action.reservation_usd)
            .or_insert(action.reservation_usd);
        self.persist_mapping().map_err(|error| error.to_string())
    }

    fn checkpoint(&mut self, state: &State) -> std::result::Result<(), String> {
        self.persist_mapping().map_err(|error| error.to_string())?;
        for (index, attempt) in state
            .attempts
            .iter()
            .enumerate()
            .filter(|(_, attempt)| !attempt.inherited && !attempt.pending)
        {
            let attempt_key = Self::attempt_key(state, index);
            let Some(entry) = self.entries.get(&attempt_key) else {
                continue;
            };
            if !self.settled.insert(entry.clone()) {
                continue;
            }
            let result = match attempt.estimated_usd {
                Some(known) => self.ledger.settle(AUDIT_BUCKET, entry, known).or_else(|_| {
                    self.ledger
                        .mark_unknown(AUDIT_BUCKET, entry, attempt.reservation_usd)
                }),
                None => self
                    .ledger
                    .mark_unknown(AUDIT_BUCKET, entry, attempt.reservation_usd),
            };
            result.map_err(|error| error.to_string())?;
        }
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::{Chunk, ChunkRequest, recovery::RequestTelemetry, review::RunChunk};

    fn action() -> Action {
        Action {
            group: 0,
            candidate: 0,
            chunk: None,
            verification_chunk: Some(0),
            verification_stage: Some(0),
            model: "gemini-2.5-flash".into(),
            key: "audit-action".into(),
            reservation_usd: 0.25,
            priced: true,
            output_limit: 1_000,
            telemetry: RequestTelemetry::default(),
            request: ChunkRequest {
                system: String::new(),
                user: String::new(),
                tool_name: String::new(),
                tool_schema: json!({}),
            },
        }
    }

    fn ledger(path: &Path) {
        durable_json(
            path,
            &json!({"buckets":{"verifier":{"known_usd":0.0,"reserved_usd":0.0,"remaining_usd":1.0},"throughput":{"known_usd":0.0,"reserved_usd":0.0,"remaining_usd":1.0}},"entries":[]}),
        )
        .unwrap();
    }

    #[test]
    fn audit_accounting_handle_is_send_for_native_execution() {
        fn assert_send<T: Send>() {}
        assert_send::<AuditAccountingHandle>();
    }

    #[test]
    fn audit_reservation_is_mapped_before_provider_admission() {
        let directory = tempfile::tempdir().unwrap();
        let ledger_path = directory.path().join("ledger.json");
        ledger(&ledger_path);
        let mapping_path = directory.path().join("mapping.json");
        let mut accounting = LedgerAuditAccounting::new(
            Ledger::open(&ledger_path),
            "trial".into(),
            mapping_path.clone(),
            BTreeMap::from([(0, 1.0)]),
        );
        accounting.reserve(&action()).unwrap();
        let map: Value = serde_json::from_slice(&fs::read(mapping_path).unwrap()).unwrap();
        assert_eq!(map["entries"]["audit-action#1"], "trial:audit-action#1");
        let status = Ledger::open(&ledger_path).status().unwrap();
        assert_eq!(status.entries[0].bucket, AUDIT_BUCKET);
        assert_eq!(status.entries[0].status, "reserved");
    }

    #[test]
    fn audit_reservation_cap_blocks_before_creating_a_ledger_entry() {
        let directory = tempfile::tempdir().unwrap();
        let ledger_path = directory.path().join("ledger.json");
        ledger(&ledger_path);
        let mapping_path = directory.path().join("mapping.json");
        let mut accounting = LedgerAuditAccounting::new(
            Ledger::open(&ledger_path),
            "trial".into(),
            mapping_path.clone(),
            BTreeMap::from([(0, 0.24)]),
        );
        assert!(accounting.reserve(&action()).is_err());
        assert!(!mapping_path.exists());
        assert!(
            Ledger::open(&ledger_path)
                .status()
                .unwrap()
                .entries
                .is_empty()
        );
    }

    #[test]
    fn audit_envelope_reserves_retry_expansion_and_reaudit_bound() {
        let value = reservation_envelope(&action(), true).unwrap();
        assert_eq!(
            value["max_attempts_per_call"],
            crate::recovery::MAX_ATTEMPTS
        );
        assert_eq!(value["max_audit_passes"], 2);
        assert_eq!(value["max_context_expansions"], 1);
        assert_eq!(
            value["maximum_calls"],
            (crate::recovery::MAX_ATTEMPTS * 3) as u64
        );
    }

    fn frozen_parent(directory: &Path) -> (PathBuf, String) {
        let source = Chunk {
            title_hint: None,
            text: "Soup\n1 cup water\nSimmer.".into(),
            doc_path: "soup.xhtml".into(),
            links: vec![],
            images: vec![],
        };
        let mut state = State::new_with_strategy(
            vec![source.clone()],
            "gemini-2.5-flash",
            2.0,
            crate::hybrid::HybridStrategy::Hybrid,
        )
        .unwrap();
        let action = state.next_action().unwrap().unwrap();
        state
            .apply(
                &action,
                json!({"recipes":[{"title":{"text":"Soup","spans":[{"start":0,"end":0}]},"description":[],"recipe_yield":[],"notes":[],"equipment":[],"sections":[{"name":{"text":"","spans":[]},"ingredients":[{"start":1,"end":1}],"instructions":[{"start":2,"end":2}]}]}],"ignored":[]}),
            )
            .unwrap();
        let output = state.groups[0].candidates[0].outputs[0].clone();
        let epub_sha256 = "a".repeat(64);
        let parent = ReviewRun {
            recovery: Some(state),
            execution_status: None,
            metadata: None,
            charges: vec![],
            version: crate::review::RUN_VERSION,
            epub_sha256: epub_sha256.clone(),
            source: "frozen.epub".into(),
            model: "gemini-2.5-flash".into(),
            prompt_version: "test".into(),
            parent: None,
            chunks: vec![RunChunk {
                id: "soup".into(),
                source,
                output,
                error: None,
                cached: false,
                usage: Usage::default(),
                model: None,
                prompt_version: None,
                request_identity: None,
            }],
            documents: vec![],
            navigation_documents: Default::default(),
            hybrid_audits: vec![],
            source_line_provenance: vec![],
            source_line_provenance_sha256: None,
            recipes: vec![],
            parsed: Value::Null,
            reserved_usd: 0.0,
            image_text: None,
        };
        let path = directory.join("parent.json");
        parent.save(&path).unwrap();
        (path, epub_sha256)
    }

    fn frozen_quality_manifest(directory: &Path, expectations: Value) -> Value {
        let bytes = durable_bytes(&expectations).unwrap();
        fs::write(directory.join("expectations.json"), &bytes).unwrap();
        json!({
            "expectations_file": "expectations.json",
            "expectations_sha256": hash(&bytes),
        })
    }

    fn replayed_parent(path: &Path) -> ReviewRun {
        let mut run = ReviewRun::read(path).unwrap();
        run.replay().unwrap();
        run
    }

    #[test]
    fn audit_quality_scores_hash_bound_frozen_source_role_bundle() {
        let directory = tempfile::tempdir().unwrap();
        let source = Chunk {
            title_hint: None,
            text: "Soup\nintro\n1 cup water\nSimmer.".into(),
            doc_path: "soup.xhtml".into(),
            links: vec![],
            images: vec![],
        };
        let mut state = State::new_with_strategy(
            vec![source.clone()],
            "gemini-2.5-flash",
            1.0,
            crate::hybrid::HybridStrategy::Hybrid,
        )
        .unwrap();
        let recipe = crate::ExtractedRecipe {
            meta: recipe_types::RecipeMeta {
                title: "Soup".into(),
                description: Some("intro".into()),
                ..Default::default()
            },
            sections: vec![recipe_types::RecipeSection {
                name: None,
                ingredients: vec!["1 cup water".into()],
                instructions: vec!["Simmer.".into()],
            }],
        };
        let mut candidate =
            crate::recovery::Candidate::from_outputs("model".into(), vec![vec![recipe.clone()]]);
        candidate.source_roles = vec![vec![
            "title".into(),
            "metadata".into(),
            "ingredient".into(),
            "method".into(),
        ]];
        candidate.verified = true;
        candidate.hybrid_assignments = ["title", "description", "ingredients", "instructions"]
            .into_iter()
            .enumerate()
            .map(|(line, field)| crate::hybrid::FieldAssignment {
                owner_chunk: Some(0),
                recipe: 0,
                section: matches!(field, "ingredients" | "instructions").then_some(0),
                field: field.into(),
                spans: vec![crate::hybrid::SourceSpan {
                    chunk: 0,
                    start: line,
                    end: line,
                }],
            })
            .collect();
        state.groups[0].candidates.push(candidate);
        state.groups[0].accepted = Some(0);
        let lines = source.text.lines().collect::<Vec<_>>();
        let assignment = |line: usize, role: &str| {
            json!({
                "original_chunk_index":9,
                "line_index":line,
                "document_line":line,
                "text":lines[line],
                "text_sha256":hash(lines[line].as_bytes()),
                "role":role,
                "recipe_title":"Soup",
            })
        };
        let epub_sha256 = "e".repeat(64);
        let source_sha256 = hash(&serde_json::to_vec(&vec![&source]).unwrap());
        let v1 = json!({
            "kind":"source-grounded-automatic-c4-cohort-expectations",
            "schema_version":1,
            "provenance":{"epub_sha256":epub_sha256,"selected_source_sha256":source_sha256},
            "recipes":[{"title":"Soup","original_chunk_index":9}],
            "line_assignments":[assignment(0,"title"),assignment(1,"headnote"),assignment(2,"ingredient"),assignment(3,"method")],
        });
        let v3 = json!({
            "kind":"source-grounded-automatic-c4-headnote-settled-expectations",
            "schema_version":3,
            "provenance":{"epub_sha256":epub_sha256,"selected_source_sha256":hash(&serde_json::to_vec(&vec![&source]).unwrap())},
            "entries":[{"coordinate":{"original_chunk_index":9,"line_index":1},"allowed_roles":["description"]}],
        });
        let v1_raw = serde_json::to_string(&v1).unwrap();
        let v3_raw = serde_json::to_string(&v3).unwrap();
        let bundle = json!({
            "kind":"frozen_source_roles_v1",
            "source_mapping":[{"source_index":0,"original_chunk_index":9}],
            "expectations_v1":v1_raw,
            "expectations_v1_sha256":hash(v1_raw.as_bytes()),
            "expectations_v3":v3_raw,
            "expectations_v3_sha256":hash(v3_raw.as_bytes()),
        });
        let output = Some(vec![recipe]);
        let child = ReviewRun {
            recovery: Some(state),
            execution_status: None,
            metadata: None,
            charges: vec![],
            version: crate::review::RUN_VERSION,
            epub_sha256,
            source: "frozen.epub".into(),
            model: "model".into(),
            prompt_version: "test".into(),
            parent: None,
            chunks: vec![RunChunk {
                id: "soup".into(),
                source,
                output,
                error: None,
                cached: false,
                usage: Usage::default(),
                model: None,
                prompt_version: None,
                request_identity: None,
            }],
            documents: vec![],
            navigation_documents: Default::default(),
            hybrid_audits: vec![],
            source_line_provenance: vec![],
            source_line_provenance_sha256: None,
            recipes: vec![],
            parsed: Value::Null,
            reserved_usd: 0.0,
            image_text: None,
        };
        let manifest = frozen_quality_manifest(directory.path(), bundle);
        let quality =
            evaluate_frozen_audit_quality(&manifest, directory.path(), &child, true).unwrap();
        assert_eq!(quality["status"], "scored");
        assert_eq!(quality["accepted"], true);
    }

    #[test]
    fn audit_quality_scores_supported_legacy_expectations() {
        let directory = tempfile::tempdir().unwrap();
        let (parent, epub_sha256) = frozen_parent(directory.path());
        let child = replayed_parent(&parent);
        let manifest = frozen_quality_manifest(
            directory.path(),
            json!({"epub_sha256":epub_sha256,"recipes":child.recipes,"ingredients":[]}),
        );

        let quality =
            evaluate_frozen_audit_quality(&manifest, directory.path(), &child, true).unwrap();
        assert_eq!(quality["status"], "scored");
        assert_eq!(quality["accepted"], true);
    }

    #[test]
    fn audit_quality_requires_complete_audit_even_when_expectations_pass() {
        let directory = tempfile::tempdir().unwrap();
        let (parent, epub_sha256) = frozen_parent(directory.path());
        let child = replayed_parent(&parent);
        let manifest = frozen_quality_manifest(
            directory.path(),
            json!({"epub_sha256":epub_sha256,"recipes":child.recipes,"ingredients":[]}),
        );

        let quality =
            evaluate_frozen_audit_quality(&manifest, directory.path(), &child, false).unwrap();
        assert_eq!(quality["status"], "scored");
        assert_eq!(quality["accepted"], false);
    }

    #[test]
    fn audit_quality_scores_supported_source_coverage_expectations() {
        let directory = tempfile::tempdir().unwrap();
        let (parent, epub_sha256) = frozen_parent(directory.path());
        let child = replayed_parent(&parent);
        let manifest = frozen_quality_manifest(
            directory.path(),
            json!({"kind":"source_coverage","epub_sha256":epub_sha256,"recipes":[]}),
        );

        let quality =
            evaluate_frozen_audit_quality(&manifest, directory.path(), &child, true).unwrap();
        assert_eq!(quality["status"], "scored");
        assert_eq!(quality["evaluation"]["kind"], "source_coverage");
        assert_eq!(quality["accepted"], false);
    }

    #[test]
    fn audit_quality_keeps_unsupported_expectations_explicitly_unscored() {
        let directory = tempfile::tempdir().unwrap();
        let (parent, epub_sha256) = frozen_parent(directory.path());
        let child = replayed_parent(&parent);
        let manifest = frozen_quality_manifest(
            directory.path(),
            json!({
                "kind":"frozen-source-v3",
                "epub_sha256":epub_sha256,
                "recipes":[],
                "ownership_labels":[]
            }),
        );

        let quality =
            evaluate_frozen_audit_quality(&manifest, directory.path(), &child, true).unwrap();
        assert_eq!(quality["status"], "unscored");
        assert_eq!(quality["reason"], "unsupported frozen expectation shape");
    }

    #[test]
    fn audit_quality_rejects_malformed_source_coverage_expectations() {
        let directory = tempfile::tempdir().unwrap();
        let (parent, epub_sha256) = frozen_parent(directory.path());
        let child = replayed_parent(&parent);
        let manifest = frozen_quality_manifest(
            directory.path(),
            json!({"kind":"source_coverage","epub_sha256":epub_sha256,"recipes":"invalid"}),
        );

        let quality =
            evaluate_frozen_audit_quality(&manifest, directory.path(), &child, true).unwrap();
        assert_eq!(quality["status"], "unscored");
        assert_eq!(
            quality["reason"],
            "malformed source_coverage frozen expectations"
        );
    }

    #[test]
    fn audit_quality_rejects_empty_legacy_expectations() {
        let directory = tempfile::tempdir().unwrap();
        let (parent, _epub_sha256) = frozen_parent(directory.path());
        let child = replayed_parent(&parent);
        let manifest = frozen_quality_manifest(directory.path(), json!({}));

        let quality =
            evaluate_frozen_audit_quality(&manifest, directory.path(), &child, true).unwrap();
        assert_eq!(quality["status"], "unscored");
        assert_eq!(quality["reason"], "malformed legacy frozen expectations");
    }

    #[test]
    fn audit_quality_rejects_frozen_hash_and_epub_identity_mismatches() {
        let directory = tempfile::tempdir().unwrap();
        let (parent, _epub_sha256) = frozen_parent(directory.path());
        let child = replayed_parent(&parent);
        let mut manifest = frozen_quality_manifest(
            directory.path(),
            json!({"epub_sha256":"b".repeat(64),"recipes":child.recipes,"ingredients":[]}),
        );
        let quality =
            evaluate_frozen_audit_quality(&manifest, directory.path(), &child, true).unwrap();
        assert_eq!(quality["status"], "unscored");
        assert!(
            quality["reason"]
                .as_str()
                .unwrap()
                .contains("different EPUB")
        );

        manifest["expectations_sha256"] = Value::String("wrong".into());
        let quality =
            evaluate_frozen_audit_quality(&manifest, directory.path(), &child, true).unwrap();
        assert_eq!(quality["status"], "unscored");
        assert_eq!(quality["reason"], "frozen expectations copy hash differs");
    }

    #[test]
    fn audit_report_scores_frozen_supported_expectations() {
        let directory = tempfile::tempdir().unwrap();
        let (parent_run, epub_sha256) = frozen_parent(directory.path());
        let parent = replayed_parent(&parent_run);
        let expectations = directory.path().join("expectations.json");
        durable_json(
            &expectations,
            &json!({"epub_sha256":epub_sha256,"recipes":parent.recipes,"ingredients":[]}),
        )
        .unwrap();
        let prepared_dir = directory.path().join("prepared");
        let manifest = prepare_audit_experiment(AuditExperimentPrepareRequest {
            parent_run: parent_run.clone(),
            expectations,
            groups: vec![0],
            reviewer_model: "gemini-2.5-flash".into(),
            correct: true,
            budget_usd: 2.0,
            output_dir: prepared_dir.clone(),
        })
        .unwrap();
        let child_path = directory.path().join("child.json");
        let child = prepared_audit_child(
            &parent_run,
            child_path.clone(),
            "gemini-2.5-flash",
            true,
            2.0,
            &[0],
        )
        .unwrap();
        child.save(&child_path).unwrap();
        let evidence = directory.path().join("evidence");
        fs::create_dir(&evidence).unwrap();
        let manifest_bytes = fs::read(&manifest.path).unwrap();
        durable_json(
            &evidence.join("preparation.json"),
            &json!({
                "manifest_sha256":hash(&manifest_bytes),
                "child_run":child_path,
            }),
        )
        .unwrap();
        let ledger_path = directory.path().join("ledger.json");
        ledger(&ledger_path);

        let report = report_audit_experiment(
            &manifest.path,
            &manifest_bytes,
            &manifest.value,
            &evidence,
            &ledger_path,
        )
        .unwrap();
        assert_eq!(report["quality"]["status"], "scored");
        // No provider audit occurred, so scoring cannot turn an incomplete
        // audit into an accepted experiment.
        assert_eq!(report["quality_accepted"], false);
    }

    #[test]
    fn frozen_reservation_preserves_exact_float_bits_through_json() {
        let reservation = 0.048524199999999997_f64;
        let encoded = durable_bytes(&json!({"reservation_usd": reservation})).unwrap();
        let decoded: Value = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(
            decoded["reservation_usd"].as_f64().unwrap().to_bits(),
            reservation.to_bits()
        );
    }

    #[test]
    fn offline_prepare_and_run_reuse_canonical_frozen_expectations() {
        let directory = tempfile::tempdir().unwrap();
        let (parent_run, epub_sha256) = frozen_parent(directory.path());
        let expectations = directory.path().join("expectations.json");
        fs::write(
            &expectations,
            format!(
                "{{\n  \"kind\": \"source_coverage\",\n  \"epub_sha256\": \"{epub_sha256}\"\n}}\n"
            ),
        )
        .unwrap();
        let prepared_dir = directory.path().join("prepared");
        let manifest = prepare_audit_experiment(AuditExperimentPrepareRequest {
            parent_run: parent_run.clone(),
            expectations,
            groups: vec![0],
            reviewer_model: "gemini-2.5-flash".into(),
            correct: true,
            budget_usd: 2.0,
            output_dir: prepared_dir.clone(),
        })
        .unwrap();
        let frozen: Value =
            serde_json::from_slice(&fs::read(prepared_dir.join("expectations.json")).unwrap())
                .unwrap();
        assert_eq!(
            manifest.value["expectations_sha256"],
            hash(&durable_bytes(&frozen).unwrap())
        );
        let outcome =
            futures::executor::block_on(run_audit_experiment(AuditExperimentRunRequest {
                manifest: manifest.path,
                parent_run,
                child_run: directory.path().join("child.json"),
                output_dir: directory.path().join("offline-run"),
                ledger: directory.path().join("unused-ledger.json"),
                dispatch: false,
            }))
            .unwrap();
        assert!(outcome.result.is_none());
        assert_eq!(outcome.preparation["dispatch_requested"], false);
    }

    #[test]
    fn mutated_frozen_request_is_rejected_before_child_or_ledger_creation() {
        let directory = tempfile::tempdir().unwrap();
        let (parent_run, epub_sha256) = frozen_parent(directory.path());
        let expectations = directory.path().join("expectations.json");
        fs::write(
            &expectations,
            format!("{{\"kind\":\"source_coverage\",\"epub_sha256\":\"{epub_sha256}\"}}"),
        )
        .unwrap();
        let prepared_dir = directory.path().join("prepared");
        let manifest = prepare_audit_experiment(AuditExperimentPrepareRequest {
            parent_run: parent_run.clone(),
            expectations,
            groups: vec![0],
            reviewer_model: "gemini-2.5-flash".into(),
            correct: true,
            budget_usd: 2.0,
            output_dir: prepared_dir.clone(),
        })
        .unwrap();
        let request_path = prepared_dir.join("audit-request-0.json");
        let mut frozen: Value = serde_json::from_slice(&fs::read(&request_path).unwrap()).unwrap();
        frozen["user"] = Value::String("tampered".into());
        durable_json(&request_path, &frozen).unwrap();

        let child_run = directory.path().join("child.json");
        let ledger_path = directory.path().join("ledger.json");
        let error = futures::executor::block_on(run_audit_experiment(AuditExperimentRunRequest {
            manifest: manifest.path,
            parent_run,
            child_run: child_run.clone(),
            output_dir: directory.path().join("offline-run"),
            ledger: ledger_path.clone(),
            dispatch: false,
        }))
        .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("audit request differs from frozen manifest")
        );
        assert!(!child_run.exists());
        assert!(!ledger_path.exists());
    }
}
