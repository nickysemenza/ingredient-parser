//! Dispatch one bounded source-role request from a private offline plan.
//!
//! This is experimental evaluation infrastructure, never normal extraction.
//! It sends exactly one provider request only with `--dispatch`; the default
//! prepare-only mode validates the plan and writes a fresh checkpoint without
//! reading credentials, mutating the ledger, or making network traffic.

use recipe_epub::{
    CallFailure, ChunkRequest, EpubError, ExtractionStats, Options, RecoveryBackend, Usage,
    experiment::Ledger,
    models,
    recovery::{Action, RequestTelemetry},
};
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    env, fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

const EVIDENCE_SCHEMA_VERSION: u32 = 1;
const EVIDENCE_KIND: &str = "paid-source-role-evaluation-v1";
const TOOL_NAME: &str = "emit_source_roles";

#[derive(Clone)]
struct Args {
    plan: PathBuf,
    pack_id: String,
    model: String,
    output_limit: u32,
    ledger: PathBuf,
    evidence: PathBuf,
    planner_tool: PathBuf,
    dispatch: bool,
}

#[derive(Debug, Clone)]
struct ValidatedPack {
    plan_sha256: String,
    epub_sha256: String,
    profile_sha256: String,
    contract_sha256: String,
    pack_id: String,
    request_sha256: String,
    coordinate_map_sha256: String,
    provider_payload_sha256: String,
    provider_payload_bytes: usize,
    request: Value,
    source_bytes: usize,
    unknown_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
struct ActionEvidence {
    key: String,
    model: String,
    output_limit: u32,
    reservation_usd: f64,
    request_bytes: usize,
    source_bytes: usize,
    schema_bytes: usize,
}

#[derive(Debug, Serialize)]
struct LedgerMap<'a> {
    schema_version: u32,
    kind: &'static str,
    bucket: &'static str,
    ledger_entry: &'a str,
    plan_sha256: &'a str,
    request_sha256: &'a str,
    action_key: &'a str,
}

#[derive(Debug, Serialize)]
struct Evidence {
    schema_version: u32,
    kind: &'static str,
    verified: bool,
    status: String,
    plan_sha256: String,
    epub_sha256: String,
    profile_sha256: String,
    contract_sha256: String,
    pack_id: String,
    request_sha256: String,
    provider_payload_sha256: String,
    coordinate_map_sha256: String,
    unknown_ids: Vec<String>,
    action: ActionEvidence,
    ledger_entry: Option<String>,
    reservation_status: String,
    settlement_status: Option<String>,
    usage: Option<Usage>,
    raw_usage: Option<Value>,
    response: Option<Value>,
    response_sha256: Option<String>,
    provider_finish_reason: Option<String>,
    provider_elapsed_ms: Option<u64>,
    provider_failure: Option<ProviderFailureEvidence>,
    importer: Option<ImporterEvidence>,
    error: Option<String>,
}

/// Safe transport diagnostics for an interrupted or rejected provider call.
/// Provider messages, bodies, URLs, request IDs, and credentials never enter
/// this evidence artifact.
#[derive(Debug, Serialize)]
struct ProviderFailureEvidence {
    category: &'static str,
    http_status: Option<u16>,
    retryable: bool,
    truncated: bool,
}

#[derive(Debug, Serialize)]
struct ImporterEvidence {
    response_path: PathBuf,
    assignments_path: PathBuf,
    assignments_sha256: Option<String>,
    accepted: bool,
    error: Option<String>,
}

fn provider_failure_evidence(failure: &CallFailure) -> ProviderFailureEvidence {
    let (category, http_status) = match &failure.error {
        EpubError::Api { status, .. } => ("api", Some(*status)),
        EpubError::Request(request) => ("request", request.status),
        #[cfg(feature = "native")]
        EpubError::Http(error) => ("transport", error.status().map(|status| status.as_u16())),
        EpubError::MissingApiKey | EpubError::MissingBaseUrl => ("configuration", None),
        EpubError::Deserialize(_) => ("decode", None),
        EpubError::Open(_) | EpubError::Cache(_) => ("local", None),
        EpubError::Proxy(_) => ("transport", None),
    };
    ProviderFailureEvidence {
        category,
        http_status,
        retryable: failure.is_retryable(),
        truncated: failure.truncated,
    }
}

fn next_value(values: &mut impl Iterator<Item = String>, name: &str) -> Result<String, String> {
    values.next().ok_or_else(|| format!("missing {name}"))
}

fn args() -> Result<Args, String> {
    let mut values = env::args().skip(1);
    let mut plan = None;
    let mut pack_id = None;
    let mut model = None;
    let mut output_limit = 6_000;
    let mut ledger = None;
    let mut evidence = None;
    let mut planner_tool = PathBuf::from("tools/plan_source_roles.py");
    let mut dispatch = false;
    while let Some(flag) = values.next() {
        match flag.as_str() {
            "--plan" => plan = Some(next_value(&mut values, "--plan value")?.into()),
            "--pack-id" => pack_id = Some(next_value(&mut values, "--pack-id value")?),
            "--model" => model = Some(next_value(&mut values, "--model value")?),
            "--output-limit" => {
                output_limit = next_value(&mut values, "--output-limit value")?
                    .parse()
                    .map_err(|_| "--output-limit must be a positive integer")?
            }
            "--ledger" => ledger = Some(next_value(&mut values, "--ledger value")?.into()),
            "--evidence" => evidence = Some(next_value(&mut values, "--evidence value")?.into()),
            "--ledger-tool" => { let _ = next_value(&mut values, "--ledger-tool value")?; },
            "--planner-tool" => planner_tool = next_value(&mut values, "--planner-tool value")?.into(),
            "--dispatch" => dispatch = true,
            "--prepare-only" | "--dry-run" => dispatch = false,
            "--help" => return Err("usage: paid_source_roles_evaluation --plan PRIVATE_PLAN --pack-id PACK --model CATALOG_MODEL --output-limit CAP --ledger PRIVATE_LEDGER --evidence NEW_PRIVATE_EVIDENCE [--prepare-only|--dry-run|--dispatch] [--ledger-tool TOOL] [--planner-tool TOOL]".into()),
            other => return Err(format!("unknown option {other}")),
        }
    }
    if output_limit == 0 {
        return Err("--output-limit must be positive".into());
    }
    Ok(Args {
        plan: plan.ok_or("--plan is required")?,
        pack_id: pack_id.ok_or("--pack-id is required")?,
        model: model.ok_or("--model is required")?,
        output_limit,
        ledger: ledger.ok_or("--ledger is required")?,
        evidence: evidence.ok_or("--evidence is required")?,
        planner_tool,
        dispatch,
    })
}

fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Python's planner hashes `json.dumps(..., ensure_ascii=False, sort_keys=True,
/// separators=(",", ":"))`. `serde_json::Value` uses sorted object keys in this
/// workspace, and its compact serializer has the same UTF-8 JSON form. The
/// cross-language identity tests below pin this compatibility for the fields
/// admitted here.
fn canonical_json(value: &Value) -> Result<Vec<u8>, String> {
    serde_json::to_vec(value).map_err(|error| error.to_string())
}

fn canonical_hash(value: &Value) -> Result<String, String> {
    Ok(sha256(&canonical_json(value)?))
}

fn object<'a>(value: &'a Value, label: &str) -> Result<&'a serde_json::Map<String, Value>, String> {
    value
        .as_object()
        .ok_or_else(|| format!("{label} must be an object"))
}

fn array<'a>(value: &'a Value, label: &str) -> Result<&'a [Value], String> {
    value
        .as_array()
        .map(Vec::as_slice)
        .ok_or_else(|| format!("{label} must be an array"))
}

fn string<'a>(value: &'a Value, label: &str) -> Result<&'a str, String> {
    value
        .as_str()
        .ok_or_else(|| format!("{label} must be a string"))
}

fn field<'a>(
    object: &'a serde_json::Map<String, Value>,
    key: &str,
    label: &str,
) -> Result<&'a Value, String> {
    object
        .get(key)
        .ok_or_else(|| format!("{label} is missing {key}"))
}

fn exact_keys(
    object: &serde_json::Map<String, Value>,
    expected: &[&str],
    label: &str,
) -> Result<(), String> {
    let actual: BTreeSet<_> = object.keys().map(String::as_str).collect();
    let expected: BTreeSet<_> = expected.iter().copied().collect();
    if actual == expected {
        Ok(())
    } else {
        Err(format!("{label} has unexpected shape"))
    }
}

fn coordinate(value: &Value, label: &str) -> Result<(), String> {
    let value = object(value, label)?;
    exact_keys(
        value,
        &["original_chunk_index", "line_index", "document_line"],
        label,
    )?;
    for key in ["original_chunk_index", "line_index", "document_line"] {
        field(value, key, label)?
            .as_u64()
            .ok_or_else(|| format!("{label}.{key} must be a non-negative integer"))?;
    }
    Ok(())
}

fn validate_pack(plan_bytes: &[u8], pack_id: &str) -> Result<ValidatedPack, String> {
    let plan: Value = serde_json::from_slice(plan_bytes)
        .map_err(|error| format!("invalid plan JSON: {error}"))?;
    let root = object(&plan, "plan")?;
    if field(root, "schema_version", "plan")?.as_u64() != Some(1)
        || string(field(root, "kind", "plan")?, "plan.kind")? != "source-role-plan-v1"
        || string(field(root, "authority", "plan")?, "plan.authority")? != "NONAUTHORITATIVE"
    {
        return Err("plan is not a nonauthoritative schema-1 source role plan".into());
    }
    let epub_sha256 = string(field(root, "epub_sha256", "plan")?, "plan.epub_sha256")?.to_owned();
    let profile_sha256 = string(
        field(root, "profile_sha256", "plan")?,
        "plan.profile_sha256",
    )?
    .to_owned();
    let contract_sha256 = string(
        field(root, "contract_sha256", "plan")?,
        "plan.contract_sha256",
    )?
    .to_owned();
    let packs = array(field(root, "request_packs", "plan")?, "plan.request_packs")?;
    let matches: Vec<_> = packs
        .iter()
        .filter(|candidate| candidate.get("pack_id").and_then(Value::as_str) == Some(pack_id))
        .collect();
    if matches.len() != 1 {
        return Err(format!(
            "plan must contain exactly one pack named {pack_id}"
        ));
    }
    let pack = object(matches[0], "plan.request_pack")?;
    let request_sha256 = string(
        field(pack, "request_sha256", "plan.request_pack")?,
        "plan.request_pack.request_sha256",
    )?
    .to_owned();
    let payload = field(pack, "provider_payload", "plan.request_pack")?;
    let payload_object = object(payload, "plan.request_pack.provider_payload")?;
    exact_keys(
        payload_object,
        &["request_sha256", "request"],
        "plan.request_pack.provider_payload",
    )?;
    let request = field(
        payload_object,
        "request",
        "plan.request_pack.provider_payload",
    )?
    .clone();
    if string(
        field(
            payload_object,
            "request_sha256",
            "plan.request_pack.provider_payload",
        )?,
        "plan.request_pack.provider_payload.request_sha256",
    )? != request_sha256
        || canonical_hash(&request)? != request_sha256
    {
        return Err("plan request hash does not bind the provider payload".into());
    }
    let declared_payload_bytes = field(pack, "provider_payload_utf8_bytes", "plan.request_pack")?
        .as_u64()
        .ok_or("plan.provider_payload_utf8_bytes must be an integer")?
        as usize;
    let actual_payload_bytes = canonical_json(payload)?.len();
    if declared_payload_bytes != actual_payload_bytes {
        return Err("plan provider payload byte count is stale".into());
    }
    let request_object = object(&request, "plan.request_pack.provider_payload.request")?;
    if string(
        field(request_object, "epub_sha256", "plan.request")?,
        "plan.request.epub_sha256",
    )? != epub_sha256
        || string(
            field(request_object, "profile_sha256", "plan.request")?,
            "plan.request.profile_sha256",
        )? != profile_sha256
        || string(
            field(request_object, "contract_sha256", "plan.request")?,
            "plan.request.contract_sha256",
        )? != contract_sha256
    {
        return Err("plan request metadata does not match its enclosing plan".into());
    }
    let coordinate_map = field(pack, "coordinate_map", "plan.request_pack")?.clone();
    let coordinate_map_object = object(&coordinate_map, "plan.request_pack.coordinate_map")?;
    let coordinate_map_sha256 = canonical_hash(&coordinate_map)?;
    if string(
        field(request_object, "coordinate_map_sha256", "plan.request")?,
        "plan.request.coordinate_map_sha256",
    )? != coordinate_map_sha256
    {
        return Err("plan coordinate map is not bound by the request".into());
    }
    let mut context_ids = BTreeSet::new();
    let mut fragment_ids = BTreeSet::new();
    let mut unknown_ids = Vec::new();
    let mut seen_unknowns = BTreeSet::new();
    let mut source_bytes = 0usize;
    for (region_index, region) in array(
        field(request_object, "regions", "plan.request")?,
        "plan.request.regions",
    )?
    .iter()
    .enumerate()
    {
        let region = object(region, "plan.request.region")?;
        let region_unknown_ids: BTreeSet<String> = array(
            field(region, "unknown_ids", "plan.request.region")?,
            "plan.request.region.unknown_ids",
        )?
        .iter()
        .map(|id| string(id, "plan.request.region.unknown_ids").map(str::to_owned))
        .collect::<Result<_, _>>()?;
        let context = array(
            field(region, "context", "plan.request.region")?,
            "plan.request.region.context",
        )?;
        if context.is_empty() {
            return Err(format!(
                "plan.request.regions[{region_index}] has no context"
            ));
        }
        for line in context {
            let line = object(line, "plan.request.context")?;
            let id = string(
                field(line, "id", "plan.request.context")?,
                "plan.request.context.id",
            )?
            .to_owned();
            if !context_ids.insert(id.clone()) {
                return Err(format!("plan repeats context ID {id}"));
            }
            let source = line.get("source");
            let source_fragments = line.get("source_fragments");
            match (source, source_fragments) {
                (Some(source), None) => {
                    exact_keys(line, &["id", "source"], "plan.request.context")?;
                    source_bytes = source_bytes
                        .saturating_add(string(source, "plan.request.context.source")?.len());
                    if region_unknown_ids.contains(&id) {
                        return Err(format!("plan target {id} must use source_fragments"));
                    }
                }
                (None, Some(fragments)) => {
                    exact_keys(line, &["id", "source_fragments"], "plan.request.context")?;
                    if !region_unknown_ids.contains(&id) {
                        return Err(format!("plan non-target {id} must use source"));
                    }
                    let fragments = array(fragments, "plan.request.context.source_fragments")?;
                    if fragments.is_empty() {
                        return Err(format!("plan target {id} has no source fragments"));
                    }
                    for (fragment_index, fragment) in fragments.iter().enumerate() {
                        let fragment = object(fragment, "plan.request.context.source_fragment")?;
                        exact_keys(
                            fragment,
                            &["id", "text"],
                            "plan.request.context.source_fragment",
                        )?;
                        let fragment_id = string(
                            field(fragment, "id", "plan.request.context.source_fragment")?,
                            "plan.request.context.source_fragment.id",
                        )?;
                        if fragment_id != format!("{id}f{fragment_index}") {
                            return Err(format!(
                                "plan source fragment {fragment_id} is not stable for {id}"
                            ));
                        }
                        if !fragment_ids.insert(fragment_id.to_owned()) {
                            return Err(format!("plan repeats source fragment ID {fragment_id}"));
                        }
                        let text = string(
                            field(fragment, "text", "plan.request.context.source_fragment")?,
                            "plan.request.context.source_fragment.text",
                        )?;
                        if text.is_empty() || text.chars().count() > 240 {
                            return Err(format!(
                                "plan source fragment {fragment_id} must contain 1..240 Unicode characters"
                            ));
                        }
                        source_bytes = source_bytes.saturating_add(text.len());
                    }
                }
                _ => {
                    return Err(format!(
                        "plan context {id} must contain exactly source or source_fragments"
                    ));
                }
            }
        }
        for id in array(
            field(region, "unknown_ids", "plan.request.region")?,
            "plan.request.region.unknown_ids",
        )? {
            let id = string(id, "plan.request.region.unknown_ids")?.to_owned();
            if !seen_unknowns.insert(id.clone()) {
                return Err(format!("plan repeats unknown ID {id}"));
            }
            unknown_ids.push(id);
        }
    }
    let map_ids: BTreeSet<String> = coordinate_map_object.keys().cloned().collect();
    if context_ids != map_ids {
        return Err("plan coordinate map does not contain exactly every context ID".into());
    }
    if !unknown_ids.iter().all(|id| context_ids.contains(id)) {
        return Err("plan unknown IDs are absent from context".into());
    }
    for (id, coordinate_value) in coordinate_map_object {
        coordinate(coordinate_value, &format!("plan.coordinate_map.{id}"))?;
    }
    Ok(ValidatedPack {
        plan_sha256: sha256(plan_bytes),
        epub_sha256,
        profile_sha256,
        contract_sha256,
        pack_id: pack_id.to_owned(),
        request_sha256,
        coordinate_map_sha256,
        provider_payload_sha256: canonical_hash(payload)?,
        provider_payload_bytes: actual_payload_bytes,
        request,
        source_bytes,
        unknown_ids,
    })
}

fn action_for_pack(pack: &ValidatedPack, model: &str, output_limit: u32) -> Result<Action, String> {
    let model_info = models::catalog()
        .into_iter()
        .find(|candidate| candidate.id == model && candidate.enabled)
        .ok_or_else(|| format!("model must be an enabled catalog model: {model}"))?;
    if u64::from(output_limit) > model_info.max_output_tokens {
        return Err(format!(
            "output limit {output_limit} exceeds catalog maximum {} for {model}",
            model_info.max_output_tokens
        ));
    }
    let request_object = object(&pack.request, "validated request")?;
    let system = string(
        field(request_object, "instructions", "validated request")?,
        "validated request.instructions",
    )?
    .to_owned();
    let schema = field(request_object, "response_schema", "validated request")?.clone();
    let user = json!({
        "request_sha256": pack.request_sha256,
        "contract_sha256": pack.contract_sha256,
        "epub_sha256": pack.epub_sha256,
        "profile_sha256": pack.profile_sha256,
        "regions": field(request_object, "regions", "validated request")?,
    });
    let request = ChunkRequest {
        system,
        user: serde_json::to_string(&user).map_err(|error| error.to_string())?,
        tool_name: TOOL_NAME.into(),
        tool_schema: schema,
    };
    let action_identity = json!({
        "kind": "paid-source-role-action-v1",
        "request_sha256": pack.request_sha256,
        "model": model,
        "provider": model_info.provider,
        "transport": model_info.transport,
        "output_limit": output_limit,
        "request": request,
    });
    let identity_bytes = canonical_json(&action_identity)?;
    let key = sha256(&identity_bytes);
    // This exactly follows recovery's conservative formula: serialized request
    // bytes times two for input, plus the operation's actual bounded cap.
    let reservation_usage = Usage {
        input_tokens: identity_bytes.len().saturating_mul(2) as u64,
        output_tokens: output_limit as u64,
        ..Usage::default()
    };
    let reservation_usd = ExtractionStats {
        model: model.to_owned(),
        usage: reservation_usage,
        ..Default::default()
    }
    .cost_usd()
    .ok_or_else(|| format!("catalog model has no usable price: {model}"))?;
    let schema_bytes = serde_json::to_vec(&request.tool_schema)
        .map_err(|error| error.to_string())?
        .len();
    let context_bytes = request.user.len().saturating_sub(pack.source_bytes);
    Ok(Action {
        group: 0,
        candidate: 0,
        // This makes the existing transport record an extraction-class call,
        // rather than mislabeling this independent classification as verify.
        chunk: Some(0),
        verification_chunk: None,
        verification_stage: None,
        model: model.to_owned(),
        request,
        key,
        reservation_usd,
        priced: true,
        output_limit,
        telemetry: RequestTelemetry {
            operation: "source_role_classification".into(),
            model: model.to_owned(),
            contract: pack.contract_sha256.clone(),
            output_limit,
            source_bytes: pack.source_bytes,
            context_bytes,
            candidate_bytes: 0,
            schema_bytes,
            queue_ms: None,
            provider_ms: None,
            retry_ms: None,
            scheduled_retry_ms: 0,
            reported_reasoning_tokens: None,
            reservation_usd,
            estimated_usd: None,
        },
    })
}

fn action_evidence(action: &Action) -> ActionEvidence {
    ActionEvidence {
        key: action.key.clone(),
        model: action.model.clone(),
        output_limit: action.output_limit,
        reservation_usd: action.reservation_usd,
        request_bytes: action.request.user.len() + action.request.system.len(),
        source_bytes: action.telemetry.source_bytes,
        schema_bytes: action.telemetry.schema_bytes,
    }
}

fn initial_evidence(pack: &ValidatedPack, action: &Action, status: &str) -> Evidence {
    Evidence {
        schema_version: EVIDENCE_SCHEMA_VERSION,
        kind: EVIDENCE_KIND,
        verified: false,
        status: status.into(),
        plan_sha256: pack.plan_sha256.clone(),
        epub_sha256: pack.epub_sha256.clone(),
        profile_sha256: pack.profile_sha256.clone(),
        contract_sha256: pack.contract_sha256.clone(),
        pack_id: pack.pack_id.clone(),
        request_sha256: pack.request_sha256.clone(),
        provider_payload_sha256: pack.provider_payload_sha256.clone(),
        coordinate_map_sha256: pack.coordinate_map_sha256.clone(),
        unknown_ids: pack.unknown_ids.clone(),
        action: action_evidence(action),
        ledger_entry: None,
        reservation_status: "not_reserved".into(),
        settlement_status: None,
        usage: None,
        raw_usage: None,
        response: None,
        response_sha256: None,
        provider_finish_reason: None,
        provider_elapsed_ms: None,
        provider_failure: None,
        importer: None,
        error: None,
    }
}

fn write_new(path: &Path, document: &impl Serialize) -> Result<(), String> {
    fs::create_dir_all(path.parent().ok_or("output path has no parent")?)
        .map_err(|error| error.to_string())?;
    let bytes = serde_json::to_vec_pretty(document).map_err(|error| error.to_string())?;
    let mut output = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| format!("refusing to create output {}: {error}", path.display()))?;
    use std::io::Write;
    output
        .write_all(&bytes)
        .map_err(|error| error.to_string())?;
    output.write_all(b"\n").map_err(|error| error.to_string())?;
    output.sync_all().map_err(|error| error.to_string())?;
    sync_parent(path)
}

fn overwrite_checkpoint(path: &Path, document: &Evidence) -> Result<(), String> {
    let temporary = path.with_extension(format!("{}.tmp", std::process::id()));
    let bytes = serde_json::to_vec_pretty(document).map_err(|error| error.to_string())?;
    let mut output = fs::File::create(&temporary).map_err(|error| error.to_string())?;
    use std::io::Write;
    output
        .write_all(&bytes)
        .map_err(|error| error.to_string())?;
    output.write_all(b"\n").map_err(|error| error.to_string())?;
    output.sync_all().map_err(|error| error.to_string())?;
    fs::rename(temporary, path)
        .map_err(|error| error.to_string())
        .and_then(|()| sync_parent(path))
}

fn sync_parent(path: &Path) -> Result<(), String> {
    fs::File::open(path.parent().ok_or("output path has no parent")?)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| error.to_string())
}

fn sidecar(path: &Path, suffix: &str) -> PathBuf {
    path.with_extension(suffix)
}

fn ledger(args: &Args, action: &str, entry: &str, amount: f64) -> Result<(), String> {
    let ledger = Ledger::open(&args.ledger);
    match action {
        "reserve" => ledger.reserve(
            "throughput",
            entry,
            amount,
            "one-pack source-role classification evaluation",
        ),
        "settle" => ledger.settle("throughput", entry, amount),
        "unknown" => ledger.mark_unknown("throughput", entry, amount),
        _ => return Err("unknown ledger operation".into()),
    }
    .map_err(|error| error.to_string())
}

fn settle_or_hold(
    args: &Args,
    entry: &str,
    reservation: f64,
    usage: Option<&Usage>,
) -> (String, Option<String>) {
    let known = usage.and_then(|usage| {
        ExtractionStats {
            model: args.model.clone(),
            usage: usage.clone(),
            ..Default::default()
        }
        .cost_usd()
    });
    match known {
        Some(amount) => match ledger(args, "settle", entry, amount) {
            Ok(()) => ("settled_known".into(), None),
            Err(error) => match ledger(args, "unknown", entry, reservation) {
                Ok(()) => (
                    "held_unknown".into(),
                    Some(format!("known settlement failed: {error}")),
                ),
                Err(hold_error) => (
                    "reservation_retained".into(),
                    Some(format!(
                        "known settlement failed: {error}; could not mark unknown: {hold_error}"
                    )),
                ),
            },
        },
        None => match ledger(args, "unknown", entry, reservation) {
            Ok(()) => ("held_unknown".into(), None),
            Err(error) => (
                "reservation_retained".into(),
                Some(format!("could not mark unknown: {error}")),
            ),
        },
    }
}

fn import_response(
    args: &Args,
    response_path: &Path,
    assignments_path: &Path,
) -> Result<(), String> {
    let status = Command::new("python3")
        .arg("-B")
        .arg(&args.planner_tool)
        .args(["import", "--plan"])
        .arg(&args.plan)
        .args(["--pack-id", &args.pack_id, "--response"])
        .arg(response_path)
        .args(["--out"])
        .arg(assignments_path)
        .stdout(Stdio::null())
        .status()
        .map_err(|error| format!("could not run strict response importer: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err("strict response importer rejected the provider payload".into())
    }
}

/// Reuse the planner's strict importer contract before any evidence, backend,
/// or ledger work. A stale plan can otherwise reserve a paid call whose reply
/// the current importer is guaranteed to reject.
fn validate_planner_contract(args: &Args) -> Result<(), String> {
    let status = Command::new("python3")
        .arg("-B")
        .arg(&args.planner_tool)
        .args(["validate", "--plan"])
        .arg(&args.plan)
        .args(["--pack-id", &args.pack_id])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|_| "planner validation could not run".to_owned())?;
    if status.success() {
        Ok(())
    } else {
        Err("planner validation rejected the selected request pack".into())
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = args().map_err(std::io::Error::other)?;
    let evidence_map = sidecar(&args.evidence, "ledger-map.json");
    let response_path = sidecar(&args.evidence, "response.json");
    let assignments_path = sidecar(&args.evidence, "assignments.json");
    for output in [
        &args.evidence,
        &evidence_map,
        &response_path,
        &assignments_path,
    ] {
        if output.exists() {
            return Err(format!(
                "refusing to resume or overwrite prior evaluation output: {}",
                output.display()
            )
            .into());
        }
    }
    validate_planner_contract(&args).map_err(std::io::Error::other)?;
    let plan_bytes = fs::read(&args.plan)?;
    let pack = validate_pack(&plan_bytes, &args.pack_id).map_err(std::io::Error::other)?;
    let action =
        action_for_pack(&pack, &args.model, args.output_limit).map_err(std::io::Error::other)?;
    let mut evidence = initial_evidence(
        &pack,
        &action,
        if args.dispatch {
            "prepared"
        } else {
            "prepare_only"
        },
    );
    write_new(&args.evidence, &evidence).map_err(std::io::Error::other)?;
    if !args.dispatch {
        println!(
            "{}",
            json!({
                "dispatch": false,
                "action_key": action.key,
                "model": action.model,
                "output_limit": action.output_limit,
                "request_bytes": pack.provider_payload_bytes,
                "reservation_usd": action.reservation_usd,
                "checkpoint": args.evidence,
                "verified": false,
            })
        );
        return Ok(());
    }

    let trial_id = sha256(args.evidence.to_string_lossy().as_bytes());
    let ledger_entry = format!("{trial_id}:{}", action.key);
    let mapping = LedgerMap {
        schema_version: EVIDENCE_SCHEMA_VERSION,
        kind: EVIDENCE_KIND,
        bucket: "throughput",
        ledger_entry: &ledger_entry,
        plan_sha256: &pack.plan_sha256,
        request_sha256: &pack.request_sha256,
        action_key: &action.key,
    };
    evidence.ledger_entry = Some(ledger_entry.clone());
    // This checkpoint plus the map identify a potential held reservation even
    // if the process is interrupted immediately after reserve and before the
    // provider can be called.
    evidence.reservation_status = "prepared_for_reservation".into();
    overwrite_checkpoint(&args.evidence, &evidence).map_err(std::io::Error::other)?;
    write_new(&evidence_map, &mapping).map_err(std::io::Error::other)?;
    // Constructing a backend is local configuration validation. The sole
    // reservation is directly before its sole provider dispatch.
    let backend = RecoveryBackend::from_env(
        &Options {
            model: Some(action.model.clone()),
            ..Default::default()
        },
        "source-role-evaluation",
    )?;
    if let Err(error) = ledger(&args, "reserve", &ledger_entry, action.reservation_usd) {
        evidence.status = "reservation_rejected".into();
        evidence.reservation_status = "not_reserved".into();
        evidence.error = Some(error);
        overwrite_checkpoint(&args.evidence, &evidence).map_err(std::io::Error::other)?;
        return Err("evaluation ledger rejected dispatch".into());
    }
    // Reserve immediately before this sole call. The pre-reserve checkpoint and
    // mapping above make an interruption here conservatively auditable.
    evidence.reservation_status = "reserved".into();

    let provider_started = std::time::Instant::now();
    let dispatched = backend.recovery_call(&action).await;
    let provider_elapsed_ms =
        u64::try_from(provider_started.elapsed().as_millis()).unwrap_or(u64::MAX);
    let raw_usage = backend.recovery_usage(&action.key);
    match dispatched {
        Ok((payload, raw_usage_normalized, finish_reason)) => {
            let usage = (raw_usage_normalized != Usage::default()).then_some(raw_usage_normalized);
            evidence.usage = usage.clone();
            evidence.raw_usage = raw_usage;
            evidence.response = payload.clone();
            evidence.response_sha256 = payload
                .as_ref()
                .map(canonical_hash)
                .transpose()
                .map_err(std::io::Error::other)?;
            evidence.provider_finish_reason = finish_reason.clone();
            evidence.provider_elapsed_ms = Some(provider_elapsed_ms);
            // The result and its billed usage are durable before any optional
            // importer/output artifact can fail.
            evidence.status = "response_received".into();
            overwrite_checkpoint(&args.evidence, &evidence).map_err(std::io::Error::other)?;
            let truncated = matches!(finish_reason.as_deref(), Some("length" | "max_tokens"));
            if truncated {
                evidence.status = "rejected_truncated".into();
                evidence.error = Some("provider response reached its output limit".into());
            } else if let Some(payload) = payload {
                if let Err(error) = write_new(&response_path, &payload) {
                    evidence.status = "response_evidence_write_failed".into();
                    evidence.error = Some(format!("could not persist response sidecar: {error}"));
                } else {
                    match import_response(&args, &response_path, &assignments_path) {
                        Ok(()) => match fs::read(&assignments_path) {
                            Ok(assignments) => {
                                evidence.status = "imported_unverified".into();
                                evidence.importer = Some(ImporterEvidence {
                                    response_path,
                                    assignments_sha256: Some(sha256(&assignments)),
                                    assignments_path,
                                    accepted: true,
                                    error: None,
                                });
                            }
                            Err(error) => {
                                evidence.status = "assignment_evidence_read_failed".into();
                                evidence.error = Some(format!(
                                    "strict importer output could not be read: {error}"
                                ));
                            }
                        },
                        Err(error) => {
                            evidence.status = "rejected_import".into();
                            evidence.error = Some(error.clone());
                            evidence.importer = Some(ImporterEvidence {
                                response_path,
                                assignments_path,
                                assignments_sha256: None,
                                accepted: false,
                                error: Some(error),
                            });
                        }
                    }
                }
            } else {
                evidence.status = "rejected_empty".into();
                evidence.error = Some("provider returned no structured payload".into());
            }
            let (settlement_status, settlement_error) = settle_or_hold(
                &args,
                &ledger_entry,
                action.reservation_usd,
                evidence.usage.as_ref(),
            );
            evidence.settlement_status = Some(settlement_status);
            if evidence.error.is_none() {
                evidence.error = settlement_error;
            }
        }
        Err(failure) => {
            // Do not serialize transport details: provider errors can contain
            // operational URLs. The durable reservation remains auditable.
            evidence.status = "provider_error".into();
            evidence.error = Some("provider request failed".into());
            evidence.raw_usage = raw_usage;
            evidence.provider_failure = Some(provider_failure_evidence(&failure));
            evidence.usage = (failure.usage != Usage::default()).then_some(failure.usage);
            evidence.provider_elapsed_ms = Some(provider_elapsed_ms);
            overwrite_checkpoint(&args.evidence, &evidence).map_err(std::io::Error::other)?;
            let (settlement_status, settlement_error) = settle_or_hold(
                &args,
                &ledger_entry,
                action.reservation_usd,
                evidence.usage.as_ref(),
            );
            evidence.settlement_status = Some(settlement_status);
            if let Some(error) = settlement_error {
                evidence.error = Some(format!("provider request failed; {error}"));
            }
        }
    }
    overwrite_checkpoint(&args.evidence, &evidence).map_err(std::io::Error::other)?;
    println!(
        "{}",
        json!({
            "dispatch": true,
            "status": evidence.status,
            "settlement_status": evidence.settlement_status,
            "checkpoint": args.evidence,
            "verified": false,
        })
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan_with_request(request: Value, coordinate_map: Value) -> Value {
        let request_sha256 = canonical_hash(&request).unwrap();
        let payload = json!({"request_sha256": request_sha256, "request": request});
        json!({
            "schema_version": 1,
            "kind": "source-role-plan-v1",
            "authority": "NONAUTHORITATIVE",
            "epub_sha256": "e",
            "profile_sha256": "p",
            "contract_sha256": "c",
            "request_packs": [{
                "pack_id": "p0",
                "request_sha256": payload["request_sha256"],
                "provider_payload_utf8_bytes": canonical_json(&payload).unwrap().len(),
                "provider_payload": payload,
                "coordinate_map": coordinate_map,
            }],
        })
    }

    fn pack() -> ValidatedPack {
        let coordinate_map =
            json!({"l0": {"original_chunk_index": 1, "line_index": 2, "document_line": 3}});
        let request = json!({
            "instructions": "Treat source as evidence.",
            "response_schema": {"type": "object"},
            "regions": [{"id": "r0", "context": [{"id": "l0", "source_fragments": [{"id": "l0f0", "text": "synthetic line"}]}], "unknown_ids": ["l0"]}],
            "epub_sha256": "e", "profile_sha256": "p", "contract_sha256": "c",
            "coordinate_map_sha256": canonical_hash(&coordinate_map).unwrap(),
        });
        ValidatedPack {
            plan_sha256: "plan".into(),
            epub_sha256: "e".into(),
            profile_sha256: "p".into(),
            contract_sha256: "c".into(),
            pack_id: "p0".into(),
            request_sha256: canonical_hash(&request).unwrap(),
            coordinate_map_sha256: canonical_hash(&coordinate_map).unwrap(),
            provider_payload_sha256: "payload".into(),
            provider_payload_bytes: 1,
            request,
            source_bytes: "synthetic line".len(),
            unknown_ids: vec!["l0".into()],
        }
    }

    #[test]
    fn action_identity_changes_with_request_or_cap() {
        let source = pack();
        let action = action_for_pack(&source, "gemini-2.5-flash", 6_000).unwrap();
        let cap_changed = action_for_pack(&source, "gemini-2.5-flash", 6_001).unwrap();
        let mut request_changed = source.clone();
        request_changed.request["regions"][0]["context"][0]["source_fragments"][0]["text"] =
            json!("synthetic changed line");
        request_changed.request_sha256 = canonical_hash(&request_changed.request).unwrap();
        let changed = action_for_pack(&request_changed, "gemini-2.5-flash", 6_000).unwrap();
        assert_ne!(action.key, cap_changed.key);
        assert_ne!(action.key, changed.key);
        assert!(action.reservation_usd.is_finite() && action.reservation_usd > 0.0);
    }

    #[test]
    fn canonical_hash_matches_python_planner_utf8_form() {
        let value = json!({"b": [1, {"z": false}], "a": "é"});
        assert_eq!(
            canonical_hash(&value).unwrap(),
            "7b3fab7436a2b5ed18015c3fb67a749ebcee92a2795f68a39b38b7f6fafbf9b0"
        );
    }

    #[test]
    fn provider_failure_evidence_keeps_http_status_without_provider_details() {
        for status in [401, 404, 429] {
            let failure = CallFailure::transport(EpubError::Api {
                status,
                body: "https://gateway.example/v1/secret?token=do-not-record".into(),
            });
            let evidence = provider_failure_evidence(&failure);
            assert_eq!(evidence.category, "api");
            assert_eq!(evidence.http_status, Some(status));
            assert!(!evidence.retryable);
            assert!(!evidence.truncated);
            let serialized = serde_json::to_string(&evidence).unwrap();
            assert!(!serialized.contains("gateway.example"));
            assert!(!serialized.contains("do-not-record"));
        }
    }

    #[test]
    fn provider_failure_evidence_sanitizes_transport_and_retains_retry_metadata() {
        let transport = CallFailure::transport(EpubError::Proxy(
            "https://gateway.example/secret?token=do-not-record".into(),
        ));
        let transport_evidence = provider_failure_evidence(&transport);
        assert_eq!(transport_evidence.category, "transport");
        assert_eq!(transport_evidence.http_status, None);
        assert!(
            !serde_json::to_string(&transport_evidence)
                .unwrap()
                .contains("do-not-record")
        );

        let decode = CallFailure::retryable_payload(
            EpubError::Deserialize(serde_json::from_str::<Value>("{").unwrap_err()),
            Usage::default(),
            true,
        );
        let decode_evidence = provider_failure_evidence(&decode);
        assert_eq!(decode_evidence.category, "decode");
        assert!(decode_evidence.retryable);
        assert!(decode_evidence.truncated);
    }

    #[test]
    fn coordinate_map_must_cover_every_context_id() {
        let plan = json!({
            "schema_version": 1, "kind": "source-role-plan-v1", "authority": "NONAUTHORITATIVE",
            "epub_sha256": "e", "profile_sha256": "p", "contract_sha256": "c",
            "request_packs": [{"pack_id": "p0", "request_sha256": "stale", "provider_payload_utf8_bytes": 0, "provider_payload": {"request_sha256": "stale", "request": {}}, "coordinate_map": {}}],
        });
        assert!(validate_pack(&serde_json::to_vec(&plan).unwrap(), "p0").is_err());
    }

    #[test]
    fn validates_fragmented_targets_and_whole_context_once() {
        let coordinate_map = json!({
            "l0": {"original_chunk_index": 1, "line_index": 2, "document_line": 3},
            "l1": {"original_chunk_index": 1, "line_index": 3, "document_line": 4},
        });
        let request = json!({
            "instructions": "Treat source as evidence.",
            "response_schema": {"type": "object"},
            "regions": [{
                "id": "r0",
                "context": [
                    {"id": "l0", "source_fragments": [
                        {"id": "l0f0", "text": "First "},
                        {"id": "l0f1", "text": "café"},
                    ]},
                    {"id": "l1", "source": "whole context"},
                ],
                "unknown_ids": ["l0"],
            }],
            "epub_sha256": "e", "profile_sha256": "p", "contract_sha256": "c",
            "coordinate_map_sha256": canonical_hash(&coordinate_map).unwrap(),
        });
        let plan = plan_with_request(request, coordinate_map);
        let validated = validate_pack(&serde_json::to_vec(&plan).unwrap(), "p0").unwrap();
        assert_eq!(validated.unknown_ids, vec!["l0"]);
        assert_eq!(validated.source_bytes, "First caféwhole context".len());
    }
}
