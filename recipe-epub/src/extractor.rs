//! The extraction backend: a trait plus a real Claude impl and a test mock,
//! and the rich recipe shape the model returns.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::json;

use recipe_scraper::RecipeSection;

use crate::library::BookMeta;
use crate::{Chunk, EpubError, Options};

// `RecipeMeta` is a plain data shape; it lives in the deps-light `recipe-types`
// crate and is re-exported here (and from the crate root) so existing
// `recipe_epub::RecipeMeta` paths are unchanged.
pub use recipe_types::RecipeMeta;

/// A recipe as segmented + labeled by the extractor (model output). Sections use
/// the shared [`recipe_scraper::RecipeSection`] type; ingredient/instruction
/// strings are **verbatim** — quantities are parsed downstream by the core
/// `ingredient` parser, never by the model.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct ExtractedRecipe {
    #[serde(flatten)]
    pub meta: RecipeMeta,
    pub sections: Vec<RecipeSection>,
}

/// Token usage reported by the model API for one call. Field names match the
/// Anthropic Messages API `usage` object; OpenAI/Gemini report the same counts
/// under different names, so a future backend maps them into this shape.
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
pub struct ChunkOutcome {
    pub recipes: Vec<ExtractedRecipe>,
    /// Token usage for the API call. Zero when served from cache.
    pub usage: Usage,
    /// True when served from the on-disk cache (no API call, no cost).
    pub cached: bool,
}

/// Turns a [`Chunk`] of cookbook text into zero or more recipes.
///
/// Static dispatch (used via generics) so we avoid the `async-trait` dep; when a
/// second backend lands, an `enum Backend { Claude, Mock }` impl keeps it
/// dep-free while allowing runtime selection.
#[allow(async_fn_in_trait)]
pub trait RecipeExtractor {
    async fn extract(&self, chunk: &Chunk) -> Result<ChunkOutcome, EpubError>;

    /// Model id for cost attribution; empty when not applicable (e.g. the mock).
    fn model(&self) -> &str {
        ""
    }
}

const ANTHROPIC_VERSION: &str = "2023-06-01";
// Default model (OpenAI-compatible backend, via the Gemini OpenAI-compat
// endpoint). Picked over gpt-4o-mini — which began dropping ~40% of a book's
// recipes — and over Haiku for being ~2.5× cheaper at full recipe coverage.
// Notes are best-effort on this model (occasionally drops a recipe's notes);
// use `--model claude-haiku-4-5` when complete notes matter.
const DEFAULT_MODEL: &str = "gemini-2.5-flash";
const CLAUDE_DEFAULT_MODEL: &str = "claude-haiku-4-5";
const TOOL_NAME: &str = "emit_recipes";

const SYSTEM_PROMPT: &str = "\
You extract structured recipes from the text of one section of a cookbook. The \
section may contain zero, one, or many recipes. For every recipe actually \
present, return an object with:\n\
- title: the recipe's name.\n\
- description: the headnote / intro blurb, if any (omit otherwise).\n\
- sections: the recipe's components as an array. Most recipes have ONE section \
(omit its name). Component recipes have several. Each section has:\n\
    - name: the component label (e.g. \"For the curry paste\"), or omit for the \
main/only section.\n\
    - ingredients: each ingredient line copied VERBATIM, one per entry. Do NOT \
parse, normalize, convert, or reword quantities or units — preserve the original \
text exactly (e.g. \"1\\u2153 cups all-purpose flour (6.1 oz / 173g)\").\n\
    - instructions: the method steps for this component, copied verbatim, one per \
entry. If the recipe has a single shared method, put all of its steps in the \
main section.\n\
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

/// Calls the Claude Messages API (directly, or via a proxy such as Cloudflare AI
/// Gateway) with forced structured (tool) output.
pub(crate) struct ClaudeExtractor {
    client: reqwest::Client,
    /// Full endpoint incl. `/v1/messages`.
    endpoint: String,
    /// `x-api-key` — optional when a gateway injects the provider key (BYOK).
    api_key: Option<String>,
    /// `cf-aig-authorization: Bearer …` — set for a Cloudflare AI Gateway.
    gateway_token: Option<String>,
    model: String,
}

impl ClaudeExtractor {
    /// Build from the environment:
    /// - `ANTHROPIC_API_KEY` → `x-api-key` (optional if a gateway injects it).
    /// - `CF_AIG_TOKEN` / `AI_GATEWAY_API_KEY` → `cf-aig-authorization` (optional).
    /// - `ANTHROPIC_BASE_URL` → base URL (required); e.g. a Cloudflare AI
    ///   Gateway `…/{account}/{gateway}/anthropic`.
    /// - `opts.model` → model id (default Haiku).
    ///
    /// At least one of the API key or gateway token must be present.
    pub fn from_env(opts: &Options) -> Result<Self, EpubError> {
        let nonempty = |v: Result<String, _>| v.ok().filter(|s| !s.is_empty());
        let api_key = nonempty(std::env::var("ANTHROPIC_API_KEY"));
        let gateway_token = nonempty(std::env::var("CF_AIG_TOKEN"))
            .or_else(|| nonempty(std::env::var("AI_GATEWAY_API_KEY")));
        if api_key.is_none() && gateway_token.is_none() {
            return Err(EpubError::MissingApiKey);
        }
        let base =
            nonempty(std::env::var("ANTHROPIC_BASE_URL")).ok_or(EpubError::MissingBaseUrl)?;
        let endpoint = format!("{}/v1/messages", base.trim_end_matches('/'));
        let model = opts
            .model
            .clone()
            .unwrap_or_else(|| CLAUDE_DEFAULT_MODEL.to_string());
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(180))
            .build()?;
        Ok(Self {
            client,
            endpoint,
            api_key,
            gateway_token,
            model,
        })
    }

    /// Issue one Anthropic Messages call with forced tool output. Returns the
    /// tool's `input` object (`None` if the model returned no tool block), the
    /// token usage, and the stop reason. Shared by [`Self::extract`] and the
    /// cookbook classifier.
    async fn call_tool(
        &self,
        system: &str,
        user: String,
        tool_name: &str,
        tool_desc: &str,
        schema: serde_json::Value,
        max_tokens: u32,
    ) -> Result<(Option<serde_json::Value>, Usage, Option<String>), EpubError> {
        let body = json!({
            "model": self.model,
            "max_tokens": max_tokens,
            // Static prefix → cache_control lets repeated calls within the TTL
            // reuse it (a no-op below the model's min cacheable size, but free).
            "system": [{
                "type": "text",
                "text": system,
                "cache_control": { "type": "ephemeral" }
            }],
            "tools": [{
                "name": tool_name,
                "description": tool_desc,
                "input_schema": schema
            }],
            "tool_choice": { "type": "tool", "name": tool_name },
            "messages": [{ "role": "user", "content": user }]
        });

        let mut req = self
            .client
            .post(&self.endpoint)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .header("content-type", "application/json");
        if let Some(key) = &self.api_key {
            req = req.header("x-api-key", key);
        }
        if let Some(token) = &self.gateway_token {
            req = req.header("cf-aig-authorization", format!("Bearer {token}"));
        }
        let resp = req.json(&body).send().await?;

        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            return Err(EpubError::Api {
                status: status.as_u16(),
                body: text,
            });
        }

        let parsed: ApiResponse = serde_json::from_str(&text)?;
        let usage = parsed.usage;
        let stop_reason = parsed.stop_reason;
        // With forced tool_choice the response carries exactly one tool_use block.
        let input = parsed.content.into_iter().find_map(|block| match block {
            ContentBlock::ToolUse { input } => Some(input),
            ContentBlock::Other => None,
        });
        Ok((input, usage, stop_reason))
    }
}

/// JSON Schema for the forced tool's input: `{ recipes: [ExtractedRecipe, ...] }`.
/// Shared by every backend (Anthropic `input_schema`, OpenAI/Gemini function
/// `parameters`).
fn recipes_tool_schema() -> serde_json::Value {
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
                                "required": ["ingredients"]
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

impl RecipeExtractor for ClaudeExtractor {
    fn model(&self) -> &str {
        &self.model
    }

    async fn extract(&self, chunk: &Chunk) -> Result<ChunkOutcome, EpubError> {
        let user_text = match &chunk.title_hint {
            Some(t) => format!("Section title: {t}\n\n{}", chunk.text),
            None => chunk.text.clone(),
        };

        let (input, usage, stop_reason) = self
            .call_tool(
                SYSTEM_PROMPT,
                user_text,
                TOOL_NAME,
                "Return every recipe found in the cookbook section.",
                recipes_tool_schema(),
                16000,
            )
            .await?;

        if stop_reason.as_deref() == Some("max_tokens") {
            tracing::warn!(
                "chunk {} hit max_tokens; some recipes may be truncated",
                chunk.doc_path
            );
        }

        let recipes = match input {
            Some(v) => serde_json::from_value::<RecipesPayload>(v)?.recipes,
            None => Vec::new(),
        };
        Ok(ChunkOutcome {
            recipes,
            usage,
            cached: false,
        })
    }
}

/// The forced tool's `input` object.
#[derive(Deserialize)]
struct RecipesPayload {
    recipes: Vec<ExtractedRecipe>,
}

#[derive(Deserialize)]
struct ApiResponse {
    content: Vec<ContentBlock>,
    #[serde(default)]
    stop_reason: Option<String>,
    #[serde(default)]
    usage: Usage,
}

#[derive(Deserialize)]
#[serde(tag = "type")]
enum ContentBlock {
    #[serde(rename = "tool_use")]
    ToolUse { input: serde_json::Value },
    // text / thinking / anything else — ignored.
    #[serde(other)]
    Other,
}

// ===========================================================================
// OpenAI-compatible backend (OpenAI + Gemini via its OpenAI-compat endpoint)
// ===========================================================================

/// Whether a model id routes to the OpenAI-compatible backend rather than Claude.
pub(crate) fn is_openai_compatible_model(model: &str) -> bool {
    let m = model.to_lowercase();
    m.starts_with("gpt")
        || m.starts_with("o1")
        || m.starts_with("o3")
        || m.starts_with("o4")
        || m.starts_with("gemini")
}

/// Calls an OpenAI-compatible `/chat/completions` endpoint with a forced
/// function call. Serves both OpenAI (`gpt-*`) and Google Gemini (`gemini-*`,
/// via its OpenAI-compatible endpoint) — the wire format is identical.
pub(crate) struct OpenAiExtractor {
    client: reqwest::Client,
    /// Full endpoint incl. `/chat/completions`.
    endpoint: String,
    /// `Authorization: Bearer …` — optional when a gateway injects the key (BYOK).
    api_key: Option<String>,
    /// `cf-aig-authorization: Bearer …` — set for a Cloudflare AI Gateway.
    gateway_token: Option<String>,
    model: String,
}

impl OpenAiExtractor {
    /// Build from the environment. Base URL resolution, in order:
    /// 1. `OPENAI_BASE_URL` / `GEMINI_BASE_URL` (provider-specific override);
    /// 2. derived from a Cloudflare gateway `ANTHROPIC_BASE_URL` ending in
    ///    `/anthropic` (swapped to `/openai` or `/google-ai-studio/v1beta/openai`).
    ///
    /// Errors if neither is set — this crate routes through a gateway by design;
    /// there is no public-API default.
    ///
    /// Auth: `OPENAI_API_KEY` / `GEMINI_API_KEY` → `Authorization: Bearer`
    /// (optional with a BYOK gateway), plus `CF_AIG_TOKEN` / `AI_GATEWAY_API_KEY`
    /// → `cf-aig-authorization`. At least one auth source must be present.
    pub fn from_env(opts: &Options) -> Result<Self, EpubError> {
        let nonempty = |v: Result<String, _>| v.ok().filter(|s| !s.is_empty());
        let model = opts
            .model
            .clone()
            .unwrap_or_else(|| DEFAULT_MODEL.to_string());
        let is_gemini = model.to_lowercase().starts_with("gemini");

        let gateway_token = nonempty(std::env::var("CF_AIG_TOKEN"))
            .or_else(|| nonempty(std::env::var("AI_GATEWAY_API_KEY")));
        let api_key = if is_gemini {
            nonempty(std::env::var("GEMINI_API_KEY"))
                .or_else(|| nonempty(std::env::var("GOOGLE_API_KEY")))
        } else {
            nonempty(std::env::var("OPENAI_API_KEY"))
        };
        if api_key.is_none() && gateway_token.is_none() {
            return Err(EpubError::MissingApiKey);
        }

        let explicit = if is_gemini {
            nonempty(std::env::var("GEMINI_BASE_URL"))
        } else {
            nonempty(std::env::var("OPENAI_BASE_URL"))
        };
        let suffix = if is_gemini {
            "google-ai-studio/v1beta/openai"
        } else {
            "openai"
        };
        let base = explicit
            .or_else(|| {
                // Derive a sibling provider route from a CF gateway base.
                nonempty(std::env::var("ANTHROPIC_BASE_URL")).and_then(|b| {
                    b.trim_end_matches('/')
                        .strip_suffix("/anthropic")
                        .map(|prefix| format!("{prefix}/{suffix}"))
                })
            })
            .ok_or(EpubError::MissingBaseUrl)?;
        let endpoint = format!("{}/chat/completions", base.trim_end_matches('/'));

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(180))
            .build()?;
        Ok(Self {
            client,
            endpoint,
            api_key,
            gateway_token,
            model,
        })
    }

    /// Issue one OpenAI-compatible chat-completions call with a forced function
    /// call. Returns the function's decoded arguments object (`None` if the model
    /// returned no tool call), token usage, and finish reason. Shared by
    /// [`Self::extract`] and the cookbook classifier.
    async fn call_tool(
        &self,
        system: &str,
        user: String,
        tool_name: &str,
        tool_desc: &str,
        schema: serde_json::Value,
        max_tokens: u32,
    ) -> Result<(Option<serde_json::Value>, Usage, Option<String>), EpubError> {
        let body = json!({
            "model": self.model,
            "max_tokens": max_tokens,
            "messages": [
                { "role": "system", "content": system },
                { "role": "user", "content": user }
            ],
            "tools": [{
                "type": "function",
                "function": {
                    "name": tool_name,
                    "description": tool_desc,
                    "parameters": schema
                }
            }],
            "tool_choice": { "type": "function", "function": { "name": tool_name } }
        });

        let mut req = self
            .client
            .post(&self.endpoint)
            .header("content-type", "application/json");
        if let Some(key) = &self.api_key {
            req = req.header("authorization", format!("Bearer {key}"));
        }
        if let Some(token) = &self.gateway_token {
            req = req.header("cf-aig-authorization", format!("Bearer {token}"));
        }
        let resp = req.json(&body).send().await?;

        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            return Err(EpubError::Api {
                status: status.as_u16(),
                body: text,
            });
        }

        let parsed: OpenAiResponse = serde_json::from_str(&text)?;
        let usage = parsed.usage.into();
        let choice = parsed.choices.into_iter().next();
        let finish_reason = choice.as_ref().and_then(|c| c.finish_reason.clone());
        let args = choice
            .and_then(|c| c.message.tool_calls)
            .and_then(|mut calls| calls.drain(..).next())
            .map(|call| call.function.arguments);
        let input = match args {
            Some(a) => Some(serde_json::from_str::<serde_json::Value>(&a)?),
            None => None,
        };
        Ok((input, usage, finish_reason))
    }
}

impl RecipeExtractor for OpenAiExtractor {
    fn model(&self) -> &str {
        &self.model
    }

    async fn extract(&self, chunk: &Chunk) -> Result<ChunkOutcome, EpubError> {
        let user_text = match &chunk.title_hint {
            Some(t) => format!("Section title: {t}\n\n{}", chunk.text),
            None => chunk.text.clone(),
        };

        let (input, usage, finish_reason) = self
            .call_tool(
                SYSTEM_PROMPT,
                user_text,
                TOOL_NAME,
                "Return every recipe found in the cookbook section.",
                recipes_tool_schema(),
                16000,
            )
            .await?;

        if finish_reason.as_deref() == Some("length") {
            tracing::warn!(
                "chunk {} hit token limit; some recipes may be truncated",
                chunk.doc_path
            );
        }

        let recipes = match input {
            Some(v) => serde_json::from_value::<RecipesPayload>(v)?.recipes,
            None => Vec::new(),
        };
        Ok(ChunkOutcome {
            recipes,
            usage,
            cached: false,
        })
    }
}

/// Minimal OpenAI chat-completions response shape (forced tool call).
#[derive(Deserialize)]
struct OpenAiResponse {
    #[serde(default)]
    choices: Vec<OpenAiChoice>,
    #[serde(default)]
    usage: OpenAiUsage,
}

#[derive(Deserialize)]
struct OpenAiChoice {
    message: OpenAiMessage,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Deserialize)]
struct OpenAiMessage {
    #[serde(default)]
    tool_calls: Option<Vec<OpenAiToolCall>>,
}

#[derive(Deserialize)]
struct OpenAiToolCall {
    function: OpenAiFunctionCall,
}

#[derive(Deserialize)]
struct OpenAiFunctionCall {
    /// JSON-encoded string of the tool input.
    arguments: String,
}

#[derive(Deserialize, Default)]
struct OpenAiUsage {
    #[serde(default)]
    prompt_tokens: u64,
    #[serde(default)]
    completion_tokens: u64,
}

impl From<OpenAiUsage> for Usage {
    fn from(u: OpenAiUsage) -> Self {
        // OpenAI/Gemini don't split out prompt-cache tokens in the basic usage
        // object, so cache fields stay zero.
        Usage {
            input_tokens: u.prompt_tokens,
            output_tokens: u.completion_tokens,
            ..Default::default()
        }
    }
}

// ===========================================================================

/// Runtime-selected extraction backend, chosen by model id.
pub(crate) enum Backend {
    Claude(ClaudeExtractor),
    OpenAi(OpenAiExtractor),
}

impl Backend {
    /// Pick a backend from `opts.model`: `gpt-*`/`o*`/`gemini-*` →
    /// [`OpenAiExtractor`], otherwise [`ClaudeExtractor`] (the default).
    pub fn from_env(opts: &Options) -> Result<Self, EpubError> {
        let model = opts.model.as_deref().unwrap_or(DEFAULT_MODEL);
        if is_openai_compatible_model(model) {
            Ok(Backend::OpenAi(OpenAiExtractor::from_env(opts)?))
        } else {
            Ok(Backend::Claude(ClaudeExtractor::from_env(opts)?))
        }
    }

    /// Ask the model which of `books` are cookbooks (one batched call). Returns a
    /// bool per input book in the same order. See [`crate::classify_cookbooks_ai`].
    pub async fn classify_cookbooks(&self, books: &[BookMeta]) -> Result<Vec<bool>, EpubError> {
        let user = classify_user_prompt(books);
        let schema = classify_tool_schema();
        let (input, _usage, _reason) = match self {
            Backend::Claude(e) => {
                e.call_tool(
                    CLASSIFY_SYSTEM_PROMPT,
                    user,
                    CLASSIFY_TOOL_NAME,
                    "Return the 1-based indices of the books that are cookbooks.",
                    schema,
                    2000,
                )
                .await?
            }
            Backend::OpenAi(e) => {
                e.call_tool(
                    CLASSIFY_SYSTEM_PROMPT,
                    user,
                    CLASSIFY_TOOL_NAME,
                    "Return the 1-based indices of the books that are cookbooks.",
                    schema,
                    2000,
                )
                .await?
            }
        };
        let payload: ClassifyPayload = match input {
            Some(v) => serde_json::from_value(v)?,
            None => ClassifyPayload::default(),
        };
        Ok(indices_to_bools(&payload.cookbook_indices, books.len()))
    }
}

const CLASSIFY_TOOL_NAME: &str = "label_cookbooks";
const CLASSIFY_SYSTEM_PROMPT: &str = "\
You are given a numbered list of books (title, plus any genre tags). Return the \
1-based indices of the books that are COOKBOOKS — books primarily of recipes or \
food/drink preparation. Exclude novels, memoirs (even food memoirs without \
recipes), diet/nutrition-science books without recipes, and reference works. \
When a book has no tags, judge from the title alone; if genuinely unsure, leave \
it out.";

/// JSON Schema for the classifier tool: `{ cookbook_indices: [int, ...] }`.
fn classify_tool_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "cookbook_indices": {
                "type": "array",
                "items": { "type": "integer" }
            }
        },
        "required": ["cookbook_indices"]
    })
}

/// Render the numbered `title [tags: …]` list the classifier labels.
fn classify_user_prompt(books: &[BookMeta]) -> String {
    let mut s = String::from("Books:\n");
    for (i, b) in books.iter().enumerate() {
        if b.subjects.is_empty() {
            s.push_str(&format!("{}. {}\n", i + 1, b.title));
        } else {
            s.push_str(&format!(
                "{}. {} [tags: {}]\n",
                i + 1,
                b.title,
                b.subjects.join(", ")
            ));
        }
    }
    s
}

/// The classifier tool's `input` object.
#[derive(Deserialize, Default)]
struct ClassifyPayload {
    #[serde(default)]
    cookbook_indices: Vec<usize>,
}

/// Map the model's 1-based cookbook indices to a `len`-long bool vector
/// (out-of-range indices are ignored).
fn indices_to_bools(indices: &[usize], len: usize) -> Vec<bool> {
    let mut out = vec![false; len];
    for &idx in indices {
        if (1..=len).contains(&idx) {
            out[idx - 1] = true;
        }
    }
    out
}

impl RecipeExtractor for Backend {
    fn model(&self) -> &str {
        match self {
            Backend::Claude(e) => e.model(),
            Backend::OpenAi(e) => e.model(),
        }
    }

    async fn extract(&self, chunk: &Chunk) -> Result<ChunkOutcome, EpubError> {
        match self {
            Backend::Claude(e) => e.extract(chunk).await,
            Backend::OpenAi(e) => e.extract(chunk).await,
        }
    }
}

/// Deterministic test extractor: a chunk returns the recipes of every `needle`
/// its text contains. Lets tests drive the whole pipeline with no network.
pub struct MockExtractor {
    rules: Vec<(String, Vec<ExtractedRecipe>)>,
}

impl MockExtractor {
    /// `rules` is a list of `(needle, recipes)`; for a given chunk, the recipes
    /// of every needle contained in `chunk.text` are returned (in rule order).
    pub fn new(rules: Vec<(String, Vec<ExtractedRecipe>)>) -> Self {
        Self { rules }
    }
}

impl RecipeExtractor for MockExtractor {
    async fn extract(&self, chunk: &Chunk) -> Result<ChunkOutcome, EpubError> {
        let mut out = Vec::new();
        for (needle, recipes) in &self.rules {
            if chunk.text.contains(needle.as_str()) {
                out.extend(recipes.iter().cloned());
            }
        }
        Ok(ChunkOutcome {
            recipes: out,
            usage: Usage::default(),
            cached: false,
        })
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn parses_tool_use_response() {
        // Shape of a real Claude tool_use response with forced tool_choice.
        let raw = json!({
            "id": "msg_1",
            "type": "message",
            "role": "assistant",
            "stop_reason": "tool_use",
            "content": [
                { "type": "text", "text": "Here are the recipes." },
                {
                    "type": "tool_use",
                    "id": "toolu_1",
                    "name": TOOL_NAME,
                    "input": {
                        "recipes": [
                            {
                                "title": "Pancakes",
                                "description": "A weekend staple.",
                                "recipe_yield": "Makes 4",
                                "times": { "total": "20 minutes" },
                                "sections": [
                                    {
                                        "ingredients": ["1 cup flour", "2 eggs"],
                                        "instructions": ["Mix.", "Cook."]
                                    }
                                ],
                                "notes": ["Best fresh off the griddle."]
                            }
                        ]
                    }
                }
            ]
        })
        .to_string();

        let parsed: ApiResponse = serde_json::from_str(&raw).unwrap();
        let input = parsed
            .content
            .into_iter()
            .find_map(|b| match b {
                ContentBlock::ToolUse { input } => Some(input),
                ContentBlock::Other => None,
            })
            .unwrap();
        let payload: RecipesPayload = serde_json::from_value(input).unwrap();
        assert_eq!(payload.recipes.len(), 1);
        let r = &payload.recipes[0];
        assert_eq!(r.meta.title, "Pancakes");
        assert_eq!(r.meta.description.as_deref(), Some("A weekend staple."));
        assert_eq!(r.meta.recipe_yield.as_deref(), Some("Makes 4"));
        assert_eq!(
            r.meta.times.as_ref().and_then(|t| t.total.as_deref()),
            Some("20 minutes")
        );
        assert_eq!(r.sections.len(), 1);
        assert_eq!(r.sections[0].ingredients, vec!["1 cup flour", "2 eggs"]);
        assert_eq!(r.meta.notes, vec!["Best fresh off the griddle."]);
    }

    #[test]
    fn parses_openai_tool_call_response() {
        // Shape from the gateway's OpenAI-compatible route (OpenAI + Gemini):
        // tool args are a JSON-encoded string.
        let raw = json!({
            "id": "chatcmpl-1",
            "object": "chat.completion",
            "model": "gemini-2.5-flash-lite",
            "choices": [{
                "index": 0,
                "finish_reason": "tool_calls",
                "message": {
                    "role": "assistant",
                    "tool_calls": [{
                        "id": "call_1",
                        "type": "function",
                        "function": {
                            "name": TOOL_NAME,
                            "arguments": "{\"recipes\":[{\"title\":\"Pancakes\",\"sections\":[{\"ingredients\":[\"1 cup flour\",\"2 eggs\"],\"instructions\":[\"Mix.\"]}]}]}"
                        }
                    }]
                }
            }],
            "usage": { "prompt_tokens": 179, "completion_tokens": 45, "total_tokens": 224 }
        })
        .to_string();

        let parsed: OpenAiResponse = serde_json::from_str(&raw).unwrap();
        let usage: Usage = parsed.usage.into();
        assert_eq!(usage.input_tokens, 179);
        assert_eq!(usage.output_tokens, 45);

        let args = parsed
            .choices
            .into_iter()
            .next()
            .and_then(|c| c.message.tool_calls)
            .and_then(|mut calls| calls.drain(..).next())
            .map(|call| call.function.arguments)
            .unwrap();
        let payload: RecipesPayload = serde_json::from_str(&args).unwrap();
        assert_eq!(payload.recipes.len(), 1);
        assert_eq!(payload.recipes[0].meta.title, "Pancakes");
        assert_eq!(
            payload.recipes[0].sections[0].ingredients,
            vec!["1 cup flour", "2 eggs"]
        );
    }

    #[test]
    fn routes_models_to_backends() {
        assert!(is_openai_compatible_model("gpt-4o-mini"));
        assert!(is_openai_compatible_model("gemini-2.5-flash-lite"));
        assert!(is_openai_compatible_model("o3-mini"));
        assert!(!is_openai_compatible_model("claude-haiku-4-5"));
        assert!(!is_openai_compatible_model("claude-sonnet-4-6"));
    }

    fn book(title: &str, subjects: &[&str]) -> BookMeta {
        BookMeta {
            path: std::path::PathBuf::from("/x.epub"),
            title: title.to_string(),
            authors: vec![],
            subjects: subjects.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn classify_prompt_numbers_books_and_tags() {
        let prompt = classify_user_prompt(&[
            book("The Joy of Cooking", &["Cooking", "Reference"]),
            book("Untagged Book", &[]),
        ]);
        assert!(prompt.contains("1. The Joy of Cooking [tags: Cooking, Reference]"));
        // No tags → bare title, no empty "[tags: ]".
        assert!(prompt.contains("2. Untagged Book\n"));
        assert!(!prompt.contains("[tags: ]"));
    }

    #[test]
    fn classify_indices_map_to_bools() {
        // 1-based indices; out-of-range ignored.
        assert_eq!(indices_to_bools(&[1, 3, 9], 3), vec![true, false, true]);
        assert_eq!(indices_to_bools(&[], 2), vec![false, false]);
    }

    #[test]
    fn parses_classifier_tool_response() {
        // The classifier reuses the same forced-tool wire shape as extraction.
        let raw = json!({
            "stop_reason": "tool_use",
            "content": [{
                "type": "tool_use",
                "name": CLASSIFY_TOOL_NAME,
                "input": { "cookbook_indices": [2, 4] }
            }]
        })
        .to_string();
        let parsed: ApiResponse = serde_json::from_str(&raw).unwrap();
        let input = parsed
            .content
            .into_iter()
            .find_map(|b| match b {
                ContentBlock::ToolUse { input } => Some(input),
                ContentBlock::Other => None,
            })
            .unwrap();
        let payload: ClassifyPayload = serde_json::from_value(input).unwrap();
        assert_eq!(
            indices_to_bools(&payload.cookbook_indices, 4),
            vec![false, true, false, true]
        );
    }
}
