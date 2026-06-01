//! Extract recipes from EPUB cookbooks.
//!
//! EPUB cookbook markup is wildly publisher-specific (ingredients live in
//! `<p class="ril">`, `<li>`, `<div class="IL_item">`, …), so instead of
//! per-publisher heuristics this crate hands cleaned text to an LLM that decides
//! *structure* — segmenting recipes into components and labeling title /
//! ingredient lines / instruction steps / yield / times / notes. The ingredient
//! *strings* come back verbatim; [`CookbookRecipe::parse`] runs the core
//! `ingredient` nom parser over them. The LLM never parses quantities.
//!
//! Entry point: [`extract_cookbook`] (selects a backend from `Options.model`) or
//! [`extract_cookbook_with`] (any [`RecipeExtractor`], e.g. a mock in tests).

mod cache;
mod epub_text;
mod extractor;

use std::path::PathBuf;

pub use extractor::{
    Backend, ChunkOutcome, ClaudeExtractor, ExtractedRecipe, MockExtractor, OpenAiExtractor,
    RecipeExtractor, RecipeMeta, RecipeTimes, Usage,
};
// Section types are shared with the web scraper — one section shape workspace-wide.
pub use recipe_scraper::{ParsedSection, RecipeSection};

use futures::stream::{self, StreamExt};
use recipe_scraper::parse_sections;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Errors from EPUB recipe extraction.
#[derive(Debug, Error)]
pub enum EpubError {
    /// The bytes were not a readable EPUB (bad zip, missing OPF, …).
    #[error("could not read epub: {0}")]
    Open(String),
    /// No auth source: neither `ANTHROPIC_API_KEY` nor a gateway token
    /// (`CF_AIG_TOKEN` / `AI_GATEWAY_API_KEY`) was set.
    #[error("no API auth: set ANTHROPIC_API_KEY, or CF_AIG_TOKEN/AI_GATEWAY_API_KEY (+ ANTHROPIC_BASE_URL) for a gateway")]
    MissingApiKey,
    /// No base URL: neither `ANTHROPIC_BASE_URL` (a Cloudflare AI Gateway
    /// `…/anthropic` endpoint) nor an explicit `OPENAI_BASE_URL`/`GEMINI_BASE_URL`
    /// was set. This crate routes through a gateway by design — no public default.
    #[error("no base URL: set ANTHROPIC_BASE_URL to your AI gateway (…/anthropic), or OPENAI_BASE_URL/GEMINI_BASE_URL")]
    MissingBaseUrl,
    /// The HTTP request to the model API failed (connection, timeout, …).
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),
    /// The model API returned a non-success status or an unexpected shape.
    #[error("model api error (status {status}): {body}")]
    Api { status: u16, body: String },
    /// JSON (de)serialization failed.
    #[error("deserialize error: {0}")]
    Deserialize(#[from] serde_json::Error),
    /// The on-disk cache could not be read or written.
    #[error("cache error: {0}")]
    Cache(String),
}

/// Tunables for [`extract_cookbook`].
#[derive(Debug, Clone)]
pub struct Options {
    /// Model id override (default: `gpt-4o-mini`, via the OpenAI-compatible backend).
    pub model: Option<String>,
    /// Whether to use the on-disk extraction cache.
    pub use_cache: bool,
    /// Cache directory (default: `$XDG_CACHE_HOME/recipe-epub` or `$TMPDIR/recipe-epub`).
    pub cache_dir: Option<PathBuf>,
    /// Max concurrent extractor calls.
    pub concurrency: usize,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            model: None,
            use_cache: true,
            cache_dir: None,
            concurrency: 8,
        }
    }
}

/// A unit of cookbook text handed to the extractor. One chunk may contain zero,
/// one, or many recipes; the extractor segments them.
#[derive(Debug, Clone)]
pub struct Chunk {
    /// A title hint from the TOC/heading, if any.
    pub title_hint: Option<String>,
    /// Cleaned, tag-stripped text (block/line breaks preserved).
    pub text: String,
    /// The originating spine-doc path, used to label `url`.
    pub doc_path: String,
}

/// A fully assembled recipe (raw verbatim strings) with provenance. This is the
/// crate's output type; call [`CookbookRecipe::parse`] to structure the lines.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CookbookRecipe {
    pub meta: RecipeMeta,
    pub sections: Vec<RecipeSection>,
    /// The book this came from (the caller's `source` label).
    pub source: String,
    /// `source#doc_path` for traceability.
    pub url: String,
}

/// [`CookbookRecipe`] with each ingredient line parsed into a structured
/// [`Ingredient`] and each instruction into a [`Rich`] (measurement-aware).
#[derive(Debug, Serialize)]
pub struct ParsedCookbookRecipe {
    pub meta: RecipeMeta,
    pub source: String,
    pub url: String,
    pub sections: Vec<ParsedSection>,
}

impl CookbookRecipe {
    /// Parse every section's verbatim lines with the shared core parser (the same
    /// [`recipe_scraper::parse_sections`] the web scraper uses).
    pub fn parse(&self) -> ParsedCookbookRecipe {
        ParsedCookbookRecipe {
            meta: self.meta.clone(),
            source: self.source.clone(),
            url: self.url.clone(),
            sections: parse_sections(&self.sections),
        }
    }

    /// Ingredient lines that look quantified (contain a digit or unicode
    /// fraction) but which the nom parser extracts **no** amount from — i.e.
    /// likely parser gaps worth adding to the accuracy corpus. Vocab-free: the
    /// only signal is "has a number but no parsed amount".
    pub fn low_confidence_lines(&self) -> Vec<String> {
        let ip = ingredient::IngredientParser::new();
        self.sections
            .iter()
            .flat_map(|s| &s.ingredients)
            .filter(|line| has_quantity_char(line) && ip.from_str(line).amounts.is_empty())
            .cloned()
            .collect()
    }
}

/// Whether a string contains an ASCII digit or a common unicode vulgar fraction.
fn has_quantity_char(s: &str) -> bool {
    s.chars()
        .any(|c| c.is_ascii_digit() || "½⅓¼¾⅔⅜⅝⅞⅛⅙⅚".contains(c))
}

/// Extract every recipe from an EPUB cookbook using the default backend.
///
/// `bytes` is the full `.epub` (a zip). `source` labels each recipe (book path
/// or title). Auth comes from the environment (see [`ClaudeExtractor::from_env`]).
pub async fn extract_cookbook(
    bytes: &[u8],
    source: &str,
    opts: &Options,
) -> Result<(Vec<CookbookRecipe>, ExtractionStats), EpubError> {
    let extractor = Backend::from_env(opts)?;
    if opts.use_cache {
        let caching = CachingExtractor {
            inner: &extractor,
            dir: opts.cache_dir.clone().unwrap_or_else(cache::default_dir),
            model: extractor.model().to_string(),
        };
        extract_cookbook_with_stats(bytes, source, opts, &caching).await
    } else {
        extract_cookbook_with_stats(bytes, source, opts, &extractor).await
    }
}

/// Token usage + cost summary for one `extract_cookbook` run.
#[derive(Debug, Clone, Default, Serialize)]
pub struct ExtractionStats {
    /// Resolved model id (empty for the mock).
    pub model: String,
    /// Total chunks the book was split into.
    pub chunks_total: usize,
    /// Chunks served from the on-disk cache (no API call).
    pub chunks_cached: usize,
    /// Summed token usage across the API calls actually made.
    pub usage: Usage,
}

impl ExtractionStats {
    /// Estimated USD cost of the API calls, or `None` if the model's pricing is
    /// unknown. Cache writes bill ~1.25× input, cache reads ~0.1× input.
    pub fn cost_usd(&self) -> Option<f64> {
        let (in_rate, out_rate) = price_per_mtok(&self.model)?;
        let u = &self.usage;
        let cost = (u.input_tokens as f64 * in_rate
            + u.cache_creation_input_tokens as f64 * in_rate * 1.25
            + u.cache_read_input_tokens as f64 * in_rate * 0.1
            + u.output_tokens as f64 * out_rate)
            / 1_000_000.0;
        Some(cost)
    }

    /// One-line human summary for CLI stderr / UI.
    pub fn summary(&self) -> String {
        let u = &self.usage;
        let cost = self
            .cost_usd()
            .map_or_else(|| "cost: n/a".to_string(), |c| format!("~${c:.4}"));
        format!(
            "{}/{} chunks cached · {} in / {} out tok · {} cache-read tok · {cost}",
            self.chunks_cached,
            self.chunks_total,
            u.input_tokens,
            u.output_tokens,
            u.cache_read_input_tokens
        )
    }
}

/// Per-million-token (input, output) USD rates for known models. Matched by
/// substring so dated ids (`claude-haiku-4-5-20251001`) resolve. `None` → the
/// cost is reported as "n/a" rather than guessed.
fn price_per_mtok(model: &str) -> Option<(f64, f64)> {
    let m = model.to_lowercase();
    let table = [
        ("haiku-4-5", (1.0, 5.0)),
        ("haiku", (1.0, 5.0)),
        ("sonnet-4", (3.0, 15.0)),
        ("sonnet", (3.0, 15.0)),
        ("opus", (5.0, 25.0)),
        ("gemini-2.5-flash-lite", (0.10, 0.40)),
        ("gemini-2.5-flash", (0.30, 2.50)),
        ("gemini-2.0-flash-lite", (0.075, 0.30)),
        ("gemini-2.0-flash", (0.10, 0.40)),
        ("gpt-4o-mini", (0.15, 0.60)),
        ("gpt-4o", (2.50, 10.0)),
    ];
    table
        .iter()
        .find(|(key, _)| m.contains(key))
        .map(|(_, rate)| *rate)
}

/// Wraps any extractor with the on-disk cache (see [`cache`]).
struct CachingExtractor<'a, E> {
    inner: &'a E,
    dir: PathBuf,
    model: String,
}

impl<E: RecipeExtractor> RecipeExtractor for CachingExtractor<'_, E> {
    fn model(&self) -> &str {
        self.inner.model()
    }

    async fn extract(&self, chunk: &Chunk) -> Result<ChunkOutcome, EpubError> {
        let key = cache::key(&self.model, &chunk.text);
        if let Some(hit) = cache::read(&self.dir, &key) {
            // Cache hit: no API call, so no usage/cost is incurred.
            return Ok(ChunkOutcome {
                recipes: hit,
                usage: Usage::default(),
                cached: true,
            });
        }
        let outcome = self.inner.extract(chunk).await?;
        if let Err(e) = cache::write(&self.dir, &key, &outcome.recipes) {
            tracing::warn!("cache write failed: {e}");
        }
        Ok(outcome)
    }
}

/// Like [`extract_cookbook`] but with a caller-supplied extractor (used by tests
/// with [`MockExtractor`]).
pub async fn extract_cookbook_with<E: RecipeExtractor>(
    bytes: &[u8],
    source: &str,
    opts: &Options,
    extractor: &E,
) -> Result<Vec<CookbookRecipe>, EpubError> {
    let (recipes, _stats) = extract_cookbook_with_stats(bytes, source, opts, extractor).await?;
    Ok(recipes)
}

/// Like [`extract_cookbook_with`] but also returns token-usage/cost stats.
pub async fn extract_cookbook_with_stats<E: RecipeExtractor>(
    bytes: &[u8],
    source: &str,
    opts: &Options,
    extractor: &E,
) -> Result<(Vec<CookbookRecipe>, ExtractionStats), EpubError> {
    let chunks = epub_text::chunk_epub(bytes)?;
    tracing::info!("epub {source}: {} chunk(s)", chunks.len());

    // Extract each chunk concurrently (bounded), preserving document order and
    // each recipe's originating doc. A single failing chunk is logged and
    // skipped rather than failing the whole book.
    let per_chunk: Vec<(String, ChunkOutcome)> = stream::iter(chunks.iter())
        .map(|chunk| async move {
            let outcome = extractor.extract(chunk).await.unwrap_or_else(|e| {
                tracing::warn!("chunk {} extraction failed: {e}", chunk.doc_path);
                ChunkOutcome {
                    recipes: Vec::new(),
                    usage: Usage::default(),
                    cached: false,
                }
            });
            (chunk.doc_path.clone(), outcome)
        })
        .buffered(opts.concurrency.max(1))
        .collect::<Vec<_>>()
        .await;

    let mut stats = ExtractionStats {
        model: extractor.model().to_string(),
        chunks_total: per_chunk.len(),
        ..Default::default()
    };
    let recipes_by_doc: Vec<(String, Vec<ExtractedRecipe>)> = per_chunk
        .into_iter()
        .map(|(doc, outcome)| {
            if outcome.cached {
                stats.chunks_cached += 1;
            }
            stats.usage.add(&outcome.usage);
            (doc, outcome.recipes)
        })
        .collect();

    let recipes = assemble(recipes_by_doc, source);
    tracing::info!(
        "epub {source}: {} recipe(s); {}",
        recipes.len(),
        stats.summary()
    );
    Ok((recipes, stats))
}

/// Attach each recipe's `source`/`url`, drop entries with no ingredients, and
/// dedup by normalized title (a safety net against the same recipe twice).
fn assemble(per_chunk: Vec<(String, Vec<ExtractedRecipe>)>, source: &str) -> Vec<CookbookRecipe> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for (doc_path, recipes) in per_chunk {
        for r in recipes {
            let title = r.meta.title.trim().to_string();
            let has_ingredients = r.sections.iter().any(|s| !s.ingredients.is_empty());
            if title.is_empty() || !has_ingredients {
                continue;
            }
            if !seen.insert(title.to_lowercase()) {
                continue;
            }
            let mut meta = r.meta;
            meta.title = title;
            out.push(CookbookRecipe {
                meta,
                sections: r.sections,
                source: source.to_string(),
                url: format!("{source}#{doc_path}"),
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn cost_usd_matches_pricing_table() {
        // 1M input + 1M output on Haiku ($1/$5) = $6.00.
        let stats = ExtractionStats {
            model: "claude-haiku-4-5-20251001".to_string(),
            chunks_total: 10,
            chunks_cached: 3,
            usage: Usage {
                input_tokens: 1_000_000,
                output_tokens: 1_000_000,
                cache_creation_input_tokens: 0,
                cache_read_input_tokens: 0,
            },
        };
        assert!((stats.cost_usd().unwrap() - 6.0).abs() < 1e-9);

        // Cache reads bill ~0.1× input: 1M cache-read on Haiku = $0.10.
        let cached = ExtractionStats {
            model: "claude-haiku-4-5".to_string(),
            usage: Usage {
                cache_read_input_tokens: 1_000_000,
                ..Default::default()
            },
            ..Default::default()
        };
        assert!((cached.cost_usd().unwrap() - 0.10).abs() < 1e-9);

        // Unknown model → no guess.
        let unknown = ExtractionStats {
            model: "mystery-model".to_string(),
            ..Default::default()
        };
        assert!(unknown.cost_usd().is_none());
    }

    fn er(title: &str, ings: &[&str]) -> ExtractedRecipe {
        ExtractedRecipe {
            meta: RecipeMeta {
                title: title.to_string(),
                ..Default::default()
            },
            sections: vec![RecipeSection {
                name: None,
                ingredients: ings.iter().map(|s| s.to_string()).collect(),
                instructions: vec!["step".to_string()],
            }],
        }
    }

    #[test]
    fn assemble_dedups_by_title_and_drops_empty() {
        let per_chunk = vec![
            (
                "c1.xhtml".to_string(),
                vec![er("Pancakes", &["1 cup flour"])],
            ),
            (
                "c2.xhtml".to_string(),
                vec![
                    er("PANCAKES", &["dupe"]), // dedup (case-insensitive)
                    er("  ", &["no name"]),    // dropped: empty name
                    er("Soup", &[]),           // dropped: no ingredients
                    er("Omelette", &["3 eggs"]),
                ],
            ),
        ];
        let out = assemble(per_chunk, "book.epub");
        let names: Vec<_> = out.iter().map(|r| r.meta.title.as_str()).collect();
        assert_eq!(names, vec!["Pancakes", "Omelette"]);
        assert_eq!(out[0].url, "book.epub#c1.xhtml");
        assert_eq!(out[1].url, "book.epub#c2.xhtml");
    }
}
