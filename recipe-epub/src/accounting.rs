//! Extraction accounting shared by native and browser readers. Transport adapters
//! supply model identity; this module retains tier attribution and incompleteness
//! through cost estimation and human summaries.

#[cfg(any(feature = "native", test))]
use crate::ExtractionStats;
use crate::{ChunkTruncation, ExtractionReport, ModelTier, Usage};
use serde::Serialize;

/// Usage attributable to one extraction tier. An absent model is unpriced.
#[derive(Debug, Clone, Serialize)]
pub struct ModelUsage {
    pub tier: ModelTier,
    pub model: Option<String>,
    pub usage: Usage,
}

/// A subtotal of known rates, explicitly distinguished from a complete estimate.
#[derive(Debug, Clone, Copy, Default, Serialize)]
pub struct CostEstimate {
    pub known_usd: f64,
    pub complete: bool,
}

impl std::fmt::Display for CostEstimate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.complete {
            write!(f, "~${:.4}", self.known_usd)
        } else {
            write!(
                f,
                "~${:.4} known subtotal (pricing incomplete)",
                self.known_usd
            )
        }
    }
}

/// Accounting for a book, retaining usage by model and incomplete extraction.
/// Created from the shared report so every reader uses the same attribution.
#[derive(Debug, Clone, Serialize)]
pub struct ExtractionAccounting {
    pub models: Vec<ModelUsage>,
    pub usage: Usage,
    pub chunks_total: usize,
    pub chunks_cached: usize,
    pub chunks_failed: usize,
    /// Successful chunks whose final result was truncated (recovered attempts excluded).
    pub chunks_truncated: usize,
    pub truncations: Vec<ChunkTruncation>,
}

impl ExtractionReport {
    /// Attribute the report to the models selected by the transport adapter.
    /// Missing identities remain unpriced; a fallback is never priced as primary.
    /// This is additive: existing browser report construction remains unchanged.
    pub fn accounting(
        &self,
        primary_model: &str,
        fallback_model: Option<&str>,
    ) -> ExtractionAccounting {
        let model = |s: &str| (!s.is_empty()).then(|| s.to_owned());
        let mut models = vec![ModelUsage {
            tier: ModelTier::Primary,
            model: model(primary_model),
            usage: self.primary_usage.clone(),
        }];
        if self.chunks.iter().any(|c| c.tier == ModelTier::Fallback)
            || self.failures.iter().any(|f| f.fallback.is_some())
            || self.fallback_usage != Usage::default()
        {
            models.push(ModelUsage {
                tier: ModelTier::Fallback,
                model: fallback_model.and_then(model),
                usage: self.fallback_usage.clone(),
            });
        }
        ExtractionAccounting {
            models,
            usage: self.usage.clone(),
            chunks_total: self.chunks.len() + self.failures.len(),
            chunks_cached: self.chunks_cached,
            chunks_failed: self.failures.len(),
            chunks_truncated: self.chunks.iter().filter(|c| c.truncated).count(),
            truncations: self.truncations.clone(),
        }
    }
}

impl ExtractionAccounting {
    pub fn cost_estimate(&self) -> CostEstimate {
        let mut estimate = CostEstimate {
            known_usd: 0.0,
            complete: true,
        };
        for model in &self.models {
            // Cache hits make no billed calls, even with an unknown model.
            if model.usage == Usage::default() {
                continue;
            }
            match model
                .model
                .as_deref()
                .and_then(|id| cost_for_usage(id, &model.usage))
            {
                Some(cost) => estimate.known_usd += cost,
                None => estimate.complete = false,
            }
        }
        estimate
    }

    /// Final chunk outcomes determine completeness. A truncated primary attempt
    /// recovered by a complete fallback remains evidence, not a missing recipe.
    pub fn is_incomplete(&self) -> bool {
        self.chunks_failed > 0 || self.chunks_truncated > 0
    }

    /// One summary for CLI and desktop readers, including tier-specific usage.
    pub fn summary(&self) -> String {
        let models = self
            .models
            .iter()
            .map(|m| {
                format!(
                    "{}: {} in / {} out / {} cache-read / {} cache-write tok",
                    m.model.as_deref().unwrap_or("unknown model"),
                    m.usage.input_tokens,
                    m.usage.output_tokens,
                    m.usage.cache_read_input_tokens,
                    m.usage.cache_creation_input_tokens,
                )
            })
            .collect::<Vec<_>>()
            .join("; ");
        format!("{} · {models}", self.status_summary())
    }

    /// Compact cost, cache and completeness summary for a visible recipe header.
    pub fn status_summary(&self) -> String {
        let status = if self.is_incomplete() {
            format!(
                " · INCOMPLETE: {} chunk(s) FAILED, {} chunk(s) truncated",
                self.chunks_failed, self.chunks_truncated
            )
        } else if !self.truncations.is_empty() {
            format!(
                " · {} truncated attempt(s), recovered",
                self.truncations.len()
            )
        } else {
            String::new()
        };
        format!(
            "{}/{} chunks cached · {}{status}",
            self.chunks_cached,
            self.chunks_total,
            self.cost_estimate()
        )
    }

    /// Compatibility projection. A single model can represent all billed usage
    /// only when every billed tier has that identity; otherwise cost is unknown.
    #[cfg(any(feature = "native", test))]
    pub(crate) fn legacy_stats(&self) -> ExtractionStats {
        let mut billed = self.models.iter().filter(|m| m.usage != Usage::default());
        let first = billed.next().or_else(|| self.models.first());
        let model = first.and_then(|m| m.model.as_deref());
        let model = if billed.all(|m| m.model.as_deref() == model) {
            model
        } else {
            None
        };
        ExtractionStats {
            model: model.unwrap_or_default().to_owned(),
            chunks_total: self.chunks_total,
            chunks_cached: self.chunks_cached,
            chunks_failed: self.chunks_failed,
            usage: self.usage.clone(),
        }
    }
}

pub(crate) fn cost_for_usage(model: &str, u: &Usage) -> Option<f64> {
    let (input, output) = price_per_mtok(model)?;
    let cached_rate = match model {
        "@cf/moonshotai/kimi-k2.6" => 0.16,
        "@cf/moonshotai/kimi-k2.7-code" => 0.19,
        _ => input * 0.1,
    };
    Some(
        (u.input_tokens as f64 * input
            + u.cache_creation_input_tokens as f64 * input * 1.25
            + u.cache_read_input_tokens as f64 * cached_rate
            + u.output_tokens as f64 * output)
            / 1_000_000.0,
    )
}

/// Exact, versioned aliases. Unknown future versions must never inherit an old rate.
pub(crate) fn price_per_mtok(model: &str) -> Option<(f64, f64)> {
    match model {
        "gemini-3.5-flash-lite" => Some((0.30, 2.50)),
        "claude-sonnet-5" => Some((2.0, 10.0)),
        "gpt-5.6-luna" => Some((0.20, 1.20)),
        "@cf/moonshotai/kimi-k2.6" | "@cf/moonshotai/kimi-k2.7-code" => Some((0.95, 4.0)),
        "gemini-3.7-flash" => Some((0.75, 3.75)),
        "claude-haiku-4-5" | "claude-haiku-4-5-20251001" => Some((1.0, 5.0)),
        "claude-sonnet-4-5" | "claude-sonnet-4-6" => Some((3.0, 15.0)),
        "claude-opus-4-5" | "claude-opus-4-5-20251101" => Some((5.0, 25.0)),
        "gemini-2.5-flash-lite" | "gemini-2.5-flash-lite-preview" => Some((0.10, 0.40)),
        "gemini-2.5-flash" | "gemini-2.5-flash-002" => Some((0.30, 2.50)),
        "gemini-2.0-flash-lite" => Some((0.075, 0.30)),
        "gemini-2.0-flash" => Some((0.10, 0.40)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::{Chunk, ChunkOutcome, EpubError, OrchestrationOptions, extract_chunks_with};
    use rstest::rstest;

    #[test]
    fn price_per_mtok_accepts_only_explicit_model_aliases() {
        // Exact aliases cannot accidentally inherit a related model's price.
        assert_eq!(
            price_per_mtok("gemini-2.5-flash-lite-preview"),
            Some((0.10, 0.40))
        );
        assert_eq!(price_per_mtok("gemini-2.5-flash-002"), Some((0.30, 2.50)));
        assert_eq!(price_per_mtok("gemini-2.0-flash-lite"), Some((0.075, 0.30)));
        assert_eq!(price_per_mtok("gemini-2.0-flash"), Some((0.10, 0.40)));
        // Known dated Anthropic aliases remain compatible.
        assert_eq!(
            price_per_mtok("claude-haiku-4-5-20251001"),
            Some((1.0, 5.0))
        );
        // Unmapped → None.
        assert_eq!(price_per_mtok("opus-4-1"), None);
        assert_eq!(price_per_mtok("new-gemini-2.5-flash-experimental"), None);
    }

    #[rstest]
    #[case(Some("claude-sonnet-4-6"), 7.0, true, true)]
    #[case(Some("claude-sonnet-4-6"), 7.0, true, false)]
    #[case(Some("unpriced"), 1.0, false, true)]
    #[case(None, 1.0, false, true)]
    #[tokio::test]
    async fn report_prices_failed_primary_and_fallback_separately(
        #[case] fallback: Option<&str>,
        #[case] cost: f64,
        #[case] complete: bool,
        #[case] truncated: bool,
    ) {
        let report = extract_chunks_with(
            vec![Chunk {
                text: "recipe".into(),
                doc_path: "one.xhtml".into(),
                title_hint: None,
                links: vec![],
                images: vec![],
            }],
            "book",
            &OrchestrationOptions {
                fallback: true,
                ..Default::default()
            },
            |_, _, tier| async move {
                if tier == ModelTier::Primary {
                    Err(crate::ChunkExtractionFailure {
                        error: EpubError::Proxy("bad payload".into()),
                        usage: Usage {
                            input_tokens: 1_000_000,
                            ..Default::default()
                        },
                        truncated: true,
                        attempts: vec![],
                    })
                } else {
                    Ok(ChunkOutcome {
                        recipes: vec![crate::ExtractedRecipe {
                            meta: crate::RecipeMeta {
                                title: "Soup".into(),
                                ..Default::default()
                            },
                            sections: vec![crate::RecipeSection::new(
                                vec!["1 cup water".into()],
                                vec![],
                            )],
                        }],
                        usage: Usage {
                            input_tokens: 2_000_000,
                            ..Default::default()
                        },
                        cached: false,
                        truncated,
                    })
                }
            },
            |_| {},
        )
        .await;
        assert_eq!(report.recipes.len(), 1);
        let accounting = report.accounting("claude-haiku-4-5", fallback);
        assert_eq!(accounting.cost_estimate().known_usd, cost);
        assert_eq!(accounting.cost_estimate().complete, complete);
        assert_eq!(accounting.models.len(), 2);
        assert_eq!(accounting.models[0].usage.input_tokens, 1_000_000);
        assert_eq!(accounting.models[1].usage.input_tokens, 2_000_000);
        assert_eq!(accounting.is_incomplete(), truncated);
        assert_eq!(accounting.chunks_failed, 0);
        assert_eq!(accounting.truncations.len(), if truncated { 2 } else { 1 });
        assert_eq!(accounting.summary().contains("INCOMPLETE"), truncated);
        assert_eq!(
            accounting.summary().contains("pricing incomplete"),
            !complete
        );
        assert!(accounting.legacy_stats().cost_usd().is_none());
    }

    #[tokio::test]
    async fn cached_unknown_model_has_complete_zero_cost() {
        let report = extract_chunks_with(
            vec![Chunk {
                text: "recipe".into(),
                doc_path: "one.xhtml".into(),
                title_hint: None,
                links: vec![],
                images: vec![],
            }],
            "book",
            &OrchestrationOptions::default(),
            |_, _, _| async {
                Ok(ChunkOutcome {
                    recipes: vec![],
                    usage: Usage::default(),
                    cached: true,
                    truncated: false,
                })
            },
            |_| {},
        )
        .await;
        let accounting = report.accounting("unpriced", None);
        assert_eq!(accounting.chunks_cached, 1);
        assert_eq!(accounting.cost_estimate().known_usd, 0.0);
        assert!(accounting.cost_estimate().complete);
        assert!(!accounting.is_incomplete());
    }
}
