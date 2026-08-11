//! Parser accuracy corpus — the project's north-star quality metric.
//!
//! `tests/corpus/corpus.jsonl` holds real ingredient strings, each with a
//! human-labeled expected parse (per the "Design Decisions" in `lib.rs`).
//!
//! Two row classes:
//! - **Committed rows** (no `xfail`): MUST parse exactly as labeled. A mismatch
//!   fails this test — this is the regression guard, a per-case ratchet stronger
//!   than an aggregate threshold (no committed row can ever silently regress).
//! - **Known gaps** (`"xfail": "reason"`): a mismatch is tolerated and reported.
//!   When the parser improves enough that an xfail row passes, the test prints a
//!   `PROMOTE` hint so the `xfail` marker can be removed.
//!
//! The headline metric is `exact matches / total`; it rises as Phase-2 work
//! closes known gaps. Grow the corpus by appending real lines: if a new line
//! parses correctly it's a committed row; if not, mark it `xfail` with a reason.
//!
//! Scope: this corpus is the home for `from_str` *accuracy*. The whole corpus is
//! also run through the traced parse path by `trace_path_matches_from_str` below,
//! so `trace.rs` only needs to assert trace-tree *structure*. Other orthogonal
//! shapes live in `parsing.rs` (`parse_amount`, `RichParser`, `Display`,
//! custom-parser config) — the row schema below cannot express those.

#![allow(clippy::unwrap_used)]
// Test-harness code: a malformed corpus line should fail the test loudly.
#![allow(clippy::panic)]

use ingredient::{IngredientParser, from_str};
use ingredient_corpus::{CorpusRow, Status, Tally};

/// Every well-formed row; a malformed line fails the suite loudly. The strict
/// policy is shared (`into_rows_strict`) so the two ratchets cannot drift.
fn load() -> Vec<CorpusRow> {
    match ingredient_corpus::parse(ingredient_corpus::embedded()).into_rows_strict() {
        Ok(rows) => rows,
        Err(problems) => panic!("invalid corpus row(s):\n{problems}"),
    }
}

#[test]
fn accuracy_corpus() {
    let rows = load();
    assert!(!rows.is_empty(), "corpus is empty");

    let mut tally = Tally::default();
    let mut regressions: Vec<(&str, Vec<String>)> = Vec::new();
    let mut promotable: Vec<&str> = Vec::new();

    for row in &rows {
        let scored = ingredient_corpus::score(row);
        tally.add(&scored);

        match scored.status {
            Status::Promote => promotable.push(&row.input),
            Status::Regression => regressions.push((
                &row.input,
                scored
                    .mismatches()
                    .map(|d| format!("{}: got {}, want {}", d.field.as_str(), d.got, d.want))
                    .collect(),
            )),
            Status::Exact | Status::Xfail => {}
        }
    }

    let total = tally.total;
    let [name_ok, amt_ok, mod_ok, opt_ok, use_ok] = tally.per_field;
    // `matched()`, NOT `tally.exact`: a passing xfail row is a match, and this
    // line has always counted it. With zero xfail rows in the corpus the two
    // are equal, so reading the wrong one would move the headline number
    // silently — see `matched_counts_promoted_rows` in ingredient-corpus.
    let matched = tally.matched();
    let pct = |n: usize| 100.0 * n as f64 / total as f64;
    eprintln!("\n========== Parser accuracy corpus ==========");
    eprintln!("rows:           {total}");
    eprintln!("exact matches:  {matched} ({:.1}%)", pct(matched));
    eprintln!("known gaps:     {} (xfail)", tally.xfail);
    eprintln!(
        "per-field:      name {name_ok}/{total}  amounts {amt_ok}/{total}  modifier {mod_ok}/{total}  optional {opt_ok}/{total}  usage {use_ok}/{total}"
    );
    eprintln!("============================================\n");

    for input in &promotable {
        eprintln!("PROMOTE (xfail now passes — remove `xfail`): {input}");
    }
    for (input, diff) in &regressions {
        eprintln!("REGRESSION: {input}");
        for d in diff {
            eprintln!("    {d}");
        }
    }

    assert!(
        regressions.is_empty(),
        "{} non-xfail corpus row(s) mismatch — see report above",
        regressions.len()
    );
}

/// Regression guard for the "name lost into the modifier" failures found on real
/// recipes (decimal commas like "1,000 grams", leading prep words, unicode inch
/// marks): a labeled ingredient line must never parse to an empty name. (A bare
/// quantity like "1/2-1 cup" may legitimately have no name, so this covers only
/// the corpus inputs plus known-tricky real lines.)
#[test]
fn never_empty_name() {
    let mut inputs: Vec<String> = load().into_iter().map(|r| r.input).collect();
    inputs.extend(
        [
            "1,000 grams (about 6 cups) quartered and pitted nectarines",
            "2/3 cup (85 grams) finely chopped, raw pistachios",
            "1/2 \u{201d} (1 cm) ginger, minced",
            "0.44 ounces salt (about 2 1/2 teaspoons) salt",
        ]
        .iter()
        .map(ToString::to_string),
    );
    for input in inputs {
        let ing = from_str(&input);
        assert!(
            !ing.name.trim().is_empty(),
            "parsed an empty name for input {input:?}"
        );
    }
}

/// The traced parse path must produce the same result as `from_str` for every
/// corpus input, and must build a non-empty trace tree. Preserves the
/// `from_str`-vs-trace equivalence that `parsing.rs::test_ingredient_parsing`
/// previously checked case by case (before those cases were ported into the
/// corpus). This is the trace path's smoke test — it runs the whole corpus, so
/// `trace.rs` needs no hand-maintained list of input shapes; that file is left
/// to assert trace-tree *structure* (nesting, outcomes, formatting, Jaeger).
#[test]
fn trace_path_matches_from_str() {
    let parser = IngredientParser::new();
    for row in load() {
        let plain = from_str(&row.input);
        let traced = parser.parse_with_trace(&row.input);
        assert_eq!(
            traced.result.unwrap(),
            plain,
            "trace path diverged from from_str for {:?}",
            row.input
        );
        // Non-empty rather than `contains("parse_ingredient")`: special-format
        // inputs (trailing-amount, "X of N", optional) parse before the core
        // `parse_ingredient` span is entered, so they root the tree under a
        // different span. Any root still formats to a non-empty tree.
        assert!(
            !traced.trace.format_tree(false).is_empty(),
            "empty trace tree for {:?}",
            row.input
        );
    }
}
