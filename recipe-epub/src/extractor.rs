//! The extraction backend: a trait plus a real Claude impl and a test mock,
//! and the rich recipe shape the model returns.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::json;

use recipe_scraper::RecipeSection;

use crate::{Chunk, EpubError, Options};

/// Recipe metadata the model returns (everything except the component sections).
/// Flattened into [`ExtractedRecipe`] and the public output types so they all
/// serialize as one flat object.
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
pub struct RecipeMeta {
    pub title: String,
    /// Headnote / intro blurb.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Yield/servings line, e.g. "Makes 1 loaf".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recipe_yield: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub times: Option<RecipeTimes>,
    /// Special-equipment lines.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub equipment: Vec<String>,
    /// Do-ahead/make-ahead notes, tips, "serve with" suggestions.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
    /// Chapter/category within the book.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    /// Page number, if printed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page: Option<String>,
}

/// Printed times. Any field may be absent.
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
pub struct RecipeTimes {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prep: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cook: Option<String>,
}

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

const DEFAULT_BASE_URL: &str = "https://api.anthropic.com";
const ANTHROPIC_VERSION: &str = "2023-06-01";
// Default to a current, cheap/fast Haiku (confirmed via the claude-api skill).
const DEFAULT_MODEL: &str = "claude-haiku-4-5";
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
- notes: do-ahead / make-ahead notes, tips, and \"serve with\" suggestions, each verbatim.\n\
- category: the chapter or category the recipe belongs to, if evident.\n\
- page: the page number, if present in the text.\n\
Ignore running chapter prose, page headers/footers, and photo captions unless \
they are recipe content. If the section contains no recipe, return an empty list.";

/// Calls the Claude Messages API (directly, or via a proxy such as Cloudflare AI
/// Gateway) with forced structured (tool) output.
pub struct ClaudeExtractor {
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
    /// - `ANTHROPIC_BASE_URL` → base URL (default `https://api.anthropic.com`);
    ///   e.g. a Cloudflare AI Gateway `…/{account}/{gateway}/anthropic`.
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
        let base = nonempty(std::env::var("ANTHROPIC_BASE_URL"))
            .unwrap_or_else(|| DEFAULT_BASE_URL.to_string());
        let endpoint = format!("{}/v1/messages", base.trim_end_matches('/'));
        let model = opts
            .model
            .clone()
            .unwrap_or_else(|| DEFAULT_MODEL.to_string());
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

    /// The resolved model id (used as part of the cache key).
    pub fn model_id(&self) -> &str {
        &self.model
    }

    /// JSON Schema for the forced tool: `{ recipes: [ExtractedRecipe, ...] }`.
    fn tool_schema() -> serde_json::Value {
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

        let body = json!({
            "model": self.model,
            "max_tokens": 16000,
            // Static prefix → cache_control lets repeated calls within the TTL
            // reuse it (a no-op below the model's min cacheable size, but free).
            "system": [{
                "type": "text",
                "text": SYSTEM_PROMPT,
                "cache_control": { "type": "ephemeral" }
            }],
            "tools": [{
                "name": TOOL_NAME,
                "description": "Return every recipe found in the cookbook section.",
                "input_schema": Self::tool_schema()
            }],
            "tool_choice": { "type": "tool", "name": TOOL_NAME },
            "messages": [{ "role": "user", "content": user_text }]
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
        if parsed.stop_reason.as_deref() == Some("max_tokens") {
            tracing::warn!(
                "chunk {} hit max_tokens; some recipes may be truncated",
                chunk.doc_path
            );
        }

        let usage = parsed.usage;
        // With forced tool_choice the response carries exactly one tool_use block.
        for block in parsed.content {
            if let ContentBlock::ToolUse { input } = block {
                let payload: RecipesPayload = serde_json::from_value(input)?;
                return Ok(ChunkOutcome {
                    recipes: payload.recipes,
                    usage,
                    cached: false,
                });
            }
        }
        Ok(ChunkOutcome {
            recipes: Vec::new(),
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
}
