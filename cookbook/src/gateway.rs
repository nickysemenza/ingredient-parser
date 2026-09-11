//! Cloudflare AI Gateway wire formats, kept pure: build the HTTP request for a
//! model and decode its response. No network, no credentials.
//!
//! Every provider is reached through the gateway root the host supplies. Four
//! routes cover the catalog: Anthropic Messages, the gateway's unified
//! OpenAI-compatible chat route (Google AI Studio, Workers AI), OpenAI chat
//! completions, and OpenAI Responses. All calls force one tool call so the
//! answer is the tool input, never prose.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::contract::{CONTRACT_VERSION, ChunkRequest};
use crate::cost::Usage;
use crate::models::{Model, Provider};
use crate::transport::{HttpRequest, HttpResponse};

const ANTHROPIC_VERSION: &str = "2023-06-01";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "wasm", derive(tsify_next::Tsify))]
#[serde(rename_all = "snake_case")]
pub enum Route {
    AnthropicMessages,
    /// `/compat/chat/completions`: the gateway maps an OpenAI-shaped request
    /// onto Google AI Studio or Workers AI.
    CompatChat,
    OpenAiChat,
    OpenAiResponses,
}

impl Route {
    pub fn path(self) -> &'static str {
        match self {
            Route::AnthropicMessages => "/anthropic/v1/messages",
            Route::CompatChat => "/compat/chat/completions",
            Route::OpenAiChat => "/openai/chat/completions",
            Route::OpenAiResponses => "/openai/responses",
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Route::AnthropicMessages => "anthropic-messages",
            Route::CompatChat => "compat-chat",
            Route::OpenAiChat => "openai-chat",
            Route::OpenAiResponses => "openai-responses",
        }
    }
}

/// Per-call tags for the gateway's `cf-aig-metadata` header (five string
/// entries, the gateway's maximum), so the dashboard can filter by any of them.
#[derive(Debug, Clone, Copy)]
pub struct CallMeta<'a> {
    pub cookbook: &'a str,
    pub chunk: &'a str,
    /// `extract`, `retry`, `second_opinion`, `escalation`, `classify`.
    pub purpose: &'a str,
    /// Let the gateway answer a repeated request from its cache.
    pub gateway_cache: bool,
}

/// How long AI Gateway keeps a cached answer: its maximum, one month. The
/// request body carries the contract version, the model and the chunk text,
/// so a cached answer is only ever reused for the same question.
pub const GATEWAY_CACHE_TTL_SECS: u32 = 30 * 24 * 60 * 60;
/// The gateway's cache verdict header; `HIT` means nothing was billed.
pub const GATEWAY_CACHE_STATUS_HEADER: &str = "cf-aig-cache-status";

/// Build the gateway request for `model`. Never carries authorization.
pub fn build_http(
    model: &Model,
    request: &ChunkRequest,
    max_tokens: u32,
    meta: &CallMeta<'_>,
) -> HttpRequest {
    let max_tokens = max_tokens.min(model.max_output_tokens);
    let cache_header = if meta.gateway_cache {
        (
            "cf-aig-cache-ttl".to_string(),
            GATEWAY_CACHE_TTL_SECS.to_string(),
        )
    } else {
        ("cf-aig-skip-cache".to_string(), "true".to_string())
    };
    let mut headers = vec![
        ("content-type".to_string(), "application/json".to_string()),
        cache_header,
        // The retry policy owns retries and their accounting.
        ("cf-aig-max-attempts".to_string(), "1".to_string()),
        (
            "cf-aig-metadata".to_string(),
            json!({
                "cookbook": meta.cookbook,
                "model": model.id,
                "chunk": meta.chunk,
                "contract": CONTRACT_VERSION,
                "purpose": meta.purpose,
            })
            .to_string(),
        ),
    ];
    let body = match model.route {
        Route::AnthropicMessages => {
            headers.push((
                "anthropic-version".to_string(),
                ANTHROPIC_VERSION.to_string(),
            ));
            json!({
                "model": model.id,
                "max_tokens": max_tokens,
                // The static prefix is cacheable across calls within the TTL.
                "system": [{"type": "text", "text": request.system, "cache_control": {"type": "ephemeral"}}],
                "tools": [{"name": request.tool_name, "description": TOOL_DESCRIPTION, "input_schema": request.tool_schema}],
                "tool_choice": {"type": "tool", "name": request.tool_name},
                "messages": [{"role": "user", "content": request.user}],
            })
        }
        Route::OpenAiResponses => json!({
            "model": model.id,
            "store": false,
            "instructions": request.system,
            "input": request.user,
            "max_output_tokens": max_tokens,
            "tools": [{"type": "function", "name": request.tool_name, "description": TOOL_DESCRIPTION, "parameters": request.tool_schema, "strict": false}],
            "tool_choice": {"type": "function", "name": request.tool_name},
        }),
        Route::CompatChat | Route::OpenAiChat => {
            let wire_model = match model.provider {
                Provider::GoogleAiStudio | Provider::WorkersAi => {
                    format!("{}/{}", model.provider.as_str(), model.id)
                }
                Provider::Anthropic | Provider::OpenAi => model.id.to_string(),
            };
            // OpenAI reasoning models and the gpt-5 family reject `max_tokens`.
            let lower = model.id.to_ascii_lowercase();
            let token_param = if matches!(model.provider, Provider::OpenAi)
                && (lower.starts_with("o1")
                    || lower.starts_with("o3")
                    || lower.starts_with("o4")
                    || lower.starts_with("gpt-5"))
            {
                "max_completion_tokens"
            } else {
                "max_tokens"
            };
            json!({
                "model": wire_model,
                token_param: max_tokens,
                "messages": [
                    {"role": "system", "content": request.system},
                    {"role": "user", "content": request.user},
                ],
                "tools": [{"type": "function", "function": {"name": request.tool_name, "description": TOOL_DESCRIPTION, "parameters": request.tool_schema}}],
                "tool_choice": {"type": "function", "function": {"name": request.tool_name}},
            })
        }
    };
    HttpRequest {
        path: model.route.path().to_string(),
        headers,
        body,
    }
}

const TOOL_DESCRIPTION: &str =
    "Report the structure of the numbered source lines as line-number selections.";

/// A decoded successful call.
#[derive(Debug, Clone, PartialEq)]
pub struct CallResult {
    /// The tool input, `None` when the model produced no tool call.
    pub input: Option<Value>,
    pub usage: Usage,
    /// The answer hit the output token limit; its tail may be missing.
    pub truncated: bool,
    pub request_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CallFailure {
    /// A non-2xx status.
    #[error("http {status}: {message}")]
    Http {
        status: u16,
        message: String,
        retry_after_secs: Option<u64>,
        request_id: Option<String>,
    },
    /// A 2xx whose body could not be decoded.
    #[error("undecodable response: {message}")]
    Payload {
        message: String,
        request_id: Option<String>,
    },
}

/// Base pause before retrying a transient failure; doubles per retry.
const TRANSIENT_BACKOFF_MS: u64 = 2_000;
/// Longest pause worth taking inside a run.
const MAX_TRANSIENT_WAIT_MS: u64 = 60_000;

/// Base pause before retrying a wholesale-pool rate limit. The pool meters
/// tokens per minute, so the retries have to spread across a minute:
/// 5 s, 10 s, 20 s, 40 s.
const WHOLESALE_BACKOFF_MS: u64 = 5_000;

/// Pause before the `retry`th (0-based) retry of a transient failure:
/// 2 s, 4 s, 8 s, …
pub fn transient_backoff_ms(retry: u32) -> u64 {
    (TRANSIENT_BACKOFF_MS << retry.min(5)).min(MAX_TRANSIENT_WAIT_MS)
}

impl CallFailure {
    /// Rate limits and server errors are worth retrying after a pause:
    /// `Retry-After` when the gateway sends one, else exponential backoff by
    /// `retry` (0-based). Anything else is the model's or the request's
    /// fault, and a wait over a minute is not worth taking.
    pub fn transient_delay_ms(&self, retry: u32) -> Option<u64> {
        match self {
            CallFailure::Http {
                status,
                retry_after_secs,
                ..
            } if *status == 429 || (500..600).contains(status) => match retry_after_secs {
                Some(secs) => (secs * 1000 <= MAX_TRANSIENT_WAIT_MS).then_some(secs * 1000),
                None if self.is_wholesale_rate_limit() => {
                    Some((WHOLESALE_BACKOFF_MS << retry.min(5)).min(MAX_TRANSIENT_WAIT_MS))
                }
                None => Some(transient_backoff_ms(retry)),
            },
            _ => None,
        }
    }

    /// Cloudflare's wholesale pool answered "Wholesale Rate limited" (gateway
    /// error 2018). It means either that the account's unified-billing credit
    /// is gone, in which case every call to the model fails for the rest of
    /// the run, or that the pool is momentarily over capacity, in which case
    /// a short pause clears it. The run tells them apart by whether the model
    /// has answered at all.
    pub fn is_wholesale_rate_limit(&self) -> bool {
        matches!(
            self,
            CallFailure::Http { status: 429, message, .. }
                if message.contains("Wholesale") || message.contains("\"code\":2018")
        )
    }

    pub fn status(&self) -> Option<u16> {
        match self {
            CallFailure::Http { status, .. } => Some(*status),
            CallFailure::Payload { .. } => None,
        }
    }

    pub fn request_id(&self) -> Option<&str> {
        match self {
            CallFailure::Http { request_id, .. } | CallFailure::Payload { request_id, .. } => {
                request_id.as_deref()
            }
        }
    }
}

/// Gateway log id, else request id, else Cloudflare ray.
pub fn request_id(response: &HttpResponse) -> Option<String> {
    ["cf-aig-log-id", "x-request-id", "cf-ray"]
        .into_iter()
        .find_map(|h| response.header(h))
        .map(str::to_string)
}

/// `Retry-After` in seconds, whether given as a count or an HTTP date.
pub fn retry_after_secs(response: &HttpResponse) -> Option<u64> {
    let value = response.header("retry-after")?.trim();
    if let Ok(secs) = value.parse::<u64>() {
        return Some(secs);
    }
    let at = jiff::fmt::rfc2822::parse(value).ok()?;
    let now = jiff::Timestamp::now();
    Some(at.timestamp().duration_since(now).as_secs().max(0) as u64)
}

/// Decode a response for `route`.
pub fn parse_response(route: Route, response: &HttpResponse) -> Result<CallResult, CallFailure> {
    let request_id = request_id(response);
    if !response.is_success() {
        return Err(CallFailure::Http {
            status: response.status,
            message: truncate(&response.body, 2000),
            retry_after_secs: retry_after_secs(response),
            request_id,
        });
    }
    let payload = |message: String| CallFailure::Payload {
        message,
        request_id: request_id.clone(),
    };
    let value: Value = serde_json::from_str(&response.body).map_err(|e| payload(e.to_string()))?;
    let (input, usage, truncated) = match route {
        Route::AnthropicMessages => {
            let usage = anthropic_usage(&value);
            let truncated = value["stop_reason"] == "max_tokens";
            let input = value["content"]
                .as_array()
                .and_then(|blocks| blocks.iter().find(|b| b["type"] == "tool_use"))
                .map(|b| b["input"].clone());
            (input, usage, truncated)
        }
        Route::CompatChat | Route::OpenAiChat => {
            let usage = openai_usage(&value);
            let choice = &value["choices"][0];
            let truncated = choice["finish_reason"] == "length";
            let input = match choice["message"]["tool_calls"][0]["function"]["arguments"].as_str() {
                Some(args) => Some(
                    serde_json::from_str(args)
                        .map_err(|e| payload(format!("tool arguments: {e}")))?,
                ),
                None => None,
            };
            (input, usage, truncated)
        }
        Route::OpenAiResponses => {
            let usage = responses_usage(&value);
            let truncated = value["status"] == "incomplete";
            let args = value["output"]
                .as_array()
                .and_then(|items| items.iter().find(|i| i["type"] == "function_call"))
                .and_then(|i| i["arguments"].as_str());
            let input = match args {
                Some(args) => Some(
                    serde_json::from_str(args)
                        .map_err(|e| payload(format!("tool arguments: {e}")))?,
                ),
                None => None,
            };
            (input, usage, truncated)
        }
    };
    Ok(CallResult {
        input,
        usage,
        truncated,
        request_id,
    })
}

/// Token usage from a raw response body, for hosts that record usage per call
/// (cubby's AI-usage table). `None` when the body carries no usage.
pub fn usage_from_response(route: Route, body: &str) -> Option<Usage> {
    let value: Value = serde_json::from_str(body).ok()?;
    value.get("usage")?;
    Some(match route {
        Route::AnthropicMessages => anthropic_usage(&value),
        Route::CompatChat | Route::OpenAiChat => openai_usage(&value),
        Route::OpenAiResponses => responses_usage(&value),
    })
}

fn anthropic_usage(value: &Value) -> Usage {
    let u = &value["usage"];
    Usage {
        input_tokens: u["input_tokens"].as_u64().unwrap_or(0),
        output_tokens: u["output_tokens"].as_u64().unwrap_or(0),
        cache_read_input_tokens: u["cache_read_input_tokens"].as_u64().unwrap_or(0),
        cache_creation_input_tokens: u["cache_creation_input_tokens"].as_u64().unwrap_or(0),
    }
}

fn openai_usage(value: &Value) -> Usage {
    let u = &value["usage"];
    let cached = u["prompt_tokens_details"]["cached_tokens"]
        .as_u64()
        .unwrap_or(0);
    Usage {
        input_tokens: u["prompt_tokens"]
            .as_u64()
            .unwrap_or(0)
            .saturating_sub(cached),
        output_tokens: u["completion_tokens"].as_u64().unwrap_or(0),
        cache_read_input_tokens: cached,
        cache_creation_input_tokens: 0,
    }
}

fn responses_usage(value: &Value) -> Usage {
    let u = &value["usage"];
    let cached = u["input_tokens_details"]["cached_tokens"]
        .as_u64()
        .unwrap_or(0);
    let written = u["input_tokens_details"]["cache_write_tokens"]
        .as_u64()
        .unwrap_or(0);
    Usage {
        input_tokens: u["input_tokens"]
            .as_u64()
            .unwrap_or(0)
            .saturating_sub(cached.saturating_add(written)),
        output_tokens: u["output_tokens"].as_u64().unwrap_or(0),
        cache_read_input_tokens: cached,
        cache_creation_input_tokens: written,
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        let end = (0..=max)
            .rev()
            .find(|&i| s.is_char_boundary(i))
            .unwrap_or(0);
        format!("{}…", &s[..end])
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::models::model;

    fn request() -> ChunkRequest {
        ChunkRequest {
            system: "sys".into(),
            user: "0: a".into(),
            tool_name: "emit_items".into(),
            tool_schema: json!({"type": "object"}),
        }
    }

    fn meta() -> CallMeta<'static> {
        CallMeta {
            cookbook: "Book",
            chunk: "k001",
            purpose: "extract",
            gateway_cache: false,
        }
    }

    #[test]
    fn gateway_cache_is_a_ttl_or_a_skip() {
        let m = model("claude-haiku-4-5").unwrap();
        let cached = build_http(
            m,
            &request(),
            50_000,
            &CallMeta {
                gateway_cache: true,
                ..meta()
            },
        );
        assert!(
            cached
                .headers
                .iter()
                .any(|(n, v)| n == "cf-aig-cache-ttl" && v == &GATEWAY_CACHE_TTL_SECS.to_string())
        );
        assert!(!cached.headers.iter().any(|(n, _)| n == "cf-aig-skip-cache"));
        let fresh = build_http(m, &request(), 50_000, &meta());
        assert!(!fresh.headers.iter().any(|(n, _)| n == "cf-aig-cache-ttl"));
    }

    #[test]
    fn anthropic_request_forces_the_tool() {
        let m = model("claude-haiku-4-5").unwrap();
        let req = build_http(m, &request(), 50_000, &meta());
        assert_eq!(req.path, "/anthropic/v1/messages");
        assert!(
            req.headers
                .iter()
                .any(|(n, v)| n == "anthropic-version" && v == ANTHROPIC_VERSION)
        );
        assert!(
            req.headers
                .iter()
                .any(|(n, v)| n == "cf-aig-skip-cache" && v == "true")
        );
        assert!(
            req.headers
                .iter()
                .any(|(n, v)| n == "cf-aig-max-attempts" && v == "1")
        );
        assert!(!req.headers.iter().any(|(n, _)| n.contains("authorization")));
        let meta: Value = serde_json::from_str(
            req.headers
                .iter()
                .find(|(n, _)| n == "cf-aig-metadata")
                .map(|(_, v)| v.as_str())
                .unwrap(),
        )
        .unwrap();
        assert_eq!(meta.as_object().unwrap().len(), 5);
        assert_eq!(meta["contract"], CONTRACT_VERSION);
        assert_eq!(
            req.body["max_tokens"],
            json!(m.max_output_tokens),
            "capped by the catalog"
        );
        assert_eq!(
            req.body["tool_choice"],
            json!({"type": "tool", "name": "emit_items"})
        );
        assert_eq!(req.body["system"][0]["cache_control"]["type"], "ephemeral");
    }

    #[test]
    fn compat_request_prefixes_the_provider() {
        let m = model("gemini-2.5-flash").unwrap();
        let req = build_http(m, &request(), 1000, &meta());
        assert_eq!(req.path, "/compat/chat/completions");
        assert_eq!(req.body["model"], "google-ai-studio/gemini-2.5-flash");
        assert_eq!(req.body["max_tokens"], 1000);
        assert_eq!(req.body["tool_choice"]["function"]["name"], "emit_items");
    }

    #[test]
    fn responses_request_for_luna() {
        let m = model("gpt-5.6-luna").unwrap();
        let req = build_http(m, &request(), 1000, &meta());
        assert_eq!(req.path, "/openai/responses");
        assert_eq!(req.body["max_output_tokens"], 1000);
        assert_eq!(req.body["tools"][0]["name"], "emit_items");
    }

    fn resp(status: u16, body: Value) -> HttpResponse {
        HttpResponse {
            status,
            headers: vec![("cf-aig-log-id".into(), "log1".into())],
            body: body.to_string(),
        }
    }

    #[test]
    fn decodes_anthropic() {
        let r = resp(
            200,
            json!({"content":[{"type":"text","text":"hi"},{"type":"tool_use","input":{"items":[]}}],
            "stop_reason":"max_tokens","usage":{"input_tokens":10,"output_tokens":5,"cache_read_input_tokens":3,"cache_creation_input_tokens":2}}),
        );
        let out = parse_response(Route::AnthropicMessages, &r).unwrap();
        assert_eq!(out.input, Some(json!({"items":[]})));
        assert!(out.truncated);
        assert_eq!(
            out.usage,
            Usage {
                input_tokens: 10,
                output_tokens: 5,
                cache_read_input_tokens: 3,
                cache_creation_input_tokens: 2
            }
        );
        assert_eq!(out.request_id.as_deref(), Some("log1"));
        assert_eq!(
            usage_from_response(Route::AnthropicMessages, &r.body),
            Some(out.usage)
        );
    }

    #[test]
    fn decodes_openai_chat_and_responses() {
        let chat = resp(
            200,
            json!({"choices":[{"finish_reason":"stop","message":{"tool_calls":[{"function":{"arguments":"{\"items\":[]}"}}]}}],
            "usage":{"prompt_tokens":100,"completion_tokens":7,"prompt_tokens_details":{"cached_tokens":40}}}),
        );
        let out = parse_response(Route::CompatChat, &chat).unwrap();
        assert_eq!(out.input, Some(json!({"items":[]})));
        assert!(!out.truncated);
        assert_eq!(
            out.usage,
            Usage {
                input_tokens: 60,
                output_tokens: 7,
                cache_read_input_tokens: 40,
                cache_creation_input_tokens: 0
            }
        );

        let responses = resp(
            200,
            json!({"status":"incomplete","output":[{"type":"function_call","arguments":"{\"items\":[]}"}],
            "usage":{"input_tokens":100,"output_tokens":9,"input_tokens_details":{"cached_tokens":10,"cache_write_tokens":5}}}),
        );
        let out = parse_response(Route::OpenAiResponses, &responses).unwrap();
        assert!(out.truncated);
        assert_eq!(
            out.usage,
            Usage {
                input_tokens: 85,
                output_tokens: 9,
                cache_read_input_tokens: 10,
                cache_creation_input_tokens: 5
            }
        );

        let none = resp(
            200,
            json!({"choices":[{"message":{"content":"no tool"}}],"usage":{}}),
        );
        assert_eq!(
            parse_response(Route::OpenAiChat, &none).unwrap().input,
            None
        );
        assert!(usage_from_response(Route::OpenAiChat, "{}").is_none());
    }

    #[test]
    fn http_failures_carry_retry_hints() {
        let mut r = resp(429, json!({"error":"slow down"}));
        r.headers.push(("retry-after".into(), "7".into()));
        let err = parse_response(Route::CompatChat, &r).unwrap_err();
        assert_eq!(err.status(), Some(429));
        assert_eq!(err.transient_delay_ms(0), Some(7000));
        assert_eq!(err.request_id(), Some("log1"));
        let no_hint =
            parse_response(Route::CompatChat, &resp(429, json!({"error":"slow"}))).unwrap_err();
        assert_eq!(no_hint.transient_delay_ms(0), Some(2000));
        assert_eq!(
            no_hint.transient_delay_ms(2),
            Some(8000),
            "backs off exponentially"
        );
        let wholesale = parse_response(
            Route::CompatChat,
            &resp(
                429,
                json!({"error":[{"code":2018,"message":"Wholesale Rate limited"}]}),
            ),
        )
        .unwrap_err();
        assert!(wholesale.is_wholesale_rate_limit());
        assert_eq!(
            wholesale.transient_delay_ms(0),
            Some(5000),
            "spread over a minute"
        );
        assert_eq!(wholesale.transient_delay_ms(3), Some(40000));
        let bad = resp(400, json!({"error":"bad"}));
        assert_eq!(
            parse_response(Route::CompatChat, &bad)
                .unwrap_err()
                .transient_delay_ms(0),
            None
        );
        let long = HttpResponse {
            status: 503,
            headers: vec![("retry-after".into(), "900".into())],
            body: String::new(),
        };
        assert_eq!(
            parse_response(Route::CompatChat, &long)
                .unwrap_err()
                .transient_delay_ms(0),
            None,
            "too long to wait"
        );
        let garbage = HttpResponse {
            status: 200,
            headers: vec![],
            body: "<html>".into(),
        };
        assert!(matches!(
            parse_response(Route::AnthropicMessages, &garbage).unwrap_err(),
            CallFailure::Payload { .. }
        ));
    }

    #[test]
    fn retry_after_accepts_http_dates() {
        let future = jiff::Timestamp::now() + jiff::SignedDuration::from_secs(90);
        let r = HttpResponse {
            status: 503,
            headers: vec![(
                "Retry-After".into(),
                jiff::fmt::rfc2822::to_string(&future.to_zoned(jiff::tz::TimeZone::UTC)).unwrap(),
            )],
            body: String::new(),
        };
        let secs = retry_after_secs(&r).unwrap();
        assert!((85..=90).contains(&secs), "{secs}");
    }
}
