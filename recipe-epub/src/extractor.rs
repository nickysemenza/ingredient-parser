//! The pure extraction contract: the recipe shape the model returns, the
//! per-chunk LLM request builder, the response parser, the forced-tool schema,
//! and the `RecipeExtractor` trait (+ a test mock). Everything here is I/O-free
//! and compiles to wasm32. The live reqwest backends live in [`crate::backend`]
//! (native-only).

use serde::{Deserialize, Serialize};
use serde_json::json;

use recipe_scraper::RecipeSection;

use crate::{Chunk, EpubError};

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

/// Parse the forced tool's `input` object (`{ recipes: [ExtractedRecipe, …] }`)
/// into recipes. The single place LLM output is decoded — shared by the native
/// backends and the wasm `assemble_recipes` path.
pub fn parse_recipes_payload(input: serde_json::Value) -> Result<Vec<ExtractedRecipe>, EpubError> {
    Ok(serde_json::from_value::<RecipesPayload>(input)?.recipes)
}

/// Turns a [`Chunk`] of cookbook text into zero or more recipes.
///
/// Static dispatch (used via generics) so we avoid the `async-trait` dep; the
/// concrete backends live in [`crate::backend`] (native), with a [`MockExtractor`]
/// here for tests.
#[allow(async_fn_in_trait)]
pub trait RecipeExtractor {
    async fn extract(&self, chunk: &Chunk) -> Result<ChunkOutcome, EpubError>;

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

/// The forced tool's `input` object. `pub(crate)` so [`crate::backend`]'s tests
/// (which decode real API payloads) can name it.
#[derive(Deserialize)]
pub(crate) struct RecipesPayload {
    pub(crate) recipes: Vec<ExtractedRecipe>,
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
            truncated: false,
        })
    }
}
