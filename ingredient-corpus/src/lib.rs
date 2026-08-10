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
/// line handling rather than reimplementing it. Crate-internal: the two row
/// shapes that exist have their own entry points.
///
/// Blank lines are skipped. A `// --- Name ---` line opens a section; any other
/// `//` line is an ordinary comment. Everything else is a row.
pub(crate) fn parse_as<T: DeserializeOwned>(source: &str) -> Corpus<T> {
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

/// How one corpus row scored.
///
/// `xfail` NEVER changes how fields are compared — only how a mismatch is
/// named. `Exact`/`Regression` are committed rows; `Promote`/`Xfail` are xfail
/// rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// Committed row, all five fields agreed.
    Exact,
    /// Committed row, something disagreed. The only status that fails CI.
    Regression,
    /// xfail row, still failing — the gap it documents is still open.
    Xfail,
    /// xfail row that now passes; the marker can be removed.
    Promote,
}

impl Status {
    pub fn label(self) -> &'static str {
        match self {
            Status::Exact => "EXACT",
            Status::Regression => "REGRESSION",
            Status::Xfail => "XFAIL",
            Status::Promote => "PROMOTE",
        }
    }
}

/// The five fields a corpus row labels, in corpus order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LabeledField {
    Name,
    Amounts,
    Modifier,
    Optional,
    Usage,
}

impl LabeledField {
    pub fn as_str(self) -> &'static str {
        match self {
            LabeledField::Name => "name",
            LabeledField::Amounts => "amounts",
            LabeledField::Modifier => "modifier",
            LabeledField::Optional => "optional",
            LabeledField::Usage => "usage",
        }
    }
}

/// One field's expected-vs-actual comparison, pre-rendered for reporting.
#[derive(Debug, Clone)]
pub struct FieldDiff {
    pub field: LabeledField,
    pub ok: bool,
    pub want: String,
    pub got: String,
}

/// A scored row: how it classified, and the per-field diff.
///
/// Deliberately does not carry the row or the parsed `Ingredient` back: both
/// were here "so a detail view costs nothing extra", and neither consumer ever
/// read them — the detail view uses the pre-rendered `want`/`got` strings, and
/// callers already hold the row they passed in. Dropping them also drops the
/// lifetime parameter.
#[derive(Debug, Clone)]
pub struct Scored {
    pub status: Status,
    pub fields: [FieldDiff; 5],
}

impl Scored {
    /// Only the fields that disagreed, in corpus order.
    pub fn mismatches(&self) -> impl Iterator<Item = &FieldDiff> {
        self.fields.iter().filter(|d| !d.ok)
    }
}

/// Score a row against `ingredient::from_str` — what every consumer does today.
pub fn score(row: &CorpusRow) -> Scored {
    let got = ingredient::from_str(&row.input);

    let diff = |field: LabeledField, ok: bool, want: String, got: String| FieldDiff {
        field,
        ok,
        want,
        got,
    };
    let fields = [
        diff(
            LabeledField::Name,
            got.name == row.name,
            format!("{:?}", row.name),
            format!("{:?}", got.name),
        ),
        diff(
            LabeledField::Amounts,
            got.amounts == row.amounts,
            format!("[{}]", render_parsed(&row.amounts)),
            format!("[{}]", render_parsed(&got.amounts)),
        ),
        diff(
            LabeledField::Modifier,
            got.modifier == row.modifier,
            format!("{:?}", row.modifier),
            format!("{:?}", got.modifier),
        ),
        diff(
            LabeledField::Optional,
            got.optional == row.optional,
            row.optional.to_string(),
            got.optional.to_string(),
        ),
        diff(
            LabeledField::Usage,
            got.usage == row.usage,
            format!("{:?}", row.usage),
            format!("{:?}", got.usage),
        ),
    ];

    let all_ok = fields.iter().all(|d| d.ok);
    let status = match (all_ok, row.xfail.is_some()) {
        (true, false) => Status::Exact,
        (true, true) => Status::Promote,
        (false, false) => Status::Regression,
        (false, true) => Status::Xfail,
    };

    Scored { status, fields }
}

/// Running counts over a scored corpus.
#[derive(Debug, Clone, Default)]
pub struct Tally {
    pub total: usize,
    pub exact: usize,
    pub regression: usize,
    pub xfail: usize,
    pub promote: usize,
    /// Per-field match counts, in [`LabeledField`] declaration order.
    pub per_field: [usize; 5],
}

impl Tally {
    pub fn add(&mut self, scored: &Scored) {
        self.total += 1;
        match scored.status {
            Status::Exact => self.exact += 1,
            Status::Regression => self.regression += 1,
            Status::Xfail => self.xfail += 1,
            Status::Promote => self.promote += 1,
        }
        for (slot, diff) in self.per_field.iter_mut().zip(&scored.fields) {
            *slot += diff.ok as usize;
        }
    }

    /// Rows where all five fields agreed — `exact + promote`, NOT `exact`.
    ///
    /// This is the headline "exact matches" number, and the distinction is a
    /// live footgun: a passing xfail row is a *match* even though its status is
    /// `Promote`. With zero xfail rows in the corpus today the two are equal, so
    /// getting it wrong would move the project's north-star metric silently and
    /// no test would notice. Hence this method rather than reading `.exact`.
    pub fn matched(&self) -> usize {
        self.exact + self.promote
    }
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
/// same transformation; `score` pre-renders with it, so no caller needs it
/// directly. It is deliberately NOT how the corpus file itself is
/// displayed — see `render_authored` in the corpus-table renderer, and the
/// `divergent_lenses` test that pins the difference.
pub(crate) fn render_parsed(amounts: &[Measure]) -> String {
    amounts
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

/// Render amounts the way the CORPUS FILE AUTHORED them: the literal unit with
/// no pluralization, an EN DASH range, no unit suffix for a bare count, and the
/// exact fraction spelling when the value is a non-terminating rational.
///
/// This is the lens for *viewing* the corpus, where re-spelling a human's row
/// is a bug. It must not be merged with [`render_parsed`]: on today's corpus
/// they differ four ways — range punctuation, fraction glyphs, unit
/// denormalization (`30 tsp` → `⅝ cup`) and pluralization. See the
/// `divergent_lenses` test, which pins both sides.
pub fn render_authored(amounts: &[Measure]) -> String {
    amounts
        .iter()
        .map(|m| {
            let unit = m.unit().to_str();
            let value = authored_value(m.value_as_fraction_str(), m.value());
            let qty = match m.upper_value() {
                Some(upper) => {
                    let upper = authored_value(m.upper_value_as_fraction_str(), upper);
                    format!("{value}–{upper}")
                }
                None => value,
            };
            if unit.is_empty() || unit == "whole" {
                qty
            } else {
                format!("{qty} {unit}")
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// One authored value: the exact fraction string when the rational is
/// non-terminating (`"2/3"`), else the number exactly as `f64`'s `Display`
/// writes it — `2` not `2.0`, but `0.125` in full.
///
/// Deliberately NOT [`ingredient::util::num_without_zeroes`], which rounds to
/// two decimals: that is a *display* helper, and using it here silently
/// rewrote `0.125 tsp` as `0.12 tsp` in the corpus viewer. The viewer must
/// show what the file says.
fn authored_value(fraction: Option<String>, val: f64) -> String {
    match fraction {
        Some(s) => s,
        None => format!("{val}"),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn measures(json: &str) -> Vec<Measure> {
        serde_json::from_str(json).unwrap()
    }

    /// Two lenses on the same measures, deliberately different, and this test
    /// exists so that stays true.
    ///
    /// `render_authored` shows the corpus AS WRITTEN — it backs the corpus
    /// viewer, where re-spelling a human's row is a bug. `render_parsed` shows
    /// it as the parser would print it — it backs got-vs-want diffs, where both
    /// sides must go through the same transformation.
    ///
    /// Merging them would silently re-spell 42 range rows, 9 fraction rows, and
    /// every row whose unit denormalizes.
    #[test]
    fn divergent_lenses() {
        let range = measures(r#"[{"unit":"cup","value":2,"upper_value":3}]"#);
        assert_eq!(render_authored(&range), "2–3 cup"); // EN DASH, literal unit
        assert_eq!(render_parsed(&range), "2 - 3 cups"); // ASCII, pluralized

        let frac = measures(r#"[{"unit":"cup","value":"2/3"}]"#);
        assert_eq!(render_authored(&frac), "2/3 cup"); // ASCII fraction
        assert_eq!(render_parsed(&frac), "⅔ cup"); // vulgar glyph

        // corpus.jsonl authors teaspoons; Display denormalizes them into cups.
        let tsp30 = measures(r#"[{"unit":"tsp","value":30}]"#);
        assert_eq!(render_authored(&tsp30), "30 tsp");
        assert_eq!(render_parsed(&tsp30), "⅝ cup");

        // A bare count agrees — by design, not by coincidence.
        let whole = measures(r#"[{"unit":"whole","value":2}]"#);
        assert_eq!(render_authored(&whole), "2");
        assert_eq!(render_parsed(&whole), "2");
    }

    /// Characterization snapshot over every real corpus row that has amounts.
    /// It does not assert the spelling is *right* — it asserts it does not
    /// *change*, which is what makes swapping the corpus-table renderer onto
    /// `render_authored` provably a no-op.
    #[test]
    fn authored_rendering_over_the_real_corpus() {
        let corpus = parse(embedded());
        let lines: Vec<String> = corpus
            .entries
            .iter()
            .filter_map(|e| e.parsed.as_ref().ok().map(|r| (e.line_no, r)))
            .filter(|(_, r)| !r.amounts.is_empty())
            .map(|(line_no, r)| format!("{line_no}\t{}\t{}", r.input, render_authored(&r.amounts)))
            .collect();
        insta::assert_snapshot!(lines.join("\n"));
    }

    fn row(json: &str) -> CorpusRow {
        serde_json::from_str(json).unwrap()
    }

    /// The four status arms. Only `Exact` and `Regression` are reachable from
    /// the real corpus (it has zero xfail rows), so `Xfail` and `Promote` would
    /// rot without these — and they are exactly the pair the headline count
    /// depends on. Ported from food-app, which had the only copy.
    #[test]
    fn status_arms() {
        let committed =
            r#"{"input":"2 cups flour","name":"flour","amounts":[{"unit":"cup","value":2}]}"#;
        assert_eq!(score(&row(committed)).status, Status::Exact);

        let wrong =
            r#"{"input":"2 cups flour","name":"WRONG","amounts":[{"unit":"cup","value":2}]}"#;
        assert_eq!(score(&row(wrong)).status, Status::Regression);

        let xfail_failing = r#"{"input":"2 cups flour","name":"WRONG","xfail":"gap"}"#;
        assert_eq!(score(&row(xfail_failing)).status, Status::Xfail);

        let xfail_passing = r#"{"input":"2 cups flour","name":"flour","amounts":[{"unit":"cup","value":2}],"xfail":"gap"}"#;
        assert_eq!(score(&row(xfail_passing)).status, Status::Promote);
    }

    /// `xfail` reclassifies; it must never change which fields compare equal.
    #[test]
    fn xfail_does_not_change_field_comparison() {
        let without =
            r#"{"input":"2 cups flour","name":"WRONG","amounts":[{"unit":"cup","value":2}]}"#;
        let with = r#"{"input":"2 cups flour","name":"WRONG","amounts":[{"unit":"cup","value":2}],"xfail":"gap"}"#;
        let (without, with) = (row(without), row(with));
        let a = score(&without);
        let b = score(&with);
        let oks_a: Vec<bool> = a.fields.iter().map(|d| d.ok).collect();
        let oks_b: Vec<bool> = b.fields.iter().map(|d| d.ok).collect();
        assert_eq!(oks_a, oks_b);
        assert_ne!(a.status, b.status);
    }

    /// The headline count is `exact + promote`, not `exact`. A passing xfail row
    /// is a match. Nothing in the real corpus exercises this today, which is
    /// precisely why it needs a test.
    #[test]
    fn matched_counts_promoted_rows() {
        let exact =
            row(r#"{"input":"2 cups flour","name":"flour","amounts":[{"unit":"cup","value":2}]}"#);
        let promote = row(
            r#"{"input":"1 teaspoon salt","name":"salt","amounts":[{"unit":"tsp","value":1}],"xfail":"gap"}"#,
        );
        let regression = row(r#"{"input":"2 cups flour","name":"WRONG"}"#);

        let mut tally = Tally::default();
        for r in [&exact, &promote, &regression] {
            tally.add(&score(r));
        }

        assert_eq!(tally.total, 3);
        assert_eq!(tally.exact, 1);
        assert_eq!(tally.promote, 1);
        assert_eq!(tally.regression, 1);
        assert_eq!(tally.matched(), 2, "a passing xfail row is still a match");
        assert_ne!(tally.matched(), tally.exact);
    }

    /// Line handling: comments and blanks dropped, `// --- Name ---` headers
    /// opening sections, malformed lines kept rather than panicked on. Ported
    /// from the corpus-table renderer, which used to own this logic.
    #[test]
    fn skips_comments_and_tracks_sections() {
        const SAMPLE: &str = r#"// header comment, ignored
//
// --- basics ---
{"input": "2 cups flour", "name": "flour"}

{"input": "2-3 cups broth", "name": "broth"}
// --- gaps ---
{"input": "1 pint berries", "name": "berries", "xfail": "pint range"}
not valid json
"#;
        let corpus = parse(SAMPLE);
        // 3 valid rows + 1 malformed = 4 entries; comments and blanks dropped.
        assert_eq!(corpus.entries.len(), 4);
        assert_eq!(corpus.sections, ["(ungrouped)", "basics", "gaps"]);

        let section_of = |i: usize| corpus.sections[corpus.entries[i].section].as_str();
        assert_eq!(section_of(0), "basics");
        assert_eq!(section_of(1), "basics");
        assert_eq!(section_of(2), "gaps");
        assert_eq!(corpus.problems().count(), 1);

        // by_section groups consecutive runs, so the viewer gets one heading each.
        let groups: Vec<(&str, usize)> = corpus.by_section().map(|(n, e)| (n, e.len())).collect();
        assert_eq!(groups, [("basics", 2), ("gaps", 2)]);
    }

    /// The corpus file's own shape, so a loader drift breaks loudly and in one
    /// place rather than as a quietly shorter corpus.
    #[test]
    fn corpus_file_shape() {
        let corpus = parse(embedded());
        assert_eq!(corpus.problems().count(), 0, "corpus has malformed rows");
        assert!(corpus.rows().count() > 400, "corpus unexpectedly small");
        assert_eq!(
            corpus.rows().filter(|r| r.xfail.is_some()).count(),
            0,
            "corpus has xfail rows; update this test if that becomes intended"
        );
        assert_eq!(corpus.sections[0], "(ungrouped)");
        assert!(corpus.sections.len() > 30, "section headers not picked up");
        for entry in &corpus.entries {
            assert!(
                corpus.sections.get(entry.section).is_some(),
                "entry {} has an unresolvable section index",
                entry.line_no
            );
        }
    }

    /// Amount equality at this layer is exact rational, never f64: a fraction
    /// string and the f64 that recovers to the same rational are equal, a
    /// truncated decimal is not, and a quoted decimal is rejected outright.
    #[test]
    fn exact_rationals_at_the_corpus_boundary() {
        let from_fraction = measures(r#"[{"unit":"cup","value":"2/3"}]"#);
        let from_number = measures(r#"[{"unit":"cup","value":0.6666666666666666}]"#);
        assert_eq!(from_fraction, from_number);

        let truncated = measures(r#"[{"unit":"cup","value":0.667}]"#);
        assert_ne!(from_fraction, truncated);

        let quoted_decimal = parse(r#"{"input":"x","amounts":[{"unit":"cup","value":"0.667"}]}"#);
        assert_eq!(
            quoted_decimal.problems().count(),
            1,
            "a quoted decimal must be rejected, not silently reinterpreted"
        );
    }

    /// `inputs`, `read` and `Status::label` are reached only from `food-cli` /
    /// `food-app`, which the coverage job excludes; `LabeledField::as_str`,
    /// `mismatches` and the `Xfail` tally arm are reached only when a row FAILS,
    /// which a healthy corpus never does. All of them run in production — so
    /// they get tests here rather than an exclusion.
    #[test]
    fn inputs_returns_the_lines_in_file_order() {
        let corpus = parse(
            "// --- s ---\n{\"input\":\"2 cups flour\"}\nnot json\n{\"input\":\"1 tsp salt\"}\n",
        );
        assert_eq!(corpus.inputs(), ["2 cups flour", "1 tsp salt"]);
    }

    #[test]
    fn read_parses_a_file_from_disk() {
        let path = std::env::temp_dir().join(format!(
            "ingredient-corpus-read-{}.jsonl",
            std::process::id()
        ));
        std::fs::write(&path, "{\"input\":\"2 cups flour\",\"name\":\"flour\"}\n").unwrap();
        let corpus = read(&path).unwrap();
        let _ = std::fs::remove_file(&path);

        assert_eq!(corpus.rows().count(), 1);
        assert_eq!(corpus.inputs(), ["2 cups flour"]);
        assert!(read(std::path::Path::new("/definitely/not/here.jsonl")).is_err());
    }

    #[test]
    fn status_and_field_labels() {
        assert_eq!(Status::Exact.label(), "EXACT");
        assert_eq!(Status::Regression.label(), "REGRESSION");
        assert_eq!(Status::Xfail.label(), "XFAIL");
        assert_eq!(Status::Promote.label(), "PROMOTE");

        let labels: Vec<&str> = [
            LabeledField::Name,
            LabeledField::Amounts,
            LabeledField::Modifier,
            LabeledField::Optional,
            LabeledField::Usage,
        ]
        .iter()
        .map(|f| f.as_str())
        .collect();
        assert_eq!(labels, ["name", "amounts", "modifier", "optional", "usage"]);
    }

    /// `mismatches` yields only the fields that disagreed, in corpus order —
    /// the report line a regression prints.
    #[test]
    fn mismatches_lists_only_failing_fields() {
        let clean =
            row(r#"{"input":"2 cups flour","name":"flour","amounts":[{"unit":"cup","value":2}]}"#);
        assert_eq!(score(&clean).mismatches().count(), 0);

        let wrong = row(r#"{"input":"2 cups flour","name":"WRONG"}"#);
        let scored = score(&wrong);
        let failed: Vec<&str> = scored.mismatches().map(|d| d.field.as_str()).collect();
        assert_eq!(failed, ["name", "amounts"]);
        // The rendered sides are what the report prints verbatim.
        let name_diff = scored.mismatches().next().unwrap();
        assert_eq!(name_diff.want, "\"WRONG\"");
        assert_eq!(name_diff.got, "\"flour\"");
    }

    /// The `Xfail` arm of the tally — unreachable from the real corpus, which
    /// has no xfail rows.
    #[test]
    fn tally_counts_a_still_failing_xfail_row() {
        let gap = row(r#"{"input":"2 cups flour","name":"WRONG","xfail":"known gap"}"#);
        let mut tally = Tally::default();
        tally.add(&score(&gap));
        assert_eq!(tally.xfail, 1);
        assert_eq!(tally.regression, 0);
        assert_eq!(tally.matched(), 0);
    }
}
