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
//!   that fired on *zero* corpus rows — a possible dead rule to investigate in
//!   Phase 2. Report-only: always exits 0.

use std::collections::BTreeMap;

use ingredient::IngredientParser;
use ingredient::trace::pipeline_stage_names;
use tabled::{builder::Builder, settings::Style};

/// One corpus row for the stage report — only `input` is needed here (accuracy is
/// the concern of `tests/accuracy.rs`, not this lint).
struct Row {
    input: String,
}

/// Load corpus rows, skipping blank lines and `//` comments (matching
/// `accuracy.rs::load`). In strict mode a malformed line is an error; the
/// returned `Vec<String>` collects those messages so the caller can report and
/// set an exit code.
fn load_rows(corpus: &str) -> (Vec<Row>, Vec<String>) {
    let mut rows = Vec::new();
    let mut errors = Vec::new();
    for raw in corpus.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with("//") {
            continue;
        }
        match serde_json::from_str::<serde_json::Value>(line) {
            Ok(v) => match v.get("input").and_then(|i| i.as_str()) {
                Some(input) => rows.push(Row {
                    input: input.to_string(),
                }),
                None => errors.push(format!("row missing string `input`: {line}")),
            },
            Err(e) => errors.push(format!("invalid JSON: {e}: {line}")),
        }
    }
    (rows, errors)
}

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
pub fn report_stages(rows: &[String]) -> StageCoverage {
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

/// Passes in `universe` that fired on zero rows — possible dead rules.
fn zero_coverage<'a>(universe: &[&'a str], counts: &FireCounts) -> Vec<&'a str> {
    universe
        .iter()
        .copied()
        .filter(|name| counts.get(*name).copied().unwrap_or(0) == 0)
        .collect()
}

/// Print the full pass-coverage report (the three stage tables plus the
/// zero-coverage section) for an already-tallied [`StageCoverage`].
fn print_report(cov: &StageCoverage) {
    let universe = pipeline_stage_names();
    let total = cov.total_rows;

    println!("Pass-coverage report over {total} corpus row(s)\n");
    println!(
        "{}\n",
        stage_table("normalize", universe.normalize, &cov.normalize, total)
    );
    println!(
        "{}\n",
        stage_table("recognize", universe.recognizers, &cov.recognize, total)
    );
    println!(
        "{}\n",
        stage_table("segment", universe.segment, &cov.segment, total)
    );
    println!(
        "{}\n",
        stage_table("refine", universe.refine, &cov.refine, total)
    );

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

    println!("ZERO CORPUS COVERAGE (possible dead rule)");
    if dead.is_empty() {
        println!("  none — every pass fired on at least one corpus row");
    } else {
        for (stage, name) in dead {
            println!("  [{stage}] {name}");
        }
    }
}

/// Entry point for the `corpus lint` subcommand. Reads `corpus_path`; with
/// `report_stages` prints the pass-coverage report, otherwise just validates
/// rows and prints the count. Always exits 0 (report-only); a malformed corpus in
/// sanity mode exits non-zero.
pub fn run(corpus_path: &str, report_stages_flag: bool) {
    let contents = match std::fs::read_to_string(corpus_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("failed to read {corpus_path}: {e}");
            std::process::exit(1);
        }
    };

    let (rows, errors) = load_rows(&contents);

    if !report_stages_flag {
        // Cheap sanity mode: report row count and fail loudly on malformed rows.
        if errors.is_empty() {
            println!("{} corpus row(s) parse as JSON", rows.len());
        } else {
            eprintln!("{} malformed corpus row(s):", errors.len());
            for e in &errors {
                eprintln!("  {e}");
            }
            std::process::exit(1);
        }
        return;
    }

    // Report mode tolerates malformed rows (skips them with a warning) so the
    // coverage report is still useful mid-edit.
    if !errors.is_empty() {
        eprintln!(
            "warning: skipping {} malformed corpus row(s) for the report",
            errors.len()
        );
    }

    let inputs: Vec<String> = rows.into_iter().map(|r| r.input).collect();
    let cov = report_stages(&inputs);
    print_report(&cov);
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
        let cov = report_stages(&rows);
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
        let cov = report_stages(&rows);
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

    #[test]
    fn load_rows_skips_comments_and_flags_bad_json() {
        let corpus = "// header\n\n{\"input\": \"2 cups flour\"}\nnot json\n";
        let (rows, errors) = load_rows(corpus);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].input, "2 cups flour");
        assert_eq!(errors.len(), 1);
    }
}
