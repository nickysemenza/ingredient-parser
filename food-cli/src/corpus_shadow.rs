//! `corpus shadow` — A/B harness for the clause-segmentation migration.
//!
//! Runs every corpus row through BOTH parse paths (legacy carve-then-repair vs
//! clause segmentation, see `ingredient::SegmentationMode`) and reports:
//!
//! - **(a) divergences** — committed `corpus.jsonl` rows where the two paths
//!   disagree (full field diff). These gate the cutover: the exit code is the
//!   number of committed-row divergences (capped at 100), so CI/scripts can
//!   ratchet on zero.
//! - **(b) improvements** — rows (committed or xfail) where the segmented
//!   output matches the human label but the legacy output does not.
//!
//! `rich_text.jsonl` inputs are also A/B'd informationally: the rich-text
//! parser itself only borrows the unit set (segmentation cannot affect
//! `RichParser`), but its `input` lines are still real text both ingredient
//! paths must agree on. They do not count toward the exit code.

use ingredient::{Ingredient, IngredientParser, IngredientUsage, SegmentationMode, unit::Measure};
use serde::Deserialize;

/// One corpus row: the input plus its human label (see `tests/accuracy.rs`,
/// which this mirrors).
#[derive(Deserialize)]
struct CorpusRow {
    input: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    amounts: Vec<Measure>,
    #[serde(default)]
    modifier: Option<String>,
    #[serde(default)]
    optional: bool,
    #[serde(default)]
    usage: IngredientUsage,
    #[serde(default)]
    xfail: Option<String>,
}

impl CorpusRow {
    /// Does a parse match this row's label on every labeled field?
    fn label_matches(&self, got: &Ingredient) -> bool {
        got.name == self.name
            && got.amounts == self.amounts
            && got.modifier == self.modifier
            && got.optional == self.optional
            && got.usage == self.usage
    }
}

/// Load corpus rows, skipping blanks and `//` comments (the accuracy.rs
/// convention). Malformed rows abort: a broken corpus makes the A/B meaningless.
fn load_rows(contents: &str, path: &str) -> Vec<CorpusRow> {
    contents
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with("//"))
        .map(|l| match serde_json::from_str(l) {
            Ok(row) => row,
            Err(e) => {
                eprintln!("malformed corpus row in {path}: {e}\n  {l}");
                std::process::exit(2);
            }
        })
        .collect()
}

fn fmt_amounts(amounts: &[Measure]) -> String {
    amounts
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

/// Human-readable field-by-field diff between the two paths' outputs.
fn field_diff(legacy: &Ingredient, segmented: &Ingredient) -> Vec<String> {
    let mut out = Vec::new();
    if legacy.name != segmented.name {
        out.push(format!(
            "name:     legacy {:?} | segmented {:?}",
            legacy.name, segmented.name
        ));
    }
    if legacy.amounts != segmented.amounts {
        out.push(format!(
            "amounts:  legacy [{}] | segmented [{}]",
            fmt_amounts(&legacy.amounts),
            fmt_amounts(&segmented.amounts)
        ));
    }
    if legacy.modifier != segmented.modifier {
        out.push(format!(
            "modifier: legacy {:?} | segmented {:?}",
            legacy.modifier, segmented.modifier
        ));
    }
    if legacy.optional != segmented.optional {
        out.push(format!(
            "optional: legacy {} | segmented {}",
            legacy.optional, segmented.optional
        ));
    }
    if legacy.usage != segmented.usage {
        out.push(format!(
            "usage:    legacy {:?} | segmented {:?}",
            legacy.usage, segmented.usage
        ));
    }
    out
}

/// Run the A/B over the ingredient corpus (and, informationally, the rich-text
/// corpus inputs). Exits with the number of committed-row divergences, capped
/// at 100.
pub fn run(corpus_path: &str, rich_corpus_path: &str) {
    let read = |path: &str| {
        std::fs::read_to_string(path).unwrap_or_else(|e| {
            eprintln!("failed to read {path}: {e}");
            std::process::exit(2);
        })
    };
    let rows = load_rows(&read(corpus_path), corpus_path);
    let legacy = IngredientParser::new();
    let segmented = IngredientParser::new().with_segmentation_mode(SegmentationMode::Segmented);

    let mut committed_diffs = 0usize;
    let mut xfail_diffs = 0usize;
    let mut improvements: Vec<&str> = Vec::new();

    for row in &rows {
        let l = legacy.from_str(&row.input);
        let s = segmented.from_str(&row.input);

        if l != s {
            let (tag, count) = if row.xfail.is_some() {
                ("XFAIL-DIVERGENCE", &mut xfail_diffs)
            } else {
                ("DIVERGENCE", &mut committed_diffs)
            };
            *count += 1;
            println!("{tag}: {}", row.input);
            for d in field_diff(&l, &s) {
                println!("    {d}");
            }
        }

        if row.label_matches(&s) && !row.label_matches(&l) {
            improvements.push(&row.input);
        }
    }

    // Rich-text corpus inputs: informational A/B only (see module docs).
    let rich_rows = load_rows(&read(rich_corpus_path), rich_corpus_path);
    let mut rich_diffs = 0usize;
    for row in &rich_rows {
        let l = legacy.from_str(&row.input);
        let s = segmented.from_str(&row.input);
        if l != s {
            rich_diffs += 1;
            println!("RICH-TEXT DIVERGENCE (informational): {}", row.input);
            for d in field_diff(&l, &s) {
                println!("    {d}");
            }
        }
    }

    for input in &improvements {
        println!("IMPROVEMENT (segmented matches label, legacy does not): {input}");
    }

    println!(
        "\nshadow: {} rows — {committed_diffs} committed divergence(s), \
         {xfail_diffs} xfail divergence(s), {rich_diffs} rich-text divergence(s), \
         {} improvement(s)",
        rows.len(),
        improvements.len()
    );

    std::process::exit(committed_diffs.min(100) as i32);
}
