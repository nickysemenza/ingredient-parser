//! The model catalog: every model the pipeline may call, with its gateway
//! route and USD rates per million tokens.
//!
//! Ids are matched exactly; a new model version never inherits an old rate.
//! Rates come from `llm_models_spider`'s generated table (LiteLLM and
//! OpenRouter, refreshed daily upstream) at compile time, so a `cargo update`
//! reprices the catalog; a model that table does not know is unpriced unless
//! its entry carries an explicit, dated rate. Those prices are approximate:
//! the table merges by bare model name, so a regional or reseller row can win
//! (Haiku 4.5 lists 10% over Anthropic's first-party price), and it carries
//! no prompt-cache tiers.
//! `DEFAULT_LADDER` is the automatic order, cheapest first; the eval harness
//! (`food-cli cookbook eval`) chooses it. `status` records each model's
//! verdict from that harness; nothing gates on it.

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

/// USD per million tokens. Cache reads and writes are priced as input; the
/// source table has no cache tiers.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "wasm", derive(tsify_next::Tsify))]
pub struct Rates {
    pub input: f64,
    pub output: f64,
}

/// How much a model may think before answering. `Default` sends nothing and
/// leaves the provider's default (Gemini 2.5 Flash thinks for ~2,300 tokens
/// and 8–10 s per chunk); `Off` sends `reasoning_effort: "none"`.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    Default,
    Serialize,
    Deserialize,
    strum::EnumString,
    strum::Display,
)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "wasm", derive(tsify_next::Tsify))]
#[serde(rename_all = "snake_case")]
#[strum(ascii_case_insensitive, serialize_all = "lowercase")]
pub enum Reasoning {
    #[default]
    Default,
    /// Parsed from `none` or `off`; printed and sent as `none`.
    #[strum(serialize = "none", serialize = "off", to_string = "none")]
    Off,
    Low,
    Medium,
    High,
}

impl Reasoning {
    /// The `reasoning_effort` value to send, `None` for the provider default.
    pub fn effort(self) -> Option<&'static str> {
        match self {
            Reasoning::Default => None,
            Reasoning::Off => Some("none"),
            Reasoning::Low => Some("low"),
            Reasoning::Medium => Some("medium"),
            Reasoning::High => Some("high"),
        }
    }

    #[cfg(test)]
    fn parses(s: &str) -> Option<Reasoning> {
        s.parse().ok()
    }
}

#[cfg(test)]
mod reasoning_tests {
    use super::Reasoning;

    #[test]
    fn reasoning_parses_aliases_case_insensitively_and_prints_the_wire_value() {
        assert_eq!(Reasoning::parses("none"), Some(Reasoning::Off));
        assert_eq!(Reasoning::parses("OFF"), Some(Reasoning::Off));
        assert_eq!(Reasoning::parses("Low"), Some(Reasoning::Low));
        assert_eq!(Reasoning::parses("default"), Some(Reasoning::Default));
        assert_eq!(Reasoning::parses("maximum"), None);
        assert_eq!(Reasoning::Off.to_string(), "none");
        assert_eq!(Reasoning::Default.to_string(), "default");
        assert_eq!(Reasoning::High.to_string(), "high");
    }
}

/// The setting a run uses for `model`: the option wins over the catalog.
pub fn effective_reasoning(model: &Model, options: &crate::report::ExtractOptions) -> Reasoning {
    options.reasoning.unwrap_or(model.reasoning)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "wasm", derive(tsify_next::Tsify))]
pub struct Model {
    pub id: &'static str,
    pub provider: Provider,
    pub route: Route,
    /// How much the model may think; see [`Reasoning`].
    pub reasoning: Reasoning,
    /// The eval harness's verdict, for `food-cli cookbook models`.
    pub status: &'static str,
    /// `None` for a model the pricing table does not list.
    pub rates: Option<Rates>,
}

/// The most output tokens one call may produce, for every model.
pub const MAX_OUTPUT_TOKENS: u32 = 16_000;

/// Placeholder until the harness picks the ladder (plan step F13).
/// Chosen on 2026-09-11 over the six answer-key books (see
/// docs/cookbook-ladder-2026-09-11.md): Gemini 2.5 Flash reads best, GPT 5.6
/// Luna is the fast, cheap second reader for retries and second opinions,
/// Haiku 4.5 the last resort. Claude Sonnet 5 was refused by the gateway.
pub const DEFAULT_LADDER: &[&str] = &["gemini-2.5-flash", "gpt-5.6-luna", "claude-haiku-4-5"];

/// The pricing table's rates for `id`, looked up at compile time so the
/// table itself is never linked. A Workers AI id (`@cf/org/name`) is looked
/// up by its bare name, which is the vendor's own price, not Cloudflare's.
/// Unknown ids and zero (unknown) prices are `None`.
const fn listed(id: &str) -> Option<Rates> {
    let bytes = id.as_bytes();
    let mut start = 0;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'/' {
            start = i + 1;
        }
        i += 1;
    }
    let (_, name) = bytes.split_at(start);
    let table = llm_models_spider::MODEL_INFO;
    let mut i = 0;
    while i < table.len() {
        let entry = &table[i];
        if bytes_eq(entry.name.as_bytes(), name) {
            if entry.cost_input_x1000 == 0 || entry.cost_output_x1000 == 0 {
                return None;
            }
            return Some(Rates {
                input: entry.cost_input_x1000 as f64 / 1000.0,
                output: entry.cost_output_x1000 as f64 / 1000.0,
            });
        }
        i += 1;
    }
    None
}

const fn bytes_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
}

static CATALOG: &[Model] = &[
    Model {
        id: "gemini-2.5-flash-lite",
        provider: Provider::GoogleAiStudio,
        route: Route::CompatChat,
        reasoning: Reasoning::Default,
        status: "Disabled: 53% recall on Nothing Fancy (2026-09-11)",
        rates: listed("gemini-2.5-flash-lite"),
    },
    Model {
        id: "gemini-2.5-flash",
        provider: Provider::GoogleAiStudio,
        route: Route::CompatChat,
        reasoning: Reasoning::Low,
        status: "Ladder head: 98% recall on the six-book eval at low reasoning (its default thinking doubled every call's latency for the same recall; no thinking lost 4 points)",
        rates: listed("gemini-2.5-flash"),
    },
    Model {
        id: "gemini-3.5-flash-lite",
        provider: Provider::GoogleAiStudio,
        route: Route::CompatChat,
        reasoning: Reasoning::Default,
        status: "Disabled: not served by the gateway (Google answers 400 Missing Authorization, 2026-09-11)",
        rates: listed("gemini-3.5-flash-lite"),
    },
    Model {
        id: "gemini-3.7-flash",
        provider: Provider::GoogleAiStudio,
        route: Route::CompatChat,
        reasoning: Reasoning::Default,
        status: "Disabled: not served by the gateway (Google answers 400 Missing Authorization, 2026-09-11)",
        rates: listed("gemini-3.7-flash"),
    },
    Model {
        id: "claude-haiku-4-5",
        provider: Provider::Anthropic,
        route: Route::AnthropicMessages,
        reasoning: Reasoning::Default,
        status: "Ladder fallback: fast, recovers flagged chunks",
        rates: listed("claude-haiku-4-5"),
    },
    Model {
        id: "claude-sonnet-5",
        provider: Provider::Anthropic,
        route: Route::AnthropicMessages,
        reasoning: Reasoning::Default,
        status: "Most accurate and fastest single reader measured (99% recall on Nothing Fancy in 19 s), at four times Gemini's price; its wholesale pool meters tokens per minute and refuses bursts with 429 code 2018",
        rates: listed("claude-sonnet-5"),
    },
    Model {
        id: "gpt-5.6-luna",
        provider: Provider::OpenAi,
        route: Route::OpenAiResponses,
        reasoning: Reasoning::Default,
        status: "Ladder candidate: 95% recall alone, fastest and cheapest",
        rates: listed("gpt-5.6-luna"),
    },
    // Workers AI models are priced and routable but off the ladder. Probed on
    // Nothing Fancy on 2026-09-11 (single model, no second opinion or
    // escalation): they think before answering, so a chunk takes 50-120 s
    // at the median and a few chunks per book hit the 180 s transport
    // timeout; a book takes 4-9 minutes.
    Model {
        id: "@cf/zai-org/glm-4.7-flash",
        provider: Provider::WorkersAi,
        route: Route::CompatChat,
        reasoning: Reasoning::Default,
        status: "Disabled: 0% recall on Nothing Fancy; 44 of 58 answers leave lines unassigned and 12 time out (464 s per book)",
        rates: listed("@cf/zai-org/glm-4.7-flash"),
    },
    Model {
        id: "@cf/zai-org/glm-5.3-flash",
        provider: Provider::WorkersAi,
        route: Route::CompatChat,
        reasoning: Reasoning::Default,
        status: "Disabled: 98% recall on Nothing Fancy at $0.07, but 49 s per chunk at the median and timeouts on long chunks (277 s per book)",
        rates: listed("@cf/zai-org/glm-5.3-flash"),
    },
    Model {
        id: "@cf/zai-org/glm-5.3",
        provider: Provider::WorkersAi,
        route: Route::CompatChat,
        reasoning: Reasoning::Default,
        status: "Disabled: 99% recall on Nothing Fancy but $0.82 per book, 57 s per chunk at the median and timeouts on long chunks (333 s per book)",
        rates: listed("@cf/zai-org/glm-5.3"),
    },
    Model {
        id: "@cf/deepseek-ai/deepseek-v4-flash-0731",
        provider: Provider::WorkersAi,
        route: Route::CompatChat,
        reasoning: Reasoning::Default,
        status: "Disabled: 100% recall on Nothing Fancy at $0.22 with no phantoms, but 54 s per chunk at the median and a timeout on long chunks (278 s per book)",
        rates: listed("@cf/deepseek-ai/deepseek-v4-flash-0731"),
    },
    Model {
        id: "@cf/google/gemma-4-26b-a4b-it",
        provider: Provider::WorkersAi,
        route: Route::CompatChat,
        reasoning: Reasoning::Default,
        status: "Disabled: 46% recall on Nothing Fancy; 28 of 49 answers invalid (no tool call, doubled lines), 122 s per chunk at the median, 6 timeouts (512 s per book)",
        rates: listed("@cf/google/gemma-4-26b-a4b-it"),
    },
    Model {
        id: "@cf/moonshotai/kimi-k2.7-code",
        provider: Provider::WorkersAi,
        route: Route::CompatChat,
        reasoning: Reasoning::Default,
        status: "Disabled: 95% recall on Nothing Fancy, 53 s per chunk at the median, timeouts and a 402 on long chunks (272 s per book)",
        rates: listed("@cf/moonshotai/kimi-k2.7-code"),
    },
    Model {
        id: "typesafe/jev",
        provider: Provider::WorkersAi,
        route: Route::WorkersAiRun,
        reasoning: Reasoning::Default,
        status: "Not a chat model: TypeSafe's closed-set decision model, listed so consumers can price it; the pipeline never calls it",
        // Not in llm_models_spider, and `listed` would read its free output
        // (one forward pass, no generation) as unknown. Cloudflare's price,
        // 2026-09-18.
        rates: Some(Rates {
            input: 0.042,
            output: 0.0,
        }),
    },
];

/// Every gateway model, on the ladder or not.
pub fn catalog() -> &'static [Model] {
    CATALOG
}

/// Exact-id lookup.
pub fn model(id: &str) -> Option<&'static Model> {
    CATALOG
        .iter()
        .chain(LOCAL_MODELS.iter())
        .find(|m| m.id == id)
}

/// Resolve a ladder of ids, or the default when empty. Unknown ids are an
/// error so a typo never silently drops a tier, and so is a catalog model
/// the pipeline cannot send a chunk to (a non-chat route).
pub fn resolve_ladder(ids: &[String]) -> Result<Vec<&'static Model>, crate::Error> {
    let ids: Vec<&str> = if ids.is_empty() {
        DEFAULT_LADDER.to_vec()
    } else {
        ids.iter().map(String::as_str).collect()
    };
    ids.into_iter()
        .map(|id| {
            model(id)
                .filter(|m| m.route.is_chat())
                .ok_or_else(|| crate::Error::UnknownModel(id.to_string()))
        })
        .collect()
}

macro_rules! local_model {
    ($id:literal, $provider:ident) => {
        Model {
            id: $id,
            provider: Provider::$provider,
            route: Route::OpenAiChat,
            reasoning: Reasoning::High,
            status: "Uses CLI subscription; availability depends on login",
            rates: None,
        }
    };
}
static LOCAL_MODELS: &[Model] = &[
    local_model!("claude-cli/opus", Anthropic),
    local_model!("codex-cli/gpt-5.6-sol", OpenAi),
    local_model!("codex-cli/gpt-6-astra", OpenAi),
];
pub fn local_models() -> &'static [Model] {
    LOCAL_MODELS
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
            assert!(!m.status.is_empty());
            let route_ok = match m.provider {
                Provider::Anthropic => m.route == Route::AnthropicMessages,
                Provider::GoogleAiStudio => m.route == Route::CompatChat,
                Provider::WorkersAi => {
                    matches!(m.route, Route::CompatChat | Route::WorkersAiRun)
                }
                Provider::OpenAi => matches!(m.route, Route::OpenAiChat | Route::OpenAiResponses),
            };
            assert!(route_ok, "{} routes wrongly", m.id);
            // A table-priced model never overrides the table; only a model
            // the table cannot price may carry an explicit rate.
            if listed(m.id).is_some() {
                assert_eq!(m.rates, listed(m.id), "{} prices another id", m.id);
            }
            if let Some(r) = m.rates {
                assert!(r.input > 0.0 && r.output >= 0.0, "{} rates {r:?}", m.id);
            }
        }
    }

    #[test]
    fn non_chat_models_are_priced_but_never_laddered() {
        let jev = model("typesafe/jev").unwrap();
        assert!(!jev.route.is_chat());
        assert_eq!(
            jev.rates,
            Some(Rates {
                input: 0.042,
                output: 0.0
            })
        );
        assert!(matches!(
            resolve_ladder(&["typesafe/jev".into()]),
            Err(crate::Error::UnknownModel(_))
        ));
    }

    #[test]
    fn default_ladder_is_priced() {
        let ladder = resolve_ladder(&[]).unwrap();
        assert_eq!(ladder.len(), DEFAULT_LADDER.len());
        assert!(ladder.iter().all(|m| m.rates.is_some()));
        assert!(matches!(
            resolve_ladder(&["nope".into()]),
            Err(crate::Error::UnknownModel(_))
        ));
        assert_eq!(
            model("gpt-4o"),
            None,
            "legacy ids are gone, not silently priced"
        );
    }
}
