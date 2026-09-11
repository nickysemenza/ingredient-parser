//! Is this EPUB a cookbook? A structural score answers the clear cases with
//! no model; the ambiguous middle gets one small, cached model call.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::cache::{CachedCall, ChunkCache, cache_key};
use crate::contract::ChunkRequest;
use crate::crosscheck::nav_recipe_titles;
use crate::gateway::{CallMeta, build_http, parse_response};
use crate::models::Model;
use crate::transport::{CancelToken, Transport};
use crate::{Book, Usage};

pub const CLASSIFY_CONTRACT: &str = "cookbook-classify-v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "wasm", derive(tsify_next::Tsify))]
#[serde(rename_all = "snake_case")]
pub enum Classification {
    Cookbook,
    NotCookbook,
    Ambiguous,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "wasm", derive(tsify_next::Tsify))]
pub struct Classified {
    pub classification: Classification,
    /// 0 = surely not, 1 = surely a cookbook.
    pub score: f32,
    /// `structure` or the model id that decided.
    pub method: String,
    pub reasons: Vec<String>,
    pub quantity_lines: usize,
    pub ingredient_runs: usize,
    pub nav_recipe_titles: usize,
}

/// Do the catalog subjects say cooking? Cheap enough for a library scan that
/// never opens the book's text.
pub fn subject_hint(subjects: &[String]) -> bool {
    let subjects = subjects.join(" ").to_lowercase();
    ["cook", "recipe", "food", "baking", "cuisine"]
        .iter()
        .any(|k| subjects.contains(k))
}

/// The offline signal: how much of the text reads as ingredient lists.
pub fn classify_structure(book: &Book) -> Classified {
    let lines = book.lines();
    let total = lines.len().max(1);
    let quantity_lines = (0..lines.len()).filter(|&i| lines.quantity_like(i)).count();
    let ingredient_runs = (0..lines.len())
        .filter(|&i| lines.ingredient_run_start(i))
        .count();
    let nav_titles = nav_recipe_titles(lines, book.nav()).len();
    let ratio = quantity_lines as f32 / total as f32;
    let subject_hint = subject_hint(&book.source().subjects);
    let mut score = (ratio / 0.10).min(1.0) * 0.6 + (ingredient_runs as f32 / 30.0).min(1.0) * 0.3;
    if subject_hint {
        score += 0.1;
    }
    let score = score.min(1.0);
    let mut reasons = vec![
        format!(
            "{quantity_lines} of {total} lines look like quantities ({:.1}%)",
            ratio * 100.0
        ),
        format!("{ingredient_runs} ingredient runs"),
        format!("{nav_titles} contents entries that name recipes"),
    ];
    if subject_hint {
        reasons.push(format!(
            "subjects mention cooking: {}",
            book.source().subjects.join(", ")
        ));
    }
    let classification = if (ratio >= 0.08 && ingredient_runs >= 10) || nav_titles >= 20 {
        Classification::Cookbook
    } else if ratio < 0.02 && ingredient_runs < 3 && nav_titles < 3 {
        Classification::NotCookbook
    } else {
        Classification::Ambiguous
    };
    Classified {
        classification,
        score,
        method: "structure".into(),
        reasons,
        quantity_lines,
        ingredient_runs,
        nav_recipe_titles: nav_titles,
    }
}

/// The model request for an ambiguous book: metadata, contents labels, and a
/// sample of lines.
pub fn classify_request(book: &Book) -> ChunkRequest {
    let source = book.source();
    let labels: Vec<&str> = book
        .nav()
        .entries
        .iter()
        .take(80)
        .map(|e| e.label.as_str())
        .collect();
    let lines = book.lines();
    let step = (lines.len() / 40).max(1);
    let sample: Vec<&str> = (0..lines.len())
        .step_by(step)
        .take(40)
        .map(|i| lines.text(i))
        .collect();
    ChunkRequest {
        system: "Decide whether this book is a cookbook: a book whose main content is recipes with ingredient lists and methods. Reference books about food without recipes, memoirs, and novels are not cookbooks. Answer through the tool.".into(),
        user: format!(
            "Title: {}\nAuthors: {}\nSubjects: {}\n\nContents:\n{}\n\nSample lines:\n{}",
            source.title,
            source.authors.join(", "),
            source.subjects.join(", "),
            labels.join("\n"),
            sample.join("\n")
        ),
        tool_name: "classify".into(),
        tool_schema: json!({
            "type": "object",
            "properties": {
                "is_cookbook": {"type": "boolean"},
                "confidence": {"type": "number", "minimum": 0, "maximum": 1},
                "reason": {"type": "string"}
            },
            "required": ["is_cookbook", "confidence", "reason"],
            "additionalProperties": false
        }),
    }
}

fn from_answer(input: &Value, model: &Model) -> Option<Classified> {
    let is_cookbook = input["is_cookbook"].as_bool()?;
    let confidence = input["confidence"].as_f64().unwrap_or(0.5) as f32;
    let reason = input["reason"].as_str().unwrap_or("").to_string();
    Some(Classified {
        classification: if is_cookbook {
            Classification::Cookbook
        } else {
            Classification::NotCookbook
        },
        score: if is_cookbook {
            confidence
        } else {
            1.0 - confidence
        },
        method: model.id.to_string(),
        reasons: vec![reason],
        quantity_lines: 0,
        ingredient_runs: 0,
        nav_recipe_titles: 0,
    })
}

/// Structure first; a model only for the ambiguous middle. The model's answer
/// is cached by request, so a library rescans for free.
pub async fn classify<T: Transport, C: ChunkCache>(
    book: &Book,
    model: &Model,
    transport: &T,
    cache: &C,
    cancel: &CancelToken,
) -> Classified {
    let structural = classify_structure(book);
    if structural.classification != Classification::Ambiguous {
        return structural;
    }
    let request = classify_request(book);
    let meta = CallMeta {
        cookbook: &book.source().label,
        chunk: "book",
        purpose: "classify",
        gateway_cache: true,
    };
    let http = build_http(model, &request, 400, &meta, model.reasoning);
    let key = cache_key(
        CLASSIFY_CONTRACT,
        model.id,
        model.route.as_str(),
        &http.body,
    );
    let response = match cache.get(&key) {
        Some(hit) => Some(hit.response),
        None => match transport.send(http, cancel).await {
            Ok(r) => {
                if r.is_success() {
                    cache.put(
                        &key,
                        &CachedCall {
                            key: key.clone(),
                            model: model.id.to_string(),
                            contract: CLASSIFY_CONTRACT.to_string(),
                            response: r.clone(),
                            usage: Usage::default(),
                            recorded_at: jiff::Timestamp::now().to_string(),
                        },
                    );
                }
                Some(r)
            }
            Err(_) => None,
        },
    };
    let answered = response
        .and_then(|r| parse_response(model.route, &r).ok())
        .and_then(|call| call.input)
        .and_then(|input| from_answer(&input, model));
    match answered {
        Some(mut c) => {
            c.quantity_lines = structural.quantity_lines;
            c.ingredient_runs = structural.ingredient_runs;
            c.nav_recipe_titles = structural.nav_recipe_titles;
            c.reasons.extend(structural.reasons);
            c
        }
        None => structural,
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::cache::MemoryCache;
    use crate::models::model;
    use crate::test_support::{ScriptedTransport, tool_response};

    #[test]
    fn fixtures_are_cookbooks_and_prose_is_not() {
        let book = Book::open(cookbook_fixtures::cookbook_epub().unwrap(), "synthetic").unwrap();
        let c = classify_structure(&book);
        assert_ne!(c.classification, Classification::NotCookbook, "{c:?}");
        assert!(c.quantity_lines >= 15);
        let prose = cookbook_fixtures::EpubBuilder::new("Novel")
            .doc("c1.xhtml", &format!("<html><body>{}</body></html>", "<p>She walked along the shore, thinking about nothing in particular, and the tide came in.</p>".repeat(40)))
            .build()
            .unwrap();
        let book = Book::open(prose, "novel").unwrap();
        let c = classify_structure(&book);
        assert_eq!(c.classification, Classification::NotCookbook, "{c:?}");
    }

    #[tokio::test]
    async fn ambiguous_books_ask_once_and_cache_the_answer() {
        // A book with a few quantities but no runs: ambiguous by structure.
        let mut body = String::new();
        for i in 0..60 {
            body.push_str(&format!("<p>Paragraph {i} about the history of bread and how it shaped cities and trade.</p>"));
            if i % 10 == 0 {
                body.push_str("<p>3 cups flour</p>");
            }
        }
        let bytes = cookbook_fixtures::EpubBuilder::new("Bread History")
            .doc("c1.xhtml", &format!("<html><body>{body}</body></html>"))
            .build()
            .unwrap();
        let book = Book::open(bytes, "history").unwrap();
        assert_eq!(
            classify_structure(&book).classification,
            Classification::Ambiguous
        );
        let m = model("gemini-2.5-flash").unwrap();
        let transport = ScriptedTransport::new(|_, _| {
            Ok(tool_response(
                crate::gateway::Route::CompatChat,
                &json!({"is_cookbook": false, "confidence": 0.9, "reason": "a history"}),
                Usage::default(),
                false,
            ))
        });
        let cache = MemoryCache::default();
        let c = classify(&book, m, &transport, &cache, &CancelToken::new()).await;
        assert_eq!(c.classification, Classification::NotCookbook);
        assert_eq!(c.method, "gemini-2.5-flash");
        assert!(c.reasons[0].contains("history"));
        let again = classify(&book, m, &transport, &cache, &CancelToken::new()).await;
        assert_eq!(again.classification, Classification::NotCookbook);
        assert_eq!(transport.calls(), 1, "answered from the cache");
    }
}
