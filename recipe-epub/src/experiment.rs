//! Native, file-backed cookbook experiment support.
//!
//! This module deliberately keeps experiment inputs and evidence outside an
//! ordinary extraction run.  A manifest freezes requests before a provider is
//! configured, and the ledger is the only authority which admits a dispatch.
//! It is native-only because durable files, locks, and the transport are not a
//! part of the portable EPUB contract.

use crate::{
    ChunkRequest, ExtractionStats, IndexedChunk, Options, RecoveryBackend, Usage,
    recovery::{Action, RequestTelemetry, State},
};
use fs2::FileExt;
use futures::{StreamExt, stream};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    time::{Instant, SystemTime, UNIX_EPOCH},
};

mod audit;
mod frozen;
pub use audit::{
    AuditExperimentPrepareRequest, AuditExperimentRunRequest, prepare_audit_experiment,
    run_audit_experiment,
};

/// Evaluate a hash-bound frozen v1/v3 source-role bundle against the current
/// selected recovery candidates. This is intentionally separate from generic
/// recipe expectations because the frozen artifacts own source coordinates and
/// role allowances that generic labels cannot express.
pub fn evaluate_frozen_audit(
    state: &State,
    epub_sha256: &str,
    bundle: &Value,
    audit_complete: bool,
) -> Result<Value> {
    frozen::evaluate(state, epub_sha256, bundle, audit_complete)
}

/// Construct the hash-bound raw v1/v3 expectation bundle consumed by
/// [`evaluate_frozen_audit`].
pub fn frozen_audit_bundle(
    source_mapping: Value,
    expectations_v1: String,
    expectations_v3: String,
) -> Value {
    frozen::bundle(source_mapping, expectations_v1, expectations_v3)
}

pub const EXTRACTION_COMPARISON_KIND: &str = "extraction-contract-comparison-prepare-v3";
pub const LEGACY_EXTRACTION_COMPARISON_KIND: &str = "extraction-contract-comparison-prepare-v2";
pub const PRIMARY_MODEL: &str = "claude-haiku-4-5";
pub const DEFAULT_CONCURRENCY: usize = 4;
const OUTPUT_LIMIT: u32 = 16_000;

#[derive(Debug, thiserror::Error)]
pub enum ExperimentError {
    #[error("experiment I/O: {0}")]
    Io(#[from] std::io::Error),
    #[error("experiment JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("experiment rejected: {0}")]
    Rejected(String),
    #[error("experiment provider setup: {0}")]
    Provider(#[from] crate::EpubError),
}

type Result<T> = std::result::Result<T, ExperimentError>;
type ManifestFiles = Vec<(String, Vec<u8>)>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExperimentContract {
    Legacy,
    Indexed,
    /// This is admitted only when the caller has placed hybrid requests in a
    /// manifest using the same frozen-source rules as the other contracts.
    Hybrid,
}

impl ExperimentContract {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Legacy => "legacy",
            Self::Indexed => "indexed",
            Self::Hybrid => "hybrid",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ExperimentPrepareRequest {
    /// A saved recovery state; it contains the frozen candidate groups.
    pub state: PathBuf,
    /// Current indexed inspection.  Supplying it rebuilds and verifies the
    /// full source identity before requests are written.
    pub indexed_source: Option<PathBuf>,
    /// Independently authored expectations frozen before any provider setup.
    pub expectations: PathBuf,
    /// A new, empty private output directory.
    pub output_dir: PathBuf,
}

#[derive(Debug, Clone)]
pub struct ExperimentRunRequest {
    pub manifest: PathBuf,
    pub indexed_source: PathBuf,
    pub contract: ExperimentContract,
    /// A new, empty private evidence directory.
    pub output_dir: PathBuf,
    pub ledger: PathBuf,
    /// Frozen expectations are identity evidence only.  This module does not
    /// relabel or mutate them to make an extraction look better.
    pub expectations: PathBuf,
    /// Network calls are impossible unless this is explicitly true.
    pub dispatch: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExperimentManifest {
    pub path: PathBuf,
    pub value: Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExperimentRun {
    pub preparation: Value,
    pub result: Option<Value>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LedgerStatus {
    pub path: PathBuf,
    pub sha256: String,
    pub buckets: BTreeMap<String, LedgerBucketStatus>,
    pub entries: Vec<LedgerEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LedgerBucketStatus {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cap_usd: Option<f64>,
    pub known_usd: f64,
    pub reserved_usd: f64,
    pub remaining_usd: f64,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// Kept schema-compatible with `tools/cookbook_paid_evaluation.py`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LedgerEntry {
    pub id: String,
    pub bucket: String,
    pub reserved_usd: f64,
    pub status: String,
    pub reserved_at: String,
    #[serde(default)]
    pub purpose: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub known_usd: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub settled_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unknown_at: Option<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct LedgerFile {
    buckets: BTreeMap<String, LedgerBucketStatus>,
    entries: Vec<LedgerEntry>,
    #[serde(flatten)]
    extra: BTreeMap<String, Value>,
}

fn rejected(message: impl Into<String>) -> ExperimentError {
    ExperimentError::Rejected(message.into())
}

fn hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn stamp() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let (year, month, day) = civil_from_days(seconds.div_euclid(86_400));
    let second_of_day = seconds.rem_euclid(86_400);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}+00:00",
        second_of_day / 3_600,
        (second_of_day % 3_600) / 60,
        second_of_day % 60,
    )
}

/// Public-domain civil-date conversion, with day zero at 1970-01-01.
fn civil_from_days(days_since_epoch: i64) -> (i64, u32, u32) {
    let z = days_since_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    (year + i64::from(month <= 2), month as u32, day as u32)
}

/// Atomically replace JSON and fsync both the file and containing directory.
pub fn durable_json(path: &Path, value: &Value) -> Result<()> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| rejected("output has no filename"))?;
    let temporary = (0..100)
        .map(|attempt| parent.join(format!(".{name}.{attempt}.pending")))
        .find(|candidate| !candidate.exists())
        .ok_or_else(|| rejected("could not allocate unique durable temporary file"))?;
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary)?;
    file.write_all(&serde_json::to_vec_pretty(value)?)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    fs::rename(&temporary, path)?;
    File::open(parent)?.sync_all()?;
    Ok(())
}

fn require_new_directory(path: &Path) -> Result<()> {
    fs::create_dir(path).map_err(|error| match error.kind() {
        std::io::ErrorKind::AlreadyExists => rejected(format!(
            "refusing to overwrite existing experiment directory: {}",
            path.display()
        )),
        _ => error.into(),
    })
}

/// Mutates an existing evaluation ledger under an exclusive advisory lock.
/// A failed post-reservation checkpoint intentionally leaves the reservation
/// held.  `unknown` never returns money to a bucket.
pub struct Ledger {
    path: PathBuf,
}

impl Ledger {
    pub fn open(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn status(&self) -> Result<LedgerStatus> {
        let bytes = fs::read(&self.path)?;
        let file: LedgerFile = serde_json::from_slice(&bytes)?;
        Ok(LedgerStatus {
            path: self.path.clone(),
            sha256: hash(&bytes),
            buckets: file.buckets,
            entries: file.entries,
        })
    }

    pub fn reserve(&self, bucket: &str, entry: &str, amount: f64, purpose: &str) -> Result<()> {
        self.update("reserve", bucket, entry, amount, purpose)
    }

    pub fn settle(&self, bucket: &str, entry: &str, amount: f64) -> Result<()> {
        self.update("settle", bucket, entry, amount, "")
    }

    pub fn mark_unknown(&self, bucket: &str, entry: &str, amount: f64) -> Result<()> {
        self.update("unknown", bucket, entry, amount, "")
    }

    fn update(
        &self,
        operation: &str,
        bucket_name: &str,
        entry_id: &str,
        amount: f64,
        purpose: &str,
    ) -> Result<()> {
        if !amount.is_finite() || amount < 0.0 {
            return Err(rejected("amount must be finite and non-negative"));
        }
        let amount = (amount * 100_000_000.0).round() / 100_000_000.0;
        let lock_path = self.path.with_extension(format!(
            "{}.lock",
            self.path
                .extension()
                .and_then(|value| value.to_str())
                .unwrap_or("")
        ));
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(lock_path)?;
        lock.lock_exclusive()?;
        let result = (|| {
            let bytes = fs::read(&self.path)?;
            let mut ledger: LedgerFile = serde_json::from_slice(&bytes)?;
            let bucket = ledger
                .buckets
                .get_mut(bucket_name)
                .ok_or_else(|| rejected("unknown ledger bucket"))?;
            let existing = ledger.entries.iter_mut().find(|item| item.id == entry_id);
            match operation {
                "reserve" => {
                    if amount <= 0.0 {
                        return Err(rejected("reservation must be positive"));
                    }
                    if existing.is_some() {
                        return Err(rejected("reservation entry id already exists"));
                    }
                    if amount > bucket.remaining_usd + 1e-9 {
                        return Err(rejected(format!("{bucket_name} cap would be exceeded")));
                    }
                    bucket.reserved_usd = round8(bucket.reserved_usd + amount);
                    bucket.remaining_usd = round8(bucket.remaining_usd - amount);
                    ledger.entries.push(LedgerEntry {
                        id: entry_id.into(),
                        bucket: bucket_name.into(),
                        reserved_usd: amount,
                        status: "reserved".into(),
                        reserved_at: stamp(),
                        purpose: purpose.into(),
                        known_usd: None,
                        settled_at: None,
                        unknown_at: None,
                        extra: BTreeMap::new(),
                    });
                }
                "settle" | "unknown" => {
                    let existing = existing.ok_or_else(|| {
                        rejected(format!("{operation} requires an existing reserved entry"))
                    })?;
                    if existing.status != "reserved" {
                        return Err(rejected(format!(
                            "{operation} requires an existing reserved entry"
                        )));
                    }
                    if existing.bucket != bucket_name {
                        return Err(rejected(
                            "settlement bucket must match the reservation bucket",
                        ));
                    }
                    let held = existing.reserved_usd;
                    if operation == "settle" {
                        if amount > held + 1e-9 {
                            return Err(rejected(
                                "known charge exceeds its conservative reservation",
                            ));
                        }
                        bucket.reserved_usd = round8(bucket.reserved_usd - held);
                        bucket.known_usd = round8(bucket.known_usd + amount);
                        bucket.remaining_usd = round8(bucket.remaining_usd + held - amount);
                        existing.status = "settled".into();
                        existing.known_usd = Some(amount);
                        existing.settled_at = Some(stamp());
                    } else {
                        if (amount - held).abs() > 1e-9 {
                            return Err(rejected(
                                "unknown charge must retain the full original reservation",
                            ));
                        }
                        existing.status = "unknown".into();
                        existing.unknown_at = Some(stamp());
                    }
                }
                _ => return Err(rejected("unknown ledger operation")),
            }
            durable_json(&self.path, &serde_json::to_value(ledger)?)
        })();
        let _ = FileExt::unlock(&lock);
        result
    }
}

fn round8(value: f64) -> f64 {
    (value * 100_000_000.0).round() / 100_000_000.0
}

pub fn ledger_status(path: impl AsRef<Path>) -> Result<LedgerStatus> {
    Ledger::open(path.as_ref()).status()
}

/// Freeze exact extraction requests and reservation envelopes. This does not
/// read credentials, touch a ledger, or make provider calls.
pub fn prepare(request: ExperimentPrepareRequest) -> Result<ExperimentManifest> {
    let input = fs::read(&request.state)?;
    let saved: State = serde_json::from_slice(&input)?;
    let state = if let Some(path) = request.indexed_source.as_ref() {
        let indexed: Vec<IndexedChunk> = serde_json::from_slice(&fs::read(path)?)?;
        with_full_context(&saved, &indexed)?
    } else {
        saved
    };
    require_new_directory(&request.output_dir)?;
    let expectation_bytes = fs::read(&request.expectations)?;
    let expectations: Value = serde_json::from_slice(&expectation_bytes)?;
    let (manifest, files) = build_manifest(&state, &expectations, &expectation_bytes)?;
    durable_json(&request.output_dir.join("expectations.json"), &expectations)?;
    // Report must rebuild candidates against these exact source slots. Keeping
    // an empty/missing output distinct from a zero-recipe output is what makes
    // continuation boundaries deterministic during offline replay.
    durable_json(
        &request.output_dir.join("source.json"),
        &serde_json::to_value(&state.source)?,
    )?;
    for (filename, bytes) in files {
        fs::write(request.output_dir.join(filename), bytes)?;
    }
    let path = request.output_dir.join("manifest.json");
    durable_json(&path, &manifest)?;
    Ok(ExperimentManifest {
        path,
        value: manifest,
    })
}

/// Produce a summary with frozen identity evidence.  With `dispatch=false`
/// this is entirely offline and cannot mutate the ledger.
pub async fn run(request: ExperimentRunRequest) -> Result<ExperimentRun> {
    let manifest_bytes = fs::read(&request.manifest)?;
    let manifest: Value = serde_json::from_slice(&manifest_bytes)?;
    let indexed: Vec<IndexedChunk> = serde_json::from_slice(&fs::read(&request.indexed_source)?)?;
    let actions = actions_from_manifest(
        &manifest,
        request
            .manifest
            .parent()
            .ok_or_else(|| rejected("manifest has no parent"))?,
        &indexed,
        request.contract,
    )?;
    require_new_directory(&request.output_dir)?;
    let expectations = fs::read(&request.expectations)?;
    let _: Value = serde_json::from_slice(&expectations)?;
    if manifest["kind"] == EXTRACTION_COMPARISON_KIND
        && manifest["expectations_sha256"] != hash(&expectations)
    {
        return Err(rejected("expectations differ from the frozen preparation"));
    }
    if request.dispatch && manifest["kind"] == LEGACY_EXTRACTION_COMPARISON_KIND {
        return Err(rejected(
            "legacy v2 manifest did not freeze expectations; prepare v3 before dispatch",
        ));
    }
    let preparation = json!({
        "kind":"haiku-primary-extraction-comparison-v1", "contract":request.contract.as_str(),
        "manifest_sha256":hash(&manifest_bytes), "expectations_sha256":hash(&expectations),
        "dispatch_requested":request.dispatch, "concurrency":DEFAULT_CONCURRENCY, "retry":false,
        "fallback":false, "local_cache":false,
        "provider_cache":"unchanged transport defaults; inspect reported cache usage",
        "verification":false,
        "actions":actions.iter().map(|a| json!({"key":a.key,"chunk":a.chunk,"reservation_usd":a.reservation_usd})).collect::<Vec<_>>()
    });
    durable_json(&request.output_dir.join("preparation.json"), &preparation)?;
    if !request.dispatch {
        return Ok(ExperimentRun {
            preparation,
            result: None,
        });
    }
    let backend = RecoveryBackend::from_env(
        &Options {
            model: Some(PRIMARY_MODEL.into()),
            use_cache: false,
            concurrency: DEFAULT_CONCURRENCY,
            ..Default::default()
        },
        "haiku-primary-extraction-comparison",
    )?;
    let trial_id = hash(request.output_dir.to_string_lossy().as_bytes());
    let ledger = Ledger::open(&request.ledger);
    let started = Instant::now();
    let output = &request.output_dir;
    let outcomes = stream::iter(actions.iter().map(|action| async {
        execute_action(
            action,
            &trial_id,
            &ledger,
            &backend,
            output,
            request.contract,
            &indexed,
        )
        .await
    }))
    .buffer_unordered(DEFAULT_CONCURRENCY)
    .collect::<Vec<_>>()
    .await;
    let result = json!({"contract":request.contract.as_str(),"wall_ms":started.elapsed().as_millis(),"outcomes":outcomes.iter().map(|value| match value { Ok(value)=>value.clone(), Err(error)=>json!({"error":error.to_string()}) }).collect::<Vec<_>>()});
    durable_json(&request.output_dir.join("result.json"), &result)?;
    if outcomes.iter().any(Result::is_err) {
        return Err(rejected(
            "comparison has interrupted or failed accounting; inspect private evidence",
        ));
    }
    Ok(ExperimentRun {
        preparation,
        result: Some(result),
    })
}

/// Offline report. It deliberately reports incomplete and held evidence rather
/// than treating a prepared or partially dispatched trial as a quality pass.
pub fn report(
    manifest: impl AsRef<Path>,
    evidence_dir: impl AsRef<Path>,
    ledger: impl AsRef<Path>,
) -> Result<Value> {
    let manifest_path = manifest.as_ref();
    let manifest_bytes = fs::read(manifest_path)?;
    let manifest: Value = serde_json::from_slice(&manifest_bytes)?;
    if audit::is_audit_manifest(&manifest) {
        return audit::report_audit_experiment(
            manifest_path,
            &manifest_bytes,
            &manifest,
            evidence_dir.as_ref(),
            ledger.as_ref(),
        );
    }
    let status = ledger_status(ledger)?;
    let preparation: Value =
        serde_json::from_slice(&fs::read(evidence_dir.as_ref().join("preparation.json"))?)?;
    if preparation["manifest_sha256"] != hash(&manifest_bytes) {
        return Err(rejected(
            "evidence preparation belongs to a different manifest",
        ));
    }
    let expected = preparation["actions"]
        .as_array()
        .ok_or_else(|| rejected("preparation has no actions"))?;
    if expected.is_empty() {
        return Err(rejected("preparation has no actions"));
    }
    let mut evidence = Vec::with_capacity(expected.len());
    let mut missing = Vec::new();
    let mut usage = Usage::default();
    let mut known_usd = 0.0;
    let mut held_usd = 0.0;
    let mut provider_ms = 0u64;
    for action in expected {
        let chunk = action["chunk"]
            .as_u64()
            .ok_or_else(|| rejected("preparation action has no chunk"))?;
        let path = evidence_dir.as_ref().join(format!("chunk-{chunk}.json"));
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(_) => {
                missing.push(chunk);
                continue;
            }
        };
        let record: Value = serde_json::from_slice(&bytes)?;
        if record["action_key"] != action["key"] || record["chunk"] != action["chunk"] {
            return Err(rejected(
                "evidence action identity does not match preparation",
            ));
        }
        if let Ok(value) = serde_json::from_value::<Usage>(record["response"]["usage"].clone()) {
            usage.input_tokens += value.input_tokens;
            usage.output_tokens += value.output_tokens;
            usage.cache_read_input_tokens += value.cache_read_input_tokens;
            usage.cache_creation_input_tokens += value.cache_creation_input_tokens;
        }
        known_usd += record["known_usd"].as_f64().unwrap_or(0.0);
        provider_ms += record["response"]["provider_ms"].as_u64().unwrap_or(0);
        evidence.push(record);
    }
    let result_path = evidence_dir.as_ref().join("result.json");
    let result = result_path
        .exists()
        .then(|| fs::read(&result_path))
        .transpose()?
        .map(|bytes| serde_json::from_slice::<Value>(&bytes))
        .transpose()?;
    for record in &evidence {
        if let Some(ledger_entry) = record["ledger_entry"]
            .as_str()
            .and_then(|entry| status.entries.iter().find(|item| item.id == entry))
            .filter(|entry| matches!(entry.status.as_str(), "reserved" | "unknown"))
        {
            held_usd += ledger_entry.reserved_usd;
        }
    }
    let complete = missing.is_empty()
        && result.as_ref().is_some_and(|value| {
            value["outcomes"].as_array().is_some_and(|outcomes| {
                outcomes.len() == expected.len()
                    && outcomes
                        .iter()
                        .all(|entry| entry.get("error").is_none() && entry["status"] == "settled")
            })
        })
        && evidence.iter().all(|record| record["status"] == "settled");
    let mut manifest_for_quality = manifest.clone();
    manifest_for_quality["_manifest_directory"] =
        json!(manifest_path.parent().unwrap_or_else(|| Path::new(".")));
    let quality = evaluate_frozen_quality(&manifest_for_quality, &evidence, complete)?;
    Ok(
        json!({"kind":"extraction-contract-comparison-report-v1","manifest_sha256":hash(&manifest_bytes),"ledger":status,"result":result,"actions":expected.len(),"missing_chunks":missing,"usage":usage,"known_usd":round8(known_usd),"held_usd":round8(held_usd),"provider_ms_sum":provider_ms,"complete":complete,"quality":quality,"quality_accepted":quality["accepted"].as_bool().unwrap_or(false),"fully_audited_duration_ms":null,"note":"Fully audited duration remains unavailable: this report evaluates decoded extraction evidence offline and does not run an AI audit."}),
    )
}

fn evaluate_frozen_quality(manifest: &Value, evidence: &[Value], complete: bool) -> Result<Value> {
    let Some(file) = manifest.get("expectations_file").and_then(Value::as_str) else {
        return Ok(
            json!({"status":"unscored","reason":"legacy manifest has no frozen expectations copy","accepted":false}),
        );
    };
    let path = manifest
        .get("_manifest_directory")
        .and_then(Value::as_str)
        .map(PathBuf::from)
        .unwrap_or_default()
        .join(file);
    let bytes = match fs::read(&path) {
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
    let expectations: Value = serde_json::from_slice(&bytes)?;
    let supported =
        expectations["kind"] == "source_coverage" || expectations.get("epub_sha256").is_some();
    if !supported {
        return Ok(
            json!({"status":"unscored","reason":"unsupported frozen expectation shape","accepted":false}),
        );
    }
    let epub_sha256 = expectations["epub_sha256"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    if epub_sha256.is_empty() {
        return Ok(
            json!({"status":"unscored","reason":"supported expectation has no root epub_sha256","accepted":false}),
        );
    }
    let Some(source_file) = manifest.get("source_file").and_then(Value::as_str) else {
        return Ok(
            json!({"status":"unscored","reason":"manifest has no frozen source slots","accepted":false}),
        );
    };
    let source_path = manifest
        .get("_manifest_directory")
        .and_then(Value::as_str)
        .map(PathBuf::from)
        .unwrap_or_default()
        .join(source_file);
    let source: Vec<crate::Chunk> = match fs::read(source_path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
    {
        Some(source) => source,
        None => {
            return Ok(
                json!({"status":"unscored","reason":"frozen source slots are unavailable","accepted":false}),
            );
        }
    };
    if let Some(expected) = manifest.get("source_sha256").and_then(Value::as_str)
        && expected != hash(&serde_json::to_vec(&source)?)
    {
        return Ok(
            json!({"status":"unscored","reason":"frozen source slots differ from manifest identity","accepted":false}),
        );
    }
    let mut outputs = vec![None; source.len()];
    for record in evidence {
        let Some(decoded) = record.get("decoded") else {
            return Ok(
                json!({"status":"unscored","reason":"one or more evidence records have no decoded output","accepted":false}),
            );
        };
        let output: Vec<crate::ExtractedRecipe> = match serde_json::from_value(decoded.clone()) {
            Ok(value) => value,
            Err(_) => {
                return Ok(
                    json!({"status":"unscored","reason":"decoded output cannot be reconstructed","accepted":false}),
                );
            }
        };
        let Some(index) = record["chunk"]
            .as_u64()
            .and_then(|index| usize::try_from(index).ok())
        else {
            return Ok(
                json!({"status":"unscored","reason":"evidence chunk coordinate is invalid","accepted":false}),
            );
        };
        let Some(slot) = outputs.get_mut(index) else {
            return Ok(
                json!({"status":"unscored","reason":"evidence chunk is outside frozen source","accepted":false}),
            );
        };
        *slot = Some(output);
    }
    let chunks = source
        .into_iter()
        .zip(outputs)
        .enumerate()
        .map(|(index, (source, output))| crate::review::RunChunk {
            id: format!("experiment-{index}"),
            source,
            output,
            error: None,
            cached: false,
            usage: Usage::default(),
            model: None,
            prompt_version: None,
            request_identity: None,
        })
        .collect();
    let mut run = crate::review::ReviewRun {
        recovery: None,
        execution_status: None,
        metadata: None,
        charges: vec![],
        version: crate::review::RUN_VERSION,
        epub_sha256,
        source: "experiment evidence".into(),
        model: PRIMARY_MODEL.into(),
        prompt_version: "experiment".into(),
        parent: None,
        chunks,
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
    // Use saved-run replay rather than a parallel assembly implementation:
    // it preserves `None` slots as continuation barriers and regenerates the
    // parsed ingredient representation used by exact expectations.
    if let Err(error) = run.replay() {
        return Ok(
            json!({"status":"unscored","reason":format!("offline replay of frozen evidence failed: {error}"),"accepted":false}),
        );
    }
    match crate::review::evaluate_document(&run, expectations) {
        Ok(evaluation) => Ok(
            json!({"status":"scored","evaluation":evaluation,"accepted":complete && !crate::review::evaluation_failed(&evaluation)}),
        ),
        Err(error) => Ok(
            json!({"status":"unscored","reason":format!("existing evaluator rejected frozen expectations: {error}"),"accepted":false}),
        ),
    }
}

fn build_manifest(
    state: &State,
    expectations: &Value,
    expectation_bytes: &[u8],
) -> Result<(Value, ManifestFiles)> {
    let provenance = expectations.get("provenance");
    let expectation_binding = expectations
        .get("epub_sha256")
        .or_else(|| expectations.get("source_sha256"))
        .or_else(|| provenance.and_then(|value| value.get("epub_sha256")))
        .or_else(|| provenance.and_then(|value| value.get("selected_source_sha256")))
        .and_then(Value::as_str)
        .ok_or_else(|| rejected("expectations must bind epub_sha256 or source_sha256"))?;
    let source_sha256 = hash(&serde_json::to_vec(&state.source)?);
    if let Some(expected_source) = expectations.get("source_sha256").and_then(Value::as_str)
        && expected_source != source_sha256
    {
        return Err(rejected(
            "expectations source_sha256 does not match the frozen source",
        ));
    }
    let mut entries = Vec::new();
    let mut files = Vec::new();
    for group in state.groups.iter().filter(|group| group.enabled) {
        for &index in &group.chunks {
            let chunk = state
                .source
                .get(index)
                .ok_or_else(|| rejected("group chunk outside source"))?;
            let evidence = crate::source::chunk_source_evidence(
                chunk,
                state.source_line_provenance.get(index).map(Vec::as_slice),
                &state.documents,
            )
            .map_err(|error| rejected(error.to_string()))?;
            let legacy = crate::build_chunk_request(chunk);
            let indexed =
                crate::indexed::build_indexed_chunk_request_with_source_evidence(chunk, &evidence)
                    .map_err(|error| rejected(error.to_string()))?;
            let hybrid =
                crate::hybrid::build_hybrid_chunk_request_with_source_evidence(chunk, &evidence)
                    .map_err(|error| rejected(error.to_string()))?;
            for (contract, request) in [
                (ExperimentContract::Legacy, legacy),
                (ExperimentContract::Indexed, indexed),
                (ExperimentContract::Hybrid, hybrid),
            ] {
                let bytes = serde_json::to_vec(&request)?;
                let filename = format!("{}-{index}.json", contract.as_str());
                let proposed_reservations = [PRIMARY_MODEL, "claude-sonnet-4-6"]
                    .into_iter()
                    .map(|model| {
                        reservation_envelope(
                            model,
                            contract,
                            index,
                            state.source_line_provenance_sha256.as_deref(),
                            &request,
                        )
                    })
                    .collect::<Result<Vec<_>>>()?;
                entries.push(json!({"proposed_reservations":proposed_reservations,"contract":contract.as_str(),"chunk":index,"file":filename,"sha256":hash(&bytes),"user_bytes":request.user.len(),"system_bytes":request.system.len(),"schema_bytes":serde_json::to_vec(&request.tool_schema)?.len(),"target_lines":chunk.text.lines().count()}));
                files.push((filename, bytes));
            }
        }
    }
    Ok((
        json!({"kind":EXTRACTION_COMPARISON_KIND,"dispatch_requested":false,"source_sha256":source_sha256,"source_line_provenance_sha256":state.source_line_provenance_sha256,"expectations_file":"expectations.json","expectations_sha256":hash(expectation_bytes),"expectations_source_binding":expectation_binding,"source_file":"source.json","context_chunks":state.source.len(),"requests":entries,"proposed_controls":{"primary_model":PRIMARY_MODEL,"fallback_model":"claude-sonnet-4-6","concurrency":DEFAULT_CONCURRENCY,"output_limit":OUTPUT_LIMIT},"scope":"Offline requests from current shared builders. Expectations are frozen before execution. No execution, pricing admission, candidate generation, or quality comparison. Legacy reproduces Cubby's request format; indexed outputs require indexed decoding before independent evaluation."}),
        files,
    ))
}

fn reservation_envelope(
    model: &str,
    contract: ExperimentContract,
    chunk: usize,
    provenance: Option<&str>,
    request: &ChunkRequest,
) -> Result<Value> {
    let identity = action_identity(model, contract, chunk, provenance, request)?;
    let usage = Usage {
        input_tokens: identity.len().saturating_mul(2) as u64,
        output_tokens: OUTPUT_LIMIT as u64,
        ..Default::default()
    };
    let reservation = ExtractionStats {
        model: model.into(),
        usage,
        ..Default::default()
    }
    .cost_usd();
    Ok(
        json!({"model":model,"enabled":crate::models::catalog().iter().any(|entry| entry.id == model && entry.enabled),"reservation_usd":reservation,"pricing_checked":crate::models::pricing_checked(model),"pricing_source":crate::models::pricing_source(model),"basis":"two input tokens per serialized action-identity byte plus 16000 output tokens; an envelope only, not ledger admission"}),
    )
}

fn action_identity(
    model: &str,
    contract: ExperimentContract,
    chunk: usize,
    provenance: Option<&str>,
    request: &ChunkRequest,
) -> Result<Vec<u8>> {
    Ok(serde_json::to_string(&json!({"kind":"extraction-contract-comparison-action-v1","model":model,"contract":contract.as_str(),"chunk":chunk,"source_line_provenance_sha256":provenance,"output_limit":OUTPUT_LIMIT,"request":request}))?.into_bytes())
}

fn actions_from_manifest(
    manifest: &Value,
    directory: &Path,
    indexed: &[IndexedChunk],
    contract: ExperimentContract,
) -> Result<Vec<Action>> {
    if manifest["kind"] != EXTRACTION_COMPARISON_KIND
        && manifest["kind"] != LEGACY_EXTRACTION_COMPARISON_KIND
    {
        return Err(rejected("unsupported preparation contract"));
    }
    if !crate::models::catalog().iter().any(|model| {
        model.id == PRIMARY_MODEL && model.enabled && model.max_output_tokens >= OUTPUT_LIMIT as u64
    }) {
        return Err(rejected("primary model is not admitted"));
    }
    let source: Vec<_> = indexed.iter().map(|entry| &entry.chunk).collect();
    if manifest["source_sha256"] != hash(&serde_json::to_vec(&source)?) {
        return Err(rejected("full source hash changed"));
    }
    let mut identity_state = State::new(
        indexed.iter().map(|entry| entry.chunk.clone()).collect(),
        PRIMARY_MODEL,
        10.0,
    )
    .map_err(rejected)?;
    identity_state
        .bind_source_line_provenance(
            &indexed
                .iter()
                .map(|entry| entry.lines.clone())
                .collect::<Vec<_>>(),
        )
        .map_err(rejected)?;
    if manifest["source_line_provenance_sha256"]
        != json!(identity_state.source_line_provenance_sha256)
    {
        return Err(rejected("source provenance hash changed"));
    }
    let entries = manifest["requests"]
        .as_array()
        .ok_or_else(|| rejected("missing requests"))?;
    let selected = |kind: ExperimentContract| {
        entries
            .iter()
            .filter(|entry| entry["contract"] == kind.as_str())
            .map(|entry| entry["chunk"].clone())
            .collect::<Vec<_>>()
    };
    if selected(ExperimentContract::Legacy) != selected(ExperimentContract::Indexed)
        || selected(ExperimentContract::Indexed) != selected(ExperimentContract::Hybrid)
    {
        return Err(rejected("comparison arms select different source chunks"));
    }
    let mut actions = Vec::new();
    for entry in entries
        .iter()
        .filter(|entry| entry["contract"] == contract.as_str())
    {
        let index = entry["chunk"]
            .as_u64()
            .and_then(|value| usize::try_from(value).ok())
            .ok_or_else(|| rejected("invalid source index"))?;
        let source = indexed
            .get(index)
            .ok_or_else(|| rejected("source index outside full book"))?;
        let filename = format!("{}-{index}.json", contract.as_str());
        if entry["file"] != filename {
            return Err(rejected("unexpected request filename"));
        }
        let bytes = fs::read(directory.join(filename))?;
        if entry["sha256"] != hash(&bytes) {
            return Err(rejected("request hash changed"));
        }
        let request: ChunkRequest = serde_json::from_slice(&bytes)?;
        let expected = match contract {
            ExperimentContract::Legacy => crate::build_chunk_request(&source.chunk),
            ExperimentContract::Indexed => {
                let evidence =
                    crate::source::chunk_source_evidence(&source.chunk, Some(&source.lines), &[])
                        .map_err(|error| rejected(error.to_string()))?;
                crate::indexed::build_indexed_chunk_request_with_source_evidence(
                    &source.chunk,
                    &evidence,
                )
                .map_err(|error| rejected(error.to_string()))?
            }
            ExperimentContract::Hybrid => {
                let evidence =
                    crate::source::chunk_source_evidence(&source.chunk, Some(&source.lines), &[])
                        .map_err(|error| rejected(error.to_string()))?;
                crate::hybrid::build_hybrid_chunk_request_with_source_evidence(
                    &source.chunk,
                    &evidence,
                )
                .map_err(|error| rejected(error.to_string()))?
            }
        };
        if serde_json::to_value(&request)? != serde_json::to_value(&expected)? {
            return Err(rejected("request differs from current source contract"));
        }
        let identity = action_identity(
            PRIMARY_MODEL,
            contract,
            index,
            manifest["source_line_provenance_sha256"].as_str(),
            &request,
        )?;
        let reservation = ExtractionStats {
            model: PRIMARY_MODEL.into(),
            usage: Usage {
                input_tokens: identity.len().saturating_mul(2) as u64,
                output_tokens: OUTPUT_LIMIT as u64,
                ..Default::default()
            },
            ..Default::default()
        }
        .cost_usd()
        .ok_or_else(|| rejected("unpriced model"))?;
        let planned = entry["proposed_reservations"]
            .as_array()
            .and_then(|items| items.iter().find(|item| item["model"] == PRIMARY_MODEL))
            .ok_or_else(|| rejected("missing primary reservation"))?;
        if planned["reservation_usd"].as_f64() != Some(reservation) || planned["enabled"] != true {
            return Err(rejected("reservation differs from frozen plan"));
        }
        actions.push(Action {
            group: actions.len(),
            candidate: 0,
            chunk: Some(index),
            verification_chunk: None,
            verification_stage: None,
            model: PRIMARY_MODEL.into(),
            key: hash(&identity),
            reservation_usd: reservation,
            priced: true,
            output_limit: OUTPUT_LIMIT,
            telemetry: RequestTelemetry {
                operation: "extraction".into(),
                model: PRIMARY_MODEL.into(),
                contract: format!("comparison-{}-v1", contract.as_str()),
                output_limit: OUTPUT_LIMIT,
                source_bytes: source.chunk.text.len(),
                context_bytes: request.user.len().saturating_sub(source.chunk.text.len()),
                schema_bytes: serde_json::to_vec(&request.tool_schema)?.len(),
                reservation_usd: reservation,
                ..Default::default()
            },
            request,
        });
    }
    if actions.len() < 8
        || actions
            .windows(2)
            .any(|pair| pair[0].chunk >= pair[1].chunk)
    {
        return Err(rejected("need at least eight unique ordered chunks"));
    }
    Ok(actions)
}

/// Persist the admission intent before asking the ledger, then persist the
/// admitted reservation before any caller is allowed to dispatch transport.
/// Keeping this small seam injectable lets the failure ordering be tested
/// without credentials or a provider.
fn reserve_after_checkpoint<L, W>(
    action: &Action,
    entry: &str,
    evidence: &mut Value,
    mut account: L,
    mut persist: W,
) -> Result<bool>
where
    L: FnMut(&str, &str, f64) -> Result<()>,
    W: FnMut(&Value) -> Result<()>,
{
    persist(evidence)?;
    if let Err(error) = account("reserve", entry, action.reservation_usd) {
        evidence["status"] = json!("reservation_rejected");
        evidence["error"] = json!(error.to_string());
        persist(evidence)?;
        return Ok(false);
    }
    evidence["status"] = json!("reserved");
    persist(evidence)?;
    Ok(true)
}

async fn execute_action(
    action: &Action,
    trial_id: &str,
    ledger: &Ledger,
    backend: &RecoveryBackend,
    output: &Path,
    contract: ExperimentContract,
    indexed: &[IndexedChunk],
) -> Result<Value> {
    let index = action
        .chunk
        .ok_or_else(|| rejected("extraction chunk missing"))?;
    let path = output.join(format!("chunk-{index}.json"));
    let entry = format!("{trial_id}:{}", action.key);
    let mut evidence = json!({"action_key":action.key,"chunk":action.chunk,"model":action.model,"reservation_usd":action.reservation_usd,"ledger_entry":entry,"bucket":"throughput","status":"prepared_for_reservation"});
    if !reserve_after_checkpoint(
        action,
        &entry,
        &mut evidence,
        |operation, id, amount| {
            debug_assert_eq!(operation, "reserve");
            ledger.reserve(
                "throughput",
                id,
                amount,
                "frozen Haiku-primary extraction-contract comparison",
            )
        },
        |value| durable_json(&path, value),
    )? {
        return Ok(json!({"chunk":index,"status":"reservation_rejected"}));
    }
    let began = Instant::now();
    let response = backend.recovery_call(action).await;
    let raw_usage = backend.recovery_usage(&action.key);
    evidence["response"] = match response {
        Ok((payload, usage, finish)) => {
            json!({"payload":payload,"usage":usage,"raw_usage":raw_usage,"finish_reason":finish,"provider_ms":began.elapsed().as_millis()})
        }
        Err(failure) => {
            json!({"payload":null,"usage":failure.usage,"raw_usage":raw_usage,"error":"provider request failed","truncated":failure.truncated,"provider_ms":began.elapsed().as_millis()})
        }
    };
    evidence["status"] = json!("response_received");
    durable_json(&path, &evidence)?;
    let usage: Option<Usage> = serde_json::from_value(evidence["response"]["usage"].clone()).ok();
    let known = usage
        .filter(|usage| *usage != Usage::default())
        .and_then(|usage| {
            ExtractionStats {
                model: action.model.clone(),
                usage,
                ..Default::default()
            }
            .cost_usd()
        });
    let settled = match known {
        Some(cost) => ledger
            .settle("throughput", &entry, cost)
            .map(|_| {
                evidence["known_usd"] = json!(cost);
                true
            })
            .or_else(|_| {
                ledger
                    .mark_unknown("throughput", &entry, action.reservation_usd)
                    .map(|_| false)
            })?,
        None => {
            ledger.mark_unknown("throughput", &entry, action.reservation_usd)?;
            false
        }
    };
    evidence["status"] = json!(if settled {
        "settled"
    } else {
        "unknown_charge_held"
    });
    if !evidence["response"]["payload"].is_null() {
        let truncated = evidence["response"]["truncated"] == true
            || matches!(
                evidence["response"]["finish_reason"].as_str(),
                Some("length" | "max_tokens")
            );
        match decode(
            contract,
            &indexed[index].chunk,
            evidence["response"]["payload"].clone(),
            truncated,
        ) {
            Ok(recipes) => {
                evidence["decoded"] = recipes;
                evidence["decode_status"] = json!("complete_unverified");
            }
            Err(error) => {
                evidence["decode_status"] = json!("rejected");
                evidence["decode_error"] = json!(error.to_string());
            }
        }
    }
    durable_json(&path, &evidence)?;
    Ok(
        json!({"chunk":index,"status":evidence["status"],"decode_status":evidence["decode_status"],"known_usd":evidence["known_usd"]}),
    )
}

fn decode(
    contract: ExperimentContract,
    chunk: &crate::Chunk,
    payload: Value,
    truncated: bool,
) -> Result<Value> {
    if truncated {
        return Err(rejected("truncated response"));
    }
    let recipes = match contract {
        ExperimentContract::Legacy => crate::parse_recipes_payload_for_chunk(chunk, payload),
        ExperimentContract::Indexed => crate::indexed::parse_indexed_recipes(chunk, payload),
        ExperimentContract::Hybrid => crate::hybrid::parse_hybrid_recipes(chunk, payload),
    }
    .map_err(|error| rejected(error.to_string()))?;
    Ok(serde_json::to_value(recipes)?)
}

fn with_full_context(saved: &State, indexed: &[IndexedChunk]) -> Result<State> {
    saved.validate().map_err(rejected)?;
    let mut state = State::new(
        indexed.iter().map(|entry| entry.chunk.clone()).collect(),
        crate::recovery::AUTOMATIC,
        saved.budget_usd,
    )
    .map_err(rejected)?;
    state.bind_documents(&saved.documents).map_err(rejected)?;
    state
        .bind_source_line_provenance(
            &indexed
                .iter()
                .map(|entry| entry.lines.clone())
                .collect::<Vec<_>>(),
        )
        .map_err(rejected)?;
    let mapping = saved
        .source
        .iter()
        .map(|chunk| {
            let expected = serde_json::to_value(chunk)?;
            let matches = state
                .source
                .iter()
                .enumerate()
                .filter_map(|(index, candidate)| {
                    (serde_json::to_value(candidate).ok().as_ref() == Some(&expected))
                        .then_some(index)
                })
                .collect::<Vec<_>>();
            match matches.as_slice() {
                [index] => Ok(*index),
                _ => Err(rejected(
                    "saved chunk must have exactly one unchanged full-source match",
                )),
            }
        })
        .collect::<Result<Vec<_>>>()?;
    if mapping.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err(rejected("saved source mapping is not unique and ordered"));
    }
    for group in &mut state.groups {
        group.enabled = false;
    }
    for saved_group in &saved.groups {
        if !saved_group.enabled {
            continue;
        }
        let mapped = saved_group
            .chunks
            .iter()
            .map(|index| mapping[*index])
            .collect::<Vec<_>>();
        let group = state
            .groups
            .iter_mut()
            .find(|group| group.chunks == mapped)
            .ok_or_else(|| rejected("full-source group differs from frozen execution group"))?;
        if saved_group.candidates.len() != 1 {
            return Err(rejected(
                "measurement requires one frozen candidate per group",
            ));
        }
        let mut candidate = saved_group.candidates[0].clone();
        if candidate.outputs.iter().any(Option::is_none) || candidate.seed_provenance.is_some() {
            return Err(rejected(
                "measurement requires complete unseeded frozen candidates",
            ));
        }
        candidate.extraction_keys.fill(None);
        candidate.cached_chunks.fill(false);
        candidate.feedback.clear();
        candidate.obsolete_feedback.clear();
        candidate.verified = false;
        candidate.verification_evidence.clear();
        candidate.obsolete_verification_evidence.clear();
        candidate.obsolete_verification_stages.clear();
        candidate.verified_chunks.clear();
        candidate.verification_stages.clear();
        group.enabled = true;
        group.candidates.push(candidate);
    }
    state.validate().map_err(rejected)?;
    Ok(state)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::{sync::Arc, thread};
    fn action() -> Action {
        Action {
            group: 0,
            candidate: 0,
            chunk: Some(0),
            verification_chunk: None,
            verification_stage: None,
            model: PRIMARY_MODEL.into(),
            key: "fixture".into(),
            reservation_usd: 1.0,
            priced: true,
            output_limit: OUTPUT_LIMIT,
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
        durable_json(path, &json!({"buckets":{"verifier":{"known_usd":0.0,"reserved_usd":0.0,"remaining_usd":4.0},"throughput":{"known_usd":0.0,"reserved_usd":0.0,"remaining_usd":6.0}},"entries":[]})).unwrap();
    }
    #[test]
    fn preserves_python_ledger_schema_and_unknown_hold() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("ledger.json");
        durable_json(&path, &json!({"schema_version":1,"ledger_id":"private-eval","authorization":{"caps_are_independent":true},"buckets":{"verifier":{"cap_usd":4.0,"known_usd":0.0,"reserved_usd":0.0,"remaining_usd":4.0},"throughput":{"cap_usd":6.0,"known_usd":0.0,"reserved_usd":0.0,"remaining_usd":6.0,"prior_unknown_usd":0.2}},"entries":[{"id":"legacy","bucket":"verifier","reserved_usd":0.1,"status":"unknown","reserved_at":"2026-01-01T00:00:00+00:00","unknown_at":"2026-01-01T00:01:00+00:00","purpose":"old","request_fingerprint":"preserved"}]})).unwrap();
        let value = Ledger::open(&path);
        value.reserve("throughput", "call", 1.0, "test").unwrap();
        value.mark_unknown("throughput", "call", 1.0).unwrap();
        let status = value.status().unwrap();
        assert_eq!(status.buckets["throughput"].remaining_usd, 5.0);
        assert_eq!(status.buckets["throughput"].reserved_usd, 1.0);
        assert_eq!(status.entries[1].status, "unknown");
        assert!(status.entries[1].known_usd.is_none());
        let persisted: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(persisted["schema_version"], 1);
        assert_eq!(persisted["authorization"]["caps_are_independent"], true);
        assert_eq!(persisted["buckets"]["throughput"]["prior_unknown_usd"], 0.2);
        assert_eq!(persisted["entries"][0]["request_fingerprint"], "preserved");
        assert!(path.with_extension("json.lock").exists());
    }
    #[test]
    fn concurrent_reservations_never_cross_cap() {
        let directory = tempfile::tempdir().unwrap();
        let path = Arc::new(directory.path().join("ledger.json"));
        ledger(&path);
        let joins = (0..12)
            .map(|index| {
                let path = path.clone();
                thread::spawn(move || {
                    Ledger::open(path.as_ref())
                        .reserve("throughput", &format!("entry-{index}"), 1.0, "test")
                        .is_ok()
                })
            })
            .collect::<Vec<_>>();
        assert_eq!(
            joins
                .into_iter()
                .filter_map(|join| join.join().ok())
                .filter(|ok| *ok)
                .count(),
            6
        );
        let status = Ledger::open(path.as_ref()).status().unwrap();
        assert_eq!(status.buckets["throughput"].remaining_usd, 0.0);
        assert_eq!(status.buckets["throughput"].reserved_usd, 6.0);
    }
    #[test]
    fn stale_or_overcharged_settlement_is_rejected() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("ledger.json");
        ledger(&path);
        let value = Ledger::open(&path);
        assert!(value.settle("throughput", "missing", 0.1).is_err());
        value.reserve("throughput", "call", 1.0, "test").unwrap();
        assert!(value.settle("throughput", "call", 1.1).is_err());
        assert_eq!(value.status().unwrap().entries[0].status, "reserved");
    }
    #[test]
    fn reservation_is_checkpointed_before_transport_can_start() {
        let events = std::cell::RefCell::new(Vec::new());
        let mut evidence = json!({"status":"prepared_for_reservation"});
        assert!(
            reserve_after_checkpoint(
                &action(),
                "entry",
                &mut evidence,
                |operation, _, _| {
                    events.borrow_mut().push(operation.to_owned());
                    Ok(())
                },
                |value| {
                    events
                        .borrow_mut()
                        .push(value["status"].as_str().unwrap().to_owned());
                    Ok(())
                }
            )
            .unwrap()
        );
        assert_eq!(
            *events.borrow(),
            ["prepared_for_reservation", "reserve", "reserved"]
        );
    }
    #[test]
    fn failed_reserved_checkpoint_leaves_admission_held_and_stops() {
        let calls = std::cell::Cell::new(0);
        let mut evidence = json!({"status":"prepared_for_reservation"});
        let result = reserve_after_checkpoint(
            &action(),
            "entry",
            &mut evidence,
            |_, _, _| {
                calls.set(calls.get() + 1);
                Ok(())
            },
            |value| {
                if value["status"] == "reserved" {
                    Err(rejected("disk failure"))
                } else {
                    Ok(())
                }
            },
        );
        assert!(result.is_err());
        assert_eq!(
            calls.get(),
            1,
            "the reservation happened, so it must remain held"
        );
    }
    #[test]
    fn rejected_admission_never_reaches_reserved_checkpoint() {
        let events = std::cell::RefCell::new(Vec::new());
        let mut evidence = json!({"status":"prepared_for_reservation"});
        assert!(
            !reserve_after_checkpoint(
                &action(),
                "entry",
                &mut evidence,
                |_, _, _| Err(rejected("cap exceeded")),
                |value| {
                    events
                        .borrow_mut()
                        .push(value["status"].as_str().unwrap().to_owned());
                    Ok(())
                }
            )
            .unwrap()
        );
        assert_eq!(
            *events.borrow(),
            ["prepared_for_reservation", "reservation_rejected"]
        );
    }
    fn frozen_manifest(directory: &Path, expectations: Value) -> Value {
        let bytes = serde_json::to_vec(&expectations).unwrap();
        fs::write(directory.join("expectations.json"), &bytes).unwrap();
        let source = vec![
            crate::Chunk {
                title_hint: Some("Soup".into()),
                text: "Soup\nA short headnote".into(),
                doc_path: "soup".into(),
                links: vec![],
                images: vec![],
            },
            crate::Chunk {
                title_hint: Some("Soup".into()),
                text: "1 cup water\nSimmer.".into(),
                doc_path: "soup".into(),
                links: vec![],
                images: vec![],
            },
            crate::Chunk {
                title_hint: None,
                text: "Closing matter".into(),
                doc_path: "closing".into(),
                links: vec![],
                images: vec![],
            },
        ];
        durable_json(
            &directory.join("source.json"),
            &serde_json::to_value(source).unwrap(),
        )
        .unwrap();
        json!({"expectations_file":"expectations.json","expectations_sha256":hash(&bytes),"source_file":"source.json","_manifest_directory":directory})
    }

    fn soup_expectations() -> Value {
        json!({"kind":"source_coverage","epub_sha256":"book","recipes":[{"document":"soup","title":"Soup","description":"A short headnote","yield":null,"sections":[{"name":null,"ingredients":["1 cup water"]}],"methods":["Simmer."]}]})
    }

    fn soup_evidence() -> Vec<Value> {
        vec![
            json!({"chunk":0,"decoded":[{"title":"Soup","description":"A short headnote","sections":[]}]}),
            json!({"chunk":1,"decoded":[{"title":"Soup","sections":[{"ingredients":["1 cup water"],"instructions":["Simmer."]}]}]}),
            // A real no-recipe result is present, unlike a missing slot.
            json!({"chunk":2,"decoded":[]}),
        ]
    }
    #[test]
    fn report_quality_scores_supported_complete_evidence() {
        let directory = tempfile::tempdir().unwrap();
        let manifest = frozen_manifest(directory.path(), soup_expectations());
        let quality = evaluate_frozen_quality(&manifest, &soup_evidence(), true).unwrap();
        assert_eq!(quality["status"], "scored");
        assert_eq!(quality["accepted"], true);
    }
    #[test]
    fn report_quality_marks_missing_or_failed_evidence_unaccepted() {
        let directory = tempfile::tempdir().unwrap();
        let missing = frozen_manifest(directory.path(), soup_expectations());
        assert_eq!(
            evaluate_frozen_quality(&missing, &[], false).unwrap()["accepted"],
            false
        );
        let failed = frozen_manifest(directory.path(), soup_expectations());
        let quality = evaluate_frozen_quality(
            &failed,
            &[
                json!({"chunk":0,"decoded":[]}),
                json!({"chunk":1,"decoded":[]}),
                json!({"chunk":2,"decoded":[]}),
            ],
            true,
        )
        .unwrap();
        assert_eq!(quality["status"], "scored");
        assert_eq!(quality["accepted"], false);
    }
    #[test]
    fn hybrid_decoder_uses_hybrid_label_contract() {
        let chunk = crate::Chunk {
            title_hint: None,
            text: "Soup\n1 cup water\nSimmer.".into(),
            doc_path: "x.xhtml".into(),
            links: vec![],
            images: vec![],
        };
        let payload = json!({"recipes":[{"title":{"text":"Soup","spans":[{"start":0,"end":0}]},"description":[],"recipe_yield":[],"notes":[],"equipment":[],"sections":[{"name":{"text":"","spans":[]},"ingredients":[{"start":1,"end":1}],"instructions":[{"start":2,"end":2}]}]}],"ignored":[]});
        assert!(decode(ExperimentContract::Hybrid, &chunk, payload.clone(), false).is_ok());
        assert!(decode(ExperimentContract::Indexed, &chunk, payload, false).is_err());
    }
}
