//! Measure warm, native cookbook preflight on a supplied development EPUB.
//!
//! The example never dispatches extraction: `extraction_preview` only inspects
//! the EPUB and builds a local preflight plan. Its output deliberately contains
//! counts, timing, and planner basis only—never source text or recipe prose.

use food_app::backend::{self, ExtractionPreview, ExtractionRequest};
use serde::Serialize;
use std::{
    collections::BTreeSet,
    env, fs,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

const SAMPLE_COUNT: usize = 100;

#[derive(Serialize)]
struct TimingSummary {
    p50_ms: f64,
    p95_ms: f64,
    max_ms: f64,
}

#[derive(Serialize)]
struct Counts {
    total: usize,
    cached: usize,
    pending: usize,
    distinct_observations: usize,
}

#[derive(Serialize)]
struct Report {
    samples: usize,
    warmup_backend_ms: f64,
    backend_ms: TimingSummary,
    elapsed_ms: TimingSummary,
    counts: Counts,
    bases: Vec<String>,
    policies: Vec<Vec<String>>,
    shared_cli_plan_parity: bool,
    measurement: &'static str,
}

fn percentile(sorted: &[Duration], index: usize) -> f64 {
    sorted[index].as_secs_f64() * 1_000.0
}

fn summarize(mut samples: Vec<Duration>) -> TimingSummary {
    samples.sort_unstable();
    TimingSummary {
        // Nearest-rank values for exactly 100 changes: 50th and 95th samples.
        p50_ms: percentile(&samples, 49),
        p95_ms: percentile(&samples, 94),
        max_ms: percentile(&samples, 99),
    }
}

fn args() -> Result<PathBuf, String> {
    let mut values = env::args().skip(1);
    match (values.next(), values.next()) {
        (Some(value), None) if value != "--help" => Ok(value.into()),
        _ => Err("usage: benchmark_preflight DEVELOPMENT.epub".into()),
    }
}

fn request(book: &Path, model: &str, budget_usd: f64, concurrency: u8) -> ExtractionRequest {
    ExtractionRequest {
        book: book.to_string_lossy().into_owned(),
        out: String::new(),
        model: model.into(),
        resume: false,
        from: None,
        allow_network: false,
        refresh: false,
        cache_dir: None,
        chunks: vec![],
        budget_usd,
        strategy: recipe_epub::hybrid::HybridStrategy::Indexed,
        // Preserve the desktop DTO shape; the backend clamps this to 1..=8.
        concurrency: Some(concurrency),
    }
}

fn shared_cli_request(request: &ExtractionRequest) -> recipe_epub::review::ExtractionRequest {
    recipe_epub::review::ExtractionRequest {
        book: request.book.clone().into(),
        out: request.out.clone().into(),
        model: request.model.clone(),
        resume: request.resume,
        from: request.from.clone().map(Into::into),
        options: recipe_epub::review::RunOptions {
            allow_network: request.allow_network,
            refresh: request.refresh,
            cache_dir: request.cache_dir.clone().map(Into::into),
            chunks: request.chunks.clone(),
            budget_usd: request.budget_usd,
            strategy: request.strategy,
            concurrency: usize::from(request.concurrency.unwrap_or(4).clamp(1, 8)),
        },
    }
}

/// The desktop preview is allowed to have different DTO field names and timing,
/// but its shared source plan must equal the CLI workflow's plan exactly.
fn has_shared_cli_plan_parity(
    preview: &ExtractionPreview,
    desktop_request: &ExtractionRequest,
) -> Result<bool, String> {
    let cli_request = shared_cli_request(desktop_request);
    let run = recipe_epub::review::prepare(&cli_request).map_err(|error| error.to_string())?;
    let plan = recipe_epub::review::preflight::plan(&run, &cli_request.options)
        .map_err(|error| error.to_string())?;
    Ok(preview.total == plan.total
        && preview.cached == plan.cached
        && preview.pending == plan.pending
        && preview.extraction == plan.extraction
        && preview.verification == plan.verification
        && preview.reservation_usd == plan.reservation_usd)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let epub = args().map_err(std::io::Error::other)?;
    if epub.extension().and_then(|extension| extension.to_str()) != Some("epub") {
        return Err("benchmark input must be an EPUB file".into());
    }
    if !fs::metadata(&epub)?.is_file() {
        return Err("benchmark input must be a readable EPUB file".into());
    }

    let warmup = backend::extraction_preview(request(&epub, "gemini-2.5-flash", 4.0, 4))?;
    let warmup_backend_ms = warmup
        .warm_ms
        .ok_or("preview did not report backend timing")?;

    let models = [
        "gemini-2.5-flash",
        "@cf/zai-org/glm-5.3-flash",
        "@cf/moonshotai/kimi-k2.7-code",
        recipe_epub::recovery::AUTOMATIC,
    ];
    let budgets = [1.0, 2.5, 4.0, 6.0, 10.0];
    let mut backend_samples = Vec::with_capacity(SAMPLE_COUNT);
    let mut elapsed_samples = Vec::with_capacity(SAMPLE_COUNT);
    let mut observations = BTreeSet::new();
    let mut bases = BTreeSet::new();
    let mut policies = BTreeSet::new();
    let mut last: Option<ExtractionPreview> = None;
    let mut final_request: Option<ExtractionRequest> = None;

    for index in 0..SAMPLE_COUNT {
        let started = Instant::now();
        let option_change = request(
            &epub,
            models[index % models.len()],
            budgets[index % budgets.len()],
            (index % 8 + 1) as u8,
        );
        let preview = backend::extraction_preview(option_change.clone())?;
        elapsed_samples.push(started.elapsed());
        backend_samples.push(Duration::from_secs_f64(
            preview
                .warm_ms
                .ok_or("preview did not report backend timing")?
                / 1_000.0,
        ));
        observations.insert((preview.total, preview.cached, preview.pending));
        bases.insert(preview.basis.clone());
        if let Some(policy) = &preview.policy {
            policies.insert(policy.clone());
        }
        last = Some(preview);
        final_request = Some(option_change);
    }
    let last = last.ok_or("no benchmark samples")?;
    let final_request = final_request.ok_or("no benchmark request")?;
    let shared_cli_plan_parity =
        has_shared_cli_plan_parity(&last, &final_request).map_err(std::io::Error::other)?;
    if !shared_cli_plan_parity {
        return Err("desktop preview differs from the shared CLI preflight plan".into());
    }
    let report = Report {
        samples: SAMPLE_COUNT,
        warmup_backend_ms,
        backend_ms: summarize(backend_samples),
        elapsed_ms: summarize(elapsed_samples),
        counts: Counts {
            total: last.total,
            cached: last.cached,
            pending: last.pending,
            distinct_observations: observations.len(),
        },
        bases: bases.into_iter().collect(),
        policies: policies.into_iter().collect(),
        shared_cli_plan_parity,
        measurement: "warm cache; 100 option changes after one warmup; UI debounce excluded; no provider dispatch",
    };
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}
