//! Native LLM extraction backends — a Claude (Anthropic Messages) impl, an
//! OpenAI-compatible impl (OpenAI + Gemini), runtime selection, and the cookbook
//! classifier. Everything here needs `reqwest` + the environment, so the whole
//! module is gated behind the `native` feature (one inner `#![cfg]` below) and
//! excluded from the wasm build, which proxies the LLM call through JS. The pure
//! pieces — request building, response parsing, the `RecipeExtractor` trait, the
//! tool schema — live in `extractor`.
#![cfg(feature = "native")]

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use futures::stream::{self, StreamExt};
use serde::Deserialize;
use serde_json::json;

use crate::library::BookMeta;
use crate::{
    assemble, build_chunk_request, cache, chunk_epub, parse_recipes_payload, resolve_references,
    Chunk, ChunkOutcome, CookbookRecipe, EpubError, ExtractProgress, ExtractedRecipe,
    ExtractionStats, Link, RecipeExtractor, Usage,
};

// ===========================================================================
// Orchestration: drive the chosen backend over an EPUB's chunks (concurrent,
// cached) and assemble the result. The public entry points; everything below is
// the per-provider HTTP plumbing.
// ===========================================================================

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

/// Like [`extract_cookbook`] but with a caller-supplied extractor (used by tests
/// with [`crate::MockExtractor`]) and a progress sink (pass `|_| {}` to ignore it).
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
/// reports per-chunk progress through `progress`.
async fn extract_cookbook_with_stats<E: RecipeExtractor>(
    bytes: &[u8],
    source: &str,
    opts: &Options,
    extractor: &E,
    progress: &(impl Fn(ExtractProgress) + Send + Sync),
) -> Result<(Vec<CookbookRecipe>, ExtractionStats), EpubError> {
    let chunks = chunk_epub(bytes)?;
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

/// Wraps any extractor with the on-disk cache (see [`crate::cache`]).
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
        let key = cache::key(
            &self.model,
            &chunk.text,
            chunk.title_hint.as_deref().unwrap_or(""),
        );
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

const ANTHROPIC_VERSION: &str = "2023-06-01";
// Default model (OpenAI-compatible backend, via the Gemini OpenAI-compat
// endpoint). Picked over Haiku for being ~2.5× cheaper at full recipe coverage.
// Notes are best-effort on this model (occasionally drops a recipe's notes);
// use `--model claude-haiku-4-5` when complete notes matter.
const DEFAULT_MODEL: &str = "gemini-2.5-flash";
const CLAUDE_DEFAULT_MODEL: &str = "claude-haiku-4-5";

// ===========================================================================
// Shared HTTP + env scaffolding (used by both provider backends)
// ===========================================================================

/// Read an env var, returning `Some` only for a present, non-empty value.
fn nonempty_env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|s| !s.is_empty())
}

/// The Cloudflare-AI-Gateway bearer token shared by both backends'
/// `from_env` (`CF_AIG_TOKEN`, falling back to `AI_GATEWAY_API_KEY`).
fn resolve_gateway_token() -> Option<String> {
    nonempty_env("CF_AIG_TOKEN").or_else(|| nonempty_env("AI_GATEWAY_API_KEY"))
}

/// The HTTP client both backends build identically (180s timeout).
fn build_client() -> Result<reqwest::Client, EpubError> {
    Ok(reqwest::Client::builder()
        .timeout(Duration::from_secs(180))
        .build()?)
}

/// POST `body` as JSON to `endpoint`, applying provider-specific `headers`
/// (auth, API version, …) plus the optional Cloudflare gateway authorization,
/// and return the response body text. Maps a non-2xx status to [`EpubError::Api`].
/// Owns the build-request / send / status-check mechanics shared by both backends.
async fn post_json(
    client: &reqwest::Client,
    endpoint: &str,
    headers: &[(&str, String)],
    gateway_token: Option<&str>,
    body: &serde_json::Value,
) -> Result<String, EpubError> {
    let mut req = client
        .post(endpoint)
        .header("content-type", "application/json");
    for (name, value) in headers {
        req = req.header(*name, value);
    }
    if let Some(token) = gateway_token {
        req = req.header("cf-aig-authorization", format!("Bearer {token}"));
    }
    let resp = req.json(body).send().await?;

    let status = resp.status();
    let text = resp.text().await?;
    if !status.is_success() {
        return Err(EpubError::Api {
            status: status.as_u16(),
            body: text,
        });
    }
    Ok(text)
}

/// Warn (once) when a finish/stop reason indicates the model's output was
/// truncated at the token limit, so the recipe tail may be missing.
fn warn_if_truncated(reason: Option<&str>, truncated_reasons: &[&str], doc_path: &str) {
    if reason.is_some_and(|r| truncated_reasons.contains(&r)) {
        tracing::warn!("chunk {doc_path} hit token limit; some recipes may be truncated");
    }
}

/// One forced-tool LLM call, abstracted over the provider wire format so callers
/// (`extract`, `classify_cookbooks`) don't switch on the backend variant.
#[allow(async_fn_in_trait)]
trait CallTool {
    /// Issue one forced-tool call. Returns the tool's decoded `input` object
    /// (`None` if the model returned no tool block), token usage, and the
    /// stop/finish reason.
    async fn call_tool(
        &self,
        system: &str,
        user: String,
        tool_name: &str,
        tool_desc: &str,
        schema: serde_json::Value,
        max_tokens: u32,
    ) -> Result<(Option<serde_json::Value>, Usage, Option<String>), EpubError>;
}

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
        let api_key = nonempty_env("ANTHROPIC_API_KEY");
        let gateway_token = resolve_gateway_token();
        if api_key.is_none() && gateway_token.is_none() {
            return Err(EpubError::MissingApiKey);
        }
        let base = nonempty_env("ANTHROPIC_BASE_URL").ok_or(EpubError::MissingBaseUrl)?;
        let endpoint = format!("{}/v1/messages", base.trim_end_matches('/'));
        let model = opts
            .model
            .clone()
            .unwrap_or_else(|| CLAUDE_DEFAULT_MODEL.to_string());
        let client = build_client()?;
        Ok(Self {
            client,
            endpoint,
            api_key,
            gateway_token,
            model,
        })
    }
}

impl CallTool for ClaudeExtractor {
    /// Issue one Anthropic Messages call with forced tool output. Returns the
    /// tool's `input` object (`None` if the model returned no tool block), the
    /// token usage, and the stop reason.
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

        let mut headers = vec![("anthropic-version", ANTHROPIC_VERSION.to_string())];
        if let Some(key) = &self.api_key {
            headers.push(("x-api-key", key.clone()));
        }
        let text = post_json(
            &self.client,
            &self.endpoint,
            &headers,
            self.gateway_token.as_deref(),
            &body,
        )
        .await?;

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

impl RecipeExtractor for ClaudeExtractor {
    fn model(&self) -> &str {
        &self.model
    }

    async fn extract(&self, chunk: &Chunk) -> Result<ChunkOutcome, EpubError> {
        let req = build_chunk_request(chunk);
        let (input, usage, stop_reason) = self
            .call_tool(
                &req.system,
                req.user,
                &req.tool_name,
                "Return every recipe found in the cookbook section.",
                req.tool_schema,
                16000,
            )
            .await?;

        warn_if_truncated(stop_reason.as_deref(), &["max_tokens"], &chunk.doc_path);

        let recipes = match input {
            Some(v) => parse_recipes_payload(v)?,
            None => Vec::new(),
        };
        Ok(ChunkOutcome {
            recipes,
            usage,
            cached: false,
        })
    }
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
        let model = opts
            .model
            .clone()
            .unwrap_or_else(|| DEFAULT_MODEL.to_string());
        let is_gemini = model.to_lowercase().starts_with("gemini");

        let gateway_token = resolve_gateway_token();
        let api_key = if is_gemini {
            nonempty_env("GEMINI_API_KEY").or_else(|| nonempty_env("GOOGLE_API_KEY"))
        } else {
            nonempty_env("OPENAI_API_KEY")
        };
        if api_key.is_none() && gateway_token.is_none() {
            return Err(EpubError::MissingApiKey);
        }

        let explicit = if is_gemini {
            nonempty_env("GEMINI_BASE_URL")
        } else {
            nonempty_env("OPENAI_BASE_URL")
        };
        let suffix = if is_gemini {
            "google-ai-studio/v1beta/openai"
        } else {
            "openai"
        };
        let base = explicit
            .or_else(|| {
                // Derive a sibling provider route from a CF gateway base.
                nonempty_env("ANTHROPIC_BASE_URL").and_then(|b| {
                    b.trim_end_matches('/')
                        .strip_suffix("/anthropic")
                        .map(|prefix| format!("{prefix}/{suffix}"))
                })
            })
            .ok_or(EpubError::MissingBaseUrl)?;
        let endpoint = format!("{}/chat/completions", base.trim_end_matches('/'));

        let client = build_client()?;
        Ok(Self {
            client,
            endpoint,
            api_key,
            gateway_token,
            model,
        })
    }
}

impl CallTool for OpenAiExtractor {
    /// Issue one OpenAI-compatible chat-completions call with a forced function
    /// call. Returns the function's decoded arguments object (`None` if the model
    /// returned no tool call), token usage, and finish reason.
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

        let headers: Vec<(&str, String)> = self
            .api_key
            .as_ref()
            .map(|key| ("authorization", format!("Bearer {key}")))
            .into_iter()
            .collect();
        let text = post_json(
            &self.client,
            &self.endpoint,
            &headers,
            self.gateway_token.as_deref(),
            &body,
        )
        .await?;

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
        let req = build_chunk_request(chunk);
        let (input, usage, finish_reason) = self
            .call_tool(
                &req.system,
                req.user,
                &req.tool_name,
                "Return every recipe found in the cookbook section.",
                req.tool_schema,
                16000,
            )
            .await?;

        warn_if_truncated(finish_reason.as_deref(), &["length"], &chunk.doc_path);

        let recipes = match input {
            Some(v) => parse_recipes_payload(v)?,
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
        let (input, _usage, _reason) = self
            .call_tool(
                CLASSIFY_SYSTEM_PROMPT,
                user,
                CLASSIFY_TOOL_NAME,
                "Return the 1-based indices of the books that are cookbooks.",
                schema,
                2000,
            )
            .await?;
        let payload: ClassifyPayload = match input {
            Some(v) => serde_json::from_value(v)?,
            None => ClassifyPayload::default(),
        };
        Ok(indices_to_bools(&payload.cookbook_indices, books.len()))
    }
}

impl CallTool for Backend {
    async fn call_tool(
        &self,
        system: &str,
        user: String,
        tool_name: &str,
        tool_desc: &str,
        schema: serde_json::Value,
        max_tokens: u32,
    ) -> Result<(Option<serde_json::Value>, Usage, Option<String>), EpubError> {
        match self {
            Backend::Claude(e) => {
                e.call_tool(system, user, tool_name, tool_desc, schema, max_tokens)
                    .await
            }
            Backend::OpenAi(e) => {
                e.call_tool(system, user, tool_name, tool_desc, schema, max_tokens)
                    .await
            }
        }
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

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::extractor::{RecipesPayload, TOOL_NAME};

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
