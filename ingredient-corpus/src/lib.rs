//! The accuracy corpus: its schema, its file format, and how a parse is scored
//! against it.
//!
//! `ingredient-parser/tests/corpus/corpus.jsonl` is the regression ratchet that
//! governs whether a parser change ships. Before this crate existed the row
//! shape was declared three times, the loader five times and the scoring rule
//! three times, across three crates — each copy carrying a comment claiming to
//! mirror `tests/accuracy.rs`, which was not callable from any of them. This is
//! the one place that knows what a corpus row is.
//!
//! The loader never panics, never exits and never returns `Err` for a malformed
//! row: a bad line becomes an [`Entry`] carrying its [`Problem`], so each caller
//! picks its own policy. The accuracy test panics, the CLI collects and reports,
//! the viewer renders the bad line inline, and the GUI shows an error banner —
//! four policies over one parse.
//!
//! ## Amount equality is exact
//!
//! `value` may be authored as a JSON number or an exact fraction string
//! (`"2/3"`, `"1 1/2"`); both land on the same [`Rational64`]-backed
//! [`Measure`], and comparison is exact rational equality, never `f64`. A
//! truncated decimal (`0.667`) is a different value, and a *quoted* decimal
//! (`"0.667"`) is rejected outright. Nothing in this crate compares floats.
//!
//! [`Rational64`]: https://docs.rs/num-rational/latest/num_rational/type.Rational64.html

use ingredient::{IngredientUsage, unit::Measure};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use std::path::Path;

/// Where the corpus lives relative to the workspace root — for a CLI `--corpus`
/// default or a GUI path field, which resolve against the process's cwd.
pub const CORPUS_RELATIVE_PATH: &str = "ingredient-parser/tests/corpus/corpus.jsonl";

/// Rich-text sibling of [`CORPUS_RELATIVE_PATH`].
pub const RICH_CORPUS_RELATIVE_PATH: &str = "ingredient-parser/tests/corpus/rich_text.jsonl";

/// This checkout's absolute path to the corpus, resolved at compile time.
pub fn source_path() -> &'static Path {
    Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../ingredient-parser/tests/corpus/corpus.jsonl"
    ))
}

/// This checkout's absolute path to the rich-text corpus.
pub fn rich_source_path() -> &'static Path {
    Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../ingredient-parser/tests/corpus/rich_text.jsonl"
    ))
}

/// The corpus compiled into the binary. The only `include_str!` of the corpus
/// in the workspace; rustc records the dependency, so editing the file triggers
/// a rebuild.
pub fn embedded() -> &'static str {
    include_str!("../../ingredient-parser/tests/corpus/corpus.jsonl")
}

/// The rich-text corpus compiled into the binary.
pub fn embedded_rich() -> &'static str {
    include_str!("../../ingredient-parser/tests/corpus/rich_text.jsonl")
}

/// One labeled corpus row: an ingredient line and the parse it must produce.
///
/// `xfail` documents a known gap — the labels describe the parse we *want*, and
/// a mismatch is reported but tolerated. It never changes how fields are
/// compared, only how a mismatch is named.
#[derive(Debug, Clone, Deserialize)]
pub struct CorpusRow {
    pub input: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub amounts: Vec<Measure>,
    #[serde(default)]
    pub modifier: Option<String>,
    #[serde(default)]
    pub optional: bool,
    /// Expected usage classification. Absent means `Normal` — a corpus-side
    /// ergonomic default only; the `Ingredient.usage` field itself has none.
    #[serde(default)]
    pub usage: IngredientUsage,
    /// When set, documents a known parser gap; the string explains it.
    #[serde(default)]
    pub xfail: Option<String>,
}

/// A line that did not deserialize, kept rather than dropped so a caller can
/// report it in place instead of silently rendering a short corpus.
#[derive(Debug, Clone)]
pub struct Problem {
    /// The offending line, trimmed, verbatim.
    pub line: String,
    /// Serde's message.
    pub message: String,
}

/// One non-blank, non-comment line of the corpus, well-formed or not.
#[derive(Debug, Clone)]
pub struct Entry<T> {
    /// 1-based line number in the source file.
    pub line_no: usize,
    /// Index into [`Corpus::sections`].
    pub section: usize,
    pub parsed: Result<T, Problem>,
}

/// A parsed corpus file: its section headers and every row in file order.
#[derive(Debug, Clone)]
pub struct Corpus<T = CorpusRow> {
    /// Section names in file order, introduced by `// --- Name ---` headers.
    /// Index 0 is always `"(ungrouped)"` — the rows before the first header.
    pub sections: Vec<String>,
    pub entries: Vec<Entry<T>>,
}

impl<T> Corpus<T> {
    /// The well-formed rows, in file order.
    pub fn rows(&self) -> impl Iterator<Item = &T> {
        self.entries.iter().filter_map(|e| e.parsed.as_ref().ok())
    }

    /// The malformed lines, in file order. Empty on a healthy corpus.
    pub fn problems(&self) -> impl Iterator<Item = (&Entry<T>, &Problem)> {
        self.entries
            .iter()
            .filter_map(|e| e.parsed.as_ref().err().map(|p| (e, p)))
    }

    /// Consecutive runs of entries grouped by section, preserving file order.
    /// The section index is monotonic, so these are subslices, not copies. A
    /// section name repeated later in the file yields a second group, matching
    /// how the file reads.
    pub fn by_section(&self) -> impl Iterator<Item = (&str, &[Entry<T>])> {
        let mut start = 0usize;
        std::iter::from_fn(move || {
            let first = self.entries.get(start)?;
            let mut end = start + 1;
            while self
                .entries
                .get(end)
                .is_some_and(|e| e.section == first.section)
            {
                end += 1;
            }
            let name = self
                .sections
                .get(first.section)
                .map_or("(ungrouped)", String::as_str);
            let group = &self.entries[start..end];
            start = end;
            Some((name, group))
        })
    }
}

impl Corpus<CorpusRow> {
    /// Just the input lines — what the stage-coverage lint feeds the parser.
    pub fn inputs(&self) -> Vec<String> {
        self.rows().map(|r| r.input.clone()).collect()
    }
}

/// Parse corpus text into rows.
pub fn parse(source: &str) -> Corpus<CorpusRow> {
    parse_as(source)
}

/// Parse corpus text into rows of any shape — the rich-text corpus shares this
/// line handling rather than reimplementing it.
///
/// Blank lines are skipped. A `// --- Name ---` line opens a section; any other
/// `//` line is an ordinary comment. Everything else is a row.
pub fn parse_as<T: DeserializeOwned>(source: &str) -> Corpus<T> {
    let mut sections = vec!["(ungrouped)".to_string()];
    let mut current = 0usize;
    let mut entries = Vec::new();

    for (idx, raw) in source.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix("//") {
            // A `// --- Section name --- ` header opens a section. Some headers
            // use four dashes and wrap onto continuation `//` lines; trimming
            // both ends handles the first and ignores the rest.
            if let Some(inner) = rest.trim().strip_prefix("---") {
                let name = inner.trim_end_matches('-').trim();
                if !name.is_empty() {
                    sections.push(name.to_string());
                    current = sections.len() - 1;
                }
            }
            continue;
        }
        let parsed = serde_json::from_str::<T>(line).map_err(|e| Problem {
            line: line.to_string(),
            message: e.to_string(),
        });
        entries.push(Entry {
            line_no: idx + 1,
            section: current,
            parsed,
        });
    }

    Corpus { sections, entries }
}

/// Read a corpus file and parse it. The only failure mode is IO — a malformed
/// row is an [`Entry`] carrying a [`Problem`], not an error.
pub fn read(path: &Path) -> std::io::Result<Corpus<CorpusRow>> {
    Ok(parse(&std::fs::read_to_string(path)?))
}

/// The rich-text corpus: instruction prose and the chunk sequence the
/// highlighter must produce for it.
///
/// A different row shape and a different scoring rule (chunk-sequence equality,
/// not per-field comparison), but the same file format — so it shares
/// [`parse_as`] rather than becoming a sixth copy of the line handling.
pub mod rich {
    use ingredient::rich_text::Chunk;
    use ingredient::unit::Measure;
    use serde::Deserialize;

    /// One expected chunk, disambiguated by its key: `{"text": …}`,
    /// `{"measure": [...]}`, or `{"ing": …}`.
    #[derive(Debug, Clone, Deserialize)]
    #[serde(untagged)]
    pub enum ExpectedChunk {
        Measure { measure: Vec<Measure> },
        Ing { ing: String },
        Text { text: String },
    }

    impl From<ExpectedChunk> for Chunk {
        fn from(e: ExpectedChunk) -> Self {
            match e {
                ExpectedChunk::Measure { measure } => Chunk::Measure(measure),
                ExpectedChunk::Ing { ing } => Chunk::Ing(ing),
                ExpectedChunk::Text { text } => Chunk::Text(text),
            }
        }
    }

    #[derive(Debug, Clone, Deserialize)]
    pub struct RichRow {
        pub input: String,
        /// Ingredient names to highlight as `Ing` chunks (default: none).
        #[serde(default)]
        pub ingredients: Vec<String>,
        pub chunks: Vec<ExpectedChunk>,
        /// When set, documents a known gap: a mismatch is reported, not failed.
        #[serde(default)]
        pub xfail: Option<String>,
    }

    /// Parse rich-text corpus text.
    pub fn parse(source: &str) -> super::Corpus<RichRow> {
        super::parse_as(source)
    }
}

/// Render amounts the way the PARSER prints them: `Measure`'s `Display`, which
/// denormalizes units (`30 tsp` → `⅝ cup`), uses vulgar-fraction glyphs, spells
/// ranges `X - Y`, and pluralizes unit words.
///
/// This is the lens for got-vs-want diffs, where both sides must go through the
/// same transformation. It is deliberately NOT how the corpus file itself is
/// displayed — see `render_authored` in the corpus-table renderer, and the
/// `divergent_lenses` test that pins the difference.
pub fn render_parsed(amounts: &[Measure]) -> String {
    amounts
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}
