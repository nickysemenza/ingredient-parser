//! The pure extraction contract: the recipe shape the model returns, the
//! per-chunk LLM request builder, the response parser, the forced-tool schema,
//! and the `RecipeExtractor` trait (+ a test mock). Everything here is I/O-free
//! and compiles to wasm32. The live reqwest backends live in [`crate::backend`]
//! (native-only).

use serde::{Deserialize, Serialize};
use serde_json::json;

use recipe_types::RecipeSection;

use crate::{Chunk, EpubError};

// `RecipeMeta` is a plain data shape; it lives in the deps-light `recipe-types`
// crate and is re-exported here (and from the crate root) so existing
// `recipe_epub::RecipeMeta` paths are unchanged.
pub use recipe_types::RecipeMeta;
use recipe_types::RecipeTimes;

/// Deserialize a `Vec<T>` from malformed LLM tool output. Tolerates an explicit
/// `null` (→ empty) and a JSON-string-encoded array — the model occasionally
/// double-encodes its whole tool `input`, sending `recipes` as the *string*
/// `"[{…}]"` rather than an array (serde rejects that with "invalid type:
/// string, expected a sequence"). A genuine array deserializes normally, and its
/// elements still run their own (equally lenient) field deserializers.
///
/// Lives here rather than in `recipe-types` because re-parsing the embedded JSON
/// needs `serde_json`, which the deps-light `recipe-types` crate omits on
/// purpose (its `null_as_empty_vec` covers the null-only leaf fields).
fn vec_lenient<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::de::DeserializeOwned,
{
    use serde::de::Error;

    let value = serde_json::Value::deserialize(deserializer)?;
    match value {
        serde_json::Value::Null => Ok(Vec::new()),
        serde_json::Value::String(s) => {
            let trimmed = s.trim();
            if trimmed.is_empty() {
                Ok(Vec::new())
            } else {
                serde_json::from_str(trimmed).map_err(Error::custom)
            }
        }
        other => serde_json::from_value(other).map_err(Error::custom),
    }
}

/// A recipe as segmented + labeled by the extractor (model output). Sections use
/// the shared [`recipe_types::RecipeSection`] type; ingredient/instruction
/// strings are **verbatim** — quantities are parsed downstream by the core
/// `ingredient` parser, never by the model.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct ExtractedRecipe {
    #[serde(flatten)]
    pub meta: RecipeMeta,
    // Lenient: a recipe with `sections: null` or missing sections degrades to an
    // empty list (its meta still survives) instead of failing the chunk.
    #[serde(default, deserialize_with = "vec_lenient")]
    pub sections: Vec<RecipeSection>,
}

/// Token usage reported by the model API for one call. Field names match the
/// Anthropic Messages API `usage` object; the OpenAI-compatible backend maps
/// its `prompt_tokens`/`completion_tokens` into this shape (see
/// `backend::OpenAiUsage`).
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
pub struct Usage {
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
    /// Tokens written to the prompt cache (billed ~1.25× input).
    #[serde(default)]
    pub cache_creation_input_tokens: u64,
    /// Tokens served from the prompt cache (billed ~0.1× input).
    #[serde(default)]
    pub cache_read_input_tokens: u64,
}

impl Usage {
    /// Accumulate another call's usage into this one.
    pub fn add(&mut self, other: &Usage) {
        self.input_tokens += other.input_tokens;
        self.output_tokens += other.output_tokens;
        self.cache_creation_input_tokens += other.cache_creation_input_tokens;
        self.cache_read_input_tokens += other.cache_read_input_tokens;
    }
}

/// One chunk's extraction result plus its cost signal.
#[derive(Debug, Clone)]
pub struct ChunkOutcome {
    pub recipes: Vec<ExtractedRecipe>,
    /// Token usage for the API call. Zero when served from cache.
    pub usage: Usage,
    /// True when served from the on-disk cache (no API call, no cost).
    pub cached: bool,
    /// True when the model's output hit the token limit, so `recipes` may be
    /// incomplete. A truncated outcome must NOT be cached — a later run (bigger
    /// limit, different model) should re-attempt the chunk.
    pub truncated: bool,
}

/// The LLM request for one chunk: the system prompt, the user text, and the
/// forced-tool definition. Built purely from a [`Chunk`], so the identical
/// request can be issued by the native backends *or* marshalled across the wasm
/// boundary and sent by a JS proxy. The prompt + recipe schema live here — one
/// source of truth, no TS mirror.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkRequest {
    /// System prompt (recipe-extraction instructions).
    pub system: String,
    /// User content: the chunk text, prefixed with a title hint when present.
    pub user: String,
    /// Forced-tool name the model must call.
    pub tool_name: String,
    /// JSON Schema for the forced tool's input (`{ recipes: [...] }`).
    pub tool_schema: serde_json::Value,
}

/// Build the LLM request for a chunk: prefix the title hint (if any), attach the
/// shared system prompt and the forced `emit_recipes` tool schema. Pure — no I/O.
pub fn build_chunk_request(chunk: &Chunk) -> ChunkRequest {
    let user = match &chunk.title_hint {
        Some(t) => format!("Section title: {t}\n\n{}", chunk.text),
        None => chunk.text.clone(),
    };
    ChunkRequest {
        system: SYSTEM_PROMPT.to_string(),
        user,
        tool_name: TOOL_NAME.to_string(),
        tool_schema: recipes_tool_schema(),
    }
}

/// Parse a freeform printed time ("30 minutes", "1 hr 15 min") into whole
/// minutes. Unlike the scraper's ISO-8601 input this is prose a model copied off
/// a cookbook page, so it is read strictly: the whole string must be hour and
/// minute counts (an optional leading approximator aside), or it isn't a number
/// we're willing to sort by. A range ("30 to 40 minutes"), a fraction ("1 1/2
/// hours"), a qualifier ("30 minutes, plus chilling") and a bare word
/// ("overnight") all yield `None` — the display string still carries them, and a
/// wrong number is worse than no number.
fn parse_freeform_duration(input: &str) -> Option<u32> {
    let lowered = input.trim().to_ascii_lowercase();
    let mut rest = lowered.as_str();
    // "About 30 minutes" states the same duration as "30 minutes"; the hedge
    // doesn't make the count ambiguous, so strip one and read on.
    for approx in [
        "about ",
        "approximately ",
        "approx. ",
        "approx ",
        "around ",
        "roughly ",
        "~",
    ] {
        if let Some(stripped) = rest.strip_prefix(approx) {
            rest = stripped.trim_start();
            break;
        }
    }

    let mut total: u32 = 0;
    let mut saw_component = false;
    let mut chars = rest.chars().peekable();
    while let Some(&c) = chars.peek() {
        if c == ' ' {
            chars.next();
            continue;
        }
        if !c.is_ascii_digit() {
            // Anything that isn't a count is prose we can't read confidently.
            return None;
        }
        let mut num = String::new();
        while let Some(&d) = chars.peek() {
            if d.is_ascii_digit() {
                num.push(d);
                chars.next();
            } else {
                break;
            }
        }
        while chars.peek() == Some(&' ') {
            chars.next();
        }
        let mut unit = String::new();
        while let Some(&u) = chars.peek() {
            if u.is_ascii_alphabetic() {
                unit.push(u);
                chars.next();
            } else {
                break;
            }
        }
        // An abbreviation may be written with a period ("1 hr. 15 min.").
        if chars.peek() == Some(&'.') {
            chars.next();
        }
        let per_unit = match unit.as_str() {
            "h" | "hr" | "hrs" | "hour" | "hours" => 60,
            "m" | "min" | "mins" | "minute" | "minutes" => 1,
            // A number with no unit, or a unit we don't measure recipe times in.
            _ => return None,
        };
        total = total.checked_add(num.parse::<u32>().ok()?.checked_mul(per_unit)?)?;
        saw_component = true;
    }

    (saw_component && total > 0).then_some(total)
}

/// Derive each `*_minutes` count from the matching display string the model
/// emitted. Only fills a count that is absent, so a model that ever starts
/// emitting the numbers itself wins over this fallback.
fn fill_time_minutes(times: &mut RecipeTimes) {
    let pairs: [(&Option<String>, &mut Option<u32>); 4] = [
        (&times.active, &mut times.active_minutes),
        (&times.total, &mut times.total_minutes),
        (&times.prep, &mut times.prep_minutes),
        (&times.cook, &mut times.cook_minutes),
    ];
    for (text, minutes) in pairs {
        if minutes.is_none() {
            *minutes = text.as_deref().and_then(parse_freeform_duration);
        }
    }
}

/// Parse the forced tool's `input` object (`{ recipes: [ExtractedRecipe, …] }`)
/// into recipes. The single place LLM output is decoded — shared by the native
/// backends and the wasm `assemble_recipes` path — and therefore the single
/// place the freeform printed times are turned into sortable minute counts.
pub fn parse_recipes_payload(input: serde_json::Value) -> Result<Vec<ExtractedRecipe>, EpubError> {
    let mut recipes = serde_json::from_value::<RecipesPayload>(input)?.recipes;
    for recipe in &mut recipes {
        if let Some(times) = recipe.meta.times.as_mut() {
            fill_time_minutes(times);
        }
    }
    Ok(recipes)
}

/// One extra attempt after the first, so one model gets at most `1 + PARSE_RETRIES`
/// calls per chunk. The model occasionally emits a payload that's valid-but-
/// unparseable (most often the whole `recipes` array double-encoded as a *string*
/// with under-escaped quotes — invalid JSON no deserializer can repair). That's
/// usually stochastic, so a re-issued identical request comes back clean. The
/// disjoint-failure escalation (a *different* model) is the caller's job.
pub const PARSE_RETRIES: usize = 1;

/// One model call's structured output: the raw tool `input` (`None` if the model
/// returned no tool block) plus caller-supplied cost/limit signals.
pub struct CallResult {
    pub input: Option<serde_json::Value>,
    pub usage: Usage,
    pub truncated: bool,
}

/// A failed model call with any usage and truncation metadata the transport was
/// still able to recover.
///
/// Use [`CallFailure::retryable_payload`] when a response arrived but its tool
/// arguments were malformed. The shared driver retries those failures under the
/// same policy as a decoded payload that fails [`parse_recipes_payload`].
#[derive(Debug)]
pub struct CallFailure {
    pub error: EpubError,
    pub usage: Usage,
    pub truncated: bool,
    retryable: bool,
}

impl CallFailure {
    /// A transport/provider failure. It is not retried as a parse failure.
    pub fn transport(error: EpubError) -> Self {
        Self {
            error,
            usage: Usage::default(),
            truncated: false,
            retryable: false,
        }
    }

    /// A transport/provider failure carrying metadata recovered from a response.
    pub fn transport_with_metadata(error: EpubError, usage: Usage, truncated: bool) -> Self {
        Self {
            error,
            usage,
            truncated,
            retryable: false,
        }
    }

    /// Malformed structured output that can be retried with the same model.
    pub fn retryable_payload(error: EpubError, usage: Usage, truncated: bool) -> Self {
        Self {
            error,
            usage,
            truncated,
            retryable: true,
        }
    }
}

impl core::fmt::Display for CallFailure {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        core::fmt::Display::fmt(&self.error, formatter)
    }
}

impl std::error::Error for CallFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.error)
    }
}

/// One failed call/parse attempt within a model tier.
#[derive(Debug, Clone, Serialize)]
pub struct FailedAttempt {
    /// Zero-based call attempt within the tier.
    pub attempt: usize,
    pub message: String,
    pub usage: Usage,
    pub truncated: bool,
}

/// A model tier that could not produce a parseable chunk after its allowed
/// attempts. Usage includes every attempt, successful HTTP response or not.
#[derive(Debug)]
pub struct ChunkExtractionFailure {
    pub error: EpubError,
    pub usage: Usage,
    pub truncated: bool,
    pub attempts: Vec<FailedAttempt>,
}

impl From<EpubError> for ChunkExtractionFailure {
    fn from(error: EpubError) -> Self {
        Self {
            // The legacy interface exposes no attempt-level information.
            attempts: Vec::new(),
            error,
            usage: Usage::default(),
            truncated: false,
        }
    }
}

impl core::fmt::Display for ChunkExtractionFailure {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        core::fmt::Display::fmt(&self.error, formatter)
    }
}

impl std::error::Error for ChunkExtractionFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.error)
    }
}

/// Recipes decoded from one model for one chunk, with accumulated usage.
#[derive(Debug, Clone)]
pub struct DrivenChunk {
    pub recipes: Vec<ExtractedRecipe>,
    pub usage: Usage,
    pub truncated: bool,
}

/// Detailed form of [`try_extract_chunk`] for transports that can report usage
/// and truncation even when the call itself fails.
pub async fn try_extract_chunk_detailed<F, Fut>(
    doc_path: &str,
    call: F,
) -> Result<DrivenChunk, ChunkExtractionFailure>
where
    F: Fn() -> Fut,
    Fut: core::future::Future<Output = Result<CallResult, CallFailure>>,
{
    try_extract_chunk_detailed_with_validator(doc_path, call, |_| Ok(())).await
}

/// Like [`try_extract_chunk_detailed`], but rejects model strings that do not
/// occur in the supplied chunk before they can enter the assembled cookbook.
///
/// Browser callbacks should use this form: it gives native and wasm callers
/// the same source-preservation retry and failure accounting. The source is the
/// exact text sent to the model; a continuation title hint is accepted only for
/// the same-document continuation that supplied it.
pub async fn try_extract_chunk_detailed_for_chunk<F, Fut>(
    chunk: &Chunk,
    call: F,
) -> Result<DrivenChunk, ChunkExtractionFailure>
where
    F: Fn() -> Fut,
    Fut: core::future::Future<Output = Result<CallResult, CallFailure>>,
{
    try_extract_chunk_detailed_with_validator(&chunk.doc_path, call, |recipes| {
        validate_chunk_recipes(chunk, recipes)
    })
    .await
}

async fn try_extract_chunk_detailed_with_validator<F, Fut, V>(
    doc_path: &str,
    call: F,
    validate: V,
) -> Result<DrivenChunk, ChunkExtractionFailure>
where
    F: Fn() -> Fut,
    Fut: core::future::Future<Output = Result<CallResult, CallFailure>>,
    V: Fn(&[ExtractedRecipe]) -> Result<(), EpubError>,
{
    let mut usage = Usage::default();
    let mut attempts = Vec::new();
    let mut attempt = 0;
    loop {
        let result = match call().await {
            Ok(result) => result,
            Err(failure) => {
                usage.add(&failure.usage);
                let failed = FailedAttempt {
                    attempt,
                    message: failure.error.to_string(),
                    usage: failure.usage,
                    truncated: failure.truncated,
                };
                let can_retry = failure.retryable && !failure.truncated && attempt < PARSE_RETRIES;
                attempts.push(failed);
                if can_retry {
                    tracing::warn!(
                        "chunk {doc_path} payload didn't decode ({}); retrying",
                        failure.error
                    );
                    attempt += 1;
                    continue;
                }
                return Err(ChunkExtractionFailure {
                    error: failure.error,
                    usage,
                    truncated: failure.truncated,
                    attempts,
                });
            }
        };
        usage.add(&result.usage);
        match result.input {
            // No tool block / no recipes is a valid empty result, not a failure.
            None => {
                return Ok(DrivenChunk {
                    recipes: Vec::new(),
                    usage,
                    truncated: result.truncated,
                });
            }
            Some(input) => match parse_recipes_payload(input) {
                Ok(recipes) => match validate(&recipes) {
                    Ok(()) => {
                        return Ok(DrivenChunk {
                            recipes,
                            usage,
                            truncated: result.truncated,
                        });
                    }
                    Err(error) => {
                        attempts.push(FailedAttempt {
                            attempt,
                            message: error.to_string(),
                            usage: result.usage,
                            truncated: result.truncated,
                        });
                        if result.truncated || attempt >= PARSE_RETRIES {
                            return Err(ChunkExtractionFailure {
                                error,
                                usage,
                                truncated: result.truncated,
                                attempts,
                            });
                        }
                        tracing::warn!(
                            "chunk {doc_path} violated source fidelity ({error}); retrying"
                        );
                        attempt += 1;
                    }
                },
                Err(error) => {
                    attempts.push(FailedAttempt {
                        attempt,
                        message: error.to_string(),
                        usage: result.usage,
                        truncated: result.truncated,
                    });
                    if result.truncated || attempt >= PARSE_RETRIES {
                        return Err(ChunkExtractionFailure {
                            error,
                            usage,
                            truncated: result.truncated,
                            attempts,
                        });
                    }
                    tracing::warn!("chunk {doc_path} payload didn't parse ({error}); retrying");
                    attempt += 1;
                }
            },
        }
    }
}

/// Enforce the extractor's literal-source contract without attempting to repair
/// model output. We only normalize whitespace introduced by XHTML inline markup
/// (including a space before punctuation or around a hyphen); letters, numbers,
/// units, ranges, and punctuation must still occur in the sent source text.
pub fn validate_chunk_recipes(chunk: &Chunk, recipes: &[ExtractedRecipe]) -> Result<(), EpubError> {
    if recipes.is_empty() {
        let has_yield = chunk.text.lines().any(|line| {
            let upper = line.trim().to_ascii_uppercase();
            upper.starts_with("SERVES ") || upper.starts_with("MAKES ")
        });
        let quantities = chunk
            .text
            .lines()
            .filter(|line| {
                line.trim_start()
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_numeric())
                    && line.len() < 160
            })
            .count();
        if has_yield && quantities >= 3 {
            return Err(EpubError::Proxy("source has explicit recipe yield and multiple ingredient quantities, but no recipe was returned".into()));
        }
    }
    let source = normalize_source_whitespace(&chunk.text);
    let hint = chunk.title_hint.as_deref().map(normalize_source_whitespace);
    for (recipe_index, recipe) in recipes.iter().enumerate() {
        validate_source_field(
            &source,
            hint.as_deref(),
            recipe_index,
            "title",
            &recipe.meta.title,
        )?;
        if hint.as_deref() != Some(normalize_source_whitespace(&recipe.meta.title).as_str())
            && !matches_complete_lines(&chunk.text, &recipe.meta.title, true)
        {
            return Err(EpubError::Proxy(format!(
                "source fidelity violation in recipe {}: title {:?} omits part of its authored line or subtitle",
                recipe_index + 1,
                recipe.meta.title
            )));
        }
        for section in &recipe.sections {
            if let Some(name) = &section.name {
                validate_source_field(&source, None, recipe_index, "section label", name)?;
            }
            for ingredient in &section.ingredients {
                validate_source_field(&source, None, recipe_index, "ingredient", ingredient)?;
                if !matches_complete_lines(&chunk.text, ingredient, false) {
                    return Err(EpubError::Proxy(format!(
                        "source fidelity violation in recipe {}: ingredient must preserve complete authored lines",
                        recipe_index + 1
                    )));
                }
            }
            for instruction in &section.instructions {
                validate_source_field(&source, None, recipe_index, "instruction", instruction)?;
            }
        }
    }
    if recipes.len() == 1
        && recipes[0]
            .sections
            .iter()
            .any(|s| !s.ingredients.is_empty())
        && recipes[0]
            .sections
            .iter()
            .all(|s| s.instructions.is_empty())
    {
        let method_lines = chunk
            .text
            .lines()
            .filter(|line| {
                line.len() > 100
                    && line.split_whitespace().next().is_some_and(|word| {
                        matches!(
                            word,
                            "Mix"
                                | "Heat"
                                | "Crush"
                                | "Boil"
                                | "Rinse"
                                | "Whisk"
                                | "Fry"
                                | "Knead"
                                | "Bake"
                        )
                    })
            })
            .count();
        if method_lines >= 2 {
            return Err(EpubError::Proxy(
                "source coverage violation: shared method paragraphs were omitted".into(),
            ));
        }
    }
    validate_component_heading_placement(chunk, recipes)?;
    Ok(())
}

/// Match whole authored lines, allowing wrapped blocks to be joined. Never
/// accept a substring that silently removes food, quantities, or a subtitle.
fn matches_complete_lines(source: &str, value: &str, title: bool) -> bool {
    let lines: Vec<_> = source
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    let wanted = normalize_source_whitespace(value);
    for start in 0..lines.len() {
        if title && start > 0 && lines[start - 1].trim_end().ends_with(['–', '—', '-']) {
            continue;
        }
        let mut joined = String::new();
        for line in &lines[start..] {
            if !joined.is_empty() {
                joined.push(' ');
            }
            joined.push_str(line);
            let normalized = normalize_source_whitespace(&joined);
            if normalized == wanted {
                return true;
            }
            if normalized.len() > wanted.len() {
                break;
            }
        }
    }
    false
}

/// Preserve authored component and serving labels structurally. A short,
/// standalone `For …` or `To serve` line is an unambiguous section heading; it
/// must therefore become a section name, never an ingredient string. We only
/// inspect spans whose emitted title maps to one unique source line. Repeated
/// titles and other uncertain boundaries are deliberately left alone.
fn validate_component_heading_placement(
    chunk: &Chunk,
    recipes: &[ExtractedRecipe],
) -> Result<(), EpubError> {
    let lines: Vec<String> = chunk
        .text
        .lines()
        .map(normalize_source_whitespace)
        .filter(|line| !line.is_empty())
        .collect();
    let mut mapped = Vec::new();
    for (recipe_index, recipe) in recipes.iter().enumerate() {
        let title = normalize_source_whitespace(&recipe.meta.title);
        let positions: Vec<usize> = lines
            .iter()
            .enumerate()
            .filter_map(|(line_index, line)| (line == &title).then_some(line_index))
            .collect();
        let output_title_count = recipes
            .iter()
            .filter(|other| normalize_source_whitespace(&other.meta.title) == title)
            .count();
        if positions.len() != 1 || output_title_count != 1 {
            // A later repeated or absent title makes every preceding span's end
            // uncertain. Do not let an ambiguous occurrence's heading be
            // attributed to a neighboring recipe.
            return Ok(());
        }
        mapped.push((positions[0], recipe_index));
    }
    mapped.sort_unstable_by_key(|(line_index, _)| *line_index);
    for (span_index, &(start, recipe_index)) in mapped.iter().enumerate() {
        let end = mapped
            .get(span_index + 1)
            .map_or(lines.len(), |(next, _)| *next);
        let recipe = &recipes[recipe_index];
        for heading in lines[start + 1..end]
            .iter()
            .filter(|line| is_component_heading(line))
        {
            let heading = heading.trim_end_matches(':').trim();
            let has_section = recipe.sections.iter().any(|section| {
                section.name.as_deref().is_some_and(|name| {
                    normalize_source_whitespace(name)
                        .trim_end_matches(':')
                        .trim()
                        == heading
                })
            });
            let heading_as_ingredient = recipe
                .sections
                .iter()
                .flat_map(|section| {
                    section.ingredients.iter().map(|ingredient| {
                        normalize_source_whitespace(ingredient)
                            .trim_end_matches(':')
                            .trim()
                            == heading
                    })
                })
                .any(|is_heading| is_heading);
            if !has_section || heading_as_ingredient {
                return Err(EpubError::Proxy(format!(
                    "source coverage violation in recipe {}: component heading must be a section label",
                    recipe_index + 1
                )));
            }
        }
    }
    Ok(())
}

fn is_component_heading(line: &str) -> bool {
    let trimmed = line.trim();
    let bare = trimmed.trim_end_matches(':').trim();
    if bare.is_empty()
        || bare.len() > 80
        || bare
            .chars()
            .last()
            .is_some_and(|c| matches!(c, '.' | '!' | '?' | ','))
    {
        return false;
    }
    let lower = bare.to_lowercase();
    lower.starts_with("for ") || lower == "to serve"
}

fn validate_source_field(
    source: &str,
    hint: Option<&str>,
    recipe_index: usize,
    field: &str,
    value: &str,
) -> Result<(), EpubError> {
    let value = normalize_source_whitespace(value);
    if value.is_empty() || source.contains(&value) || hint == Some(value.as_str()) {
        return Ok(());
    }
    Err(EpubError::Proxy(format!(
        "source fidelity violation in recipe {}: {field} is not present in the supplied text",
        recipe_index + 1
    )))
}

pub(crate) fn normalize_source_whitespace(value: &str) -> String {
    let collapsed = value.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut out = String::with_capacity(collapsed.len());
    let mut chars = collapsed.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == ' ' {
            let previous = out.chars().last();
            let next = chars.peek().copied();
            if next.is_some_and(|c| {
                matches!(c, ',' | '.' | ';' | ':' | ')' | ']' | '}' | '–' | '—' | '-')
            }) || previous.is_some_and(|c| matches!(c, '(' | '[' | '{' | '–' | '—' | '-'))
            {
                continue;
            }
        }
        out.push(ch);
    }
    out
}

/// Drive ONE model over one chunk: call it, decode the payload, and retry the
/// call up to [`PARSE_RETRIES`] times when the payload won't parse (malformed
/// JSON a fresh call usually avoids). Never retries on truncation — a same-size
/// retry would just truncate again. Returns `Err` once the attempts are spent, so
/// the caller can escalate to a different model or salvage (skip) the chunk.
///
/// `call` performs one extraction call and is the ONLY I/O — supplied by the
/// native reqwest backend or the wasm JS-callback driver — so this retry/parse
/// policy is shared verbatim across both. `doc_path` only labels log lines.
pub async fn try_extract_chunk<F, Fut>(doc_path: &str, call: F) -> Result<DrivenChunk, EpubError>
where
    F: Fn() -> Fut,
    Fut: core::future::Future<Output = Result<CallResult, EpubError>>,
{
    try_extract_chunk_detailed(doc_path, || async {
        call().await.map_err(CallFailure::transport)
    })
    .await
    .map_err(|failure| failure.error)
}

/// Turns a [`Chunk`] of cookbook text into zero or more recipes.
///
/// Static dispatch (used via generics) so we avoid the `async-trait` dep; the
/// concrete backends live in [`crate::backend`] (native), with a [`MockExtractor`]
/// here for tests.
#[allow(async_fn_in_trait)]
pub trait RecipeExtractor {
    async fn extract(&self, chunk: &Chunk) -> Result<ChunkOutcome, EpubError>;

    /// Detailed adapter used by whole-book orchestration. Existing implementations
    /// remain source-compatible; their legacy error type cannot report usage or
    /// truncation that was lost before returning `Err`.
    async fn extract_detailed(
        &self,
        chunk: &Chunk,
    ) -> Result<ChunkOutcome, ChunkExtractionFailure> {
        self.extract(chunk)
            .await
            .map_err(ChunkExtractionFailure::from)
    }

    /// Model id for cost attribution; empty when not applicable (e.g. the mock).
    fn model(&self) -> &str {
        ""
    }
}

// The forced tool's name + the system prompt + the input schema are the LLM
// contract; they live here (pure) so both the native backends and the wasm
// request builder share one definition. `pub(crate)` items are reached by
// [`crate::backend`] (and its tests).
pub(crate) const TOOL_NAME: &str = "emit_recipes";

const SYSTEM_PROMPT: &str = "\
You extract structured recipes from the text of one section of a cookbook. The \
section may contain zero, one, or many recipes. For every recipe actually \
present, return an object with:\n\
- title: the recipe's complete name copied VERBATIM, including a subtitle or \
translation on the following line. Do not abbreviate, expand, or reword it.\n\
- description: the headnote / intro blurb, if any (omit otherwise).\n\
- sections: the recipe's components as an array. Most recipes have ONE section \
(omit its name). Component recipes have several. Each section has:\n\
    - name: the component label copied VERBATIM (e.g. \"For the curry paste\"). \
A standalone `For …` or `To serve` label is a section name, NEVER an ingredient; \
Short standalone labels such as Masala, Batter, Filling, Paste, Salad, or Garnish \
are also component headings when followed by that component’s ingredients. \
Keep their ingredients grouped beneath those labels. \
omit the name only for the main/only section.\n\
    - ingredients: each ingredient line copied VERBATIM, one per entry. Do NOT \
parse, normalize, convert, or reword quantities or units — preserve the original \
text exactly. Preserve an apparent source typo or two ingredients printed on \
one line; do not silently split or repair it. For a wrapped ingredient, join only its complete source lines (e.g. \"1\\u2153 cups all-purpose flour (6.1 oz / 173g)\").\n\
    - instructions: the method steps for this component, copied verbatim, one per \
entry. If the recipe has a single shared method, put all of its steps in the \
main section. Never omit the shared method when there are several ingredient \
groups: use the unnamed main section for every shared step, in source order. \
Every section must include an instructions array, even when empty.\n\
- recipe_yield: the yield/servings line if present (e.g. \"Makes 1 loaf\", \"Serves 4\").\n\
- times: an object with any of active / total / prep / cook (e.g. \"Active Time: \
30 minutes\"); omit fields not present and omit the object if there are none.\n\
- equipment: special-equipment lines, if listed.\n\
- notes: every do-ahead / make-ahead note, tip, \"serve with\" suggestion, and \
numbered footnote/endnote (markers like ①②③ and their explanations, \
or a \"Do Ahead\" / \"Make Ahead\" block), each as a SEPARATE entry copied \
VERBATIM. Capture ALL of them — do not summarize, merge, or drop any. When the \
section holds several recipes, attach each note to the recipe it belongs to \
(use the inline ①②③ markers to map a footnote back to its recipe); \
never copy one recipe's notes onto another.\n\
- category: the chapter or category the recipe belongs to, if evident.\n\
- page: the page number, if present in the text.\n\
Ignore running chapter prose, page headers/footers, and photo captions unless \
they are recipe content. If the section contains no recipe, return an empty list.";

/// JSON Schema for the forced tool's input: `{ recipes: [ExtractedRecipe, ...] }`.
/// Shared by every backend (Anthropic `input_schema`, OpenAI/Gemini function
/// `parameters`) and by [`build_chunk_request`] for the wasm/proxy path.
pub fn recipes_tool_schema() -> serde_json::Value {
    let string = json!({ "type": "string" });
    let string_array = json!({ "type": "array", "items": { "type": "string" } });
    json!({
        "type": "object",
        "properties": {
            "recipes": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "title": string,
                        "description": string,
                        "sections": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "name": string,
                                    "ingredients": string_array,
                                    "instructions": string_array
                                },
                                "required": ["ingredients", "instructions"]
                            }
                        },
                        "recipe_yield": string,
                        "times": {
                            "type": "object",
                            "properties": {
                                "active": string, "total": string,
                                "prep": string, "cook": string
                            }
                        },
                        "equipment": string_array,
                        "notes": string_array,
                        "category": string,
                        "page": string
                    },
                    "required": ["title", "sections"]
                }
            }
        },
        "required": ["recipes"]
    })
}

/// The forced tool's `input` object. `pub(crate)` so [`crate::backend`]'s tests
/// (which decode real API payloads) can name it.
#[derive(Deserialize)]
pub(crate) struct RecipesPayload {
    // Lenient: tolerates `recipes: null` and the double-encoded `recipes:
    // "[{…}]"` string the model emits on a fraction of chunks.
    #[serde(default, deserialize_with = "vec_lenient")]
    pub(crate) recipes: Vec<ExtractedRecipe>,
}

/// What a [`MockExtractor`] rule matches on.
#[derive(Debug, Clone)]
pub enum MockMatch {
    /// The chunk's text contains this needle.
    Text(String),
    /// The chunk carries this continuation [`Chunk::title_hint`].
    ///
    /// A real model is asked, via the hint, to re-emit a recipe a hard split cut
    /// in two so `assemble` can merge the halves. Matching on it is what lets a
    /// test drive that contract offline.
    TitleHint(String),
}

impl MockMatch {
    fn matches(&self, chunk: &Chunk) -> bool {
        match self {
            MockMatch::Text(needle) => chunk.text.contains(needle.as_str()),
            MockMatch::TitleHint(title) => chunk.title_hint.as_deref() == Some(title.as_str()),
        }
    }
}

/// Deterministic test extractor: a chunk returns the recipes of every rule it
/// matches. Lets tests drive the whole pipeline with no network.
pub struct MockExtractor {
    rules: Vec<(MockMatch, Vec<ExtractedRecipe>)>,
}

impl MockExtractor {
    /// Text-matching rules: for a given chunk, the recipes of every needle
    /// contained in `chunk.text` are returned (in rule order).
    pub fn new(rules: Vec<(String, Vec<ExtractedRecipe>)>) -> Self {
        Self::with_rules(
            rules
                .into_iter()
                .map(|(needle, recipes)| (MockMatch::Text(needle), recipes))
                .collect(),
        )
    }

    /// Rules that may match on text *or* on the continuation title hint.
    pub fn with_rules(rules: Vec<(MockMatch, Vec<ExtractedRecipe>)>) -> Self {
        Self { rules }
    }
}

impl RecipeExtractor for MockExtractor {
    async fn extract(&self, chunk: &Chunk) -> Result<ChunkOutcome, EpubError> {
        let mut out = Vec::new();
        for (rule, recipes) in &self.rules {
            if rule.matches(chunk) {
                out.extend(recipes.iter().cloned());
            }
        }
        Ok(ChunkOutcome {
            recipes: out,
            usage: Usage::default(),
            cached: false,
            truncated: false,
        })
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use serde_json::json;

    use super::{parse_freeform_duration, parse_recipes_payload};
    use rstest::rstest;

    // ============================================================================
    // parse_freeform_duration() Tests
    // ============================================================================

    #[rstest]
    #[case::minutes("30 minutes", Some(30))]
    #[case::min_abbrev("45 min", Some(45))]
    #[case::one_minute("1 minute", Some(1))]
    #[case::hours("2 hours", Some(120))]
    #[case::hour_abbrev("1 hr", Some(60))]
    #[case::hour_and_min("1 hr 15 min", Some(75))]
    #[case::hour_and_minutes("1 hour 30 minutes", Some(90))]
    #[case::abbrev_with_periods("1 hr. 15 min.", Some(75))]
    #[case::no_space("30m", Some(30))]
    #[case::mixed_case("1 Hour 5 Minutes", Some(65))]
    // A hedge doesn't change the count.
    #[case::about("about 30 minutes", Some(30))]
    #[case::tilde("~45 min", Some(45))]
    // Ambiguous or unreadable — the display string keeps these, the count doesn't.
    #[case::range("30 to 40 minutes", None)]
    #[case::hyphen_range("30-40 minutes", None)]
    #[case::fraction("1 1/2 hours", None)]
    #[case::qualified("30 minutes, plus chilling", None)]
    #[case::word_only("overnight", None)]
    #[case::no_unit("30", None)]
    #[case::unknown_unit("2 days", None)]
    #[case::zero("0 minutes", None)]
    #[case::empty("", None)]
    fn test_parse_freeform_duration(#[case] input: &str, #[case] expected: Option<u32>) {
        assert_eq!(parse_freeform_duration(input), expected);
    }

    #[test]
    fn payload_fills_time_minutes_from_prose() {
        let v = json!({ "recipes": [
            { "title": "X",
              "sections": [],
              "times": { "prep": "20 minutes", "cook": "1 hr 30 min", "total": "overnight" } }
        ]});
        let r = parse_recipes_payload(v).unwrap();
        let times = r[0].meta.times.clone().unwrap();
        assert_eq!(times.prep_minutes, Some(20));
        assert_eq!(times.cook_minutes, Some(90));
        // Unreadable prose keeps its display string but gets no count.
        assert_eq!(times.total.as_deref(), Some("overnight"));
        assert_eq!(times.total_minutes, None);
        // A time the model never emitted stays absent on both halves.
        assert_eq!(times.active, None);
        assert_eq!(times.active_minutes, None);
    }

    // Regression: real `claude-haiku-4-5` output on Tartine Book No. 3 produced
    // four malformed chunks that each `?`-aborted the cookbook import. The lenient
    // deserializers must now absorb every shape instead of erroring. See the
    // `food-cli debug-epub` taxonomy: missing `ingredients`, double-encoded
    // `recipes` string, plus `null` arrays (the original browser failure).

    #[test]
    fn well_formed_payload_parses() {
        let v = json!({ "recipes": [
            { "title": "X", "sections": [{ "ingredients": ["1 cup flour"], "instructions": ["Mix."] }] }
        ]});
        let r = parse_recipes_payload(v).unwrap();
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].sections[0].ingredients, vec!["1 cup flour"]);
    }

    #[test]
    fn section_missing_ingredients_degrades_to_empty() {
        // chapters 33/48/60: a section object with no `ingredients` key.
        let v = json!({ "recipes": [
            { "title": "X", "sections": [{ "instructions": ["Stir."] }] }
        ]});
        let r = parse_recipes_payload(v).unwrap();
        assert!(r[0].sections[0].ingredients.is_empty());
        assert_eq!(r[0].sections[0].instructions, vec!["Stir."]);
    }

    #[test]
    fn double_encoded_recipes_string_is_reparsed() {
        // chapter 64: the whole `recipes` value arrives as a JSON string.
        let inner = json!([
            { "title": "Sablés", "sections": [{ "ingredients": ["150 g hazelnuts"] }] }
        ])
        .to_string();
        let v = json!({ "recipes": inner });
        let r = parse_recipes_payload(v).unwrap();
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].meta.title, "Sablés");
    }

    #[test]
    fn explicit_null_arrays_become_empty() {
        // The original browser failure: "invalid type: null, expected a sequence".
        // `#[serde(default)]` alone does NOT rescue an explicit null.
        let v = json!({ "recipes": [
            { "title": "X",
              "sections": [{ "ingredients": null, "instructions": null }],
              "notes": null, "equipment": null }
        ]});
        let r = parse_recipes_payload(v).unwrap();
        assert!(r[0].sections[0].ingredients.is_empty());
        assert!(r[0].meta.notes.is_empty());
    }

    #[test]
    fn top_level_recipes_null_is_empty() {
        let r = parse_recipes_payload(json!({ "recipes": null })).unwrap();
        assert!(r.is_empty());
    }

    #[test]
    fn recipe_sections_null_is_empty() {
        let v = json!({ "recipes": [{ "title": "X", "sections": null }] });
        let r = parse_recipes_payload(v).unwrap();
        assert!(r[0].sections.is_empty());
    }

    // --- try_extract_chunk: the shared call+parse+retry policy (native + wasm) ---

    use std::cell::Cell;

    use super::{
        CallFailure, CallResult, Usage, try_extract_chunk, try_extract_chunk_detailed,
        try_extract_chunk_detailed_for_chunk, validate_chunk_recipes,
    };
    use crate::Chunk;

    fn call_result(input: serde_json::Value, truncated: bool) -> CallResult {
        CallResult {
            input: Some(input),
            usage: Usage::default(),
            truncated,
        }
    }

    fn source_chunk(text: &str) -> Chunk {
        Chunk {
            title_hint: None,
            text: text.to_string(),
            doc_path: "chapter.xhtml".to_string(),
            links: Vec::new(),
            images: Vec::new(),
        }
    }

    #[test]
    fn source_fidelity_rejects_invented_measurement_alternative() {
        let chunk =
            source_chunk("Carrot salad\n100ml ( cup) neutral oil\nStir with a wooden spoon.");
        let recipes = parse_recipes_payload(json!({ "recipes": [{
            "title": "Carrot salad",
            "sections": [{
                "ingredients": ["100ml (¾ cup) neutral oil"],
                "instructions": ["Stir with a wooden spoon."]
            }]
        }] }))
        .unwrap();
        let error = validate_chunk_recipes(&chunk, &recipes).unwrap_err();
        assert!(error.to_string().contains("source fidelity violation"));
        assert!(error.to_string().contains("ingredient"));
    }

    #[tokio::test]
    async fn source_fidelity_retries_then_accounts_for_valid_output() {
        let chunk = source_chunk("Carrot salad\n100ml ( cup) neutral oil");
        let calls = Cell::new(0);
        let driven = try_extract_chunk_detailed_for_chunk(&chunk, || {
            let call = calls.get();
            calls.set(call + 1);
            async move {
                let ingredient = if call == 0 {
                    "100ml (¾ cup) neutral oil"
                } else {
                    "100ml ( cup) neutral oil"
                };
                Ok(CallResult {
                    input: Some(json!({ "recipes": [{
                        "title": "Carrot salad",
                        "sections": [{ "ingredients": [ingredient] }]
                    }] })),
                    usage: Usage {
                        input_tokens: 5,
                        ..Default::default()
                    },
                    truncated: false,
                })
            }
        })
        .await
        .unwrap();
        assert_eq!(calls.get(), 2);
        assert_eq!(driven.usage.input_tokens, 10);
        assert_eq!(
            driven.recipes[0].sections[0].ingredients,
            vec!["100ml ( cup) neutral oil"]
        );
    }

    #[test]
    fn wok_component_headings_must_be_section_names() {
        let chunk = source_chunk(
            "BEER-BATTERED FISH WITH EASY GARLIC AIOLI\nFOR THE GARLIC AIOLI:\n1 cup mayonnaise\nFOR THE BATTER:\n2 cups flour\nTO SERVE:\nlemon wedges",
        );
        let misplaced = parse_recipes_payload(json!({ "recipes": [{
            "title": "BEER-BATTERED FISH WITH EASY GARLIC AIOLI",
            "sections": [
                { "name": "FOR THE GARLIC AIOLI", "ingredients": ["1 cup mayonnaise", "FOR THE BATTER:"] },
                { "ingredients": ["2 cups flour", "TO SERVE:", "lemon wedges"] }
            ]
        }] }))
        .unwrap();
        assert!(
            validate_chunk_recipes(&chunk, &misplaced)
                .unwrap_err()
                .to_string()
                .contains("component heading must be a section label")
        );

        let preserved = parse_recipes_payload(json!({ "recipes": [{
            "title": "BEER-BATTERED FISH WITH EASY GARLIC AIOLI",
            "sections": [
                { "name": "FOR THE GARLIC AIOLI", "ingredients": ["1 cup mayonnaise"] },
                { "name": "FOR THE BATTER", "ingredients": ["2 cups flour"] },
                { "name": "TO SERVE", "ingredients": ["lemon wedges"] }
            ]
        }] }))
        .unwrap();
        validate_chunk_recipes(&chunk, &preserved).unwrap();

        let mut duplicated_in_ingredients = preserved.clone();
        duplicated_in_ingredients[0].sections[2]
            .ingredients
            .push("TO SERVE:".to_string());
        assert!(
            validate_chunk_recipes(&chunk, &duplicated_in_ingredients)
                .unwrap_err()
                .to_string()
                .contains("component heading must be a section label")
        );
    }

    #[test]
    fn heading_guard_skips_ordinary_sentences_and_repeated_titles() {
        let ordinary = source_chunk("Salad\n1 cup greens\nTo serve, scatter herbs over the salad.");
        let recipe = parse_recipes_payload(json!({ "recipes": [{
            "title": "Salad",
            "sections": [{ "ingredients": ["1 cup greens"], "instructions": ["To serve, scatter herbs over the salad."] }]
        }] }))
        .unwrap();
        validate_chunk_recipes(&ordinary, &recipe).unwrap();

        let repeated = source_chunk("Sauce\nFOR THE DRESSING:\nSauce\nFOR THE DRESSING:");
        let recipes = parse_recipes_payload(json!({ "recipes": [
            { "title": "Sauce", "sections": [] },
            { "title": "Sauce", "sections": [] }
        ] }))
        .unwrap();
        validate_chunk_recipes(&repeated, &recipes).unwrap();

        // Even though Salad has a unique title, its end is uncertain once the
        // chunk contains the repeated Sauce occurrence, so the whole guard
        // conservatively declines to assign Sauce's heading to Salad.
        let mixed = source_chunk("Salad\n1 cup greens\nSauce\nFOR THE DRESSING:\nSauce");
        let recipes = parse_recipes_payload(json!({ "recipes": [
            { "title": "Salad", "sections": [{ "ingredients": ["1 cup greens"] }] },
            { "title": "Sauce", "sections": [] },
            { "title": "Sauce", "sections": [] }
        ] }))
        .unwrap();
        validate_chunk_recipes(&mixed, &recipes).unwrap();
    }

    #[rstest]
    #[case(false)]
    #[case(true)]
    #[tokio::test]
    async fn absent_tool_output_is_empty_success_with_metadata(#[case] truncated: bool) {
        let calls = Cell::new(0);
        let usage = Usage {
            input_tokens: 10,
            output_tokens: 4,
            cache_creation_input_tokens: 3,
            cache_read_input_tokens: 2,
        };
        let driven = try_extract_chunk_detailed("empty.xhtml", || {
            calls.set(calls.get() + 1);
            let usage = usage.clone();
            async move {
                Ok(CallResult {
                    input: None,
                    usage,
                    truncated,
                })
            }
        })
        .await
        .unwrap();
        assert_eq!(calls.get(), 1);
        assert!(driven.recipes.is_empty());
        assert_eq!(driven.usage, usage);
        assert_eq!(driven.truncated, truncated);
    }

    #[tokio::test]
    async fn truncated_raw_payload_error_preserves_its_source_without_retry() {
        use std::error::Error;

        let calls = Cell::new(0);
        let failure = try_extract_chunk_detailed("cut.xhtml", || {
            calls.set(calls.get() + 1);
            async {
                let error = serde_json::from_str::<serde_json::Value>("{").unwrap_err();
                let failed_call = CallFailure::retryable_payload(
                    error.into(),
                    Usage {
                        output_tokens: 16_000,
                        ..Default::default()
                    },
                    true,
                );
                assert_eq!(
                    failed_call.to_string(),
                    failed_call.source().unwrap().to_string()
                );
                Err(failed_call)
            }
        })
        .await
        .unwrap_err();

        assert_eq!(calls.get(), 1);
        assert_eq!(failure.usage.output_tokens, 16_000);
        assert!(failure.truncated);
        assert_eq!(failure.attempts.len(), 1);
        assert_eq!(failure.to_string(), failure.source().unwrap().to_string());
        assert!(failure.to_string().contains("EOF"));
    }

    #[tokio::test]
    async fn try_extract_chunk_retries_then_succeeds() {
        let calls = Cell::new(0usize);
        let driven = try_extract_chunk("doc", || {
            let attempt = calls.get();
            calls.set(attempt + 1);
            async move {
                // First call: double-encoded but invalid JSON (unparseable).
                // Second: a clean array.
                let input = if attempt == 0 {
                    json!({ "recipes": "[oops not json" })
                } else {
                    json!({ "recipes": [{ "title": "X", "sections": [{ "ingredients": ["a"] }] }] })
                };
                Ok(call_result(input, false))
            }
        })
        .await
        .unwrap();
        assert_eq!(driven.recipes.len(), 1);
        assert_eq!(calls.get(), 2, "should retry exactly once");
    }

    #[tokio::test]
    async fn try_extract_chunk_gives_up_after_retries() {
        let calls = Cell::new(0usize);
        let res = try_extract_chunk("doc", || {
            calls.set(calls.get() + 1);
            async move { Ok(call_result(json!({ "recipes": "[bad" }), false)) }
        })
        .await;
        assert!(
            res.is_err(),
            "exhausted retries → Err so the caller can escalate"
        );
        assert_eq!(calls.get(), 2, "1 + PARSE_RETRIES attempts");
    }

    #[tokio::test]
    async fn try_extract_chunk_no_retry_on_truncation() {
        let calls = Cell::new(0usize);
        let res = try_extract_chunk("doc", || {
            calls.set(calls.get() + 1);
            async move { Ok(call_result(json!({ "recipes": "[bad" }), true)) }
        })
        .await;
        assert!(res.is_err());
        assert_eq!(
            calls.get(),
            1,
            "truncated → no retry (same-size retry would truncate again)"
        );
    }
}
