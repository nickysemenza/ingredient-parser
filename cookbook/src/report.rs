//! Everything an extraction reports besides the book: estimates, live progress,
//! and the run report with every model call.

use serde::{Deserialize, Serialize};

use crate::cost::Usage;
use crate::model::{BookSource, Cookbook};

/// The result of one extraction run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "wasm", derive(tsify_next::Tsify))]
#[cfg_attr(feature = "wasm", tsify(into_wasm_abi, from_wasm_abi))]
pub struct Extraction {
    pub cookbook: Cookbook,
    pub report: RunReport,
}

/// Tunables. `ladder` empty means the catalog default.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "wasm", derive(tsify_next::Tsify))]
#[cfg_attr(feature = "wasm", tsify(into_wasm_abi, from_wasm_abi))]
#[serde(default)]
pub struct ExtractOptions {
    /// The caller's label for the book (shown in reports and gateway metadata).
    pub label: String,
    /// Chunks in flight at once.
    pub concurrency: usize,
    /// Model ids, cheapest first. Empty = `models::DEFAULT_LADDER`.
    pub ladder: Vec<String>,
    /// Ask a second model to re-read chunks that were flagged.
    pub second_opinion: bool,
    /// Re-run the whole book with a stronger model when too much is flagged.
    pub whole_book_escalation: bool,
    pub max_output_tokens: u32,
}

impl Default for ExtractOptions {
    fn default() -> Self {
        Self {
            label: String::new(),
            concurrency: 16,
            ladder: Vec::new(),
            second_opinion: true,
            whole_book_escalation: true,
            max_output_tokens: 16_000,
        }
    }
}

/// A cost and time estimate computed before any model call.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "wasm", derive(tsify_next::Tsify))]
#[cfg_attr(feature = "wasm", tsify(into_wasm_abi, from_wasm_abi))]
pub struct Estimate {
    pub chunks: usize,
    pub lines: usize,
    pub chars: usize,
    /// Chunks the cache already answers.
    pub cache_hits: usize,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub calls_low: usize,
    pub calls_high: usize,
    pub wall_ms_low: u64,
    pub wall_ms_high: u64,
    pub cost_usd_low: f64,
    pub cost_usd_high: f64,
    pub ladder: Vec<String>,
    pub concurrency: usize,
    /// Human-readable assumptions the numbers rest on.
    pub assumptions: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "wasm", derive(tsify_next::Tsify))]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Open,
    Clean,
    Nav,
    Chunk,
    Extract,
    Crosscheck,
    SecondOpinion,
    Escalation,
    Assemble,
    Refs,
    Names,
    Parse,
    Done,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "wasm", derive(tsify_next::Tsify))]
pub struct StageTiming {
    pub stage: Phase,
    pub ms: u64,
}

/// Remaining time and cost, as a range.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "wasm", derive(tsify_next::Tsify))]
pub struct Eta {
    pub remaining_low_ms: u64,
    pub remaining_high_ms: u64,
    pub projected_cost_usd: f64,
}

/// Emitted after every completed call.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "wasm", derive(tsify_next::Tsify))]
#[cfg_attr(feature = "wasm", tsify(into_wasm_abi, from_wasm_abi))]
pub struct Progress {
    pub phase: Phase,
    /// Chunks settled (extracted, cached, or failed).
    pub done: usize,
    pub total: usize,
    pub in_flight: usize,
    pub failed: usize,
    pub cached: usize,
    pub recipes_so_far: usize,
    pub cost_so_far_usd: f64,
    pub elapsed_ms: u64,
    pub eta: Eta,
    /// Models currently answering calls.
    pub active_models: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "wasm", derive(tsify_next::Tsify))]
#[serde(rename_all = "snake_case")]
pub enum CallPurpose {
    Extract,
    /// Same model, validation feedback appended.
    Retry,
    /// A different model re-reading a flagged chunk.
    SecondOpinion,
    /// The whole-book re-run with a stronger model.
    Escalation,
    /// Cookbook-or-not classification.
    Classify,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "wasm", derive(tsify_next::Tsify))]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum CallOutcome {
    Ok,
    /// The model answered but the answer failed validation.
    Invalid {
        faults: Vec<String>,
    },
    /// No usable answer: HTTP status, timeout, cancellation.
    Transport {
        kind: String,
        message: String,
    },
}

/// One model call, cached or live.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "wasm", derive(tsify_next::Tsify))]
pub struct CallRecord {
    pub seq: usize,
    pub chunk_id: String,
    pub model: String,
    /// Position of `model` in the ladder.
    pub tier: usize,
    pub purpose: CallPurpose,
    /// 1-based attempt on this chunk with this model.
    pub attempt: usize,
    /// Milliseconds since the run started.
    pub started_ms: u64,
    pub latency_ms: u64,
    pub cached: bool,
    pub status: Option<u16>,
    pub request_id: Option<String>,
    pub usage: Usage,
    /// `None` when the model is unpriced.
    pub cost_usd: Option<f64>,
    pub truncated: bool,
    pub outcome: CallOutcome,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "wasm", derive(tsify_next::Tsify))]
#[serde(rename_all = "snake_case")]
pub enum ChunkStatus {
    Ok,
    /// Every model in the ladder failed; the chunk's lines are unassigned.
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "wasm", derive(tsify_next::Tsify))]
#[serde(rename_all = "snake_case")]
pub enum Chosen {
    First,
    Second,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "wasm", derive(tsify_next::Tsify))]
pub struct SecondOpinion {
    pub model: String,
    pub chosen: Chosen,
    /// The rule that decided.
    pub reason: String,
}

/// A soft signal that a chunk's extraction may be wrong. Flags never fail a
/// chunk; they route it to a second opinion.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "wasm", derive(tsify_next::Tsify))]
#[serde(tag = "flag", rename_all = "snake_case")]
pub enum Flag {
    LowAmountParseRate { rate: f32, lines: usize },
    IngredientLikeIgnored { count: usize },
    RecipeWithoutSteps { title: String },
    MissingNavTitle { title: String, line: usize },
    PhantomTitle { title: String },
    Truncated,
    CaptionAsTitle { line: usize },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "wasm", derive(tsify_next::Tsify))]
pub struct ChunkReport {
    pub id: String,
    /// Global line range, end exclusive.
    pub start: usize,
    pub end: usize,
    pub chars: usize,
    pub status: ChunkStatus,
    pub final_model: Option<String>,
    pub attempts: usize,
    pub flags: Vec<Flag>,
    pub second_opinion: Option<SecondOpinion>,
    pub recipes: usize,
    pub cached: bool,
}

/// Book-level check of extracted titles against the table of contents.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "wasm", derive(tsify_next::Tsify))]
pub struct CrossCheck {
    pub nav_titles: usize,
    pub matched: usize,
    pub missing: Vec<String>,
    pub phantom: Vec<String>,
    /// `None` when the book has too few contents entries to judge.
    pub recall: Option<f32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "wasm", derive(tsify_next::Tsify))]
pub struct Escalation {
    pub reason: String,
    pub flagged_fraction: f32,
    pub from_model: String,
    pub to_model: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "wasm", derive(tsify_next::Tsify))]
pub struct UnresolvedRef {
    pub item_id: String,
    pub line: usize,
    pub text: String,
    pub attempted: Vec<crate::model::RefMethod>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "wasm", derive(tsify_next::Tsify))]
pub struct ModelUsage {
    pub model: String,
    pub calls: usize,
    pub usage: Usage,
    pub cost_usd: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "wasm", derive(tsify_next::Tsify))]
pub struct EtaSample {
    pub elapsed_ms: u64,
    pub remaining_low_ms: u64,
    pub remaining_high_ms: u64,
}

/// The full diagnostic record of a run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "wasm", derive(tsify_next::Tsify))]
pub struct RunReport {
    pub run_id: String,
    /// RFC 3339.
    pub started_at: String,
    pub finished_at: String,
    pub book: BookSource,
    pub options: ExtractOptions,
    /// The estimate taken before the first call, for comparison.
    pub estimate: Estimate,
    pub stages: Vec<StageTiming>,
    pub calls: Vec<CallRecord>,
    pub chunks: Vec<ChunkReport>,
    pub crosscheck: CrossCheck,
    pub escalation: Option<Escalation>,
    pub unresolved_refs: Vec<UnresolvedRef>,
    pub usage_by_model: Vec<ModelUsage>,
    pub total_cost_usd: f64,
    /// `false` when some call used an unpriced model.
    pub cost_complete: bool,
    pub eta_trace: Vec<EtaSample>,
    pub wall_ms: u64,
    /// Some chunk failed every model, or escalation ran out of ladder.
    pub incomplete: bool,
    pub cancelled: bool,
}
