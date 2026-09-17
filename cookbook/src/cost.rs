//! Token usage and pricing.

use serde::{Deserialize, Serialize};

/// Token counts for one model call (or a sum of calls). Cache fields are zero
/// for providers that do not report them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "wasm", derive(tsify_next::Tsify))]
#[cfg_attr(feature = "wasm", tsify(into_wasm_abi, from_wasm_abi))]
#[serde(default)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_input_tokens: u64,
    pub cache_creation_input_tokens: u64,
    /// Thinking tokens, when the provider reports them; already inside
    /// `output_tokens` for pricing.
    pub reasoning_tokens: u64,
}

impl Usage {
    pub fn add(&mut self, other: &Usage) {
        self.input_tokens += other.input_tokens;
        self.output_tokens += other.output_tokens;
        self.cache_read_input_tokens += other.cache_read_input_tokens;
        self.cache_creation_input_tokens += other.cache_creation_input_tokens;
        self.reasoning_tokens += other.reasoning_tokens;
    }

    pub fn is_zero(&self) -> bool {
        *self == Usage::default()
    }
}

/// USD for `usage` at `model`'s rates; `None` when the model is unpriced.
/// Cache reads and writes are priced as plain input (see [`Rates`]), which
/// overstates a cache-heavy run by about a tenth.
///
/// [`Rates`]: crate::models::Rates
pub fn cost_for_usage(model: &crate::models::Model, usage: &Usage) -> Option<f64> {
    let r = model.rates?;
    let input =
        usage.input_tokens + usage.cache_creation_input_tokens + usage.cache_read_input_tokens;
    Some((input as f64 * r.input + usage.output_tokens as f64 * r.output) / 1e6)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn prices_every_token_class() {
        let usage = Usage {
            input_tokens: 1_000_000,
            output_tokens: 1_000_000,
            cache_read_input_tokens: 1_000_000,
            cache_creation_input_tokens: 1_000_000,
            reasoning_tokens: 0,
        };
        let m = crate::models::model("claude-haiku-4-5").unwrap();
        let r = m.rates.unwrap();
        let cost = cost_for_usage(m, &usage).unwrap();
        assert!((cost - (3.0 * r.input + r.output)).abs() < 1e-9, "{cost}");
        let mut sum = Usage::default();
        sum.add(&usage);
        sum.add(&usage);
        assert_eq!(sum.output_tokens, 2_000_000);
        assert!(Usage::default().is_zero());
    }
}
