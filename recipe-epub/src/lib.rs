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
mod library;

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

pub use extractor::{
    ChunkOutcome, ExtractedRecipe, MockExtractor, RecipeExtractor, RecipeMeta, Usage,
};
// Library scanning: list + classify the cookbooks in a directory of epubs.
pub use library::{
    book_metadata, classify_by_tags, classify_cookbooks_ai, find_epubs, BookMeta, CookbookGuess,
};
// Backend selection + the concrete extractors are internal; callers go through
// `extract_cookbook` (auto-selects) or `extract_cookbook_with` (supply your own).
use extractor::Backend;
// Section + time types are shared with the web scraper — one shape workspace-wide.
pub use recipe_scraper::{ParsedSection, RecipeSection, RecipeTimes};

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
    /// Model id override (default: `gemini-2.5-flash`, via the OpenAI-compatible backend).
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

/// An EPUB internal hyperlink found inside an ingredient/text line — the visible
/// link text plus its href fragment target (e.g. `<a href="…#piecrust">The Only
/// Piecrust</a>`). The author's literal pointer to another recipe; used to
/// confirm/strengthen a title-match reference (Layer 2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Link {
    /// The visible text inside the `<a>` tag.
    pub text: String,
    /// The href target (internal links only: `#frag` or `…html#frag`).
    pub href: String,
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
    /// Internal anchor links found anywhere in this chunk's text, with the line
    /// they appeared on. Used to confirm cross-recipe references.
    pub links: Vec<Link>,
}

// The assembled-recipe data shapes (`CookbookRecipe`, `RecipeRef`,
// `RefConfidence`) are plain data and live in the deps-light `recipe-types` crate
// so the cookbook JSON contract can be depended on without this crate's EPUB/LLM
// stack. Re-exported here so existing `recipe_epub::CookbookRecipe` (etc.) paths
// are unchanged. The parser-aware operations live in [`CookbookRecipeExt`] below.
pub use recipe_types::{CookbookRecipe, RecipeRef, RefConfidence};

/// [`CookbookRecipe`] with each ingredient line parsed into a structured
/// [`Ingredient`] and each instruction into a [`Rich`] (measurement-aware).
#[derive(Debug, Serialize)]
pub struct ParsedCookbookRecipe {
    pub meta: RecipeMeta,
    pub source: String,
    pub url: String,
    pub sections: Vec<ParsedSection>,
    /// Cross-recipe references, carried through from [`CookbookRecipe`].
    #[serde(default)]
    pub references: Vec<RecipeRef>,
}

/// Parser-aware operations on a [`CookbookRecipe`]. These live here rather than on
/// the type itself (which is now in the deps-light `recipe-types` crate) because
/// they run the core `ingredient` parser. Bring this trait into scope to call
/// `recipe.parse()` / `recipe.low_confidence_lines()`.
pub trait CookbookRecipeExt {
    /// Parse every section's verbatim lines with the shared core parser (the same
    /// [`recipe_scraper::parse_sections`] the web scraper uses).
    fn parse(&self) -> ParsedCookbookRecipe;

    /// Ingredient lines that look quantified (contain a digit or unicode
    /// fraction) but which the nom parser extracts **no** amount from — i.e.
    /// likely parser gaps worth adding to the accuracy corpus. Vocab-free: the
    /// only signal is "has a number but no parsed amount".
    fn low_confidence_lines(&self) -> Vec<String>;
}

impl CookbookRecipeExt for CookbookRecipe {
    fn parse(&self) -> ParsedCookbookRecipe {
        ParsedCookbookRecipe {
            meta: self.meta.clone(),
            source: self.source.clone(),
            url: self.url.clone(),
            sections: parse_sections(&self.sections),
            references: self.references.clone(),
        }
    }

    fn low_confidence_lines(&self) -> Vec<String> {
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
    extract_cookbook_with_progress(bytes, source, opts, |_| {}).await
}

/// Progress snapshot emitted during [`extract_cookbook_with_progress`]: how many
/// chunks have finished extracting (`done`) out of `total`, and how many of those
/// came from the on-disk cache (`cached`). Each snapshot is internally consistent
/// (the counts come from monotonic atomic increments).
#[derive(Debug, Clone, Copy, Default)]
pub struct ExtractProgress {
    /// Chunks finished so far.
    pub done: usize,
    /// Total chunks the book was split into (known once chunking completes).
    pub total: usize,
    /// Of the finished chunks, how many were served from cache (no API call).
    pub cached: usize,
}

/// Like [`extract_cookbook`] but reports progress as each chunk completes. The
/// sink is called once with `done == 0` when the chunk count is known, then once
/// per finished chunk. It runs from the concurrent extraction tasks, so it must
/// be `Send + Sync` (e.g. a closure writing to shared atomics).
pub async fn extract_cookbook_with_progress(
    bytes: &[u8],
    source: &str,
    opts: &Options,
    progress: impl Fn(ExtractProgress) + Send + Sync,
) -> Result<(Vec<CookbookRecipe>, ExtractionStats), EpubError> {
    let extractor = Backend::from_env(opts)?;
    if opts.use_cache {
        let caching = CachingExtractor {
            inner: &extractor,
            dir: opts.cache_dir.clone().unwrap_or_else(cache::default_dir),
            model: extractor.model().to_string(),
        };
        extract_cookbook_with_stats(bytes, source, opts, &caching, &progress).await
    } else {
        extract_cookbook_with_stats(bytes, source, opts, &extractor, &progress).await
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
/// with [`MockExtractor`]) and a progress sink (pass `|_| {}` to ignore it).
pub async fn extract_cookbook_with<E: RecipeExtractor>(
    bytes: &[u8],
    source: &str,
    opts: &Options,
    extractor: &E,
    progress: impl Fn(ExtractProgress) + Send + Sync,
) -> Result<Vec<CookbookRecipe>, EpubError> {
    let (recipes, _stats) =
        extract_cookbook_with_stats(bytes, source, opts, extractor, &progress).await?;
    Ok(recipes)
}

/// Like [`extract_cookbook_with`] but also returns token-usage/cost stats and
/// reports per-chunk progress through `progress` (see [`extract_cookbook_with_progress`]).
pub(crate) async fn extract_cookbook_with_stats<E: RecipeExtractor>(
    bytes: &[u8],
    source: &str,
    opts: &Options,
    extractor: &E,
    progress: &(impl Fn(ExtractProgress) + Send + Sync),
) -> Result<(Vec<CookbookRecipe>, ExtractionStats), EpubError> {
    let chunks = epub_text::chunk_epub(bytes)?;
    let total = chunks.len();
    tracing::info!("epub {source}: {total} chunk(s)");
    // Emit the initial snapshot now that the total is known, so the UI can switch
    // from an indeterminate spinner to a determinate bar before any chunk lands.
    progress(ExtractProgress {
        done: 0,
        total,
        cached: 0,
    });

    // Book-wide internal anchor links (author hyperlinks between recipes) —
    // the Layer 2 confirmation signal for cross-recipe references.
    let links: Vec<Link> = chunks.iter().flat_map(|c| c.links.clone()).collect();

    // Extract each chunk concurrently (bounded), preserving document order and
    // each recipe's originating doc. A single failing chunk is logged and
    // skipped rather than failing the whole book. `done`/`cached` are shared
    // atomics so each completing task can emit a consistent, monotonic snapshot.
    let done = AtomicUsize::new(0);
    let cached = AtomicUsize::new(0);
    let per_chunk: Vec<(String, ChunkOutcome)> = stream::iter(chunks.iter())
        .map(|chunk| async {
            let outcome = extractor.extract(chunk).await.unwrap_or_else(|e| {
                tracing::warn!("chunk {} extraction failed: {e}", chunk.doc_path);
                ChunkOutcome {
                    recipes: Vec::new(),
                    usage: Usage::default(),
                    cached: false,
                }
            });
            if outcome.cached {
                cached.fetch_add(1, Ordering::Relaxed);
            }
            let done_now = done.fetch_add(1, Ordering::Relaxed) + 1;
            progress(ExtractProgress {
                done: done_now,
                total,
                cached: cached.load(Ordering::Relaxed),
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

    let mut recipes = assemble(recipes_by_doc, source);
    resolve_references(&mut recipes, &links);
    tracing::info!(
        "epub {source}: {} recipe(s); {}",
        recipes.len(),
        stats.summary()
    );
    Ok((recipes, stats))
}

/// Attach each recipe's `source`/`url` and drop entries with no ingredients.
///
/// A recipe long enough to span a chunk boundary is emitted twice — once by its
/// title-bearing chunk and once by the title-hinted continuation chunk (see
/// [`epub_text`]) — so a second recipe with an already-seen title is *merged*
/// into the first (sections, instructions, and notes are unioned) rather than
/// dropped. This recovers the recipe's tail (extra steps, "Do Ahead" notes)
/// instead of discarding it. For the common single-chunk recipe it's a no-op.
fn assemble(per_chunk: Vec<(String, Vec<ExtractedRecipe>)>, source: &str) -> Vec<CookbookRecipe> {
    let mut index: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let mut out: Vec<CookbookRecipe> = Vec::new();
    for (doc_path, recipes) in per_chunk {
        for r in recipes {
            let title = r.meta.title.trim().to_string();
            let has_ingredients = r.sections.iter().any(|s| !s.ingredients.is_empty());
            if title.is_empty() || !has_ingredients {
                continue;
            }
            match index.get(&title.to_lowercase()) {
                Some(&i) => merge_recipe(&mut out[i], r),
                None => {
                    index.insert(title.to_lowercase(), out.len());
                    let mut meta = r.meta;
                    meta.title = title;
                    out.push(CookbookRecipe {
                        meta,
                        sections: r.sections,
                        source: source.to_string(),
                        url: format!("{source}#{doc_path}"),
                        references: Vec::new(),
                    });
                }
            }
        }
    }
    out
}

/// Fold a continuation half of a recipe into the already-assembled one: append
/// its sections and union its notes (and any newly-present metadata), de-duping
/// identical strings so a recipe that merely repeats across the seam doesn't
/// double up.
fn merge_recipe(into: &mut CookbookRecipe, from: ExtractedRecipe) {
    into.sections.extend(from.sections);
    for note in from.meta.notes {
        if !into.meta.notes.contains(&note) {
            into.meta.notes.push(note);
        }
    }
    // Fill metadata only the continuation chunk happened to capture.
    let m = &mut into.meta;
    m.description = m.description.take().or(from.meta.description);
    m.recipe_yield = m.recipe_yield.take().or(from.meta.recipe_yield);
    m.times = m.times.take().or(from.meta.times);
    m.category = m.category.take().or(from.meta.category);
    m.page = m.page.take().or(from.meta.page);
    for eq in from.meta.equipment {
        if !m.equipment.contains(&eq) {
            m.equipment.push(eq);
        }
    }
}

/// Normalize a title or ingredient line for substring matching: lowercase, drop
/// a leading article ("the "/"a "/"an "), replace every non-alphanumeric run
/// with a single space, and trim. Padding with surrounding spaces lets callers
/// test whole-token containment (" needle " in " haystack ").
fn normalize_title(s: &str) -> String {
    let lower = s.to_lowercase();
    let mut squashed = String::with_capacity(lower.len());
    let mut prev_space = false;
    for c in lower.chars() {
        if c.is_alphanumeric() {
            squashed.push(c);
            prev_space = false;
        } else if !prev_space {
            squashed.push(' ');
            prev_space = true;
        }
    }
    let trimmed = squashed.trim();
    let stripped = trimmed
        .strip_prefix("the ")
        .or_else(|| trimmed.strip_prefix("an "))
        .or_else(|| trimmed.strip_prefix("a "))
        .unwrap_or(trimmed);
    stripped.to_string()
}

/// Markers that signal an ingredient line points at another recipe in the book
/// (e.g. "(this page)", "1 recipe …", "see page 212"). Lets short/generic titles
/// match only when the line itself looks like a cross-reference.
fn has_cross_ref_marker(normalized_line: &str) -> bool {
    const MARKERS: &[&str] = &[
        "recipe",
        "this page",
        "see page",
        "page",
        "see recipe",
        "opposite",
    ];
    MARKERS.iter().any(|m| normalized_line.contains(m))
}

/// Whether `needle` occurs in `haystack` on whole-token boundaries. Both inputs
/// are already `normalize_title`d (single-spaced, alphanumeric). Checks that the
/// match isn't glued to adjacent word characters.
fn contains_whole_tokens(haystack: &str, needle: &str) -> bool {
    if needle.is_empty() || needle.len() > haystack.len() {
        return false;
    }
    let h = haystack.as_bytes();
    let n = needle.as_bytes();
    let mut i = 0;
    while i + n.len() <= h.len() {
        if &h[i..i + n.len()] == n {
            let before_ok = i == 0 || h[i - 1] == b' ';
            let after = i + n.len();
            let after_ok = after == h.len() || h[after] == b' ';
            if before_ok && after_ok {
                return true;
            }
        }
        i += 1;
    }
    false
}

/// Detect cross-recipe references: for each recipe, scan its ingredient lines for
/// the (normalized) title of any *other* recipe in the same book. Generic/short
/// titles only match when the line also carries a cross-reference marker
/// ("recipe", "this page", …), and when several titles match a line the longest
/// wins — so "The Only Piecrust" beats "Piecrust". A match is upgraded to
/// `RefConfidence::Linked` when an EPUB internal anchor (`links`) has link text
/// matching the referenced title — the author's literal pointer (Layer 2). Pure
/// post-processing over the assembled recipes (no API calls).
fn resolve_references(recipes: &mut [CookbookRecipe], links: &[Link]) {
    // (normalized_title, real_title, idx), longest normalized title first so the
    // most specific match wins.
    let mut titles: Vec<(String, String, usize)> = recipes
        .iter()
        .enumerate()
        .map(|(i, r)| (normalize_title(&r.meta.title), r.meta.title.clone(), i))
        .filter(|(norm, _, _)| !norm.is_empty())
        .collect();
    titles.sort_by_key(|t| std::cmp::Reverse(t.0.len()));

    // Normalized link texts (e.g. "the only piecrust" from <a>The Only Piecrust</a>)
    // — the set of titles the author hyperlinked anywhere in the book.
    let linked_titles: std::collections::HashSet<String> =
        links.iter().map(|l| normalize_title(&l.text)).collect();

    // Compute each recipe's references against the shared title index, then
    // assign in a second pass (read-all / write-each avoids aliasing `recipes`).
    let resolved: Vec<Vec<RecipeRef>> = recipes
        .iter()
        .enumerate()
        .map(|(idx, recipe)| {
            let mut found: Vec<RecipeRef> = Vec::new();
            for line in recipe.sections.iter().flat_map(|s| &s.ingredients) {
                let norm_line = normalize_title(line);
                if norm_line.is_empty() {
                    continue;
                }
                for (norm_title, real_title, t_idx) in &titles {
                    if *t_idx == idx {
                        continue; // no self-references
                    }
                    // A short/generic title (< 3 words and < 12 chars) only counts
                    // when the line looks like a cross-reference.
                    let words = norm_title.split(' ').count();
                    let specific = words >= 3 || norm_title.len() >= 12;
                    if !specific && !has_cross_ref_marker(&norm_line) {
                        continue;
                    }
                    if contains_whole_tokens(&norm_line, norm_title) {
                        // Dedup by target title; keep the first (longest-title)
                        // hit per line so "Piecrust" doesn't also fire after the
                        // full "The Only Piecrust" already matched this line.
                        let already = found
                            .iter()
                            .any(|r| r.title == *real_title && r.line == *line);
                        if !already {
                            // Upgrade to Linked when the author hyperlinked this
                            // exact title somewhere — a confirmed reference.
                            let confidence = if linked_titles.contains(norm_title) {
                                RefConfidence::Linked
                            } else {
                                RefConfidence::TitleMatch
                            };
                            found.push(RecipeRef {
                                title: real_title.clone(),
                                line: line.clone(),
                                confidence,
                            });
                        }
                        break; // one (best) reference per line
                    }
                }
            }
            found
        })
        .collect();

    for (recipe, refs) in recipes.iter_mut().zip(resolved) {
        recipe.references = refs;
    }
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
    fn assemble_merges_by_title_and_drops_empty() {
        let per_chunk = vec![
            (
                "c1.xhtml".to_string(),
                vec![er("Pancakes", &["1 cup flour"])],
            ),
            (
                "c2.xhtml".to_string(),
                vec![
                    er("PANCAKES", &["dupe"]), // merged (case-insensitive)
                    er("  ", &["no name"]),    // dropped: empty name
                    er("Soup", &[]),           // dropped: no ingredients
                    er("Omelette", &["3 eggs"]),
                ],
            ),
        ];
        let out = assemble(per_chunk, "book.epub");
        let names: Vec<_> = out.iter().map(|r| r.meta.title.as_str()).collect();
        assert_eq!(names, vec!["Pancakes", "Omelette"]);
        // The first-seen title keeps its identity + url; the dup folds into it.
        assert_eq!(out[0].url, "book.epub#c1.xhtml");
        assert_eq!(out[0].sections.len(), 2); // both halves' sections retained
        assert_eq!(out[1].url, "book.epub#c2.xhtml");
    }

    /// A recipe split across a chunk boundary: the title chunk has the body, the
    /// (title-hinted) continuation chunk re-emits the same title with only the
    /// tail / notes. They must merge into one recipe whose notes are the union.
    #[test]
    fn assemble_recovers_continuation_chunk_notes() {
        let title_half = ExtractedRecipe {
            meta: RecipeMeta {
                title: "Chocolate Chip Cookies".to_string(),
                notes: vec!["Note A".to_string()],
                ..Default::default()
            },
            sections: vec![RecipeSection {
                name: None,
                ingredients: vec!["2 cups flour".to_string()],
                instructions: vec!["mix".to_string()],
            }],
        };
        let cont_half = ExtractedRecipe {
            meta: RecipeMeta {
                title: "Chocolate Chip Cookies".to_string(),
                // Repeats Note A (across the seam) and adds the DO AHEAD tail.
                notes: vec!["Note A".to_string(), "Note B".to_string()],
                recipe_yield: Some("Makes 18".to_string()),
                ..Default::default()
            },
            sections: vec![RecipeSection {
                name: None,
                ingredients: vec!["1 tsp salt".to_string()],
                instructions: vec!["bake".to_string()],
            }],
        };
        let out = assemble(
            vec![
                ("c1.xhtml".to_string(), vec![title_half]),
                ("c2.xhtml".to_string(), vec![cont_half]),
            ],
            "book.epub",
        );
        assert_eq!(out.len(), 1);
        // Union of notes, no duplicate of the seam-repeated "Note A".
        assert_eq!(out[0].meta.notes, vec!["Note A", "Note B"]);
        assert_eq!(out[0].sections.len(), 2);
        // Metadata only the continuation chunk captured is filled in.
        assert_eq!(out[0].meta.recipe_yield.as_deref(), Some("Makes 18"));
    }

    #[test]
    fn normalize_title_strips_punct_and_articles() {
        assert_eq!(normalize_title("The Only Piecrust"), "only piecrust");
        assert_eq!(
            normalize_title("Soft & Pillowy Flatbread!"),
            "soft pillowy flatbread"
        );
        assert_eq!(normalize_title("A Simple Syrup"), "simple syrup");
        assert_eq!(normalize_title("  spaced   out  "), "spaced out");
    }

    #[test]
    fn contains_whole_tokens_respects_boundaries() {
        assert!(contains_whole_tokens(
            "1 recipe only piecrust this page",
            "only piecrust"
        ));
        // Not glued inside a larger word.
        assert!(!contains_whole_tokens("piecrustless pie", "piecrust"));
        assert!(contains_whole_tokens("only piecrust", "only piecrust"));
        assert!(!contains_whole_tokens("only", "only piecrust"));
    }

    /// Build an assembled recipe directly (post-`assemble` shape) for ref tests.
    fn recipe(title: &str, ings: &[&str]) -> CookbookRecipe {
        CookbookRecipe {
            meta: RecipeMeta {
                title: title.to_string(),
                ..Default::default()
            },
            sections: vec![RecipeSection {
                name: None,
                ingredients: ings.iter().map(|s| s.to_string()).collect(),
                instructions: vec![],
            }],
            source: "book.epub".to_string(),
            url: "book.epub#c.xhtml".to_string(),
            references: Vec::new(),
        }
    }

    #[test]
    fn resolve_references_finds_specific_titles() {
        let mut recipes = vec![
            recipe("The Only Piecrust", &["2 cups flour", "1 cup butter"]),
            recipe(
                "Apple Galette",
                &["1 recipe The Only Piecrust (this page)", "3 apples"],
            ),
        ];
        resolve_references(&mut recipes, &[]);
        // Piecrust references nothing; Galette references the Piecrust.
        assert!(recipes[0].references.is_empty());
        assert_eq!(recipes[1].references.len(), 1);
        assert_eq!(recipes[1].references[0].title, "The Only Piecrust");
        assert_eq!(
            recipes[1].references[0].confidence,
            RefConfidence::TitleMatch
        );
    }

    #[test]
    fn resolve_references_skips_self_and_generic_without_marker() {
        let mut recipes = vec![
            // A short/generic title: must NOT match a bare mention with no marker.
            recipe("Syrup", &["1 cup syrup", "2 cups water"]),
            // Mentions "Syrup" but only as a plain word, no cross-ref marker.
            recipe("Iced Tea", &["2 cups water", "splash of syrup"]),
            // Same generic title WITH a marker → should match.
            recipe("Lemonade", &["1 cup Syrup (recipe follows)", "lemons"]),
        ];
        resolve_references(&mut recipes, &[]);
        assert!(recipes[0].references.is_empty(), "self-ref skipped");
        assert!(
            recipes[1].references.is_empty(),
            "generic title without marker must not match"
        );
        assert_eq!(
            recipes[2].references.len(),
            1,
            "generic title with marker matches"
        );
        assert_eq!(recipes[2].references[0].title, "Syrup");
    }

    #[test]
    fn resolve_references_prefers_longest_title() {
        let mut recipes = vec![
            recipe("Piecrust", &["flour"]),
            recipe("The Only Piecrust", &["flour", "butter"]),
            recipe("Tart", &["1 recipe The Only Piecrust (this page)"]),
        ];
        resolve_references(&mut recipes, &[]);
        // The Tart's line matches both "Piecrust" and "The Only Piecrust"; the
        // longer, more specific title wins and only one ref is recorded.
        assert_eq!(recipes[2].references.len(), 1);
        assert_eq!(recipes[2].references[0].title, "The Only Piecrust");
    }
}
