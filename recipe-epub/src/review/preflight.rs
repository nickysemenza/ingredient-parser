//! Read-only planning shared by both native entry points.
use super::{ReviewRun, RunOptions, error};
use crate::{Chunk, EpubError, Usage};
use serde::Serialize;
#[derive(Debug, Clone, Serialize)]
pub struct Preflight {
    pub total: usize,
    pub cached: usize,
    pub pending: usize,
    pub estimated_low_usd: Option<f64>,
    pub estimated_high_usd: Option<f64>,
    pub reservation_usd: Option<f64>,
    pub basis: &'static str,
}
pub fn reservation(model: &str, source: &Chunk) -> Result<f64, EpubError> {
    let bytes = serde_json::to_vec(&crate::indexed::build_indexed_chunk_request(source))?.len()
        as u64
        + 4096;
    crate::accounting::cost_for_usage(
        model,
        &Usage {
            input_tokens: bytes * 2,
            output_tokens: 32000,
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
    let directory = options
        .cache_dir
        .clone()
        .unwrap_or_else(crate::cache::default_dir);
    let mut result = Preflight {
        total: 0,
        cached: 0,
        pending: 0,
        estimated_low_usd: Some(0.0),
        estimated_high_usd: Some(0.0),
        reservation_usd: Some(0.0),
        basis: "Approximate range calibrated on a small cookbook sample (2026-09-09); includes possible retries. Token-based estimate, not a billing statement; excludes credit-purchase fees.",
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
        result.estimated_low_usd = result
            .estimated_low_usd
            .zip(cost(request_bytes / 5, (lines * 5).max(16)))
            .map(|(a, b)| a + b);
        result.estimated_high_usd = result
            .estimated_high_usd
            .zip(cost(request_bytes * 2 / 3, (lines * 80).clamp(64, 32000)))
            .map(|(a, b)| a + b);
        result.reservation_usd = result
            .reservation_usd
            .zip(reservation(&run.model, &c.source).ok())
            .map(|(a, b)| a + b);
    }
    Ok(result)
}
