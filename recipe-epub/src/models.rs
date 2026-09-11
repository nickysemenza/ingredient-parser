//! Curated model choices. Availability is explicit rather than guessed from IDs.
use serde::Serialize;
#[derive(Debug, Clone, Serialize)]
pub struct Model {
    pub id: &'static str,
    pub label: &'static str,
    pub provider: &'static str,
    pub enabled: bool,
    pub status: &'static str,
    pub transport: &'static str,
    pub max_output_tokens: u64,
    pub pricing_checked: &'static str,
    pub pricing_source: &'static str,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Rates {
    pub input: f64,
    pub output: f64,
    pub cache_read: f64,
    pub cache_write: f64,
}
struct Entry {
    model: Model,
    listed: bool,
    rates: Option<Rates>,
}
include!("model_catalog.rs");

pub fn catalog() -> Vec<Model> {
    ENTRIES
        .iter()
        .filter(|e| e.listed)
        .map(|e| e.model.clone())
        .collect()
}
fn entry(id: &str) -> Option<&'static Entry> {
    ENTRIES.iter().find(|e| e.model.id == id)
}
/// Only exact catalog IDs are supported, including unlisted legacy models.
pub fn provider(id: &str) -> Option<&'static str> {
    entry(id).map(|e| e.model.provider)
}
/// The model-specific source used by new accounting snapshots.
pub fn pricing_source(id: &str) -> Option<&'static str> {
    entry(id).map(|e| e.model.pricing_source)
}
/// Verification date for the rate snapshot, when the model is known.
pub fn pricing_checked(id: &str) -> Option<&'static str> {
    entry(id).map(|e| e.model.pricing_checked)
}
pub(crate) fn rates(id: &str) -> Option<Rates> {
    entry(id).and_then(|e| e.rates)
}

/// Maximum response tokens accepted by this exact catalog model. Operation
/// policies must cap their requested output to this value instead of assuming a
/// provider-wide limit.
pub fn max_output_tokens(id: &str) -> Option<u64> {
    entry(id).map(|e| e.model.max_output_tokens)
}

#[cfg(test)]
mod catalog_tests {
    #[test]
    fn catalog_metadata_is_valid() {
        let mut ids = std::collections::HashSet::new();
        for e in super::ENTRIES {
            let m = &e.model;
            assert!(!m.id.is_empty() && ids.insert(m.id), "duplicate/empty ID");
            assert!(m.max_output_tokens > 0 && !m.label.is_empty());
            assert!(m.pricing_source.starts_with("https://"));
            assert_eq!(m.pricing_checked.len(), 10);
            assert!(
                match m.provider {
                    "workers-ai" | "google-ai-studio" =>
                        m.transport == "gateway-unified-chat-completions",
                    "anthropic" => m.transport == "messages",
                    "openai" => matches!(m.transport, "responses" | "chat-completions"),
                    _ => false,
                },
                "unsupported routing for {}",
                m.id
            );
            if let Some(r) = e.rates {
                assert!(
                    [r.input, r.output, r.cache_read, r.cache_write]
                        .iter()
                        .all(|n| n.is_finite() && *n >= 0.0)
                );
            }
        }
    }
    #[test]
    fn preserves_curated_and_legacy_boundaries() {
        let models = super::catalog();
        assert_eq!(models.len(), 14);
        assert!(
            models
                .iter()
                .any(|m| m.id == super::DEFAULT_MODEL && m.enabled)
        );
        assert!(!models.iter().any(|m| m.id == "gpt-4o"));
        assert_eq!(super::provider("gpt-4o"), Some("openai"));
        assert!(super::rates("gpt-4o").is_none());
        assert!(super::provider("gemini-2.5-flash-future").is_none());
        assert!(super::rates("gemini-2.5-flash-future").is_none());
    }
    #[test]
    fn explicit_cache_rates_are_used() {
        let usage = crate::Usage {
            input_tokens: 1_000_000,
            output_tokens: 1_000_000,
            cache_read_input_tokens: 1_000_000,
            cache_creation_input_tokens: 1_000_000,
        };
        let cost = crate::accounting::cost_for_usage("@cf/zai-org/glm-5.3-flash", &usage);
        assert!(cost.is_some_and(|c| (c - (0.15 + 0.50 + 0.03 + 0.1875)).abs() < 1e-12));
    }
}
