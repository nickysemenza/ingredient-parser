//! Read-only planning shared by both native entry points.
use super::{ReviewRun, RunOptions, error};
use crate::{Chunk, EpubError, Usage};
use serde::Serialize;
#[derive(Debug, Clone, Serialize)]
pub struct Preflight {
    pub recovery: Option<RecoveryPreflight>,
    pub total: usize,
    pub cached: usize,
    pub pending: usize,
    pub estimated_low_usd: Option<f64>,
    pub estimated_high_usd: Option<f64>,
    pub reservation_usd: Option<f64>,
    pub basis: &'static str,
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

fn recovery_plan(
    _run: &ReviewRun,
    options: &RunOptions,
    initial: &crate::recovery::State,
) -> Result<Preflight, EpubError> {
    let mut state = initial.clone();
    let directory = options
        .cache_dir
        .clone()
        .unwrap_or_else(crate::cache::default_dir)
        .join("verified-recovery-v1");
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
            extraction_remaining_usd: (options.budget_usd * 0.8 - state.allocated(false)).max(0.0),
            verification_remaining_usd: (options.budget_usd * 0.2 - state.allocated(true)).max(0.0),
            verification_pending: 0,
            verification_cached: 0,
        }),
        total,
        cached,
        pending: 0,
        estimated_low_usd: Some(0.0),
        estimated_high_usd: Some(0.0),
        reservation_usd: Some(0.0),
        basis: "Approximate initial extraction and verification cost; further recovery is conditional. The spending limit reserves 80% for extraction/recovery and 20% for verification. Token-based estimates, not a billing statement.",
    };
    let mut synthetic = std::collections::HashSet::new();
    while let Some(action) = state.next_action().map_err(error)? {
        let cached = !options.refresh
            && !synthetic.contains(&action.group)
            && std::fs::read(directory.join(format!("{}.json", action.key)))
                .ok()
                .and_then(|bytes| serde_json::from_slice(&bytes).ok())
                .is_some_and(|value| state.apply(&action, value).is_ok());
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
            let output = if action.chunk.is_some() {
                bytes / 3
            } else {
                bytes / 2
            };
            let cost = crate::accounting::cost_for_usage(
                &action.model,
                &Usage {
                    input_tokens: bytes.div_ceil(3),
                    output_tokens: output.clamp(64, 16000),
                    ..Usage::default()
                },
            );
            result.estimated_low_usd = result.estimated_low_usd.zip(cost).map(|(a, b)| a + b * 0.5);
            result.estimated_high_usd = result
                .estimated_high_usd
                .zip(cost)
                .map(|(a, b)| a + b * 2.0);
            result.reservation_usd = result.reservation_usd.map(|a| a + action.reservation_usd);
        }
        synthetic.insert(action.group);
        if let Some(chunk) = action.chunk {
            let group = &mut state.groups[action.group];
            if let Some(pos) = group.chunks.iter().position(|i| *i == chunk) {
                group.candidates[action.candidate].outputs[pos] = Some(vec![]);
            }
        } else {
            // Planning-only placeholder: never persisted or counted as verified.
            if let Some(target) = action.verification_chunk {
                let g = &mut state.groups[action.group];
                let c = &mut g.candidates[action.candidate];
                c.verified_chunks.push(target);
                c.verified = c.verified_chunks.len() == g.chunks.len();
                if c.verified {
                    g.accepted = Some(action.candidate);
                }
            }
        }
    }
    Ok(result)
}
