//! Run a bounded, cold-cache source cohort through automatic recovery.
//!
//! This is evaluation-only infrastructure. It never dispatches by default:
//! preparation re-inspects the saved EPUB, binds its current source documents,
//! and writes a source-free action manifest. `--dispatch` is required before a
//! ledger reservation, provider configuration, cache directory mutation, or
//! native transport call can occur.

use recipe_epub::{
    Options, RecoveryBackend, Usage,
    experiment::Ledger,
    hybrid::HybridStrategy,
    recovery::{Action, Adapter, Reply, State, run},
    review::ReviewRun,
};
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, HashSet},
    env, fs,
    io::Write,
    path::{Path, PathBuf},
    rc::Rc,
    time::Instant,
};

const PREPARE_SCHEMA_VERSION: u32 = 4;
const PREPARE_KIND: &str = "paid-throughput-evaluation-prepare-v4";

#[derive(Clone)]
struct Args {
    run: PathBuf,
    audit_state: Option<PathBuf>,
    correct: bool,
    chunks: Vec<usize>,
    ledger: Option<PathBuf>,
    checkpoint: Option<PathBuf>,
    cold_cache: Option<PathBuf>,
    prepare_out: PathBuf,
    model: String,
    strategy: HybridStrategy,
    concurrency: usize,
    dispatch: bool,
}

#[derive(Serialize)]
struct LedgerMapping<'a> {
    bucket: &'static str,
    original_chunk_indices: &'a [usize],
    selected_original_chunk_indices: &'a [usize],
    source_chunks: Vec<SourceChunkMapping<'a>>,
    ledger_entries: &'a HashMap<String, String>,
}

#[derive(Serialize)]
struct SourceChunkMapping<'a> {
    source_index: usize,
    original_chunk_index: usize,
    original_chunk_id: &'a str,
}

#[derive(Serialize)]
struct PreparedCohort {
    schema_version: u32,
    kind: &'static str,
    dispatch_requested: bool,
    operation: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    audit_parent_sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    audit_correct: Option<bool>,
    /// Source-free evidence of offline child assembly changes, separate from
    /// paid audit results. Full before/after evidence lives in the checkpoint.
    assembly_migrations: Vec<PreparedAssemblyMigration>,
    source_identity: SourceIdentity,
    policy: String,
    strategy: HybridStrategy,
    models: Vec<String>,
    trial_verifiers: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    automatic_branches: Option<Vec<AutomaticBranch>>,
    max_attempts: usize,
    concurrency: usize,
    context_chunk_count: usize,
    selected_chunks: Vec<PreparedSourceChunk>,
    groups: Vec<PreparedGroup>,
    immediate_actions: Vec<PreparedAction>,
    initial_wave_groups: Vec<usize>,
    initial_wave_reservation_usd: f64,
    preparation_ms: u64,
}

#[derive(Serialize)]
struct PreparedAssemblyMigration {
    group: usize,
    candidate: usize,
    candidate_revision: u64,
    history_entries: usize,
    outputs_sha256: String,
}

#[derive(Serialize)]
struct SourceIdentity {
    saved_run_sha256: String,
    epub_sha256: String,
    selected_source_sha256: String,
    context_source_sha256: String,
    document_set_sha256: String,
    source_line_provenance_sha256: Option<String>,
    catalog_sha256: String,
}

#[derive(Serialize)]
struct AutomaticBranch {
    extraction_model: String,
    extraction_output_limit: u32,
    established_verifier_model: String,
    established_verifier_output_limit: u32,
}

#[derive(Serialize)]
struct PreparedSourceChunk {
    source_index: usize,
    original_chunk_index: usize,
    original_chunk_id: String,
    source_lines: usize,
    exact_provenance_lines: usize,
    raw_dom_evidence_bytes: usize,
}

#[derive(Serialize)]
struct PreparedGroup {
    group: usize,
    source_indices: Vec<usize>,
    original_chunk_indices: Vec<usize>,
    chunk_count: usize,
    seeded: bool,
}

struct PrepareContext<'a> {
    original_chunk_indices: &'a [usize],
    source_chunk_ids: &'a [String],
    saved_run_sha256: String,
    epub_sha256: String,
    audit_parent_sha256: Option<String>,
    audit_correct: bool,
}

#[derive(Serialize)]
struct PreparedAction {
    key: String,
    group: usize,
    candidate: usize,
    chunk: Option<usize>,
    verification_chunk: Option<usize>,
    verification_stage: Option<usize>,
    model: String,
    output_limit: u32,
    reservation_usd: f64,
    request_bytes: usize,
    request_sha256: String,
}

fn next_value(values: &mut impl Iterator<Item = String>, name: &str) -> Result<String, String> {
    values.next().ok_or_else(|| format!("missing {name}"))
}

fn args() -> Result<Args, String> {
    let mut values = env::args().skip(1);
    let mut run = None;
    let mut audit_state = None;
    let mut correct = false;
    let mut chunks = vec![];
    let mut ledger = None;
    let mut checkpoint = None;
    let mut cold_cache = None;
    let mut prepare_out = None;
    let mut model = recipe_epub::recovery::AUTOMATIC.to_owned();
    let mut strategy = HybridStrategy::Indexed;
    let mut concurrency = 4;
    let mut dispatch = false;
    while let Some(flag) = values.next() {
        match flag.as_str() {
            "--run" => run = Some(next_value(&mut values, "--run value")?.into()),
            "--audit-state" => {
                audit_state = Some(next_value(&mut values, "--audit-state value")?.into())
            }
            "--correct" => correct = true,
            "--chunk" => chunks.push(
                next_value(&mut values, "--chunk value")?
                    .parse()
                    .map_err(|_| "--chunk must be a zero-based integer")?,
            ),
            "--ledger" => ledger = Some(next_value(&mut values, "--ledger value")?.into()),
            "--checkpoint" => {
                checkpoint = Some(next_value(&mut values, "--checkpoint value")?.into())
            }
            "--cold-cache" => {
                cold_cache = Some(next_value(&mut values, "--cold-cache value")?.into())
            }
            "--prepare-out" => {
                prepare_out = Some(next_value(&mut values, "--prepare-out value")?.into())
            }
            "--model" => model = next_value(&mut values, "--model value")?,
            "--strategy" => {
                strategy = match next_value(&mut values, "--strategy value")?.as_str() {
                    "indexed" => HybridStrategy::Indexed,
                    "hybrid" => HybridStrategy::Hybrid,
                    _ => return Err("--strategy must be indexed or hybrid".into()),
                }
            }
            "--concurrency" => {
                concurrency = next_value(&mut values, "--concurrency value")?
                    .parse()
                    .map_err(|_| "--concurrency must be an integer")?
            }
            "--ledger-tool" => { let _ = next_value(&mut values, "--ledger-tool value")?; }
            "--dispatch" => dispatch = true,
            "--prepare-only" | "--dry-run" => dispatch = false,
            "--help" => return Err("usage: paid_throughput_evaluation --run RUN --chunk INDEX [--chunk INDEX ...] --prepare-out NEW_PRIVATE_PLAN.json [--prepare-only|--dry-run|--dispatch] [--model MODEL] [--strategy indexed|hybrid] [--audit-state PARENT_STATE --correct] [--concurrency 1..8] [--ledger LEDGER --checkpoint OUT --cold-cache EMPTY_DIRECTORY]".into()),
            other => return Err(format!("unknown option {other}")),
        }
    }
    let prepare_out = prepare_out
        .or_else(|| {
            checkpoint
                .as_ref()
                .map(|path: &PathBuf| path.with_extension("prepare.json"))
        })
        .ok_or("--prepare-out is required unless --checkpoint supplies a derived path")?;
    if dispatch
        && (ledger.is_none()
            || checkpoint.is_none()
            || (audit_state.is_none() && cold_cache.is_none()))
    {
        return Err(
            "--dispatch requires --ledger and --checkpoint; extraction also requires --cold-cache"
                .into(),
        );
    }
    Ok(Args {
        run: run.ok_or("--run is required")?,
        audit_state,
        correct,
        chunks,
        ledger,
        checkpoint,
        cold_cache,
        prepare_out,
        model,
        strategy,
        concurrency,
        dispatch,
    })
}

fn ledger(args: &Args, action: &str, entry: &str, amount: f64) -> Result<(), String> {
    let ledger_path = args
        .ledger
        .as_deref()
        .ok_or("ledger is unavailable without --dispatch")?;
    let ledger = Ledger::open(ledger_path);
    match action {
        "reserve" => ledger.reserve(
            "throughput",
            entry,
            amount,
            "cold-cache source-cohort throughput action",
        ),
        "settle" => ledger.settle("throughput", entry, amount),
        "unknown" => ledger.mark_unknown("throughput", entry, amount),
        _ => return Err("unknown ledger operation".into()),
    }
    .map_err(|error| error.to_string())
}

fn checkpoint(path: &Path, state: &State) -> Result<(), String> {
    durable_write(
        path,
        &serde_json::to_vec_pretty(state).map_err(|error| error.to_string())?,
    )
}

fn storage_parent(path: &Path) -> &Path {
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
}

/// Replace a checkpoint only after both its contents and directory entry have
/// reached stable storage. Settlement always runs after this returns.
fn durable_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = storage_parent(path);
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("checkpoint path has no filename")?;
    let temporary = (0..100)
        .map(|attempt| parent.join(format!(".{name}.{attempt}.pending")))
        .find(|candidate| !candidate.exists())
        .ok_or("could not allocate durable checkpoint temporary")?;
    let mut file = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary)
        .map_err(|error| error.to_string())?;
    let result = (|| {
        file.write_all(bytes).map_err(|error| error.to_string())?;
        file.sync_all().map_err(|error| error.to_string())?;
        fs::rename(&temporary, path).map_err(|error| error.to_string())?;
        fs::File::open(parent)
            .and_then(|directory| directory.sync_all())
            .map_err(|error| error.to_string())
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}

fn write_new(path: &Path, value: &impl Serialize) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(value).map_err(|error| error.to_string())?;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| format!("could not create preparation output: {error}"))?;
    file.write_all(&bytes).map_err(|error| error.to_string())?;
    file.sync_all().map_err(|error| error.to_string())?;
    fs::File::open(storage_parent(path))
        .and_then(|directory| directory.sync_all())
        .map_err(|error| error.to_string())
}

/// A durable ledger mapping is part of dispatch admission. The reservation is
/// intentionally not released if checkpointing that mapping fails: provider
/// usage is then unknown, so the ledger hold is the safe accounting result.
fn reserve_mapping_before_provider<T>(
    reserve: impl FnOnce() -> Result<(), String>,
    persist_mapping: impl FnOnce() -> Result<(), String>,
    initialize_provider: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    reserve()?;
    persist_mapping()?;
    initialize_provider()
}

fn save_mapping(
    path: &Path,
    selected_original_chunk_indices: &[usize],
    source_chunk_ids: &[String],
    entries: &HashMap<String, String>,
) -> Result<(), String> {
    let original_chunk_indices: Vec<_> = (0..source_chunk_ids.len()).collect();
    let source_chunks = original_chunk_indices
        .iter()
        .enumerate()
        .map(|(source_index, original_chunk_index)| SourceChunkMapping {
            source_index,
            original_chunk_index: *original_chunk_index,
            original_chunk_id: source_chunk_ids[source_index].as_str(),
        })
        .collect();
    let value = LedgerMapping {
        bucket: "throughput",
        original_chunk_indices: &original_chunk_indices,
        selected_original_chunk_indices,
        source_chunks,
        ledger_entries: entries,
    };
    durable_write(
        path,
        &serde_json::to_vec_pretty(&value).map_err(|error| error.to_string())?,
    )
}

fn nth_entry(key: &str, attempts: &[recipe_epub::recovery::Attempt], index: usize) -> String {
    let ordinal = attempts[..=index]
        .iter()
        .filter(|attempt| !attempt.inherited && attempt.key == key)
        .count();
    format!("{key}#{ordinal}")
}

fn no_cache(_: &Action) -> Option<Value> {
    None
}
fn no_cache_write(_: &Action, _: &Value) -> Result<(), String> {
    Ok(())
}

fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn serialized_sha256(value: &impl Serialize) -> Result<String, String> {
    serde_json::to_vec(value)
        .map(|bytes| sha256(&bytes))
        .map_err(|error| error.to_string())
}

fn source_chunk_matches(
    saved: &recipe_epub::review::RunChunk,
    fresh: &recipe_epub::review::RunChunk,
) -> Result<bool, String> {
    Ok(saved.id == fresh.id
        && serde_json::to_vec(&saved.source).map_err(|error| error.to_string())?
            == serde_json::to_vec(&fresh.source).map_err(|error| error.to_string())?)
}

fn validate_fresh_layout(saved: &ReviewRun, fresh: &ReviewRun) -> Result<(), String> {
    if saved.epub_sha256 != fresh.epub_sha256 || saved.chunks.len() != fresh.chunks.len() {
        return Err("fresh EPUB inspection does not match the saved run".into());
    }
    for (saved_chunk, fresh_chunk) in saved.chunks.iter().zip(&fresh.chunks) {
        if !source_chunk_matches(saved_chunk, fresh_chunk)? {
            return Err("fresh EPUB inspection changed saved source coordinates".into());
        }
    }
    Ok(())
}

fn current_inspection(saved: &ReviewRun, model: &str) -> Result<ReviewRun, String> {
    let epub =
        fs::read(&saved.source).map_err(|error| format!("could not read saved EPUB: {error}"))?;
    if recipe_epub::review::hash(&epub) != saved.epub_sha256 {
        return Err("saved run EPUB hash does not match its current source file".into());
    }
    let inspection_model = if model == recipe_epub::recovery::AUTOMATIC {
        recipe_epub::recovery::ORDER[0]
    } else {
        model
    };
    let fresh = ReviewRun::inspect(&epub, &saved.source, inspection_model)
        .map_err(|error| format!("could not inspect current EPUB: {error}"))?;
    validate_fresh_layout(saved, &fresh)?;
    Ok(fresh)
}

fn selected_chunks<'a>(
    fresh: &'a ReviewRun,
    indices: &[usize],
) -> Result<Vec<&'a recipe_epub::review::RunChunk>, String> {
    indices
        .iter()
        .map(|&index| {
            fresh
                .chunks
                .get(index)
                .ok_or_else(|| "chunk is out of range".to_owned())
        })
        .collect()
}

/// Mirrors the established (non-trial) branch in `State::verifier_models`.
/// The manifest carries policy metadata only; it does not construct a
/// candidate-dependent verifier request or reservation.
fn established_verifier(extraction_model: &str) -> &'static str {
    if extraction_model.starts_with("@cf/zai-org/glm")
        || extraction_model.starts_with("@cf/moonshotai/kimi")
    {
        "gemini-2.5-flash"
    } else if extraction_model.starts_with("gemini-") {
        "@cf/zai-org/glm-5.3"
    } else {
        "gemini-2.5-flash"
    }
}

fn automatic_branches(models: &[String]) -> Result<Vec<AutomaticBranch>, String> {
    models
        .iter()
        .map(|extraction_model| {
            let established_verifier_model = established_verifier(extraction_model);
            Ok(AutomaticBranch {
                extraction_model: extraction_model.clone(),
                extraction_output_limit: recipe_epub::recovery::output_limit(
                    extraction_model,
                    false,
                )?,
                established_verifier_model: established_verifier_model.into(),
                established_verifier_output_limit: recipe_epub::recovery::output_limit(
                    established_verifier_model,
                    true,
                )?,
            })
        })
        .collect()
}

fn planned_action(action: &Action) -> Result<PreparedAction, String> {
    let request = serde_json::to_vec(&action.request).map_err(|error| error.to_string())?;
    Ok(PreparedAction {
        key: action.key.clone(),
        group: action.group,
        candidate: action.candidate,
        chunk: action.chunk,
        verification_chunk: action.verification_chunk,
        verification_stage: action.verification_stage,
        model: action.model.clone(),
        output_limit: action.output_limit,
        reservation_usd: action.reservation_usd,
        request_bytes: request.len(),
        request_sha256: sha256(&request),
    })
}

fn prepare_cohort(
    state: &State,
    context: PrepareContext<'_>,
    dispatch_requested: bool,
    concurrency: usize,
    preparation_ms: u64,
) -> Result<PreparedCohort, String> {
    let PrepareContext {
        original_chunk_indices,
        source_chunk_ids,
        saved_run_sha256,
        epub_sha256,
        audit_parent_sha256,
        audit_correct,
    } = context;
    let actions = state.planned_actions()?;
    let immediate_actions = actions
        .iter()
        .map(planned_action)
        .collect::<Result<Vec<_>, _>>()?;
    let mut seen_groups = HashSet::new();
    let first_wave = actions
        .iter()
        .filter(|action| seen_groups.insert(action.group))
        .take(concurrency)
        .collect::<Vec<_>>();
    let initial_wave_groups = first_wave.iter().map(|action| action.group).collect();
    let initial_wave_reservation_usd = first_wave.iter().map(|action| action.reservation_usd).sum();
    let selected_chunks = original_chunk_indices
        .iter()
        .map(|original_chunk_index| {
            let source_index = *original_chunk_index;
            let evidence = recipe_epub::source::chunk_source_evidence(
                &state.source[source_index],
                state
                    .source_line_provenance
                    .get(source_index)
                    .map(Vec::as_slice),
                &state.documents,
            )?;
            Ok(PreparedSourceChunk {
                source_index,
                original_chunk_index: *original_chunk_index,
                original_chunk_id: source_chunk_ids[source_index].clone(),
                source_lines: state.source[source_index].text.lines().count(),
                exact_provenance_lines: evidence
                    .lines
                    .iter()
                    .filter(|line| line.provenance == "indexed" && line.match_state == "exact")
                    .count(),
                raw_dom_evidence_bytes: serde_json::to_vec(&evidence.request_projection())
                    .map_err(|error| error.to_string())?
                    .len(),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let groups = state
        .groups
        .iter()
        .enumerate()
        .filter(|(_, group)| group.enabled)
        .map(|(group, value)| PreparedGroup {
            group,
            source_indices: value.chunks.clone(),
            original_chunk_indices: value.chunks.clone(),
            chunk_count: value.chunks.len(),
            seeded: value
                .candidates
                .iter()
                .any(|candidate| candidate.seed_provenance.is_some()),
        })
        .collect();
    Ok(PreparedCohort {
        schema_version: PREPARE_SCHEMA_VERSION,
        kind: PREPARE_KIND,
        dispatch_requested,
        operation: if state.audit_only {
            "audit_only"
        } else {
            "recovery"
        },
        audit_parent_sha256,
        audit_correct: state.audit_only.then_some(audit_correct),
        assembly_migrations: state
            .groups
            .iter()
            .enumerate()
            .flat_map(|(group, value)| {
                value
                    .candidates
                    .iter()
                    .enumerate()
                    .filter(|(_, candidate)| !candidate.hybrid_assembly_migrations.is_empty())
                    .map(move |(candidate, value)| {
                        Ok(PreparedAssemblyMigration {
                            group,
                            candidate,
                            candidate_revision: value.revision,
                            history_entries: value.hybrid_assembly_migrations.len(),
                            outputs_sha256: serialized_sha256(&value.outputs)?,
                        })
                    })
            })
            .collect::<Result<Vec<_>, String>>()?,
        source_identity: SourceIdentity {
            saved_run_sha256,
            epub_sha256,
            selected_source_sha256: recipe_epub::recovery::canonical_source_sha256(
                &original_chunk_indices
                    .iter()
                    .map(|index| state.source[*index].clone())
                    .collect::<Vec<_>>(),
            )?,
            context_source_sha256: recipe_epub::recovery::canonical_source_sha256(&state.source)?,
            document_set_sha256: serialized_sha256(&state.document_hashes)?,
            source_line_provenance_sha256: state.source_line_provenance_sha256.clone(),
            catalog_sha256: serialized_sha256(&recipe_epub::models::catalog())?,
        },
        policy: state.policy.clone(),
        strategy: state.strategy,
        models: state.models.clone(),
        trial_verifiers: state.trial_verifiers,
        automatic_branches: (!state.audit_only)
            .then(|| automatic_branches(&state.models))
            .transpose()?,
        max_attempts: recipe_epub::recovery::MAX_ATTEMPTS,
        concurrency,
        context_chunk_count: state.source.len(),
        selected_chunks,
        groups,
        immediate_actions,
        initial_wave_groups,
        initial_wave_reservation_usd,
        preparation_ms,
    })
}

fn accepted_groups(state: &State) -> usize {
    state
        .groups
        .iter()
        .filter(|group| group.accepted.is_some())
        .count()
}

/// The first durable point when every selected recovery group has a fully
/// extracted candidate. Verification/audit work can continue after this.
fn primary_extraction_complete(state: &State) -> bool {
    state
        .groups
        .iter()
        .filter(|group| group.enabled)
        .all(|group| {
            group.candidates.iter().any(|candidate| {
                candidate.outputs.len() == group.chunks.len()
                    && candidate.outputs.iter().all(Option::is_some)
            })
        })
}

fn validate_audit_source_identity(state: &State, fresh: &ReviewRun) -> Result<(), String> {
    let source: Vec<_> = fresh
        .chunks
        .iter()
        .map(|chunk| chunk.source.clone())
        .collect();
    if serialized_sha256(&state.source)? != serialized_sha256(&source)? {
        return Err("audit parent source differs from fresh EPUB inspection".into());
    }
    if serialized_sha256(&state.documents)? != serialized_sha256(&fresh.documents)? {
        return Err("audit parent document evidence differs from fresh EPUB inspection".into());
    }
    if state.navigation_documents != fresh.navigation_documents {
        return Err("audit parent navigation semantics differ from fresh EPUB inspection".into());
    }
    if serialized_sha256(&state.source_line_provenance)?
        != serialized_sha256(&fresh.source_line_provenance)?
    {
        return Err(
            "audit parent source-line provenance differs from fresh EPUB inspection".into(),
        );
    }
    state.validate()
}

fn selected_groups_have_complete_candidates(state: &State) -> Result<(), String> {
    if state
        .groups
        .iter()
        .filter(|group| group.enabled)
        .all(|group| {
            group.candidates.iter().any(|candidate| {
                candidate.outputs.len() == group.chunks.len()
                    && candidate.outputs.iter().all(Option::is_some)
            })
        })
    {
        Ok(())
    } else {
        Err("audit selection requires one complete frozen candidate per group".into())
    }
}

fn inherited_ordinals<'a>(keys: impl IntoIterator<Item = &'a str>) -> HashMap<String, usize> {
    let mut ordinals = HashMap::new();
    for key in keys {
        *ordinals.entry(key.to_owned()).or_insert(0) += 1;
    }
    ordinals
}

fn audit_actions_only(actions: &[Action]) -> Result<(), String> {
    if actions.iter().any(|action| action.chunk.is_some()) {
        Err("audit-only state planned an extraction action".into())
    } else {
        Ok(())
    }
}

/// Persist recovery state and its ledger mapping before any ledger settlement.
/// An interruption or filesystem error therefore leaves the prior reservation
/// held instead of claiming a settled charge without a replayable checkpoint.
fn checkpoint_before_settlement(
    persist: impl FnOnce() -> Result<(), String>,
    settle: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    persist()?;
    settle()
}

/// Retain the entire book for linked context; limit execution without cutting
/// a source-derived recovery group or silently adding paid work.
fn select_execution_groups(state: &mut State, selected: &[usize]) -> Result<(), String> {
    if selected.is_empty() || selected.iter().any(|index| *index >= state.source.len()) {
        return Err("selected source chunks are empty or out of range".into());
    }
    for group in &mut state.groups {
        let count = group
            .chunks
            .iter()
            .filter(|index| selected.contains(index))
            .count();
        if count > 0 && count != group.chunks.len() {
            return Err(
                "cohort selection cuts a recovery group; select the complete source group".into(),
            );
        }
        group.enabled = count > 0;
    }
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let started = Instant::now();
    let args = args().map_err(std::io::Error::other)?;
    if args.chunks.len() < 8 {
        return Err("throughput cohorts require at least eight selected source chunks".into());
    }
    if args.chunks.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err("--chunk values must be unique and in original source order".into());
    }
    if !(1..=8).contains(&args.concurrency) {
        return Err("--concurrency must be in 1..=8".into());
    }

    let saved_run_bytes = fs::read(&args.run)?;
    let saved_run_sha256 = sha256(&saved_run_bytes);
    let saved_run = ReviewRun::read(&args.run)?;
    let fresh = current_inspection(&saved_run, &args.model).map_err(std::io::Error::other)?;
    selected_chunks(&fresh, &args.chunks).map_err(std::io::Error::other)?;
    let source_chunk_ids: Vec<_> = fresh.chunks.iter().map(|chunk| chunk.id.clone()).collect();
    let (mut state, audit_parent_sha256) = if let Some(parent_path) = &args.audit_state {
        if args.model == recipe_epub::recovery::AUTOMATIC {
            return Err("--audit-state requires an explicit reviewer --model".into());
        }
        let parent_bytes = fs::read(parent_path)?;
        let mut state: State = serde_json::from_slice(&parent_bytes)?;
        validate_audit_source_identity(&state, &fresh).map_err(std::io::Error::other)?;
        if state.attempts.iter().any(|attempt| attempt.pending) {
            return Err("audit parent has pending attempts with unknown completion/billing".into());
        }
        select_execution_groups(&mut state, &args.chunks).map_err(std::io::Error::other)?;
        selected_groups_have_complete_candidates(&state).map_err(std::io::Error::other)?;
        // Preserve historical spending/attempt evidence in the child state;
        // only future audit calls receive new ledger entries.
        for attempt in &mut state.attempts {
            attempt.inherited = true;
        }
        state.models = vec![args.model.clone()];
        state
            .migrate_historical_hybrid_assembly()
            .map_err(std::io::Error::other)?;
        state
            .begin_audit(args.correct)
            .map_err(std::io::Error::other)?;
        (state, Some(sha256(&parent_bytes)))
    } else {
        let source: Vec<_> = fresh
            .chunks
            .iter()
            .map(|chunk| chunk.source.clone())
            .collect();
        let mut state = State::new_with_strategy(source, &args.model, 10.0, args.strategy)?;
        select_execution_groups(&mut state, &args.chunks).map_err(std::io::Error::other)?;
        state.bind_documents(&fresh.documents)?;
        state.bind_navigation_documents(fresh.navigation_documents.clone());
        state.bind_source_line_provenance(&fresh.source_line_provenance)?;
        state.set_trial_verifiers(false);
        (state, None)
    };

    if state.audit_only {
        let actions = state.planned_actions().map_err(std::io::Error::other)?;
        audit_actions_only(&actions).map_err(std::io::Error::other)?;
    }
    let prepared = prepare_cohort(
        &state,
        PrepareContext {
            original_chunk_indices: &args.chunks,
            source_chunk_ids: &source_chunk_ids,
            saved_run_sha256,
            epub_sha256: fresh.epub_sha256.clone(),
            audit_parent_sha256,
            audit_correct: args.correct,
        },
        args.dispatch,
        args.concurrency,
        started.elapsed().as_millis() as u64,
    )
    .map_err(std::io::Error::other)?;
    write_new(&args.prepare_out, &prepared).map_err(std::io::Error::other)?;
    if !args.dispatch {
        // An explicitly requested offline child makes migrations reviewable
        // before spending. Never overwrite the source or an earlier child.
        if let Some(path) = &args.checkpoint {
            write_new(path, &state).map_err(std::io::Error::other)?;
        }
        println!("{}", serde_json::to_string_pretty(&prepared)?);
        return Ok(());
    }

    let cold_cache = args
        .cold_cache
        .as_ref()
        .ok_or("cold cache is unavailable without --dispatch")?;
    fs::create_dir_all(cold_cache)?;
    if fs::read_dir(cold_cache)?.next().is_some() {
        return Err("--cold-cache must be an empty directory".into());
    }
    let checkpoint_path = args
        .checkpoint
        .as_ref()
        .ok_or("checkpoint is unavailable without --dispatch")?;
    let remaining = serde_json::from_slice::<Value>(&fs::read(
        args.ledger
            .as_ref()
            .ok_or("ledger is unavailable without --dispatch")?,
    )?)?
    .pointer("/buckets/throughput/remaining_usd")
    .and_then(Value::as_f64)
    .filter(|amount| amount.is_finite() && *amount >= 0.0)
    .ok_or("throughput ledger has no finite remaining balance")?;
    if prepared.initial_wave_reservation_usd > remaining + 1e-9 {
        return Err("throughput ledger cannot cover the next full concurrency wave".into());
    }

    let mapping_path = checkpoint_path.with_extension("ledger-map.json");
    let entries = Rc::new(RefCell::new(HashMap::<String, String>::new()));
    let settled = Rc::new(RefCell::new(HashSet::new()));
    // New audit calls can reuse an inherited action key. Start their durable
    // ordinal after every parent occurrence so `nth_entry` finds only the
    // entry created by this child and never settles parent charges again.
    let ordinals = Rc::new(RefCell::new(inherited_ordinals(
        state
            .attempts
            .iter()
            .filter(|attempt| attempt.inherited)
            .map(|attempt| attempt.key.as_str()),
    )));
    let admission_stopped = Rc::new(Cell::new(false));
    let source_label = fresh.source.clone();
    let call_args = args.clone();
    let save_args = args;
    let final_args = save_args.clone();
    // Stable engine actions intentionally reuse their key on a cold rerun;
    // ledger entries must stay unique to this durable trial checkpoint.
    let trial_id = sha256(final_args.prepare_out.to_string_lossy().as_bytes());
    let call_entries = entries.clone();
    let call_ordinals = ordinals.clone();
    let call_stopped = admission_stopped.clone();
    let cancelled_stopped = admission_stopped.clone();
    let call_trial_id = trial_id.clone();
    let call_mapping_path = mapping_path.clone();
    let call_selected_chunks = save_args.chunks.clone();
    let call_source_chunk_ids = source_chunk_ids.clone();
    let save_entries = entries.clone();
    let save_settled = settled.clone();
    let scheduler_started = Instant::now();
    let extraction_completed_ms = Rc::new(Cell::new(None::<u64>));
    let save_extraction_completed_ms = extraction_completed_ms.clone();
    let save_scheduler_started = scheduler_started;
    let mut adapter = Adapter {
        concurrency: save_args.concurrency,
        allow_network: true,
        refresh: true,
        now: || Some(recipe_epub::review::store::now()),
        cancelled: move || cancelled_stopped.get(),
        wait: |seconds| async move { tokio::time::sleep(std::time::Duration::from_secs(seconds)).await },
        load: no_cache,
        store: no_cache_write,
        call: move |action: Action| {
            let ordinal = call_ordinals
                .borrow_mut()
                .entry(action.key.clone())
                .and_modify(|number| *number += 1)
                .or_insert(1)
                .to_owned();
            let attempt_key = format!("{}#{ordinal}", action.key);
            let entry = format!("{call_trial_id}:{attempt_key}");
            let source_label = source_label.clone();
            let mut backend = None;
            let admitted = reserve_mapping_before_provider(
                || ledger(&call_args, "reserve", &entry, action.reservation_usd),
                || {
                    call_entries
                        .borrow_mut()
                        .insert(attempt_key.clone(), entry.clone());
                    save_mapping(
                        &call_mapping_path,
                        &call_selected_chunks,
                        &call_source_chunk_ids,
                        &call_entries.borrow(),
                    )
                },
                || {
                    backend = Some(
                        RecoveryBackend::from_env(
                            &Options {
                                model: Some(action.model.clone()),
                                ..Default::default()
                            },
                            &source_label,
                        )
                        .map_err(|error| error.to_string())?,
                    );
                    Ok(())
                },
            );
            if admitted.is_err() {
                call_stopped.set(true);
            }
            async move {
                if let Err(error) = admitted {
                    return Reply {
                        error: Some(format!(
                            "throughput dispatch was not started; reservation remains held: {error}"
                        )),
                        ..Default::default()
                    };
                }
                let Some(backend) = backend else {
                    return Reply {
                        error: Some(
                            "throughput dispatch admission completed without a provider".into(),
                        ),
                        ..Default::default()
                    };
                };
                match backend.recovery_call(&action).await {
                    Ok((payload, usage, reason)) => Reply {
                        payload,
                        usage: (usage != Usage::default()).then_some(usage),
                        raw_usage: backend.recovery_usage(&action.key),
                        error: matches!(reason.as_deref(), Some("length" | "max_tokens"))
                            .then(|| "truncated response".into()),
                        failure: None,
                    },
                    Err(failure) => Reply {
                        payload: None,
                        usage: (failure.usage != Usage::default()).then_some(failure.usage),
                        raw_usage: backend.recovery_usage(&action.key),
                        error: Some(failure.error.to_string()),
                        failure: match failure.error {
                            recipe_epub::EpubError::Request(details) => Some(*details),
                            _ => None,
                        },
                    },
                }
            }
        },
        save: move |state: &State| {
            checkpoint_before_settlement(
                || {
                    checkpoint(
                        save_args
                            .checkpoint
                            .as_deref()
                            .ok_or("checkpoint is unavailable without --dispatch")?,
                        state,
                    )?;
                    save_mapping(
                        &mapping_path,
                        &save_args.chunks,
                        &source_chunk_ids,
                        &save_entries.borrow(),
                    )
                },
                || {
                    for (index, attempt) in state
                        .attempts
                        .iter()
                        .enumerate()
                        .filter(|(_, attempt)| !attempt.inherited && !attempt.pending)
                    {
                        let attempt_key = nth_entry(&attempt.key, &state.attempts, index);
                        let entry = save_entries.borrow().get(&attempt_key).cloned();
                        if let Some(entry) =
                            entry.filter(|entry| save_settled.borrow_mut().insert(entry.clone()))
                        {
                            match attempt.estimated_usd {
                                Some(known) => ledger(&save_args, "settle", &entry, known)?,
                                None => {
                                    ledger(&save_args, "unknown", &entry, attempt.reservation_usd)?
                                }
                            }
                        }
                    }
                    Ok(())
                },
            )?;
            if save_extraction_completed_ms.get().is_none() && primary_extraction_complete(state) {
                save_extraction_completed_ms
                    .set(Some(save_scheduler_started.elapsed().as_millis() as u64));
            }
            Ok(())
        },
    };
    run(&mut state, &mut adapter)
        .await
        .map_err(std::io::Error::other)?;
    checkpoint(
        final_args
            .checkpoint
            .as_deref()
            .ok_or("checkpoint is unavailable without --dispatch")?,
        &state,
    )
    .map_err(std::io::Error::other)?;
    if extraction_completed_ms.get().is_none() && primary_extraction_complete(&state) {
        extraction_completed_ms.set(Some(scheduler_started.elapsed().as_millis() as u64));
    }
    println!(
        "{}",
        serde_json::json!({
            "complete": state.complete(),
            "accepted_groups": accepted_groups(&state),
            "attempts": state.attempts.len(),
            "budget_constrained": admission_stopped.get(),
            "initial_wave_reservation_usd": prepared.initial_wave_reservation_usd,
            "preparation_ms": prepared.preparation_ms,
            "primary_extraction_completion_ms": extraction_completed_ms.get(),
            "full_recovery_elapsed_ms": scheduler_started.elapsed().as_millis() as u64,
            "full_elapsed_ms": started.elapsed().as_millis() as u64,
            "quality": "pending_offline_source_evaluation",
            "fully_accepted": state.complete(),
            "checkpoint": final_args.checkpoint,
        })
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use recipe_epub::{Chunk, Usage, review::RunChunk};

    fn chunk(id: &str, text: &str) -> RunChunk {
        RunChunk {
            id: id.into(),
            source: Chunk {
                title_hint: None,
                text: text.into(),
                doc_path: "source.xhtml".into(),
                links: vec![],
                images: vec![],
            },
            output: None,
            error: None,
            cached: false,
            usage: Usage::default(),
            model: None,
            prompt_version: None,
            request_identity: None,
        }
    }

    fn run(chunks: Vec<RunChunk>) -> ReviewRun {
        ReviewRun {
            recovery: None,
            execution_status: None,
            metadata: None,
            charges: vec![],
            version: recipe_epub::review::RUN_VERSION,
            epub_sha256: "a".repeat(64),
            source: "/private/book.epub".into(),
            model: "gemini-2.5-flash".into(),
            prompt_version: "test".into(),
            parent: None,
            chunks,
            documents: vec![],
            navigation_documents: Default::default(),
            hybrid_audits: vec![],
            source_line_provenance: vec![],
            source_line_provenance_sha256: None,
            recipes: vec![],
            parsed: Value::Array(vec![]),
            reserved_usd: 0.0,
            image_text: None,
        }
    }

    #[test]
    fn fresh_layout_requires_exact_saved_chunk_coordinates() {
        let saved = run(vec![chunk("first", "One"), chunk("second", "Two")]);
        let fresh = run(vec![chunk("first", "One"), chunk("second", "Changed")]);
        assert!(validate_fresh_layout(&saved, &fresh).is_err());
    }

    #[test]
    fn checkpoint_failure_does_not_settle_a_reserved_action() {
        let events = RefCell::new(Vec::new());
        let result = checkpoint_before_settlement(
            || {
                events.borrow_mut().push("checkpoint");
                Err("disk failure".into())
            },
            || {
                events.borrow_mut().push("settle");
                Ok(())
            },
        );
        assert!(result.is_err());
        assert_eq!(*events.borrow(), ["checkpoint"]);
    }

    #[test]
    fn checkpoint_precedes_settlement() {
        let events = RefCell::new(Vec::new());
        checkpoint_before_settlement(
            || {
                events.borrow_mut().push("checkpoint");
                Ok(())
            },
            || {
                events.borrow_mut().push("settle");
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(*events.borrow(), ["checkpoint", "settle"]);
    }

    #[test]
    fn reservation_mapping_is_durable_before_provider_initialization() {
        let events = RefCell::new(Vec::new());
        reserve_mapping_before_provider(
            || {
                events.borrow_mut().push("reserve");
                Ok(())
            },
            || {
                events.borrow_mut().push("mapping");
                Ok(())
            },
            || {
                events.borrow_mut().push("provider");
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(*events.borrow(), ["reserve", "mapping", "provider"]);
    }

    #[test]
    fn mapping_failure_blocks_provider_and_keeps_reservation_unreleased() {
        let events = RefCell::new(Vec::new());
        let result = reserve_mapping_before_provider(
            || {
                events.borrow_mut().push("reserve");
                Ok(())
            },
            || {
                events.borrow_mut().push("mapping");
                Err("mapping disk failure".into())
            },
            || {
                events.borrow_mut().push("provider");
                Ok(())
            },
        );
        assert!(result.is_err());
        assert_eq!(
            *events.borrow(),
            ["reserve", "mapping"],
            "the caller retains the successful reservation; this seam never releases it"
        );
    }

    #[test]
    fn storage_parent_uses_current_directory_for_bare_checkpoint_names() {
        assert_eq!(storage_parent(Path::new("checkpoint.json")), Path::new("."));
        assert_eq!(
            storage_parent(Path::new("/tmp/throughput/checkpoint.json")),
            Path::new("/tmp/throughput")
        );
    }

    #[test]
    fn child_ledger_ordinals_exclude_inherited_attempts_with_the_same_key() {
        let mut state = State::new(
            vec![chunk("first", "Soup\n1 cup water\nCook.").source],
            "gemini-2.5-flash",
            10.0,
        )
        .unwrap();
        let action = state.next_action().unwrap().unwrap();
        state.reserve(&action).unwrap();
        let mut fresh = state.attempts[0].clone();
        state.attempts[0].inherited = true;
        state.attempts[0].pending = false;
        fresh.inherited = false;
        state.attempts.push(fresh);
        assert_eq!(
            nth_entry(&action.key, &state.attempts, 1),
            format!("{}#1", action.key)
        );
    }

    #[test]
    fn cohort_retains_context_without_scheduling_outside_work() {
        let source = (0..3)
            .map(|index| {
                let mut source = chunk("source", "Copyright").source;
                source.doc_path = format!("doc-{index}.xhtml");
                source
            })
            .collect();
        let mut state = State::new(source, "gemini-2.5-flash", 10.0).unwrap();
        select_execution_groups(&mut state, &[1]).unwrap();
        assert_eq!(state.source.len(), 3);
        let actions = state.planned_actions().unwrap();
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].chunk, Some(1));
        let action = state.next_action().unwrap().unwrap();
        assert_eq!(action.key, actions[0].key);
        state
            .apply(&action, serde_json::json!({"recipes": [], "ignored": [0]}))
            .unwrap();
        let verification = state.planned_actions().unwrap();
        assert_eq!(verification.len(), 1);
        assert_eq!(verification[0].verification_chunk, Some(1));
        let request: Value = serde_json::from_str(&verification[0].request.user).unwrap();
        assert!(
            request["source"]
                .as_array()
                .unwrap()
                .iter()
                .any(|source| source["chunk"] == 2)
        );
        let ids = ["first".into(), "selected".into(), "last".into()];
        let manifest = prepare_cohort(
            &state,
            PrepareContext {
                original_chunk_indices: &[1],
                source_chunk_ids: &ids,
                saved_run_sha256: "saved".into(),
                epub_sha256: "epub".into(),
                audit_parent_sha256: None,
                audit_correct: false,
            },
            false,
            1,
            0,
        )
        .unwrap();
        assert_eq!(manifest.context_chunk_count, 3);
        assert_eq!(manifest.groups.len(), 1);
        assert_eq!(manifest.selected_chunks[0].source_index, 1);
        assert_eq!(manifest.selected_chunks[0].original_chunk_id, "selected");
        assert_ne!(
            manifest.source_identity.context_source_sha256,
            manifest.source_identity.selected_source_sha256
        );
    }

    #[test]
    fn cohort_rejects_partial_continuation_groups() {
        let source = chunk("source", "Copyright").source;
        let mut state = State::new(vec![source.clone(), source], "gemini-2.5-flash", 10.0).unwrap();
        assert!(select_execution_groups(&mut state, &[1]).is_err());
        select_execution_groups(&mut state, &[0, 1]).unwrap();
        assert!(state.groups[0].enabled);
        assert!(select_execution_groups(&mut state, &[2]).is_err());
    }

    #[test]
    fn preparation_manifest_binds_documents_without_serializing_source_prose() {
        let source = Chunk {
            title_hint: None,
            text: "private recipe text".into(),
            doc_path: "source.xhtml".into(),
            links: vec![],
            images: vec![],
        };
        let mut state = State::new(vec![source], "gemini-2.5-flash", 10.0).unwrap();
        state
            .bind_documents(&[recipe_epub::source::SourceDocument {
                path: "source.xhtml".into(),
                anchors: vec![],
                images: vec![],
                blocks: vec![],
            }])
            .unwrap();
        let original_chunk_indices = [0];
        let source_chunk_ids = ["original-chunk".into()];
        let manifest = prepare_cohort(
            &state,
            PrepareContext {
                original_chunk_indices: &original_chunk_indices,
                source_chunk_ids: &source_chunk_ids,
                saved_run_sha256: "b".repeat(64),
                epub_sha256: "c".repeat(64),
                audit_parent_sha256: None,
                audit_correct: false,
            },
            false,
            1,
            0,
        )
        .unwrap();
        let encoded = serde_json::to_string(&manifest).unwrap();
        assert!(!encoded.contains("private recipe text"));
        assert_eq!(manifest.selected_chunks[0].original_chunk_index, 0);
        assert!(!manifest.source_identity.document_set_sha256.is_empty());
        assert_eq!(manifest.immediate_actions.len(), 1);
        assert!(!manifest.trial_verifiers);
        assert_eq!(manifest.strategy, HybridStrategy::Indexed);
        assert_eq!(manifest.automatic_branches.as_ref().unwrap().len(), 1);
    }

    #[test]
    fn audit_parent_rejects_changed_fresh_source() {
        let state =
            State::new(vec![chunk("first", "One").source], "gemini-2.5-flash", 10.0).unwrap();
        let fresh = run(vec![chunk("first", "Changed")]);
        assert!(validate_audit_source_identity(&state, &fresh).is_err());
    }

    #[test]
    fn audit_mode_plans_no_extraction_actions() {
        let mut state = State::new_with_strategy(
            vec![chunk("first", "Soup\n1 cup water\nCook.").source],
            "gemini-2.5-flash",
            10.0,
            HybridStrategy::Hybrid,
        )
        .unwrap();
        let extraction = state.next_action().unwrap().unwrap();
        state
            .apply(
                &extraction,
                serde_json::json!({"recipes":[{"title":{"text":"Soup","spans":[{"start":0,"end":0}]},"sections":[{"name":{"text":"","spans":[]},"ingredients":[{"start":1,"end":1}],"instructions":[{"start":2,"end":2}]}]}],"ignored":[]}),
            )
            .unwrap();
        state.begin_audit(false).unwrap();
        let actions = state.planned_actions().unwrap();
        assert!(!actions.is_empty());
        assert!(audit_actions_only(&actions).is_ok());
    }

    #[test]
    fn inherited_ordinals_start_new_key_after_parent_attempts() {
        let mut ordinals = inherited_ordinals(["same", "same", "other"]);
        let next = ordinals
            .entry("same".into())
            .and_modify(|number| *number += 1)
            .or_insert(1);
        assert_eq!(*next, 3);
        assert_eq!(ordinals["other"], 1);
    }
}
