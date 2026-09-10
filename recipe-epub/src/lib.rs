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
//! [`extract_chunks_with`] drives a book using caller-supplied transport on
//! native or browser runtimes. With the `native` feature, `extract_cookbook`
//! selects a configured backend and `extract_cookbook_with` accepts any
//! [`RecipeExtractor`], including mocks.

// `backend`, `cache`, and `library` are native-only — each gates itself with an
// inner `#![cfg(feature = "native")]`, so their `mod` lines stay unconditional
// here. The other modules form the runtime-independent contract.
mod backend;
mod cache;
pub mod cache_contract;
mod epub_text;
mod extractor;
mod library;
pub mod models;
mod orchestration;
pub mod recovery;

// Pure extraction API — compiles to wasm32: EPUB unzip + text chunking
// (`chunk_epub`), per-chunk request building (`build_chunk_request`), LLM
// response parsing (`parse_recipes_payload`), whole-book scheduling
// (`extract_chunks_with`), and assembly (`assemble_recipes`).
//
// These APIs support external browser callers with `default-features = false`;
// preserve downstream compatibility even when an API has no in-tree caller.
// CI tests the non-native library and compiles the browser callback example
// for wasm32.
mod accounting;
pub mod indexed;
#[cfg(feature = "native")]
pub mod review;
pub mod source;
pub use accounting::{CostEstimate, ExtractionAccounting, ModelUsage};
pub use epub_text::chunk_epub;
pub use extractor::{
    CallFailure, CallResult, ChunkExtractionFailure, ChunkOutcome, ChunkRequest, DrivenChunk,
    ExtractedRecipe, FailedAttempt, MockExtractor, MockMatch, PARSE_RETRIES, RecipeExtractor,
    RecipeMeta, Usage, build_chunk_request, parse_recipes_payload, recipes_tool_schema,
    try_extract_chunk, try_extract_chunk_detailed, try_extract_chunk_detailed_for_chunk,
};
pub use orchestration::{
    ChunkFailure, ChunkReport, ChunkTruncation, ExtractionProgress, ExtractionReport, ModelTier,
    OrchestrationOptions, TierFailure, extract_chunks_with,
};
// Library scanning: list + classify the cookbooks in a directory of epubs
// (native: needs std::fs + the LLM classifier).
#[cfg(feature = "native")]
pub use library::{
    BookMeta, CookbookGuess, book_cover, book_metadata, classify_by_tags, classify_cookbooks_ai,
    find_epubs,
};
// Native entrypoints configure backends and caching, then use the shared driver.
// Keep their existing crate-root paths stable.
#[cfg(feature = "native")]
pub use backend::{
    ChunkDebug, CookbookExtractionReport, Options, debug_extract_cookbook, extract_cookbook,
    extract_cookbook_detailed, extract_cookbook_detailed_with_progress, extract_cookbook_report,
    extract_cookbook_report_with_progress, extract_cookbook_with, extract_cookbook_with_progress,
};
// Section + time types are shared with the web scraper — one shape workspace-wide.
pub use recipe_parsing::ParsedSection;
pub use recipe_types::{RecipeSection, RecipeTimes};

use std::io::Cursor;

use epub::doc::EpubDoc;
use recipe_parsing::parse_sections;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Errors from EPUB recipe extraction.
#[derive(Debug, Error)]
pub enum EpubError {
    /// The bytes were not a readable EPUB (bad zip, missing OPF, …).
    #[error("could not read epub: {0}")]
    Open(String),
    /// No gateway token: neither `AI_GATEWAY_API_KEY` nor `CF_AIG_TOKEN` was set.
    /// The Cloudflare AI Gateway authenticates the caller with it (BYOK).
    #[error(
        "no gateway token: set AI_GATEWAY_API_KEY (or CF_AIG_TOKEN) for the Cloudflare AI Gateway"
    )]
    MissingApiKey,
    /// No gateway URL: `CLOUDFLARE_AI_GATEWAY_BASE_URL` was not set. All model
    /// traffic routes through the gateway by design — there is no direct-provider path.
    #[error(
        "no gateway URL: set CLOUDFLARE_AI_GATEWAY_BASE_URL to your Cloudflare AI Gateway root (…/v1/<account>/<gateway>)"
    )]
    MissingBaseUrl,
    /// The HTTP request to the model API failed (connection, timeout, …).
    #[cfg(feature = "native")]
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),
    /// The model API returned a non-success status or an unexpected shape.
    #[error("model api error (status {status}): {body}")]
    Api { status: u16, body: String },
    /// Structured native request failure; credentials and URLs are omitted.
    #[error("{0}")]
    Request(Box<RequestFailure>),
    /// JSON (de)serialization failed.
    #[error("deserialize error: {0}")]
    Deserialize(#[from] serde_json::Error),
    /// The on-disk cache could not be read or written.
    #[error("cache error: {0}")]
    Cache(String),
    /// The chunk-extraction call supplied by the caller failed. Used by the wasm
    /// driver, whose "call" is a JS proxy callback (threw or rejected); the
    /// native backends raise [`EpubError::Http`]/[`EpubError::Api`] instead.
    #[error("chunk extraction call failed: {0}")]
    Proxy(String),
}

#[derive(Debug, thiserror::Error, Serialize, Deserialize)]
#[error("request {kind}: {message} (status {status:?}, request {request_id:?})")]
pub struct RequestFailure {
    pub kind: String,
    pub message: String,
    pub status: Option<u16>,
    pub request_id: Option<String>,
    pub retry_after_secs: Option<u64>,
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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Chunk {
    /// A title hint from the TOC/heading, if any.
    pub title_hint: Option<String>,
    /// Cleaned, tag-stripped text (block/line breaks preserved).
    pub text: String,
    /// The originating spine-doc path, used to label `url`.
    pub doc_path: String,
    /// Internal anchor links found anywhere in this chunk's text (chunk-wide —
    /// the line they appeared on is not preserved). Used book-wide to confirm
    /// cross-recipe references.
    pub links: Vec<Link>,
    /// Embedded images found in this chunk, each tagged with the 0-based index of
    /// the `text` line (after splitting on `\n`) it sits nearest. The line index
    /// is the proximity coordinate used to bind a hero photo to a recipe title.
    pub images: Vec<(usize, ImageRef)>,
}

// The assembled-recipe data shapes (`CookbookRecipe`, `RecipeRef`,
// `RefConfidence`) are plain data and live in the deps-light `recipe-types` crate
// so the cookbook JSON contract can be depended on without this crate's EPUB/LLM
// stack. Re-exported here so existing `recipe_epub::CookbookRecipe` (etc.) paths
// are unchanged. The parser-aware operations live in [`CookbookRecipeExt`] below.
pub use recipe_types::{CookbookRecipe, ImageRef, RecipeRef, RefConfidence};

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
    /// [`recipe_parsing::parse_sections`] the web scraper uses).
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
            // Use the parser's native "has a digit but no parsed amount" signal
            // (the single source of truth) rather than re-deriving it here.
            .filter(|line| ip.from_str(line).parse_notes.unparsed_digit)
            .cloned()
            .collect()
    }
}

/// Progress snapshot emitted during [`extract_cookbook_with_progress`]: how many
/// chunks have finished extracting (`done`) out of `total`, and how many of those
/// came from the on-disk cache (`cached`). Counts are monotonic and internally
/// consistent.
#[derive(Debug, Clone, Copy, Default)]
pub struct ExtractProgress {
    /// Chunks finished so far.
    pub done: usize,
    /// Total chunks the book was split into (known once chunking completes).
    pub total: usize,
    /// Of the finished chunks, how many were served from cache (no API call).
    pub cached: usize,
}

/// Token usage + cost summary for one `extract_cookbook` run.
#[derive(Debug, Clone, Default, Serialize)]
pub struct ExtractionStats {
    /// Resolved model id. Empty for mocks or mixed-model extraction, whose
    /// cost cannot be represented here; use the detailed extraction entry points.
    pub model: String,
    /// Total chunks the book was split into.
    pub chunks_total: usize,
    /// Chunks served from the on-disk cache (no API call).
    pub chunks_cached: usize,
    /// Chunks whose extraction failed (network error or unparseable model
    /// output, after retries/escalation) and were skipped, contributing zero
    /// recipes. Distinguishes "the model found nothing here" from "this
    /// content was silently dropped" — a non-zero count means the returned
    /// recipe list is incomplete.
    pub chunks_failed: usize,
    /// Summed token usage across the API calls actually made.
    pub usage: Usage,
}

impl ExtractionStats {
    /// Estimated USD cost of the API calls, or `None` if the model's pricing is
    /// unknown. Cache writes bill ~1.25× input, cache reads ~0.1× input.
    pub fn cost_usd(&self) -> Option<f64> {
        accounting::cost_for_usage(&self.model, &self.usage)
    }

    /// One-line human summary for CLI stderr / UI.
    pub fn summary(&self) -> String {
        let u = &self.usage;
        let cost = self
            .cost_usd()
            .map_or_else(|| "cost: n/a".to_string(), |c| format!("~${c:.4}"));
        // Only called out when non-zero, so a clean run's summary is unchanged
        // — but a lossy one can't be mistaken for "the book just had fewer
        // recipes than expected".
        let failed = if self.chunks_failed > 0 {
            format!(
                " · {} chunk(s) FAILED (recipes missing)",
                self.chunks_failed
            )
        } else {
            String::new()
        };
        format!(
            "{}/{} chunks cached · {} in / {} out tok · {} cache-read tok · {cost}{failed}",
            self.chunks_cached,
            self.chunks_total,
            u.input_tokens,
            u.output_tokens,
            u.cache_read_input_tokens
        )
    }
}

/// Assemble per-chunk extractor output into final recipes and resolve
/// cross-recipe references — the pure post-LLM stage, no I/O.
///
/// `per_chunk` is `(Chunk, recipes)` in spine reading order (one entry per
/// chunk, including an empty recipe vector for each failed or empty extraction).
/// Omitting a gap loses adjacency information and can incorrectly attach a
/// continuation; callers must preserve every original slot. `links` is the book-wide set of internal anchor links (the Layer-2
/// reference-confirmation signal, gathered from every chunk). This is the wasm
/// boundary's counterpart to [`chunk_epub`]: the browser runs the LLM per chunk,
/// then hands the parsed outputs here to build the `CookbookRecipe[]`.
pub fn assemble_recipes(
    per_chunk: Vec<(Chunk, Vec<ExtractedRecipe>)>,
    links: Vec<Link>,
    source: &str,
) -> Vec<CookbookRecipe> {
    let mut recipes = assemble(per_chunk, source);
    resolve_references(&mut recipes, &links);
    recipes
}

/// Assemble recipe occurrences in complete chunk order. Only the first recipe
/// of a hinted chunk can continue the last occurrence of the immediately prior
/// chunk. Empty chunks are barriers; repeated titles alone are not identity.
pub(crate) fn assemble(
    per_chunk: Vec<(Chunk, Vec<ExtractedRecipe>)>,
    source: &str,
) -> Vec<CookbookRecipe> {
    assemble_slots(per_chunk.into_iter().map(Some), source)
}

fn continuation_title(title: &str) -> String {
    title
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// Missing extraction slots are retained as barriers, including in previews.
pub(crate) fn assemble_slots(
    slots: impl IntoIterator<Item = Option<(Chunk, Vec<ExtractedRecipe>)>>,
    source: &str,
) -> Vec<CookbookRecipe> {
    let mut out: Vec<CookbookRecipe> = Vec::new();
    // Original occurrence coordinates and the assembled output it belongs to.
    let mut previous: Option<((usize, usize), usize)> = None;
    for (chunk_index, slot) in slots.into_iter().enumerate() {
        let prior = previous.take();
        let Some((chunk, recipes)) = slot else {
            continue;
        };
        for (recipe_index, r) in recipes.into_iter().enumerate() {
            // Even an invalid last occurrence prevents attaching past it.
            previous = None;
            let title = r.meta.title.trim().to_string();
            if title.is_empty() {
                continue;
            }
            let normalized = continuation_title(&title);
            let continuation = prior.filter(|((prior_chunk, _), output)| {
                recipe_index == 0
                    && *prior_chunk + 1 == chunk_index
                    && chunk
                        .title_hint
                        .as_deref()
                        .is_some_and(|hint| continuation_title(hint) == normalized)
                    && continuation_title(&out[*output].meta.title) == normalized
            });
            let hero = hero_for(&chunk, &title);
            let output = if let Some((_, output)) = continuation {
                merge_recipe(&mut out[output], r, hero);
                output
            } else {
                // A long introduction may be split before the first ingredient.
                // Keep the head until adjacent hinted tails have had a chance
                // to complete it; discard ingredient-less noise after assembly.
                let mut meta = r.meta;
                meta.title = title;
                out.push(CookbookRecipe {
                    meta,
                    sections: r.sections,
                    source: source.to_string(),
                    url: format!("{source}#{}", chunk.doc_path),
                    references: Vec::new(),
                    image: hero,
                });
                out.len() - 1
            };
            previous = Some(((chunk_index, recipe_index), output));
        }
    }
    out.retain(|recipe| {
        recipe
            .sections
            .iter()
            .any(|section| !section.ingredients.is_empty())
    });
    out
}

/// The hero photo for a recipe: the image in `chunk` sitting nearest the recipe's
/// title line. Heroes sit just before their title (a `<figure>` then the heading),
/// so the nearest image *at or above* the title wins; only when none precede it do
/// we take the nearest below. `None` when the chunk has no images or the title
/// is absent or ambiguous in the chunk text. Reuses the reference-matching helpers so a
/// lightly-reformatted title still locates its line.
fn hero_for(chunk: &Chunk, title: &str) -> Option<ImageRef> {
    let norm_title = normalize_title(title);
    if chunk.images.is_empty() || norm_title.is_empty() {
        return None;
    }
    let mut exact_lines = Vec::new();
    let mut containing_lines = Vec::new();
    for (index, line) in chunk.text.split('\n').enumerate() {
        let normalized = normalize_title(line);
        if normalized == norm_title {
            exact_lines.push(index);
        } else if contains_whole_tokens(&normalized, &norm_title) {
            containing_lines.push(index);
        }
    }
    // A complete title line is stronger evidence than an ingredient/reference
    // mentioning that title. Only fall back to a partial title when no complete
    // title line exists. Multiple candidates at either level remain ambiguous.
    let candidates = if exact_lines.is_empty() {
        &containing_lines
    } else {
        &exact_lines
    };
    let [title_idx] = candidates.as_slice() else {
        return None;
    };
    let title_idx = *title_idx;
    let nearest_above = chunk
        .images
        .iter()
        .filter(|(li, _)| *li <= title_idx)
        .min_by_key(|(li, _)| title_idx - *li);
    nearest_above
        .or_else(|| {
            chunk
                .images
                .iter()
                .filter(|(li, _)| *li > title_idx)
                .min_by_key(|(li, _)| *li - title_idx)
        })
        .map(|(_, img)| img.clone())
}

/// Fold a continuation half of a recipe into the already-assembled one: append
/// its sections without deduplicating authored ingredients or instructions.
/// Preserve the existing exact union for notes/equipment and fill metadata
/// absent from the head with values supplied by the continuation.
fn merge_recipe(into: &mut CookbookRecipe, from: ExtractedRecipe, hero: Option<ImageRef>) {
    into.sections.extend(from.sections);
    for note in from.meta.notes {
        if !into.meta.notes.contains(&note) {
            into.meta.notes.push(note);
        }
    }
    // Keep the first hero found; fill from the continuation chunk only if the
    // title-bearing chunk had none.
    if into.image.is_none() {
        into.image = hero;
    }
    // Fill metadata only the continuation chunk happened to capture.
    let m = &mut into.meta;
    m.description = match (m.description.take(), from.meta.description) {
        (Some(head), Some(tail)) => Some(format!("{head}\n\n{tail}")),
        (head, tail) => head.or(tail),
    };
    m.recipe_yield = m.recipe_yield.take().or(from.meta.recipe_yield);
    m.times = m.times.take().or(from.meta.times);
    m.category = m.category.take().or(from.meta.category);
    m.page = m.page.take().or(from.meta.page);
    for equipment in from.meta.equipment {
        if !m.equipment.contains(&equipment) {
            m.equipment.push(equipment);
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
    let bytes = haystack.as_bytes();
    let mut start = 0;
    // `str::find` only returns char-boundary offsets, so no match is skipped even
    // when the haystack contains multi-byte UTF-8 (accented Latin, CJK, …). The
    // `b' '` boundary checks stay valid because `normalize_title` guarantees
    // ASCII-space separators.
    while let Some(pos) = haystack[start..].find(needle) {
        let abs = start + pos;
        let before_ok = abs == 0 || bytes[abs - 1] == b' ';
        let after = abs + needle.len();
        let after_ok = after == haystack.len() || bytes[after] == b' ';
        if before_ok && after_ok {
            return true;
        }
        // Advance by one whole char, not one byte: `abs + 1` can land mid-char
        // when `needle` begins with a multi-byte glyph (accented Latin, CJK, …),
        // panicking on the next `haystack[start..]` slice. Stepping a full char
        // stays on a UTF-8 boundary and preserves overlapping-match semantics.
        let step = haystack[abs..].chars().next().map_or(1, char::len_utf8);
        start = abs + step;
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
pub(crate) fn resolve_references(recipes: &mut [CookbookRecipe], links: &[Link]) {
    // (normalized_title, real_title, idx), longest normalized title first so the
    // most specific match wins.
    let mut titles: Vec<(String, String, usize)> = recipes
        .iter()
        .enumerate()
        .map(|(i, r)| (normalize_title(&r.meta.title), r.meta.title.clone(), i))
        .filter(|(norm, _, _)| !norm.is_empty())
        .collect();
    let mut title_counts = std::collections::HashMap::new();
    for (normalized, _, _) in &titles {
        *title_counts.entry(normalized.clone()).or_insert(0usize) += 1;
    }
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
                        if title_counts[norm_title] > 1 {
                            break; // Ambiguity must not fall through to a shorter title.
                        }
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

// ===========================================================================
// Image materialization. References (`ImageRef`) are carried in the recipe data;
// the actual bytes are read lazily from the EPUB by these helpers when something
// (the app, an exporter) needs to display or save a photo. All pure byte ops over
// the in-memory `.epub` — no fs/network — so they compile everywhere `chunk_epub`
// does. The file-backed `book_cover` (scanning a library by path) lives in
// `library.rs` (native).
// ===========================================================================

/// The cover photo of an EPUB as an [`ImageRef`] (path + mime, not bytes), or
/// `None` if the book declares no cover. Pair with [`read_image`] to get the bytes.
pub fn cover_image_ref(bytes: &[u8]) -> Option<ImageRef> {
    let mut doc = EpubDoc::from_reader(Cursor::new(bytes)).ok()?;
    cover_ref_from_doc(&mut doc)
}

/// Read one image resource's bytes (+ mime) from an EPUB by its archive path —
/// the lazy half of the reference model. `None` if the path isn't in the archive.
pub fn read_image(bytes: &[u8], path: &str) -> Option<(Vec<u8>, String)> {
    let mut doc = EpubDoc::from_reader(Cursor::new(bytes)).ok()?;
    let data = doc.get_resource_by_path(path)?;
    let mime = doc
        .get_resource_mime_by_path(path)
        .filter(|m| !m.is_empty())
        .or_else(|| epub_text::mime_from_ext(path))
        .unwrap_or_else(|| "application/octet-stream".to_string());
    Some((data, mime))
}

/// Read, in a single EPUB open, the cover plus the bytes of every recipe's hero
/// photo — the convenience the app's load worker calls so all image I/O happens
/// off the UI thread. Returns the cover reference and a de-duplicated
/// `(archive_path, bytes)` list covering the cover and all heroes.
pub fn collect_recipe_images(
    bytes: &[u8],
    recipes: &[CookbookRecipe],
) -> (Option<ImageRef>, Vec<(String, Vec<u8>)>) {
    let Ok(mut doc) = EpubDoc::from_reader(Cursor::new(bytes)) else {
        return (None, Vec::new());
    };
    let cover = cover_ref_from_doc(&mut doc);

    // Unique archive paths to materialize: the cover, then each recipe's hero.
    let mut paths: Vec<String> = Vec::new();
    let push_unique = |p: &str, paths: &mut Vec<String>| {
        if !paths.iter().any(|q| q == p) {
            paths.push(p.to_string());
        }
    };
    if let Some(c) = &cover {
        push_unique(&c.path, &mut paths);
    }
    for r in recipes {
        if let Some(img) = &r.image {
            push_unique(&img.path, &mut paths);
        }
    }

    let items = paths
        .into_iter()
        .filter_map(|p| doc.get_resource_by_path(&p).map(|data| (p, data)))
        .collect();
    (cover, items)
}

/// Resolve an open EPUB's cover id to an [`ImageRef`]. Shared by
/// [`cover_image_ref`] and [`collect_recipe_images`].
fn cover_ref_from_doc<R: std::io::Read + std::io::Seek>(doc: &mut EpubDoc<R>) -> Option<ImageRef> {
    let id = doc.get_cover_id()?;
    let item = doc.resources.get(&id)?;
    let path = item.path.to_string_lossy().into_owned();
    let mime = if item.mime.is_empty() {
        epub_text::mime_from_ext(&path).unwrap_or_else(|| "application/octet-stream".to_string())
    } else {
        item.mime.clone()
    };
    Some(ImageRef {
        path,
        mime,
        alt: None,
    })
}

// ===========================================================================
// Book-level OPF metadata (title / authors / subjects). The wasm-safe path —
// reads only the OPF the EPUB already parsed on open, no content decompression.
// `library.rs::book_metadata` is the file-backed counterpart (it adds the path
// and a file-stem title fallback); both share `meta_from_doc`.
// ===========================================================================

/// Book-level metadata from an EPUB's OPF, available from any open document
/// (file- or reader-backed). The wasm-safe sibling of [`BookMeta`], which adds
/// the source file path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EpubMeta {
    /// OPF `<dc:title>`; empty when the book declares none. The reader-backed
    /// path has no file name to fall back on, so the caller supplies a default.
    pub title: String,
    /// OPF `<dc:creator>` entries.
    pub authors: Vec<String>,
    /// OPF `<dc:subject>` tags (Calibre genres). Often empty.
    pub subjects: Vec<String>,
    /// OPF `<dc:identifier>` values, verbatim and unfiltered. A book usually
    /// declares several in different schemes — `urn:uuid:...`, `urn:isbn:...`,
    /// a bare ISBN, a Calibre id — and the OPF's `unique-identifier` attribute
    /// names whichever one it considers canonical. Nothing here interprets or
    /// ranks them: which scheme a consumer wants is the consumer's business,
    /// and dropping the ones this crate doesn't recognise would be lossy.
    pub identifiers: Vec<String>,
}

/// Extract title/authors/subjects/identifiers from an already-open EPUB's OPF.
/// Shared by the wasm-safe [`epub_metadata`] and the file-backed
/// [`book_metadata`].
pub(crate) fn meta_from_doc<R: std::io::Read + std::io::Seek>(doc: &EpubDoc<R>) -> EpubMeta {
    let collect = |prop: &str| -> Vec<String> {
        doc.metadata
            .iter()
            .filter(|m| m.property == prop)
            .map(|m| m.value.trim().to_string())
            .filter(|v| !v.is_empty())
            .collect()
    };
    EpubMeta {
        title: doc
            .get_title()
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty())
            .unwrap_or_default(),
        authors: collect("creator"),
        subjects: collect("subject"),
        identifiers: collect("identifier"),
    }
}

/// Title/authors/subjects/identifiers from an in-memory EPUB's OPF — the wasm-safe metadata
/// read (`EpubDoc::from_reader`, no fs). `None` if the bytes aren't a readable
/// EPUB. Pure: parses only the OPF the EPUB loads on open.
pub fn epub_metadata(bytes: &[u8]) -> Option<EpubMeta> {
    let doc = EpubDoc::from_reader(Cursor::new(bytes)).ok()?;
    Some(meta_from_doc(&doc))
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
            ..Default::default()
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

    #[test]
    fn summary_calls_out_failed_chunks_only_when_nonzero() {
        // A clean run's summary must not mention failures at all.
        let clean = ExtractionStats {
            model: "claude-haiku-4-5".to_string(),
            chunks_total: 5,
            chunks_cached: 2,
            ..Default::default()
        };
        assert!(!clean.summary().contains("FAILED"));

        // A lossy run must surface the count so it can't be mistaken for "the
        // book just had fewer recipes than expected".
        let lossy = ExtractionStats {
            model: "claude-haiku-4-5".to_string(),
            chunks_total: 5,
            chunks_cached: 2,
            chunks_failed: 2,
            ..Default::default()
        };
        assert!(lossy.summary().contains("2 chunk(s) FAILED"));
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

    /// A bare chunk carrying just a `doc_path` — text/images empty, so `hero_for`
    /// yields `None`. For assemble tests that don't exercise photo binding.
    /// A chunk carrying the continuation hint the segmenter sets after a hard
    /// mid-recipe split.
    fn continuation_chunk(doc_path: &str, hint: &str) -> Chunk {
        Chunk {
            title_hint: Some(hint.to_string()),
            ..chunk(doc_path)
        }
    }

    fn chunk(doc_path: &str) -> Chunk {
        Chunk {
            title_hint: None,
            text: String::new(),
            doc_path: doc_path.to_string(),
            links: Vec::new(),
            images: Vec::new(),
        }
    }

    fn img(path: &str) -> ImageRef {
        ImageRef {
            path: path.to_string(),
            mime: "image/jpeg".to_string(),
            alt: None,
        }
    }

    /// An ingredient-less recipe merges only when THIS chunk is the continuation
    /// the title_hint mechanism created. A plain later chunk re-emitting the same
    /// title with no ingredients is model noise, and appending its notes to the
    /// real recipe would corrupt it.
    #[test]
    fn ingredient_less_merge_requires_the_continuation_hint() {
        let tail = |note: &str| ExtractedRecipe {
            meta: RecipeMeta {
                title: "Pancakes".to_string(),
                notes: vec![note.to_string()],
                ..Default::default()
            },
            sections: vec![RecipeSection {
                name: None,
                ingredients: vec![],
                instructions: vec!["Bake.".to_string()],
            }],
        };

        // Hinted: the legitimate tail of a split recipe — merges.
        let hinted = assemble(
            vec![
                (chunk("c1.xhtml"), vec![er("Pancakes", &["1 cup flour"])]),
                (
                    continuation_chunk("c2.xhtml", "Pancakes"),
                    vec![tail("do-ahead")],
                ),
            ],
            "book.epub",
        );
        assert_eq!(hinted.len(), 1);
        assert!(hinted[0].meta.notes.iter().any(|n| n == "do-ahead"));

        // Unhinted: same shape, no continuation evidence — dropped as noise.
        let unhinted = assemble(
            vec![
                (chunk("c1.xhtml"), vec![er("Pancakes", &["1 cup flour"])]),
                (chunk("c2.xhtml"), vec![tail("noise")]),
            ],
            "book.epub",
        );
        assert_eq!(unhinted.len(), 1);
        assert!(
            unhinted[0].meta.notes.is_empty(),
            "model noise was merged into a real recipe: {:?}",
            unhinted[0].meta.notes
        );
    }

    #[test]
    fn assemble_preserves_same_title_occurrences_and_drops_empty() {
        let per_chunk = vec![
            (chunk("c1.xhtml"), vec![er("Pancakes", &["1 cup flour"])]),
            (
                chunk("c2.xhtml"),
                vec![
                    er("PANCAKES", &["dupe"]), // distinct occurrence
                    er("  ", &["no name"]),    // dropped: empty name
                    er("Soup", &[]),           // dropped: no ingredients
                    er("Omelette", &["3 eggs"]),
                ],
            ),
        ];
        let out = assemble(per_chunk, "book.epub");
        let names: Vec<_> = out.iter().map(|r| r.meta.title.as_str()).collect();
        assert_eq!(names, vec!["Pancakes", "PANCAKES", "Omelette"]);
        // Each occurrence keeps its original source.
        assert_eq!(out[0].url, "book.epub#c1.xhtml");
        assert_eq!(out[0].sections.len(), 1);
        assert_eq!(out[1].url, "book.epub#c2.xhtml");
    }

    /// A recipe split across a chunk boundary: the title chunk has the body, the
    /// (title-hinted) continuation chunk re-emits the same title with only the
    /// tail / notes. They must merge into one recipe whose notes are the union.
    #[test]
    fn continuation_keeps_a_headnote_before_the_first_ingredient() {
        let head = ExtractedRecipe {
            meta: RecipeMeta {
                title: "Long recipe".into(),
                description: Some("Source introduction before the split".into()),
                ..Default::default()
            },
            sections: vec![],
        };
        let tail = ExtractedRecipe {
            meta: RecipeMeta {
                title: "Long recipe".into(),
                description: Some("Source introduction after the split".into()),
                ..Default::default()
            },
            sections: vec![RecipeSection {
                name: None,
                ingredients: vec!["1 cup beans".into()],
                instructions: vec!["Cook until tender.".into()],
            }],
        };
        let result = assemble(
            vec![
                (chunk("chapter.xhtml"), vec![head.clone()]),
                (
                    continuation_chunk("chapter.xhtml", "Long recipe"),
                    vec![tail],
                ),
            ],
            "book",
        );
        assert_eq!(result.len(), 1);
        assert_eq!(
            result[0].meta.description.as_deref(),
            Some("Source introduction before the split\n\nSource introduction after the split")
        );
        assert!(assemble(vec![(chunk("chapter.xhtml"), vec![head])], "book").is_empty());
    }

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
                (chunk("c1.xhtml"), vec![title_half]),
                (
                    continuation_chunk("c2.xhtml", "Chocolate Chip Cookies"),
                    vec![cont_half],
                ),
            ],
            "book.epub",
        );
        assert_eq!(out.len(), 1);
        // Preserve the exact union of notes while appending all sections.
        assert_eq!(out[0].meta.notes, vec!["Note A", "Note B"]);
        assert_eq!(out[0].sections.len(), 2);
        // Metadata only the continuation chunk captured is filled in.
        assert_eq!(out[0].meta.recipe_yield.as_deref(), Some("Makes 18"));
    }

    #[rstest::rstest]
    #[case(false)]
    #[case(true)]
    fn empty_and_missing_chunks_break_continuations(#[case] missing: bool) {
        let barrier = if missing {
            None
        } else {
            Some((chunk("gap"), vec![]))
        };
        let out = assemble_slots(
            vec![
                Some((chunk("head"), vec![er("Cake", &["flour"])])),
                barrier,
                Some((
                    continuation_chunk("tail", "Cake"),
                    vec![er("Cake", &["sugar"])],
                )),
            ],
            "book",
        );
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn continuation_attaches_only_first_to_previous_last_and_can_chain() {
        let out = assemble(
            vec![
                (
                    chunk("one"),
                    vec![er("Cake", &["flour"]), er("Soup", &["stock"])],
                ),
                (
                    continuation_chunk("two", " SOUP "),
                    vec![er("soup", &["stock"])],
                ),
                (
                    continuation_chunk("three", "Soup"),
                    vec![er("Soup", &[]), er("Soup", &["water"])],
                ),
            ],
            "book",
        );
        assert_eq!(out.len(), 3);
        assert_eq!(out[1].sections.len(), 3);
        assert_eq!(
            out[1].sections[0].ingredients,
            out[1].sections[1].ingredients
        );
        assert_eq!(
            out[1].sections[0].instructions,
            out[1].sections[1].instructions
        );
        assert_eq!(out[2].sections.len(), 1);
        let out = assemble(
            vec![
                (
                    chunk("one"),
                    vec![er("Cake", &["flour"]), er("Soup", &["stock"])],
                ),
                (
                    continuation_chunk("two", "Cake"),
                    vec![er("Cake", &["sugar"])],
                ),
            ],
            "book",
        );
        assert_eq!(out.len(), 3);
    }

    #[test]
    fn repeated_recipe_titles_are_ambiguous_reference_targets() {
        let out = assemble_recipes(
            vec![(
                chunk("one"),
                vec![
                    er("Chocolate Cake", &["flour"]),
                    er("Chocolate Cake", &["sugar"]),
                    er("Dessert", &["1 recipe Chocolate Cake"]),
                ],
            )],
            vec![],
            "book",
        );
        assert_eq!(out.len(), 3);
        assert!(out[2].references.is_empty());
    }

    #[test]
    fn hero_for_binds_nearest_image_per_recipe() {
        // One chunk with two recipes; each title's image sits on its own line
        // (the common case — an empty <figure> binds the image to the heading).
        let c = Chunk {
            title_hint: None,
            text: [
                "Chocolate Cake",
                "2 cups flour",
                "Vanilla Cake",
                "1 cup sugar",
            ]
            .join("\n"),
            doc_path: "c.xhtml".to_string(),
            links: Vec::new(),
            images: vec![(0, img("choc.jpg")), (2, img("vanilla.jpg"))],
        };
        // Each recipe binds the image nearest its own title.
        assert_eq!(hero_for(&c, "Chocolate Cake").unwrap().path, "choc.jpg");
        assert_eq!(hero_for(&c, "Vanilla Cake").unwrap().path, "vanilla.jpg");
        // A title not in the chunk text binds nothing.
        assert!(hero_for(&c, "Lemon Tart").is_none());
        // No images → no hero.
        assert!(hero_for(&chunk("c.xhtml"), "Chocolate Cake").is_none());
    }

    #[test]
    fn full_title_line_disambiguates_recipe_references_for_hero() {
        let c = Chunk {
            text: "1 recipe Sauce (this page)\nSauce\n1 item".to_string(),
            images: vec![(0, img("dish.jpg")), (1, img("sauce.jpg"))],
            ..chunk("recipes.xhtml")
        };
        assert_eq!(hero_for(&c, "Sauce").unwrap().path, "sauce.jpg");
    }

    #[test]
    fn same_title_occurrences_do_not_share_an_arbitrary_hero() {
        let recipes = assemble(
            vec![(
                Chunk {
                    text: "Cake\nflour\nCake\nsugar".to_string(),
                    images: vec![(0, img("first.jpg")), (2, img("second.jpg"))],
                    ..chunk("recipes.xhtml")
                },
                vec![er("Cake", &["flour"]), er("Cake", &["sugar"])],
            )],
            "book.epub",
        );
        assert_eq!(recipes.len(), 2);
        assert!(recipes.iter().all(|recipe| recipe.image.is_none()));
    }

    #[test]
    fn hero_for_prefers_image_above_the_title() {
        // An image just before the title (a figure → heading) beats one just after.
        let c = Chunk {
            title_hint: None,
            text: ["plated dish", "Apple Pie", "next thing"].join("\n"),
            doc_path: "c.xhtml".to_string(),
            links: Vec::new(),
            images: vec![(0, img("above.jpg")), (2, img("below.jpg"))],
        };
        assert_eq!(hero_for(&c, "Apple Pie").unwrap().path, "above.jpg");
    }

    #[test]
    fn assemble_attaches_and_keeps_first_hero_on_merge() {
        // The title-bearing chunk's hero wins; a later same-title chunk can't replace it.
        let c1 = Chunk {
            text: "Cake".to_string(),
            images: vec![(0, img("first.jpg"))],
            ..chunk("c1.xhtml")
        };
        let c2 = Chunk {
            text: "Cake".to_string(),
            images: vec![(0, img("second.jpg"))],
            ..continuation_chunk("c2.xhtml", "Cake")
        };
        let out = assemble(
            vec![
                (c1, vec![er("Cake", &["flour"])]),
                (c2, vec![er("Cake", &["sugar"])]),
            ],
            "book.epub",
        );
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].image.as_ref().unwrap().path, "first.jpg");

        // When the first chunk had no hero, the continuation supplies one.
        let p1 = Chunk {
            text: "Pie".to_string(),
            ..chunk("c1.xhtml")
        };
        let p2 = Chunk {
            text: "Pie".to_string(),
            images: vec![(0, img("late.jpg"))],
            ..continuation_chunk("c2.xhtml", "Pie")
        };
        let out2 = assemble(
            vec![(p1, vec![er("Pie", &["a"])]), (p2, vec![er("Pie", &["b"])])],
            "book.epub",
        );
        assert_eq!(out2[0].image.as_ref().unwrap().path, "late.jpg");
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

    #[test]
    fn contains_whole_tokens_handles_multibyte_titles() {
        // Accented Latin: the needle's leading char precedes a multi-byte char in
        // the haystack, so a naive `i += 1` byte-walk could step over the match.
        assert!(contains_whole_tokens("crème brûlée tart", "crème brûlée"));
        // Still respects whole-token boundaries with multi-byte chars present.
        assert!(!contains_whole_tokens("crème brûléed tart", "crème brûlée"));
        // CJK title found as a whole token.
        assert!(contains_whole_tokens(
            "1 recipe 麻婆豆腐 this page",
            "麻婆豆腐"
        ));
        // Regression: the needle's leading multi-byte char first appears *glued*
        // inside a larger word ("ôti" inside "rôti"), so the boundary check fails
        // and the scan must advance past it. The old `start = abs + 1` landed on
        // the second byte of 'ô' and panicked on the next slice; stepping a whole
        // char must instead skip cleanly and report no whole-token match.
        assert!(!contains_whole_tokens("rôti chicken", "ôti"));
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
            image: None,
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
