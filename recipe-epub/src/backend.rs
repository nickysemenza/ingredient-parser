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
    let extractor = Backend::from_env(opts, source)?;
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
    let per_chunk: Vec<(Chunk, ChunkOutcome)> = stream::iter(chunks.iter())
        .map(|chunk| async {
            let outcome = extractor.extract(chunk).await.unwrap_or_else(|e| {
                tracing::warn!("chunk {} extraction failed: {e}", chunk.doc_path);
                ChunkOutcome {
                    recipes: Vec::new(),
                    usage: Usage::default(),
                    cached: false,
                    truncated: false,
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
            // Carry the whole chunk (its text lines + image positions) so the
            // assembler can bind each recipe's hero photo by title proximity.
            (chunk.clone(), outcome)
        })
        .buffered(opts.concurrency.max(1))
        .collect::<Vec<_>>()
        .await;

    let mut stats = ExtractionStats {
        model: extractor.model().to_string(),
        chunks_total: per_chunk.len(),
        ..Default::default()
    };
    let recipes_by_chunk: Vec<(Chunk, Vec<ExtractedRecipe>)> = per_chunk
        .into_iter()
        .map(|(chunk, outcome)| {
            if outcome.cached {
                stats.chunks_cached += 1;
            }
            stats.usage.add(&outcome.usage);
            (chunk, outcome.recipes)
        })
        .collect();

    let mut recipes = assemble(recipes_by_chunk, source);
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
                truncated: false,
            });
        }
        let outcome = self.inner.extract(chunk).await?;
        // Never cache a truncated outcome: it would silently serve the partial
        // recipe list on every future run. Leaving it uncached lets a later run
        // (bigger limit, different model) re-attempt the chunk.
        if outcome.truncated {
            tracing::warn!("chunk {} truncated; not caching", chunk.doc_path);
        } else if let Err(e) = cache::write(&self.dir, &key, &outcome.recipes) {
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
/// `from_env` (`AI_GATEWAY_API_KEY`, falling back to `CF_AIG_TOKEN`). Required —
/// the gateway authenticates the caller with it and injects each provider's key
/// (BYOK).
fn resolve_gateway_token() -> Result<String, EpubError> {
    nonempty_env("AI_GATEWAY_API_KEY")
        .or_else(|| nonempty_env("CF_AIG_TOKEN"))
        .ok_or(EpubError::MissingApiKey)
}

/// The Cloudflare AI Gateway root, e.g.
/// `https://gateway.ai.cloudflare.com/v1/<account>/<gateway>` — NO provider
/// suffix. Each backend appends its own provider path (`/anthropic/v1/messages`,
/// `/openai/chat/completions`, `/google-ai-studio/v1beta/openai/chat/completions`).
/// All model traffic routes through the gateway; there is no direct-provider path.
fn gateway_base() -> Result<String, EpubError> {
    nonempty_env("CLOUDFLARE_AI_GATEWAY_BASE_URL")
        .map(|b| b.trim_end_matches('/').to_string())
        .ok_or(EpubError::MissingBaseUrl)
}

/// The HTTP client both backends build identically (180s timeout).
fn build_client() -> Result<reqwest::Client, EpubError> {
    Ok(reqwest::Client::builder()
        .timeout(Duration::from_secs(180))
        .build()?)
}

/// POST `body` as JSON to `endpoint`, applying provider-specific `headers`
/// (auth, API version, …) plus the Cloudflare gateway authorization, and
/// return the response body text. Maps a non-2xx status to [`EpubError::Api`].
/// Owns the build-request / send / status-check mechanics shared by both backends.
async fn post_json(
    client: &reqwest::Client,
    endpoint: &str,
    headers: &[(&str, String)],
    gateway_token: &str,
    body: &serde_json::Value,
) -> Result<String, EpubError> {
    let mut req = client
        .post(endpoint)
        .header("content-type", "application/json")
        .header("cf-aig-authorization", format!("Bearer {gateway_token}"));
    for (name, value) in headers {
        req = req.header(*name, value);
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

/// Per-call context for the Cloudflare AI Gateway `cf-aig-metadata` header, so
/// each request is filterable in the gateway logs. The whole-run bits (cookbook,
/// model) come from the extractor itself; these are the bits that vary per call.
struct CallMeta<'a> {
    /// Originating spine-doc path for an extract call; `""` for library-wide
    /// calls (e.g. classification).
    doc_path: &'a str,
    /// Which call this is — `"extract"` or `"classify"`.
    call: &'a str,
}

/// Build the `cf-aig-metadata` header tagging a gateway request with the
/// cookbook, model, doc, prompt version, and call type — exactly five entries
/// (the gateway's max), all string-valued. Lets the user filter the gateway logs
/// and analytics by any of these dimensions.
fn aig_metadata_header(cookbook: &str, model: &str, meta: &CallMeta<'_>) -> (&'static str, String) {
    (
        "cf-aig-metadata",
        json!({
            "cookbook": cookbook,
            "model": model,
            "doc_path": meta.doc_path,
            "prompt_version": cache::PROMPT_VERSION,
            "call": meta.call,
        })
        .to_string(),
    )
}

/// The provider-agnostic spec for one forced-tool call. Each backend turns this
/// into its own wire format (Anthropic Messages vs OpenAI chat-completions).
struct ToolCall<'a> {
    system: &'a str,
    user: String,
    tool_name: &'a str,
    tool_desc: &'a str,
    schema: serde_json::Value,
    max_tokens: u32,
}

/// The Cloudflare AI Gateway connection shared by both provider backends: the
/// HTTP client, the resolved endpoint, the gateway bearer token, the model id,
/// and the cookbook label for request metadata. Owns the auth + metadata-header
/// + POST mechanics so each backend only builds its body and parses its response.
struct GatewayClient {
    client: reqwest::Client,
    endpoint: String,
    gateway_token: String,
    model: String,
    /// Cookbook source (book path/title) emitted as gateway metadata; `""` when
    /// not book-scoped (e.g. the classifier).
    cookbook_source: String,
}

impl GatewayClient {
    /// Build the shared pieces from the environment ([`build_client`],
    /// [`resolve_gateway_token`]). `endpoint` is the provider-specific URL the
    /// caller assembled from [`gateway_base`]; `source` labels gateway requests
    /// via `cf-aig-metadata` (`""` if unknown).
    fn from_env(endpoint: String, model: String, source: &str) -> Result<Self, EpubError> {
        Ok(Self {
            client: build_client()?,
            endpoint,
            gateway_token: resolve_gateway_token()?,
            model,
            cookbook_source: source.to_string(),
        })
    }

    /// POST a provider request `body`, attaching `extra_headers` (provider-specific,
    /// e.g. `anthropic-version`), the gateway authorization, and the
    /// `cf-aig-metadata` tag. Returns the response body text.
    async fn post_tool(
        &self,
        body: &serde_json::Value,
        mut extra_headers: Vec<(&str, String)>,
        meta: &CallMeta<'_>,
    ) -> Result<String, EpubError> {
        extra_headers.push(aig_metadata_header(
            &self.cookbook_source,
            &self.model,
            meta,
        ));
        post_json(
            &self.client,
            &self.endpoint,
            &extra_headers,
            &self.gateway_token,
            body,
        )
        .await
    }
}

/// Whether a finish/stop reason indicates the model's output was truncated at
/// the token limit, so the result tail may be missing.
fn is_truncated(reason: Option<&str>, truncated_reasons: &[&str]) -> bool {
    reason.is_some_and(|r| truncated_reasons.contains(&r))
}

/// Both providers' truncation tokens (Anthropic `max_tokens`, OpenAI `length`).
const TRUNCATED_REASONS: &[&str] = &["max_tokens", "length"];

fn warn_truncated(doc_path: &str) {
    tracing::warn!("chunk {doc_path} hit token limit; some recipes may be truncated");
}

/// One forced-tool LLM call, abstracted over the provider wire format so callers
/// (`extract`, `classify_cookbooks`) don't switch on the backend variant.
#[allow(async_fn_in_trait)]
trait CallTool {
    /// Issue one forced-tool `call`, tagged for the gateway logs with `meta`.
    /// Returns the tool's decoded `input` object (`None` if the model returned no
    /// tool block), token usage, and the stop/finish reason.
    async fn call_tool(
        &self,
        call: ToolCall<'_>,
        meta: &CallMeta<'_>,
    ) -> Result<(Option<serde_json::Value>, Usage, Option<String>), EpubError>;
}

/// Run one chunk through any [`CallTool`] backend: build the request, issue the
/// forced-tool call tagged for the gateway logs, warn on truncation, and decode
/// the recipes. `truncated_reasons` are the provider's stop/finish tokens that
/// signal the output was cut at the token limit (Anthropic `max_tokens`, OpenAI
/// `length`). Shared by both backends' [`RecipeExtractor::extract`].
async fn extract_chunk<T: CallTool>(
    backend: &T,
    chunk: &Chunk,
    truncated_reasons: &[&str],
) -> Result<ChunkOutcome, EpubError> {
    let req = build_chunk_request(chunk);
    let (input, usage, reason) = backend
        .call_tool(
            ToolCall {
                system: &req.system,
                user: req.user,
                tool_name: &req.tool_name,
                tool_desc: "Return every recipe found in the cookbook section.",
                schema: req.tool_schema,
                max_tokens: 16000,
            },
            &CallMeta {
                doc_path: &chunk.doc_path,
                call: "extract",
            },
        )
        .await?;

    let truncated = is_truncated(reason.as_deref(), truncated_reasons);
    if truncated {
        warn_truncated(&chunk.doc_path);
    }

    let recipes = match input {
        Some(v) => parse_recipes_payload(v)?,
        None => Vec::new(),
    };
    Ok(ChunkOutcome {
        recipes,
        usage,
        cached: false,
        truncated,
    })
}

/// Calls the Claude Messages API (directly, or via a proxy such as Cloudflare AI
/// Gateway) with forced structured (tool) output.
pub(crate) struct ClaudeExtractor {
    conn: GatewayClient,
}

impl ClaudeExtractor {
    /// Build from the environment. All traffic routes through the Cloudflare AI
    /// Gateway (BYOK — the gateway injects the provider key):
    /// - [`gateway_base`] → `…/anthropic/v1/messages`.
    /// - [`resolve_gateway_token`] → `cf-aig-authorization` (required).
    /// - `opts.model` → model id (default Haiku).
    ///
    /// `source` labels gateway requests via `cf-aig-metadata` (`""` if unknown).
    pub fn from_env(opts: &Options, source: &str) -> Result<Self, EpubError> {
        let endpoint = format!("{}/anthropic/v1/messages", gateway_base()?);
        let model = opts
            .model
            .clone()
            .unwrap_or_else(|| CLAUDE_DEFAULT_MODEL.to_string());
        Ok(Self {
            conn: GatewayClient::from_env(endpoint, model, source)?,
        })
    }
}

impl CallTool for ClaudeExtractor {
    /// Issue one Anthropic Messages call with forced tool output. Returns the
    /// tool's `input` object (`None` if the model returned no tool block), the
    /// token usage, and the stop reason.
    async fn call_tool(
        &self,
        call: ToolCall<'_>,
        meta: &CallMeta<'_>,
    ) -> Result<(Option<serde_json::Value>, Usage, Option<String>), EpubError> {
        let body = json!({
            "model": self.conn.model,
            "max_tokens": call.max_tokens,
            // Static prefix → cache_control lets repeated calls within the TTL
            // reuse it (a no-op below the model's min cacheable size, but free).
            "system": [{
                "type": "text",
                "text": call.system,
                "cache_control": { "type": "ephemeral" }
            }],
            "tools": [{
                "name": call.tool_name,
                "description": call.tool_desc,
                "input_schema": call.schema
            }],
            "tool_choice": { "type": "tool", "name": call.tool_name },
            "messages": [{ "role": "user", "content": call.user }]
        });

        let extra = vec![("anthropic-version", ANTHROPIC_VERSION.to_string())];
        let text = self.conn.post_tool(&body, extra, meta).await?;

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
        &self.conn.model
    }

    async fn extract(&self, chunk: &Chunk) -> Result<ChunkOutcome, EpubError> {
        extract_chunk(self, chunk, &["max_tokens"]).await
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
    conn: GatewayClient,
}

impl OpenAiExtractor {
    /// Build from the environment. All traffic routes through the Cloudflare AI
    /// Gateway (BYOK — the gateway injects the provider key); the provider path is
    /// appended to [`gateway_base`]: `gemini-*` → `/google-ai-studio/v1beta/openai`,
    /// otherwise `/openai`. Auth: [`resolve_gateway_token`] (required).
    ///
    /// `source` labels gateway requests via `cf-aig-metadata` (`""` if unknown).
    pub fn from_env(opts: &Options, source: &str) -> Result<Self, EpubError> {
        let model = opts
            .model
            .clone()
            .unwrap_or_else(|| DEFAULT_MODEL.to_string());
        let provider_path = if model.to_lowercase().starts_with("gemini") {
            "google-ai-studio/v1beta/openai"
        } else {
            "openai"
        };
        let endpoint = format!("{}/{provider_path}/chat/completions", gateway_base()?);
        Ok(Self {
            conn: GatewayClient::from_env(endpoint, model, source)?,
        })
    }
}

impl CallTool for OpenAiExtractor {
    /// Issue one OpenAI-compatible chat-completions call with a forced function
    /// call. Returns the function's decoded arguments object (`None` if the model
    /// returned no tool call), token usage, and finish reason.
    async fn call_tool(
        &self,
        call: ToolCall<'_>,
        meta: &CallMeta<'_>,
    ) -> Result<(Option<serde_json::Value>, Usage, Option<String>), EpubError> {
        // OpenAI reasoning models (o1/o3/o4) and the gpt-5 family reject
        // `max_tokens` with a 400; they require `max_completion_tokens`.
        // Gemini's OpenAI-compat endpoint still takes `max_tokens`.
        let m = self.conn.model.to_lowercase();
        let token_param = if m.starts_with("o1")
            || m.starts_with("o3")
            || m.starts_with("o4")
            || m.starts_with("gpt-5")
        {
            "max_completion_tokens"
        } else {
            "max_tokens"
        };
        let body = json!({
            "model": self.conn.model,
            token_param: call.max_tokens,
            "messages": [
                { "role": "system", "content": call.system },
                { "role": "user", "content": call.user }
            ],
            "tools": [{
                "type": "function",
                "function": {
                    "name": call.tool_name,
                    "description": call.tool_desc,
                    "parameters": call.schema
                }
            }],
            "tool_choice": { "type": "function", "function": { "name": call.tool_name } }
        });

        // BYOK gateway: no provider-specific headers; the gateway injects the
        // provider's `Authorization: Bearer` key server-side.
        let text = self.conn.post_tool(&body, Vec::new(), meta).await?;

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
        &self.conn.model
    }

    async fn extract(&self, chunk: &Chunk) -> Result<ChunkOutcome, EpubError> {
        extract_chunk(self, chunk, &["length"]).await
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
    ///
    /// `source` labels gateway requests via `cf-aig-metadata`; pass `""` for
    /// library-wide work like classification.
    pub fn from_env(opts: &Options, source: &str) -> Result<Self, EpubError> {
        let model = opts.model.as_deref().unwrap_or(DEFAULT_MODEL);
        if is_openai_compatible_model(model) {
            Ok(Backend::OpenAi(OpenAiExtractor::from_env(opts, source)?))
        } else {
            Ok(Backend::Claude(ClaudeExtractor::from_env(opts, source)?))
        }
    }

    /// Ask the model which of `books` are cookbooks (one batched call). Returns a
    /// bool per input book in the same order. See [`crate::classify_cookbooks_ai`].
    pub async fn classify_cookbooks(&self, books: &[BookMeta]) -> Result<Vec<bool>, EpubError> {
        let user = classify_user_prompt(books);
        let schema = classify_tool_schema();
        let (input, _usage, reason) = self
            .call_tool(
                ToolCall {
                    system: CLASSIFY_SYSTEM_PROMPT,
                    user,
                    tool_name: CLASSIFY_TOOL_NAME,
                    tool_desc: "Return the 1-based indices of the books that are cookbooks.",
                    schema,
                    max_tokens: 2000,
                },
                &CallMeta {
                    doc_path: "",
                    call: "classify",
                },
            )
            .await?;
        // A truncated index list silently mislabels the tail books as
        // non-cookbooks — at minimum, say so.
        if is_truncated(reason.as_deref(), TRUNCATED_REASONS) {
            tracing::warn!(
                "cookbook classification hit the token limit; some books may be mislabeled"
            );
        }
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
        call: ToolCall<'_>,
        meta: &CallMeta<'_>,
    ) -> Result<(Option<serde_json::Value>, Usage, Option<String>), EpubError> {
        match self {
            Backend::Claude(e) => e.call_tool(call, meta).await,
            Backend::OpenAi(e) => e.call_tool(call, meta).await,
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

    /// Inner extractor returning a fixed outcome, for cache-policy tests.
    struct FixedExtractor {
        truncated: bool,
    }

    impl RecipeExtractor for FixedExtractor {
        async fn extract(&self, _chunk: &Chunk) -> Result<ChunkOutcome, EpubError> {
            Ok(ChunkOutcome {
                recipes: Vec::new(),
                usage: Usage::default(),
                cached: false,
                truncated: self.truncated,
            })
        }
    }

    /// A truncated outcome must NOT be cached (it would silently serve the
    /// partial recipe list forever); a complete one must be.
    #[tokio::test]
    async fn truncated_outcome_is_not_cached() {
        let chunk = Chunk {
            title_hint: None,
            text: "some cookbook text".to_string(),
            doc_path: "c1.xhtml".to_string(),
            links: Vec::new(),
            images: Vec::new(),
        };
        for (truncated, expect_cached_on_rerun) in [(true, false), (false, true)] {
            let dir = std::env::temp_dir().join(format!(
                "recipe-epub-trunc-test-{truncated}-{}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            let caching = CachingExtractor {
                inner: &FixedExtractor { truncated },
                dir: dir.clone(),
                model: "test-model".to_string(),
            };
            let first = caching.extract(&chunk).await.unwrap();
            assert!(!first.cached);
            let second = caching.extract(&chunk).await.unwrap();
            assert_eq!(
                second.cached, expect_cached_on_rerun,
                "truncated={truncated}"
            );
            let _ = std::fs::remove_dir_all(&dir);
        }
    }

    #[test]
    fn aig_metadata_header_is_valid_json_with_five_entries() {
        let (name, value) = aig_metadata_header(
            "The NoMad Cookbook",
            "gemini-2.5-flash",
            &CallMeta {
                doc_path: "c12.xhtml",
                call: "extract",
            },
        );
        assert_eq!(name, "cf-aig-metadata");
        let meta: serde_json::Value = serde_json::from_str(&value).unwrap();
        let obj = meta.as_object().unwrap();
        // The gateway saves at most five metadata entries — stay at/under that.
        assert!(obj.len() <= 5, "must not exceed the gateway's 5-entry cap");
        assert_eq!(obj["cookbook"], "The NoMad Cookbook");
        assert_eq!(obj["model"], "gemini-2.5-flash");
        assert_eq!(obj["doc_path"], "c12.xhtml");
        assert_eq!(obj["call"], "extract");
        assert_eq!(obj["prompt_version"], cache::PROMPT_VERSION);
        // All values must be strings (the gateway accepts string/number/bool).
        assert!(obj.values().all(serde_json::Value::is_string));
    }

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
