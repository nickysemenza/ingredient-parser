//! Portable application services: the one implementation behind both the
//! desktop app (called through Tauri) and the web build (called through WASM in
//! a worker). Every command takes and returns the serde types declared here, so
//! the frontend sees the same JSON from either host.
//!
//! A [`Host`] supplies what differs between hosts — where file bytes come from
//! and which saved runs exist. [`dispatch`] is the shared command table.

pub mod bindings;
pub mod books;
pub mod corpus;
pub mod ingredients;
pub mod recipes;

use cookbook::RunSummary;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

pub type AppResult<T> = Result<T, String>;

/// What a host provides. Paths are opaque keys: real files on the desktop,
/// names of files the user opened in the browser.
pub trait Host {
    /// The bytes of a file.
    fn read(&self, path: &str) -> AppResult<Vec<u8>>;
    /// A value that changes whenever the file's content may have changed.
    fn stamp(&self, path: &str) -> AppResult<String>;
    /// Saved runs of the book with this content hash, newest first.
    fn runs_for(&self, sha256: &str) -> AppResult<Vec<RunSummary>>;
}

fn args<T: DeserializeOwned>(command: &str, value: Value) -> AppResult<T> {
    serde_json::from_value(value).map_err(|e| format!("Invalid arguments for {command}: {e}"))
}

fn reply<T: serde::Serialize>(value: AppResult<T>) -> AppResult<Value> {
    serde_json::to_value(value?).map_err(|e| e.to_string())
}

#[derive(Deserialize)]
struct Input {
    input: String,
}
#[derive(Deserialize)]
struct RichText {
    text: String,
    names: Vec<String>,
}
#[derive(Deserialize)]
struct Html {
    html: String,
    url: String,
}
#[derive(Deserialize)]
struct Scale {
    source: Value,
    factor: f64,
}
#[derive(Deserialize)]
struct MaybePath {
    path: Option<String>,
}
#[derive(Deserialize)]
struct Path {
    path: String,
}
#[derive(Deserialize)]
struct Image {
    book: String,
    image: String,
}
#[derive(Deserialize)]
struct Range {
    path: String,
    start: usize,
    end: usize,
}

/// The portable commands, by name. Hosts register this table once; command
/// names and argument keys match the frontend's `api` module.
pub const COMMANDS: &[&str] = &[
    "parse_lines",
    "inspect_ingredient",
    "rich_text",
    "recipe_from_html",
    "scale_recipe",
    "score_corpus",
    "open_book",
    "book_image",
    "book_cover",
    "book_source",
    "open_run",
];

/// Run one portable command. `args` is the command's argument object.
pub fn dispatch(host: &dyn Host, command: &str, value: Value) -> AppResult<Value> {
    match command {
        "parse_lines" => {
            let a: Input = args(command, value)?;
            reply(Ok(ingredients::parse_lines(&a.input)))
        }
        "inspect_ingredient" => {
            let a: Input = args(command, value)?;
            reply(Ok(ingredients::inspect(&a.input)))
        }
        "rich_text" => {
            let a: RichText = args(command, value)?;
            reply(Ok(recipes::rich_text(&a.text, a.names)))
        }
        "recipe_from_html" => {
            let a: Html = args(command, value)?;
            reply(recipes::from_html(&a.html, &a.url))
        }
        "scale_recipe" => {
            let a: Scale = args(command, value)?;
            reply(recipes::scale(a.source, a.factor))
        }
        "score_corpus" => {
            let a: MaybePath = args(command, value)?;
            reply(corpus::score(host, a.path))
        }
        "open_book" => {
            let a: Path = args(command, value)?;
            reply(books::open(host, &a.path))
        }
        "book_image" => {
            let a: Image = args(command, value)?;
            reply(books::image(host, &a.book, &a.image))
        }
        "book_cover" => {
            let a: Path = args(command, value)?;
            reply(books::cover(host, &a.path))
        }
        "book_source" => {
            let a: Range = args(command, value)?;
            reply(books::source(host, &a.path, a.start, a.end))
        }
        "open_run" => {
            let a: Path = args(command, value)?;
            reply(books::open_run(host, &a.path))
        }
        other => Err(format!("Unknown command {other}")),
    }
}

#[cfg(test)]
pub(crate) mod test_host {
    use super::*;
    use std::collections::HashMap;

    /// Files in memory, as the browser host keeps them.
    #[derive(Default)]
    pub struct MemoryHost {
        pub files: HashMap<String, Vec<u8>>,
    }

    impl Host for MemoryHost {
        fn read(&self, path: &str) -> AppResult<Vec<u8>> {
            self.files
                .get(path)
                .cloned()
                .ok_or_else(|| format!("{path} is not open"))
        }
        fn stamp(&self, path: &str) -> AppResult<String> {
            self.read(path).map(|b| b.len().to_string())
        }
        fn runs_for(&self, _: &str) -> AppResult<Vec<RunSummary>> {
            Ok(vec![])
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use serde_json::json;

    #[test]
    fn every_listed_command_is_dispatched() {
        let host = test_host::MemoryHost::default();
        for command in COMMANDS {
            let error = dispatch(&host, command, Value::Null).unwrap_err();
            assert!(error.starts_with("Invalid arguments"), "{command}: {error}");
        }
        assert_eq!(
            dispatch(&host, "nope", json!({})).unwrap_err(),
            "Unknown command nope"
        );
    }
}
