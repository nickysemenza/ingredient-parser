//! Curated model choices. Availability is explicit rather than guessed from IDs.
use serde::Serialize;
pub const DEFAULT_MODEL: &str = "gemini-2.5-flash";
#[derive(Debug, Clone, Serialize)]
pub struct Model {
    pub id: &'static str,
    pub label: &'static str,
    pub provider: &'static str,
    pub enabled: bool,
    pub status: &'static str,
    pub transport: &'static str,
    pub max_output_tokens: u64,
    pub pricing_checked: &'static str,
    pub pricing_source: &'static str,
}
pub fn catalog() -> Vec<Model> {
    vec![
        Model {
            id: DEFAULT_MODEL,
            label: "Gemini 2.5 Flash",
            provider: "google-ai-studio",
            transport: "chat-completions",
            max_output_tokens: 16000,
            pricing_checked: "2026-09-09",
            pricing_source: "https://ai.google.dev/gemini-api/docs/pricing",
            enabled: true,
            status: "Existing baseline",
        },
        Model {
            id: "gemini-2.5-flash-lite",
            label: "Gemini 2.5 Flash-Lite",
            provider: "google-ai-studio",
            transport: "chat-completions",
            max_output_tokens: 16000,
            pricing_checked: "2026-09-09",
            pricing_source: "https://ai.google.dev/gemini-api/docs/pricing",
            enabled: true,
            status: "Compatible; not benchmarked",
        },
        Model {
            id: "claude-haiku-4-5",
            label: "Claude Haiku 4.5",
            provider: "anthropic",
            transport: "messages",
            max_output_tokens: 16000,
            pricing_checked: "2026-09-09",
            pricing_source: "https://platform.claude.com/docs/en/about-claude/pricing",
            enabled: true,
            status: "Experimental; source validation failures in evaluation",
        },
        Model {
            id: "gemini-3.5-flash-lite",
            label: "Gemini 3.5 Flash-Lite",
            provider: "google-ai-studio",
            transport: "chat-completions",
            max_output_tokens: 16000,
            pricing_checked: "2026-09-09",
            pricing_source: "https://ai.google.dev/gemini-api/docs/pricing",
            enabled: true,
            status: "Experimental; evaluated 2026-09-09",
        },
        Model {
            id: "gemini-3.7-flash",
            label: "Gemini 3.7 Flash",
            provider: "google-ai-studio",
            transport: "chat-completions",
            max_output_tokens: 16000,
            pricing_checked: "2026-09-09",
            pricing_source: "https://ai.google.dev/gemini-api/docs/pricing",
            enabled: true,
            status: "Experimental; evaluated 2026-09-09",
        },
        Model {
            id: "claude-sonnet-5",
            label: "Claude Sonnet 5",
            provider: "anthropic",
            transport: "messages",
            max_output_tokens: 16000,
            pricing_checked: "2026-09-09",
            pricing_source: "https://platform.claude.com/docs/en/about-claude/pricing",
            enabled: true,
            status: "Experimental; evaluated 2026-09-09",
        },
        Model {
            id: "gpt-5.6-luna",
            label: "GPT-5.6 Luna",
            provider: "openai",
            transport: "responses",
            max_output_tokens: 16000,
            pricing_checked: "2026-09-09",
            pricing_source: "https://developers.openai.com/api/docs/models/gpt-5.6-luna",
            enabled: true,
            status: "Experimental; Responses verified 2026-09-09",
        },
        Model {
            id: "@cf/moonshotai/kimi-k2.6",
            label: "Kimi K2.6",
            provider: "workers-ai",
            transport: "gateway-unified-chat-completions",
            max_output_tokens: 16000,
            pricing_checked: "2026-09-09",
            pricing_source: "https://developers.cloudflare.com/workers-ai/platform/pricing/",
            enabled: false,
            status: "Experimental; Gateway authenticated, recipe requests timed out",
        },
        Model {
            id: "@cf/moonshotai/kimi-k2.7-code",
            label: "Kimi K2.7 Code",
            provider: "workers-ai",
            transport: "gateway-unified-chat-completions",
            max_output_tokens: 16000,
            pricing_checked: "2026-09-09",
            pricing_source: "https://developers.cloudflare.com/workers-ai/platform/pricing/",
            enabled: true,
            status: "Experimental; Gateway and structured output verified",
        },
    ]
}

/// Explicit legacy aliases remain supported for downstream callers.
pub fn provider(id: &str) -> Option<&'static str> {
    if let Some(m) = catalog().into_iter().find(|m| m.id == id) {
        return Some(m.provider);
    }
    match id {
        "claude-haiku-4-5-20251001"
        | "claude-sonnet-4-5"
        | "claude-sonnet-4-6"
        | "claude-opus-4-5"
        | "claude-opus-4-5-20251101" => Some("anthropic"),
        "gemini-2.0-flash"
        | "gemini-2.0-flash-lite"
        | "gemini-2.5-flash-002"
        | "gemini-2.5-flash-lite-preview" => Some("google-ai-studio"),
        "gpt-4o-mini" | "gpt-4o" | "gpt-4.1-mini" | "o3-mini" | "o1" | "o3" | "o4-mini" => {
            Some("openai")
        }
        _ => None,
    }
}

/// Primary pricing references; snapshots are dated in each saved charge.
pub fn pricing_source(id: &str) -> Option<&'static str> {
    match provider(id)? {
        "google-ai-studio" => Some("https://ai.google.dev/gemini-api/docs/pricing"),
        "anthropic" => Some("https://platform.claude.com/docs/en/about-claude/pricing"),
        "openai" => Some("https://developers.openai.com/api/docs/pricing"),
        "workers-ai" => Some("https://developers.cloudflare.com/workers-ai/platform/pricing/"),
        _ => None,
    }
}
