//! The model catalog: every model the pipeline may call, with its gateway
//! route, USD rates per million tokens, and throughput priors for estimates.
//!
//! Ids are matched exactly; a new model version never inherits an old rate.
//! `DEFAULT_LADDER` is the automatic order, cheapest first; the eval harness
//! (`food-cli cookbook eval`) chooses it and measures the priors.

use serde::{Deserialize, Serialize};

use crate::gateway::Route;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "wasm", derive(tsify_next::Tsify))]
#[serde(rename_all = "kebab-case")]
pub enum Provider {
    Anthropic,
    OpenAi,
    GoogleAiStudio,
    WorkersAi,
}

impl Provider {
    /// The gateway's provider segment, also the model-name prefix on the
    /// unified chat route.
    pub fn as_str(self) -> &'static str {
        match self {
            Provider::Anthropic => "anthropic",
            Provider::OpenAi => "openai",
            Provider::GoogleAiStudio => "google-ai-studio",
            Provider::WorkersAi => "workers-ai",
        }
    }
}

/// USD per million tokens.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "wasm", derive(tsify_next::Tsify))]
pub struct Rates {
    pub input: f64,
    pub output: f64,
    pub cache_read: f64,
    pub cache_write: f64,
}

/// Throughput priors for the cold estimate. `measured` is the date the harness
/// produced them, or `"unmeasured"` for the conservative default.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "wasm", derive(tsify_next::Tsify))]
pub struct Priors {
    pub ttft_ms_p50: u32,
    pub ttft_ms_p90: u32,
    pub output_tps: f32,
    /// Share of calls that need a retry (validation or transient failure).
    pub retry_rate: f32,
    pub measured: &'static str,
}

pub const UNMEASURED: Priors = Priors {
    ttft_ms_p50: 2_500,
    ttft_ms_p90: 6_000,
    output_tps: 60.0,
    retry_rate: 0.15,
    measured: "unmeasured",
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "wasm", derive(tsify_next::Tsify))]
pub struct Model {
    pub id: &'static str,
    pub label: &'static str,
    pub provider: Provider,
    pub route: Route,
    /// Disabled models stay priced and routable but are never chosen
    /// automatically.
    pub enabled: bool,
    pub status: &'static str,
    pub max_output_tokens: u32,
    pub rates: Option<Rates>,
    pub priors: Priors,
    pub pricing_checked: &'static str,
    pub pricing_source: &'static str,
}

/// Placeholder until the harness picks the ladder (plan step F13).
pub const DEFAULT_LADDER: &[&str] = &["gemini-2.5-flash", "claude-haiku-4-5", "claude-sonnet-5"];

const CF_PRICING: &str = "https://developers.cloudflare.com/workers-ai/platform/pricing/";
const GEMINI_PRICING: &str = "https://ai.google.dev/gemini-api/docs/pricing";
const CLAUDE_PRICING: &str = "https://platform.claude.com/docs/en/about-claude/pricing";

macro_rules! rates {
    ($i:expr, $o:expr, $r:expr, $w:expr) => {
        Some(Rates {
            input: $i,
            output: $o,
            cache_read: $r,
            cache_write: $w,
        })
    };
}

static CATALOG: &[Model] = &[
    Model {
        id: "gemini-2.5-flash-lite",
        label: "Gemini 2.5 Flash-Lite",
        provider: Provider::GoogleAiStudio,
        route: Route::CompatChat,
        enabled: false,
        status: "Disabled: 53% recall on Nothing Fancy (2026-09-11)",
        max_output_tokens: 16_000,
        rates: rates!(0.10, 0.40, 0.01, 0.125),
        priors: Priors {
            ttft_ms_p50: 500,
            ttft_ms_p90: 1500,
            output_tps: 297.0,
            retry_rate: 0.69,
            measured: "2026-09-11 six-book eval",
        },
        pricing_checked: "2026-09-09",
        pricing_source: GEMINI_PRICING,
    },
    Model {
        id: "gemini-2.5-flash",
        label: "Gemini 2.5 Flash",
        provider: Provider::GoogleAiStudio,
        route: Route::CompatChat,
        enabled: true,
        status: "Ladder head: 98% recall on the six-book eval; slow first token",
        max_output_tokens: 16_000,
        rates: rates!(0.30, 2.50, 0.03, 0.375),
        priors: Priors {
            ttft_ms_p50: 13900,
            ttft_ms_p90: 27000,
            output_tps: 148.0,
            retry_rate: 0.23,
            measured: "2026-09-11 six-book eval",
        },
        pricing_checked: "2026-09-09",
        pricing_source: GEMINI_PRICING,
    },
    Model {
        id: "gemini-3.5-flash-lite",
        label: "Gemini 3.5 Flash-Lite",
        provider: Provider::GoogleAiStudio,
        route: Route::CompatChat,
        enabled: false,
        status: "Disabled: not served by the gateway (Google answers 400 Missing Authorization, 2026-09-11)",
        max_output_tokens: 16_000,
        rates: rates!(0.30, 2.50, 0.03, 0.375),
        priors: UNMEASURED,
        pricing_checked: "2026-09-09",
        pricing_source: GEMINI_PRICING,
    },
    Model {
        id: "gemini-3.7-flash",
        label: "Gemini 3.7 Flash",
        provider: Provider::GoogleAiStudio,
        route: Route::CompatChat,
        enabled: false,
        status: "Disabled: not served by the gateway (Google answers 400 Missing Authorization, 2026-09-11)",
        max_output_tokens: 16_000,
        rates: rates!(0.75, 3.75, 0.075, 0.9375),
        priors: UNMEASURED,
        pricing_checked: "2026-09-09",
        pricing_source: GEMINI_PRICING,
    },
    Model {
        id: "claude-haiku-4-5",
        label: "Claude Haiku 4.5",
        provider: Provider::Anthropic,
        route: Route::AnthropicMessages,
        enabled: true,
        status: "Ladder fallback: fast, recovers flagged chunks",
        max_output_tokens: 16_000,
        rates: rates!(1.0, 5.0, 0.10, 1.25),
        priors: Priors {
            ttft_ms_p50: 1450,
            ttft_ms_p90: 2900,
            output_tps: 290.0,
            retry_rate: 0.35,
            measured: "2026-09-11 six-book eval",
        },
        pricing_checked: "2026-09-09",
        pricing_source: CLAUDE_PRICING,
    },
    Model {
        id: "claude-sonnet-5",
        label: "Claude Sonnet 5",
        provider: Provider::Anthropic,
        route: Route::AnthropicMessages,
        enabled: true,
        status: "Escalation candidate; the gateway refused it on 2026-09-11 (Wholesale Rate limited, code 2018)",
        max_output_tokens: 16_000,
        rates: rates!(2.0, 10.0, 0.20, 2.50),
        priors: Priors {
            ttft_ms_p50: 1400,
            ttft_ms_p90: 2800,
            output_tps: 155.0,
            retry_rate: 0.15,
            measured: "2026-09-11 six-book eval",
        },
        pricing_checked: "2026-09-09",
        pricing_source: CLAUDE_PRICING,
    },
    Model {
        id: "gpt-5.6-luna",
        label: "GPT-5.6 Luna",
        provider: Provider::OpenAi,
        route: Route::OpenAiResponses,
        enabled: true,
        status: "Ladder candidate: 95% recall alone, fastest and cheapest",
        max_output_tokens: 16_000,
        rates: rates!(0.20, 1.20, 0.02, 0.25),
        priors: Priors {
            ttft_ms_p50: 630,
            ttft_ms_p90: 5700,
            output_tps: 100.0,
            retry_rate: 0.20,
            measured: "2026-09-11 six-book eval",
        },
        pricing_checked: "2026-09-09",
        pricing_source: "https://developers.openai.com/api/docs/models/gpt-5.6-luna",
    },
    // Workers AI models are priced and routable but disabled until their
    // timeouts on long chunks are resolved.
    Model {
        id: "@cf/zai-org/glm-4.7-flash",
        label: "GLM 4.7 Flash",
        provider: Provider::WorkersAi,
        route: Route::CompatChat,
        enabled: false,
        status: "Disabled: validation failures and timeouts",
        max_output_tokens: 16_000,
        rates: rates!(0.06, 0.40, 0.006, 0.075),
        priors: UNMEASURED,
        pricing_checked: "2026-09-09",
        pricing_source: CF_PRICING,
    },
    Model {
        id: "@cf/zai-org/glm-5.3-flash",
        label: "GLM 5.3 Flash",
        provider: Provider::WorkersAi,
        route: Route::CompatChat,
        enabled: false,
        status: "Disabled: timeouts on long chunks",
        max_output_tokens: 16_000,
        rates: rates!(0.15, 0.50, 0.03, 0.1875),
        priors: UNMEASURED,
        pricing_checked: "2026-09-09",
        pricing_source: CF_PRICING,
    },
    Model {
        id: "@cf/zai-org/glm-5.3",
        label: "GLM 5.3",
        provider: Provider::WorkersAi,
        route: Route::CompatChat,
        enabled: false,
        status: "Disabled: timeouts on long chunks",
        max_output_tokens: 16_000,
        rates: rates!(1.40, 4.40, 0.26, 1.75),
        priors: UNMEASURED,
        pricing_checked: "2026-09-09",
        pricing_source: CF_PRICING,
    },
    Model {
        id: "@cf/deepseek-ai/deepseek-v4-flash-0731",
        label: "DeepSeek V4 Flash",
        provider: Provider::WorkersAi,
        route: Route::CompatChat,
        enabled: false,
        status: "Disabled: timeouts on long chunks",
        max_output_tokens: 16_000,
        rates: rates!(0.44, 1.32, 0.014, 0.55),
        priors: UNMEASURED,
        pricing_checked: "2026-09-09",
        pricing_source: CF_PRICING,
    },
    Model {
        id: "@cf/google/gemma-4-26b-a4b-it",
        label: "Gemma 4 26B",
        provider: Provider::WorkersAi,
        route: Route::CompatChat,
        enabled: false,
        status: "Disabled: unevaluated",
        max_output_tokens: 16_000,
        rates: rates!(0.10, 0.30, 0.01, 0.125),
        priors: UNMEASURED,
        pricing_checked: "2026-09-09",
        pricing_source: CF_PRICING,
    },
    Model {
        id: "@cf/moonshotai/kimi-k2.7-code",
        label: "Kimi K2.7 Code",
        provider: Provider::WorkersAi,
        route: Route::CompatChat,
        enabled: false,
        status: "Disabled: provider and schema failures",
        max_output_tokens: 16_000,
        rates: rates!(0.95, 4.00, 0.19, 1.1875),
        priors: UNMEASURED,
        pricing_checked: "2026-09-09",
        pricing_source: CF_PRICING,
    },
];

/// Every catalog entry, enabled or not.
pub fn catalog() -> &'static [Model] {
    CATALOG
}

/// Exact-id lookup.
pub fn model(id: &str) -> Option<&'static Model> {
    CATALOG.iter().find(|m| m.id == id)
}

pub fn enabled() -> impl Iterator<Item = &'static Model> {
    CATALOG.iter().filter(|m| m.enabled)
}

/// Resolve a ladder of ids, or the default when empty. Unknown ids are an
/// error so a typo never silently drops a tier.
pub fn resolve_ladder(ids: &[String]) -> Result<Vec<&'static Model>, crate::Error> {
    let ids: Vec<&str> = if ids.is_empty() {
        DEFAULT_LADDER.to_vec()
    } else {
        ids.iter().map(String::as_str).collect()
    };
    ids.into_iter()
        .map(|id| model(id).ok_or_else(|| crate::Error::UnknownModel(id.to_string())))
        .collect()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    #[test]
    fn catalog_is_well_formed() {
        let mut ids = std::collections::HashSet::new();
        for m in catalog() {
            assert!(ids.insert(m.id), "duplicate id {}", m.id);
            assert!(!m.label.is_empty());
            assert!(m.max_output_tokens > 0);
            assert!(m.pricing_source.starts_with("https://"));
            assert_eq!(m.pricing_checked.len(), 10);
            let route_ok = match m.provider {
                Provider::Anthropic => m.route == Route::AnthropicMessages,
                Provider::GoogleAiStudio | Provider::WorkersAi => m.route == Route::CompatChat,
                Provider::OpenAi => matches!(m.route, Route::OpenAiChat | Route::OpenAiResponses),
            };
            assert!(route_ok, "{} routes wrongly", m.id);
            let r = m.rates.expect("every catalog model is priced");
            assert!(
                [r.input, r.output, r.cache_read, r.cache_write]
                    .iter()
                    .all(|n| n.is_finite() && *n >= 0.0)
            );
            assert!(m.priors.output_tps > 0.0);
        }
    }

    #[test]
    fn default_ladder_is_enabled_and_priced() {
        let ladder = resolve_ladder(&[]).unwrap();
        assert_eq!(ladder.len(), DEFAULT_LADDER.len());
        assert!(ladder.iter().all(|m| m.enabled && m.rates.is_some()));
        assert!(matches!(
            resolve_ladder(&["nope".into()]),
            Err(crate::Error::UnknownModel(_))
        ));
        assert!(enabled().all(|m| m.provider != Provider::WorkersAi));
        assert_eq!(
            model("gpt-4o"),
            None,
            "legacy ids are gone, not silently priced"
        );
    }
}
