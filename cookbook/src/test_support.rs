//! Transports for tests: canned answers, and an oracle that derives a correct
//! answer from a fixture's class vocabulary so pipelines can be exercised
//! end to end without a model.
//!
//! Gated behind the `test-support` feature so downstream crates (the desktop
//! app's contract tests) can reuse it.

use std::sync::Mutex;

use serde_json::{Value, json};

use crate::chunk::Chunk;
use crate::cost::Usage;
use crate::gateway::Route;
use crate::lines::BookLines;
use crate::models::model;
use crate::transport::{CancelToken, HttpRequest, HttpResponse, Transport, TransportError};

/// The `(model, chunk, purpose)` a request carries in its gateway metadata.
pub fn request_meta(request: &HttpRequest) -> Option<(String, String, String)> {
    let raw = request
        .headers
        .iter()
        .find(|(n, _)| n == "cf-aig-metadata")?
        .1
        .as_str();
    let meta: Value = serde_json::from_str(raw).ok()?;
    Some((
        meta["model"].as_str()?.to_string(),
        meta["chunk"].as_str()?.to_string(),
        meta["purpose"].as_str()?.to_string(),
    ))
}

/// The user message of a built request, whatever the route.
pub fn request_user_text(request: &HttpRequest) -> String {
    let b = &request.body;
    if let Some(s) = b["messages"]
        .as_array()
        .and_then(|m| m.iter().find(|m| m["role"] == "user"))
        .and_then(|m| m["content"].as_str())
    {
        return s.to_string();
    }
    b["input"].as_str().unwrap_or("").to_string()
}

/// A provider-shaped 200 response carrying `payload` as the tool input.
pub fn tool_response(route: Route, payload: &Value, usage: Usage, truncated: bool) -> HttpResponse {
    let body = match route {
        Route::AnthropicMessages => json!({
            "content": [{"type": "tool_use", "name": "emit_items", "input": payload}],
            "stop_reason": if truncated { "max_tokens" } else { "tool_use" },
            "usage": {"input_tokens": usage.input_tokens, "output_tokens": usage.output_tokens,
                      "cache_read_input_tokens": usage.cache_read_input_tokens,
                      "cache_creation_input_tokens": usage.cache_creation_input_tokens},
        }),
        Route::CompatChat | Route::OpenAiChat => json!({
            "choices": [{"finish_reason": if truncated { "length" } else { "tool_calls" },
                         "message": {"tool_calls": [{"function": {"name": "emit_items", "arguments": payload.to_string()}}]}}],
            "usage": {"prompt_tokens": usage.input_tokens + usage.cache_read_input_tokens,
                      "completion_tokens": usage.output_tokens,
                      "prompt_tokens_details": {"cached_tokens": usage.cache_read_input_tokens}},
        }),
        Route::OpenAiResponses => json!({
            "status": if truncated { "incomplete" } else { "completed" },
            "output": [{"type": "function_call", "name": "emit_items", "arguments": payload.to_string()}],
            "usage": {"input_tokens": usage.input_tokens + usage.cache_read_input_tokens + usage.cache_creation_input_tokens,
                      "output_tokens": usage.output_tokens,
                      "input_tokens_details": {"cached_tokens": usage.cache_read_input_tokens,
                                               "cache_write_tokens": usage.cache_creation_input_tokens}},
        }),
    };
    HttpResponse {
        status: 200,
        headers: vec![("cf-aig-log-id".into(), "scripted".into())],
        body: body.to_string(),
    }
}

pub fn error_response(status: u16, message: &str) -> HttpResponse {
    HttpResponse {
        status,
        headers: vec![],
        body: json!({"error": message}).to_string(),
    }
}

type Responder =
    Box<dyn Fn(&HttpRequest, usize) -> Result<HttpResponse, TransportError> + Send + Sync>;

/// Answers every request through a closure that also sees how many requests
/// preceded it, and records every request.
pub struct ScriptedTransport {
    responder: Responder,
    pub requests: Mutex<Vec<HttpRequest>>,
    pub sleeps_ms: Mutex<Vec<u64>>,
}

impl ScriptedTransport {
    pub fn new(
        responder: impl Fn(&HttpRequest, usize) -> Result<HttpResponse, TransportError>
        + Send
        + Sync
        + 'static,
    ) -> Self {
        Self {
            responder: Box::new(responder),
            requests: Mutex::new(Vec::new()),
            sleeps_ms: Mutex::new(Vec::new()),
        }
    }

    pub fn calls(&self) -> usize {
        self.requests
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .len()
    }

    /// Requests whose metadata `purpose` equals `purpose`.
    pub fn calls_with_purpose(&self, purpose: &str) -> usize {
        self.requests
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .filter(|r| request_meta(r).is_some_and(|(_, _, p)| p == purpose))
            .count()
    }
}

impl Transport for ScriptedTransport {
    async fn send(
        &self,
        request: HttpRequest,
        _cancel: &CancelToken,
    ) -> Result<HttpResponse, TransportError> {
        let n = {
            let mut requests = self.requests.lock().unwrap_or_else(|e| e.into_inner());
            requests.push(request.clone());
            requests.len() - 1
        };
        (self.responder)(&request, n)
    }

    async fn sleep(&self, ms: u64) {
        self.sleeps_ms
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(ms);
    }
}

/// Which block classes play which role in a fixture. Text rules cover the
/// cases a Calibre conversion leaves to typography alone.
#[derive(Debug, Clone, Default)]
pub struct RoleMap {
    pub title: &'static [&'static str],
    pub variation_title: &'static [&'static str],
    /// Chapter or essay headings: an essay when content follows, else a
    /// chapter heading.
    pub heading: &'static [&'static str],
    pub ingredient: &'static [&'static str],
    pub step: &'static [&'static str],
    pub yield_: &'static [&'static str],
    pub headnote: &'static [&'static str],
    pub section_name: &'static [&'static str],
    pub note: &'static [&'static str],
    pub equipment: &'static [&'static str],
    pub caption: &'static [&'static str],
    /// A title-class line starting with one of these is a yield instead.
    pub yield_prefixes: &'static [&'static str],
    /// A step- or headnote-class line starting with one of these is a note.
    pub note_prefixes: &'static [&'static str],
    /// Headnote-class lines that start with a digit are numbered steps.
    pub numbered_steps: bool,
    /// Headings whose content is navigation, not an essay.
    pub ignored_headings: &'static [&'static str],
}

pub fn split_spine_roles() -> RoleMap {
    RoleMap {
        title: &["calibre_3"],
        heading: &["calibre_7"],
        ingredient: &["calibre_21", "calibre_22"],
        step: &["calibre_5"],
        headnote: &["calibre_11"],
        yield_prefixes: &["serves", "makes"],
        note_prefixes: &["DO AHEAD", "NOTE"],
        numbered_steps: true,
        ignored_headings: &["Contents"],
        ..RoleMap::default()
    }
}

pub fn epub3_roles() -> RoleMap {
    RoleMap {
        title: &["rt"],
        variation_title: &["rt-small"],
        ingredient: &["rilf", "ril"],
        step: &["rpf", "rp"],
        yield_: &["ry"],
        headnote: &["rhnf"],
        note: &["rul", "tx", "doahead", "tip", "tiph"],
        equipment: &["se"],
        caption: &["cap"],
        ..RoleMap::default()
    }
}

pub fn typographic_roles() -> RoleMap {
    RoleMap {
        title: &["sub-head-fp"],
        variation_title: &["star-list"],
        heading: &["chap-head"],
        ingredient: &["hang"],
        step: &["indentsp"],
        yield_: &["inge-head"],
        headnote: &["noindent", "indent", "noindentsp"],
        note: &["noindentsp1"],
        ..RoleMap::default()
    }
}

#[derive(Default)]
struct Draft {
    kind: &'static str,
    title: Vec<usize>,
    variation_of: Vec<usize>,
    description: Vec<usize>,
    recipe_yield: Vec<usize>,
    equipment: Vec<usize>,
    notes: Vec<usize>,
    sections: Vec<(Vec<usize>, Vec<usize>, Vec<usize>)>,
    heading: bool,
    ignore: bool,
}

impl Draft {
    fn section(&mut self) -> &mut (Vec<usize>, Vec<usize>, Vec<usize>) {
        if self.sections.is_empty() {
            self.sections.push(Default::default());
        }
        self.sections.last_mut().unwrap_or_else(|| unreachable!())
    }

    fn content_lines(&self) -> usize {
        self.description.len()
            + self.recipe_yield.len()
            + self.equipment.len()
            + self.notes.len()
            + self
                .sections
                .iter()
                .map(|(n, i, s)| n.len() + i.len() + s.len())
                .sum::<usize>()
    }
}

/// Derives the correct answer for a chunk from block classes and text rules.
pub struct Oracle {
    roles: RoleMap,
}

impl Oracle {
    pub fn new(roles: RoleMap) -> Self {
        Self { roles }
    }

    pub fn payload(&self, chunk: &Chunk, book: &BookLines) -> Value {
        let r = &self.roles;
        let has =
            |classes: &[String], set: &[&str]| classes.iter().any(|c| set.contains(&c.as_str()));
        let mut drafts: Vec<Draft> = Vec::new();
        let mut captions = Vec::new();
        let mut chapter_headings = Vec::new();
        let mut ignored = Vec::new();
        let mut last_title_local: Option<usize> = None;
        for local in 0..chunk.lines() {
            let line = &book.lines[chunk.global(local)];
            let classes = &line.clean.classes;
            let text = line.text();
            let lower = text.to_ascii_lowercase();
            let starts = |prefixes: &[&str]| {
                prefixes
                    .iter()
                    .any(|p| lower.starts_with(&p.to_ascii_lowercase()))
            };
            if line.clean.in_figure || has(classes, r.caption) {
                captions.push(local);
                continue;
            }
            if has(classes, r.title) && !starts(r.yield_prefixes) {
                drafts.push(Draft {
                    kind: "recipe",
                    title: vec![local],
                    ..Default::default()
                });
                last_title_local = Some(local);
                continue;
            }
            if has(classes, r.variation_title) {
                drafts.push(Draft {
                    kind: "variation",
                    title: vec![local],
                    variation_of: last_title_local.into_iter().collect(),
                    ..Default::default()
                });
                continue;
            }
            if has(classes, r.heading) {
                let ignore = r
                    .ignored_headings
                    .iter()
                    .any(|h| text.eq_ignore_ascii_case(h));
                drafts.push(Draft {
                    kind: "essay",
                    title: vec![local],
                    heading: true,
                    ignore,
                    ..Default::default()
                });
                continue;
            }
            if drafts.is_empty() {
                // Content before any title continues the previous chunk's
                // recipe when a hint says so; otherwise it is front matter.
                if chunk.title_hint.is_some() {
                    drafts.push(Draft {
                        kind: "recipe",
                        ..Default::default()
                    });
                } else {
                    ignored.push(local);
                    continue;
                }
            }
            let Some(draft) = drafts.last_mut() else {
                continue;
            };
            if draft.ignore {
                ignored.push(local);
            } else if draft.heading {
                draft.description.push(local);
            } else if has(classes, r.title) && starts(r.yield_prefixes) || has(classes, r.yield_) {
                draft.recipe_yield.push(local);
            } else if has(classes, r.section_name) {
                draft.sections.push((vec![local], Vec::new(), Vec::new()));
            } else if has(classes, r.ingredient) {
                draft.section().1.push(local);
            } else if starts(r.note_prefixes) && (has(classes, r.step) || has(classes, r.headnote))
            {
                draft.notes.push(local);
            } else if has(classes, r.step) {
                draft.section().2.push(local);
            } else if has(classes, r.headnote) {
                if r.numbered_steps && text.chars().next().is_some_and(|c| c.is_ascii_digit()) {
                    draft.section().2.push(local);
                } else {
                    draft.description.push(local);
                }
            } else if has(classes, r.note) {
                draft.notes.push(local);
            } else if has(classes, r.equipment) {
                draft.equipment.push(local);
            } else {
                ignored.push(local);
            }
        }

        let mut items = Vec::new();
        for mut d in drafts {
            if d.ignore {
                ignored.extend(d.title.iter().copied());
                continue;
            }
            if d.heading && d.content_lines() == 0 {
                chapter_headings.extend(d.title.iter().copied());
                continue;
            }
            let ingredients: usize = d.sections.iter().map(|s| s.1.len()).sum();
            let steps: usize = d.sections.iter().map(|s| s.2.len()).sum();
            let kind = match d.kind {
                "variation" => {
                    if ingredients == 0 {
                        // Prose-only variation: everything becomes notes.
                        let mut notes = std::mem::take(&mut d.notes);
                        notes.extend(std::mem::take(&mut d.description));
                        for (_, _, s) in &mut d.sections {
                            notes.extend(std::mem::take(s));
                        }
                        d.notes = notes;
                        d.sections.clear();
                    }
                    "variation"
                }
                "essay" => "essay",
                _ if ingredients > 0 => "recipe",
                _ if steps > 0 => "technique",
                _ => "essay",
            };
            let sections: Vec<Value> = d
                .sections
                .iter()
                .map(|(n, i, s)| json!({"name": n, "ingredients": i, "steps": s}))
                .collect();
            items.push(json!({
                "kind": kind,
                "title": d.title,
                "variation_of": d.variation_of,
                "description": d.description,
                "recipe_yield": d.recipe_yield,
                "equipment": d.equipment,
                "notes": d.notes,
                "sections": sections,
            }));
        }
        json!({"items": items, "captions": captions, "chapter_headings": chapter_headings, "ignored": ignored})
    }
}

/// A transport that answers every extraction request with the oracle's
/// payload for the chunk named in the request metadata.
pub fn oracle_transport(oracle: Oracle, book: BookLines, chunks: Vec<Chunk>) -> ScriptedTransport {
    ScriptedTransport::new(move |request, _| {
        let (model_id, chunk_id, _) =
            request_meta(request).ok_or_else(|| TransportError::Other("no metadata".into()))?;
        let route = model(&model_id)
            .map(|m| m.route)
            .unwrap_or(Route::CompatChat);
        let chunk = chunks
            .iter()
            .find(|c| c.id == chunk_id)
            .ok_or_else(|| TransportError::Other(format!("unknown chunk {chunk_id}")))?;
        let payload = oracle.payload(chunk, &book);
        let (input, output) = crate::eta::chunk_tokens(chunk);
        Ok(tool_response(
            route,
            &payload,
            Usage {
                input_tokens: input,
                output_tokens: output,
                ..Usage::default()
            },
            false,
        ))
    })
}
