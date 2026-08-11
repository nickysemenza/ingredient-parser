//! `corpus lint` — validate the accuracy corpus and, with `--report-stages`,
//! build a pass-coverage report over it.
//!
//! Two modes:
//! - **sanity** (default): every non-comment line must parse as JSON; prints the
//!   row count. Cheap guard against a malformed corpus edit.
//! - **`--report-stages`**: parse every row through the *traced* path, bucket the
//!   fired normalize rewrites / matched recognizer / fired refine passes, and
//!   print per-stage rows-per-pass tables. A closing section lists any pass in the
//!   parser's static universe (from [`ingredient::trace::pipeline_stage_names`])
//!   that fired on *zero* corpus rows — a possible dead pass to investigate in
//!   Phase 2. Report-only: always exits 0.

use std::collections::BTreeMap;

use ingredient::IngredientParser;
use ingredient::trace::pipeline_stage_names;
use tabled::{builder::Builder, settings::Style};

/// Fire counts for one stage's passes: pass name → number of rows it fired on.
/// A `BTreeMap` keeps the "zero coverage" listing deterministic; ordering for the
/// main table is taken from the pass universe instead (pipeline order).
type FireCounts = BTreeMap<String, usize>;

/// The tallied result of running the corpus through the traced parser.
pub struct StageCoverage {
    pub total_rows: usize,
    pub normalize: FireCounts,
    pub recognize: FireCounts,
    pub segment: FireCounts,
    pub refine: FireCounts,
}

/// Parse each row through the traced path and tally, per stage, how many rows
/// each pass fired on. Pure over the input rows so it can be unit-tested without
/// touching the filesystem.
pub fn report_stages_over(rows: &[String]) -> StageCoverage {
    let parser = IngredientParser::new();
    let mut cov = StageCoverage {
        total_rows: rows.len(),
        normalize: FireCounts::new(),
        recognize: FireCounts::new(),
        segment: FireCounts::new(),
        refine: FireCounts::new(),
    };

    // The report counts ROWS per pass, so a label that fires several times on
    // one line (e.g. two `prep_chain` clause decisions in a multi-clause
    // modifier) must still count that row once — otherwise percentages can
    // exceed 100. Collect each row's fired labels into a set before tallying.
    fn tally_once<'a>(counts: &mut FireCounts, names: impl Iterator<Item = &'a str>) {
        let fired: std::collections::BTreeSet<&str> = names.collect();
        for name in fired {
            *counts.entry(name.to_string()).or_default() += 1;
        }
    }

    for input in rows {
        let stages = parser.parse_with_trace(input).trace.stages();
        // A normalize rewrite / refine pass appears in the report only when it
        // changed the line, so mere presence == it fired. A recognizer appears
        // for every attempt, so it fired only when it produced output. Segment
        // nodes appear per clause decision / assembly repair that fired.
        tally_once(
            &mut cov.normalize,
            stages.normalize.iter().map(|rw| rw.name.as_str()),
        );
        tally_once(
            &mut cov.recognize,
            stages
                .recognizers
                .iter()
                .filter(|rec| rec.output.is_some())
                .map(|rec| rec.name.as_str()),
        );
        tally_once(
            &mut cov.segment,
            stages.segment.iter().map(|seg| seg.name.as_str()),
        );
        tally_once(
            &mut cov.refine,
            stages.refine.iter().map(|pass| pass.name.as_str()),
        );
    }
    cov
}

/// Render one stage's rows-per-pass table in pipeline order (`universe`), so a
/// zero-firing pass still shows a `0` row rather than vanishing.
fn stage_table(title: &str, universe: &[&str], counts: &FireCounts, total: usize) -> String {
    let mut b = Builder::default();
    b.push_record(["pass", "rows", "%"]);
    for &name in universe {
        let n = counts.get(name).copied().unwrap_or(0);
        let pct = if total == 0 {
            0.0
        } else {
            100.0 * n as f64 / total as f64
        };
        b.push_record([name.to_string(), n.to_string(), format!("{pct:.1}")]);
    }
    format!("{title}\n{}", b.build().with(Style::rounded()))
}

/// Passes in `universe` that fired on zero rows — possible dead passs.
fn zero_coverage<'a>(universe: &[&'a str], counts: &FireCounts) -> Vec<&'a str> {
    universe
        .iter()
        .copied()
        .filter(|name| counts.get(*name).copied().unwrap_or(0) == 0)
        .collect()
}

/// Render the full pass-coverage report (the four stage tables plus the
/// zero-coverage section) for an already-tallied [`StageCoverage`].
///
/// Returns the text rather than printing it: this is a library verb, and the
/// binary owns stdout. That also makes the report assertable in a test.
pub fn render_report(cov: &StageCoverage) -> String {
    use std::fmt::Write as _;

    let universe = pipeline_stage_names();
    let total = cov.total_rows;
    let mut out = String::new();

    // Writing into a String is infallible, so the `let _ =` discards a Result
    // that cannot be Err rather than unwrapping (denied by the workspace lints).
    let _ = writeln!(out, "Pass-coverage report over {total} corpus row(s)\n");
    for (name, universe_names, counts) in [
        ("normalize", universe.normalize, &cov.normalize),
        ("recognize", universe.recognizers, &cov.recognize),
        ("segment", universe.segment, &cov.segment),
        ("refine", universe.refine, &cov.refine),
    ] {
        let _ = writeln!(
            out,
            "{}\n",
            stage_table(name, universe_names, counts, total)
        );
    }

    let dead: Vec<(&str, &str)> = zero_coverage(universe.normalize, &cov.normalize)
        .into_iter()
        .map(|n| ("normalize", n))
        .chain(
            zero_coverage(universe.recognizers, &cov.recognize)
                .into_iter()
                .map(|n| ("recognize", n)),
        )
        .chain(
            zero_coverage(universe.segment, &cov.segment)
                .into_iter()
                .map(|n| ("segment", n)),
        )
        .chain(
            zero_coverage(universe.refine, &cov.refine)
                .into_iter()
                .map(|n| ("refine", n)),
        )
        .collect();

    let _ = writeln!(out, "ZERO CORPUS COVERAGE (possible dead pass)");
    if dead.is_empty() {
        let _ = writeln!(out, "  none — every pass fired on at least one corpus row");
    } else {
        for (stage, name) in dead {
            let _ = writeln!(out, "  [{stage}] {name}");
        }
    }
    out
}

/// What `corpus lint` found. Returned, not printed: the binary owns stdout and
/// the exit code (see `lib.rs`).
pub struct LintOutcome {
    /// Text for stdout — the row count, or the full coverage report.
    pub report: String,
    /// Malformed rows. Fatal in sanity mode, a warning in report mode.
    pub problems: Vec<String>,
}

/// Lint an already-loaded corpus. With `report_stages`, build the pass-coverage
/// report; otherwise just count the rows.
pub fn lint(corpus: &ingredient_corpus::Corpus, report_stages: bool) -> LintOutcome {
    let problems: Vec<String> = corpus
        .problems()
        .map(|(e, p)| format!("line {}: {}: {}", e.line_no, p.message, p.line))
        .collect();

    let report = if report_stages {
        render_report(&report_stages_over(&corpus.inputs()))
    } else {
        format!("{} corpus row(s) parse as JSON\n", corpus.rows().count())
    };

    LintOutcome { report, problems }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_stages_counts_a_known_pass() {
        // "chopped walnuts" fires the `extract_adjectives_from_name` refine pass;
        // "(1 cup walnuts)" matches the `optional_wrapped` recognizer. Assert both
        // show up with a positive count.
        let rows = vec![
            "2 cups chopped walnuts".to_string(),
            "(1 cup walnuts)".to_string(),
            "2 cups flour".to_string(),
        ];
        let cov = report_stages_over(&rows);
        assert_eq!(cov.total_rows, 3);
        assert!(
            cov.refine
                .get("extract_adjectives_from_name")
                .copied()
                .unwrap_or(0)
                > 0,
            "expected extract_adjectives_from_name to fire; got {:?}",
            cov.refine
        );
        assert!(
            cov.recognize.get("optional_wrapped").copied().unwrap_or(0) > 0,
            "expected optional_wrapped recognizer to fire; got {:?}",
            cov.recognize
        );
    }

    #[test]
    fn report_stages_counts_rows_not_trace_nodes() {
        // A multi-clause line can fire the same segment label more than once
        // (two prep-chain clauses here). The report is rows-per-pass, so no
        // count may ever exceed the row total.
        let rows = vec![
            "1/2 cup deribbed, seeded, and roughly chopped fresh hot green chiles, such as serrano"
                .to_string(),
        ];
        let cov = report_stages_over(&rows);
        for (stage, counts) in [
            ("normalize", &cov.normalize),
            ("recognize", &cov.recognize),
            ("segment", &cov.segment),
            ("refine", &cov.refine),
        ] {
            for (name, n) in counts {
                assert!(
                    *n <= cov.total_rows,
                    "{stage} pass {name} counted {n} > {} rows",
                    cov.total_rows
                );
            }
        }
    }

    #[test]
    fn zero_coverage_flags_unfired_passes() {
        let universe = ["a", "b", "c"];
        let mut counts = FireCounts::new();
        counts.insert("a".to_string(), 3);
        // "b" and "c" never fired.
        assert_eq!(zero_coverage(&universe, &counts), vec!["b", "c"]);
    }

    // The loader this module used to own now lives in `ingredient-corpus`;
    // its comment/blank/malformed handling is tested there, once, for all five
    // former copies. See `skips_comments_and_tracks_sections`.

    /// The report renders every stage table and the zero-coverage section, and
    /// returns the text rather than printing it — the property that makes this
    /// verb callable from somewhere other than `main`.
    #[test]
    fn render_report_covers_every_stage() {
        let cov = report_stages_over(&["2 cups flour, sifted".to_string()]);
        let report = render_report(&cov);
        assert!(report.contains("Pass-coverage report over 1 corpus row(s)"));
        for stage in ["normalize", "recognize", "segment", "refine"] {
            assert!(report.contains(stage), "missing {stage} table");
        }
        assert!(report.contains("ZERO CORPUS COVERAGE"));
    }
}
