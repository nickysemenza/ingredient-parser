//! Offline frozen source-role evaluation for a saved native audit state.
//!
//! This creates a reusable bundle from the exact historical expectation bytes,
//! then evaluates the current saved recovery state. It never prepares requests,
//! dispatches a provider call, or changes the input state.
use recipe_epub::{experiment, recovery::State, review, review::ReviewRun};
use serde_json::{Value, json};
use std::{collections::BTreeSet, env, fs, path::PathBuf};

fn usage() -> &'static str {
    "usage: evaluate_frozen_audit STATE EXPECTATIONS_V1 EXPECTATIONS_V3 MAPPING_JSON OUTPUT_NEW [--source-plan PLAN] [--bundle-out NEW_BUNDLE]"
}

fn read_utf8(path: &PathBuf, label: &str) -> Result<(Vec<u8>, String), Box<dyn std::error::Error>> {
    let bytes = fs::read(path)?;
    let text = String::from_utf8(bytes.clone())
        .map_err(|_| format!("{label} must be UTF-8 JSON so its exact bytes can be frozen"))?;
    Ok((bytes, text))
}

fn require_new(path: &std::path::Path, label: &str) -> Result<(), Box<dyn std::error::Error>> {
    if path.exists() {
        return Err(format!(
            "{label} already exists; choose a new path: {}",
            path.display()
        )
        .into());
    }
    Ok(())
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn mapped_source<'a>(
    state: &'a State,
    source_mapping: &Value,
) -> Result<Vec<&'a recipe_epub::Chunk>, Box<dyn std::error::Error>> {
    source_mapping
        .as_array()
        .ok_or("MAPPING_JSON must be a JSON array of source_index/original_chunk_index entries")?
        .iter()
        .map(|entry| {
            let index = entry["source_index"]
                .as_u64()
                .and_then(|index| usize::try_from(index).ok())
                .ok_or("MAPPING_JSON has an invalid source_index")?;
            state
                .source
                .get(index)
                .ok_or_else(|| "MAPPING_JSON source_index is outside STATE".into())
        })
        .collect()
}

fn validate_source_plan(
    path: &PathBuf,
    state: &State,
    source_mapping: &Value,
) -> Result<String, Box<dyn std::error::Error>> {
    let plan: Value = serde_json::from_slice(&fs::read(path)?)?;
    let identity = plan["source_identity"]
        .as_object()
        .ok_or("source plan has no source_identity")?;
    let epub_sha256 = identity
        .get("epub_sha256")
        .and_then(Value::as_str)
        .filter(|value| valid_sha256(value))
        .ok_or("source plan has no valid source_identity.epub_sha256")?;
    let expected_full_source = identity
        .get("context_source_sha256")
        .and_then(Value::as_str)
        .filter(|value| valid_sha256(value))
        .ok_or("source plan has no valid source_identity.context_source_sha256")?;
    let full_source_sha256 = review::hash(&serde_json::to_vec(&state.source)?);
    if expected_full_source != full_source_sha256 {
        return Err("raw STATE full source differs from source plan context identity".into());
    }
    let expected_selected_source = identity
        .get("selected_source_sha256")
        .and_then(Value::as_str)
        .filter(|value| valid_sha256(value))
        .ok_or("source plan has no valid source_identity.selected_source_sha256")?;
    let selected_source_sha256 =
        review::hash(&serde_json::to_vec(&mapped_source(state, source_mapping)?)?);
    if expected_selected_source != selected_source_sha256 {
        return Err("raw STATE mapped source differs from source plan selected identity".into());
    }
    if let Some(expected) = identity
        .get("source_line_provenance_sha256")
        .and_then(Value::as_str)
    {
        let actual = state
            .source_line_provenance_sha256
            .as_deref()
            .ok_or("raw STATE has no source provenance required by source plan")?;
        if expected != actual {
            return Err("raw STATE source provenance differs from source plan".into());
        }
    }
    Ok(epub_sha256.to_owned())
}

fn native_state(
    bytes: &[u8],
    source_plan: Option<&PathBuf>,
    source_mapping: &Value,
) -> Result<(State, String), Box<dyn std::error::Error>> {
    if let Ok(run) = serde_json::from_slice::<ReviewRun>(bytes) {
        let state = run.recovery.ok_or("STATE has no recovery state")?;
        state
            .validate()
            .map_err(|error| format!("STATE is invalid: {error}"))?;
        if !valid_sha256(&run.epub_sha256) {
            return Err("STATE has no valid EPUB SHA-256 identity".into());
        }
        if let Some(plan) = source_plan {
            let plan_epub = validate_source_plan(plan, &state, source_mapping)?;
            if plan_epub != run.epub_sha256 {
                return Err("STATE EPUB identity differs from explicit source plan".into());
            }
        }
        return Ok((state, run.epub_sha256));
    }
    let state: State = serde_json::from_slice(bytes)
        .map_err(|_| "STATE must be a full native saved review run or raw recovery State JSON")?;
    state
        .validate()
        .map_err(|error| format!("STATE is invalid: {error}"))?;
    let plan = source_plan
        .ok_or("raw recovery State requires --source-plan PLAN because it has no EPUB identity")?;
    let epub_sha256 = validate_source_plan(plan, &state, source_mapping)?;
    Ok((state, epub_sha256))
}

fn scoped_audit_complete(
    state: &State,
    source_mapping: &Value,
) -> Result<bool, Box<dyn std::error::Error>> {
    let source_indexes = source_mapping
        .as_array()
        .ok_or("MAPPING_JSON must be a JSON array of source_index/original_chunk_index entries")?
        .iter()
        .map(|entry| {
            entry["source_index"]
                .as_u64()
                .and_then(|index| usize::try_from(index).ok())
                .filter(|index| *index < state.source.len())
                .ok_or("MAPPING_JSON has an invalid source_index")
        })
        .collect::<Result<BTreeSet<_>, _>>()?;
    if source_indexes.is_empty() || state.attempts.iter().any(|attempt| attempt.pending) {
        return Ok(false);
    }
    let groups = state.groups.iter().filter(|group| {
        group.enabled
            && group
                .chunks
                .iter()
                .any(|chunk| source_indexes.contains(chunk))
    });
    let mut found = false;
    for group in groups {
        found = true;
        let Some(candidate) = group.accepted.and_then(|index| group.candidates.get(index)) else {
            return Ok(false);
        };
        if !candidate.verified
            || !candidate.feedback.is_empty()
            || candidate.outputs.iter().any(Option::is_none)
        {
            return Ok(false);
        }
    }
    Ok(found)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = env::args().skip(1).collect::<Vec<_>>();
    if args.len() < 5 {
        return Err(usage().into());
    }
    let state_path = PathBuf::from(&args[0]);
    let v1_path = PathBuf::from(&args[1]);
    let v3_path = PathBuf::from(&args[2]);
    let mapping_path = PathBuf::from(&args[3]);
    let output_path = PathBuf::from(&args[4]);
    let mut bundle_path = None;
    let mut source_plan = None;
    let mut flags = args[5..].iter();
    while let Some(flag) = flags.next() {
        let value = flags.next().ok_or_else(|| usage().to_owned())?;
        match flag.as_str() {
            "--bundle-out" if bundle_path.is_none() => bundle_path = Some(PathBuf::from(value)),
            "--source-plan" if source_plan.is_none() => source_plan = Some(PathBuf::from(value)),
            _ => return Err(usage().into()),
        }
    }
    require_new(&output_path, "evaluation output")?;
    if let Some(bundle_path) = &bundle_path {
        require_new(bundle_path, "bundle output")?;
    }

    let source_mapping: Value = serde_json::from_slice(&fs::read(&mapping_path)?)?;
    let state_bytes = fs::read(&state_path)?;
    let (state, epub_sha256) = native_state(&state_bytes, source_plan.as_ref(), &source_mapping)?;
    let (v1_bytes, expectations_v1) = read_utf8(&v1_path, "EXPECTATIONS_V1")?;
    let (v3_bytes, expectations_v3) = read_utf8(&v3_path, "EXPECTATIONS_V3")?;
    let audit_complete = scoped_audit_complete(&state, &source_mapping)?;
    let bundle = experiment::frozen_audit_bundle(source_mapping, expectations_v1, expectations_v3);
    if bundle["expectations_v1_sha256"] != review::hash(&v1_bytes)
        || bundle["expectations_v3_sha256"] != review::hash(&v3_bytes)
    {
        return Err("frozen audit bundle did not preserve exact expectation bytes".into());
    }
    // The evaluator independently validates the mapping and selected groups.
    // This bound covers only mapped audit groups, never unrelated book context.
    let evaluation =
        experiment::evaluate_frozen_audit(&state, &epub_sha256, &bundle, audit_complete)?;
    if let Some(bundle_path) = &bundle_path {
        experiment::durable_json(bundle_path, &bundle)?;
    }
    let output = json!({
        "kind": "frozen-source-roles-evaluation-v1",
        "state": state_path,
        "state_sha256": review::hash(&state_bytes),
        "epub_sha256": epub_sha256,
        "bundle_sha256": review::hash(&serde_json::to_vec(&bundle)?),
        "bundle_out": bundle_path,
        "source_plan": source_plan,
        "audit_complete": audit_complete,
        "evaluation": evaluation,
        "scope": "Offline evaluation of mapped audit groups using each selected accepted candidate or latest proposal; acceptance is gated separately by scoped audit completion and evaluator findings. No provider calls, budget reservations, or input-state mutation.",
    });
    experiment::durable_json(&output_path, &output)?;
    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}
