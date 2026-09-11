//! Time and cost estimates: a cold estimate before the first call, refined
//! live from observed latencies.
//!
//! Token counts come from chunk size; latencies from the catalog's priors
//! until a few real calls have been observed, then from an exponential moving
//! average per model. Every number is a range with its assumptions spelled
//! out, because a book's chunks vary and a second opinion or an escalation can
//! double the work.

use std::collections::HashMap;

use crate::chunk::Chunk;
use crate::cost::{Usage, cost_for_usage};
use crate::models::Model;
use crate::report::{Estimate, Eta};

/// Share of chunks assumed to need a second opinion in the high estimate.
const SECOND_OPINION_SHARE: f64 = 0.25;
/// Retries assumed in the low estimate.
const LOW_RETRY_SHARE: f64 = 0.10;

/// Tokens the model will read and write for one chunk. Input is the chunk
/// text plus the prompt and schema; output is index lists, a few tokens per
/// claimed line.
pub fn chunk_tokens(chunk: &Chunk) -> (u64, u64) {
    let input = (chunk.chars as f64 / 3.6) as u64 + 1_400;
    let output = 80 + (3.4 * chunk.lines() as f64) as u64;
    (input, output)
}

fn call_ms(model: &Model, output_tokens: u64, p90: bool) -> f64 {
    let ttft = if p90 {
        model.priors.ttft_ms_p90
    } else {
        model.priors.ttft_ms_p50
    } as f64;
    ttft + output_tokens as f64 / model.priors.output_tps as f64 * 1000.0
}

fn call_cost(model: &Model, input: u64, output: u64) -> f64 {
    cost_for_usage(
        model,
        &Usage {
            input_tokens: input,
            output_tokens: output,
            ..Usage::default()
        },
    )
    .unwrap_or(0.0)
}

/// The estimate shown before any call. `cache_hits` chunks are assumed free
/// and instant.
pub fn cold_estimate(
    chunks: &[Chunk],
    ladder: &[&Model],
    concurrency: usize,
    cache_hits: usize,
) -> Estimate {
    let concurrency = concurrency.max(1);
    let primary = ladder.first();
    let second = ladder.get(1).or(primary);
    let tokens: Vec<(u64, u64)> = chunks.iter().map(chunk_tokens).collect();
    let (input_tokens, output_tokens) = tokens
        .iter()
        .fold((0, 0), |(i, o), (ci, co)| (i + ci, o + co));
    let pending = chunks.len().saturating_sub(cache_hits);
    let mut assumptions = Vec::new();
    let (Some(primary), Some(second)) = (primary, second) else {
        assumptions.push("no models in the ladder".to_string());
        return Estimate {
            chunks: chunks.len(),
            lines: chunks.iter().map(Chunk::lines).sum(),
            chars: chunks.iter().map(|c| c.chars).sum(),
            cache_hits,
            input_tokens,
            output_tokens,
            calls_low: 0,
            calls_high: 0,
            wall_ms_low: 0,
            wall_ms_high: 0,
            cost_usd_low: 0.0,
            cost_usd_high: 0.0,
            ladder: Vec::new(),
            concurrency,
            assumptions,
        };
    };
    // Only chunks that still need a call cost anything; take the largest
    // `pending` chunks as the pessimistic set for the high bound.
    let mut pending_tokens: Vec<(u64, u64)> = tokens.clone();
    pending_tokens.sort_by_key(|(i, _)| std::cmp::Reverse(*i));
    pending_tokens.truncate(pending);
    let outputs: Vec<u64> = pending_tokens.iter().map(|(_, o)| *o).collect();
    let median_out = median(&outputs);
    let max_out = outputs.iter().copied().max().unwrap_or(0);
    let retry = primary.priors.retry_rate as f64;

    let calls_low = pending + (LOW_RETRY_SHARE * pending as f64).ceil() as usize;
    let calls_high = pending
        + (retry * pending as f64).ceil() as usize
        + (SECOND_OPINION_SHARE * pending as f64).ceil() as usize;
    let waves_low = pending.div_ceil(concurrency) as f64;
    let waves_high = calls_high.div_ceil(concurrency) as f64;
    let wall_ms_low = (waves_low * call_ms(primary, median_out, false)) as u64;
    let wall_ms_high =
        (waves_high * call_ms(primary, max_out, true) + call_ms(second, max_out, true)) as u64;

    let cost_low: f64 = pending_tokens
        .iter()
        .map(|(i, o)| call_cost(primary, *i, *o))
        .sum();
    let second_cost: f64 = pending_tokens
        .iter()
        .map(|(i, o)| call_cost(second, *i, *o))
        .sum();
    let cost_high = cost_low * (1.0 + retry) + SECOND_OPINION_SHARE * second_cost;

    for m in [primary, second] {
        assumptions.push(format!(
            "{}: {:.1} s to first token, {:.0} tok/s, {:.0}% retries ({})",
            m.id,
            m.priors.ttft_ms_p50 as f64 / 1000.0,
            m.priors.output_tps,
            m.priors.retry_rate * 100.0,
            m.priors.measured
        ));
        if m.id == second.id && primary.id == second.id {
            break;
        }
    }
    assumptions.push(format!(
        "at most {}% of chunks need a second opinion",
        (SECOND_OPINION_SHARE * 100.0) as u32
    ));
    assumptions.push("no whole-book escalation".to_string());
    if cache_hits > 0 {
        assumptions.push(format!(
            "{cache_hits} of {} chunks answered from the cache",
            chunks.len()
        ));
    }
    Estimate {
        chunks: chunks.len(),
        lines: chunks.iter().map(Chunk::lines).sum(),
        chars: chunks.iter().map(|c| c.chars).sum(),
        cache_hits,
        input_tokens,
        output_tokens,
        calls_low,
        calls_high,
        wall_ms_low,
        wall_ms_high,
        cost_usd_low: cost_low,
        cost_usd_high: cost_high,
        ladder: ladder.iter().map(|m| m.id.to_string()).collect(),
        concurrency,
        assumptions,
    }
}

fn median(values: &[u64]) -> u64 {
    if values.is_empty() {
        return 0;
    }
    let mut v = values.to_vec();
    v.sort_unstable();
    v[v.len() / 2]
}

/// Live refinement. Per-model latency starts at the prior and becomes an
/// exponential moving average once three calls have been seen.
#[derive(Debug, Clone)]
pub struct EtaTracker {
    ewma: HashMap<String, (f64, usize)>,
    settled: usize,
    flagged: usize,
}

const EWMA_KEEP: f64 = 0.7;
const MIN_SAMPLES: usize = 3;

impl EtaTracker {
    pub fn new() -> Self {
        Self {
            ewma: HashMap::new(),
            settled: 0,
            flagged: 0,
        }
    }

    pub fn record_call(&mut self, model: &str, latency_ms: u64) {
        let entry = self
            .ewma
            .entry(model.to_string())
            .or_insert((latency_ms as f64, 0));
        entry.0 = if entry.1 == 0 {
            latency_ms as f64
        } else {
            EWMA_KEEP * entry.0 + (1.0 - EWMA_KEEP) * latency_ms as f64
        };
        entry.1 += 1;
    }

    pub fn record_chunk(&mut self, flagged: bool) {
        self.settled += 1;
        if flagged {
            self.flagged += 1;
        }
    }

    /// Expected latency for one call to `model`: the observed average after
    /// enough samples, else the prior.
    pub fn latency_ms(&self, model: &Model, output_tokens: u64) -> f64 {
        match self.ewma.get(model.id) {
            Some((ewma, n)) if *n >= MIN_SAMPLES => *ewma,
            _ => call_ms(model, output_tokens, false),
        }
    }

    pub fn flagged_rate(&self) -> f64 {
        if self.settled == 0 {
            SECOND_OPINION_SHARE
        } else {
            self.flagged as f64 / self.settled as f64
        }
    }

    /// Remaining time and projected final cost given the current state.
    pub fn remaining(&self, state: &RemainingInput<'_>) -> Eta {
        let concurrency = state.concurrency.max(1) as f64;
        let ladder = state.ladder;
        let (Some(primary), Some(second)) = (ladder.first(), ladder.get(1).or(ladder.first()))
        else {
            return Eta {
                remaining_low_ms: 0,
                remaining_high_ms: 0,
                projected_cost_usd: state.cost_so_far,
            };
        };
        let primary_ms = self.latency_ms(primary, state.typical_output_tokens);
        let second_ms = self.latency_ms(second, state.typical_output_tokens);
        let remaining = state.remaining as f64;
        let waves = (remaining / concurrency).ceil();
        let low = waves * primary_ms;
        let tail = state
            .oldest_in_flight_ms
            .map(|age| (primary_ms - age as f64).max(0.0))
            .unwrap_or(0.0);
        let high = low + self.flagged_rate() * remaining * second_ms / concurrency + tail;
        let per_chunk = if state.settled_chunks > 0 {
            state.cost_so_far / state.settled_chunks as f64
        } else {
            0.0
        };
        Eta {
            remaining_low_ms: low as u64,
            remaining_high_ms: high.max(low) as u64,
            projected_cost_usd: state.cost_so_far + per_chunk * remaining,
        }
    }
}

/// What [`EtaTracker::remaining`] needs to know about the run right now.
#[derive(Debug, Clone, Copy)]
pub struct RemainingInput<'a> {
    /// Chunks not yet settled.
    pub remaining: usize,
    /// Age of the oldest call in flight.
    pub oldest_in_flight_ms: Option<u64>,
    pub concurrency: usize,
    pub ladder: &'a [&'static Model],
    pub typical_output_tokens: u64,
    pub cost_so_far: f64,
    pub settled_chunks: usize,
}

impl Default for EtaTracker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::chunk::Boundary;
    use crate::models::model;

    fn chunk(index: usize, lines: usize, chars: usize) -> Chunk {
        Chunk {
            id: format!("k{index:03}"),
            index,
            start: 0,
            end: lines,
            chars,
            title_hint: None,
            boundary: Boundary::Start,
        }
    }

    fn ladder() -> Vec<&'static Model> {
        vec![
            model("gemini-2.5-flash").unwrap(),
            model("claude-haiku-4-5").unwrap(),
        ]
    }

    #[test]
    fn cold_estimate_scales_with_chunks_and_shrinks_with_cache() {
        let one: Vec<Chunk> = (0..10).map(|i| chunk(i, 100, 12_000)).collect();
        let e1 = cold_estimate(&one, &ladder(), 16, 0);
        let two: Vec<Chunk> = (0..20).map(|i| chunk(i, 100, 12_000)).collect();
        let e2 = cold_estimate(&two, &ladder(), 16, 0);
        assert!(e2.cost_usd_low > e1.cost_usd_low);
        assert!(e2.wall_ms_high >= e1.wall_ms_high);
        assert!(e1.wall_ms_low <= e1.wall_ms_high);
        assert!(e1.cost_usd_low <= e1.cost_usd_high);
        assert_eq!(e1.calls_low, 11);
        assert!(e1.calls_high > e1.calls_low);
        let cached = cold_estimate(&one, &ladder(), 16, 10);
        assert_eq!(cached.cost_usd_low, 0.0);
        assert_eq!(cached.wall_ms_low, 0);
        assert!(
            cached
                .assumptions
                .iter()
                .any(|a| a.contains("answered from the cache"))
        );
        assert_eq!(e1.ladder, ["gemini-2.5-flash", "claude-haiku-4-5"]);
        assert!(e1.assumptions.iter().any(|a| a.contains("unmeasured")));
        // 10 chunks at concurrency 16: one wave.
        assert_eq!(
            e1.wall_ms_low,
            cold_estimate(&one, &ladder(), 10, 0).wall_ms_low
        );
        assert!(cold_estimate(&one, &ladder(), 1, 0).wall_ms_low > e1.wall_ms_low);
    }

    #[test]
    fn tokens_follow_chunk_size() {
        let (i, o) = chunk_tokens(&chunk(0, 100, 12_000));
        assert_eq!(i, 3_333 + 1_400);
        assert_eq!(o, 80 + 340);
    }

    #[test]
    fn tracker_converges_to_observations() {
        let mut t = EtaTracker::new();
        let m = model("gemini-2.5-flash").unwrap();
        let prior = t.latency_ms(m, 400);
        t.record_call(m.id, 1000);
        t.record_call(m.id, 1000);
        assert_eq!(t.latency_ms(m, 400), prior, "prior until three samples");
        t.record_call(m.id, 1000);
        assert!((t.latency_ms(m, 400) - 1000.0).abs() < 1e-9);
        for _ in 0..20 {
            t.record_call(m.id, 3000);
        }
        assert!(t.latency_ms(m, 400) > 2900.0);
        t.record_chunk(true);
        t.record_chunk(false);
        assert!((t.flagged_rate() - 0.5).abs() < 1e-9);
        let l = ladder();
        let eta = t.remaining(&RemainingInput {
            remaining: 8,
            oldest_in_flight_ms: Some(500),
            concurrency: 4,
            ladder: &l,
            typical_output_tokens: 400,
            cost_so_far: 0.10,
            settled_chunks: 2,
        });
        assert!(eta.remaining_low_ms > 0);
        assert!(eta.remaining_high_ms >= eta.remaining_low_ms);
        assert!(
            (eta.projected_cost_usd - 0.50).abs() < 1e-9,
            "0.10 over 2 chunks → 0.05 each × 8 more"
        );
        let idle = t.remaining(&RemainingInput {
            remaining: 0,
            oldest_in_flight_ms: None,
            concurrency: 4,
            ladder: &l,
            typical_output_tokens: 400,
            cost_so_far: 0.10,
            settled_chunks: 2,
        });
        assert_eq!(idle.remaining_low_ms, 0);
    }
}
