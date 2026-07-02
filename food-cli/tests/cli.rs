#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::process::Command;

fn food_cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_food-cli"))
}

#[test]
fn parse_ingredient_emits_json() {
    let output = food_cli()
        .args(["parse-ingredient", "1 cup flour, sifted"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout should be JSON");
    assert_eq!(json["name"], "flour");
    assert_eq!(json["modifier"], "sifted");
    assert!(json["amounts"].is_array());
    assert!(!json["amounts"].as_array().unwrap().is_empty());
}

#[test]
fn parse_amount_success_json() {
    let output = food_cli()
        .args(["parse-amount", "2 cups", "--json"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(json.is_array());
    assert_eq!(json[0]["unit"], "cup");
    assert_eq!(json[0]["value"], 2.0);
}

#[test]
fn parse_amount_invalid_exits_nonzero() {
    let output = food_cli()
        .args(["parse-amount", "not an amount", "--json"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(!output.stderr.is_empty());
}

#[test]
fn emit_corpus_row_fraction_and_modifier() {
    // A non-terminating fraction is emitted as the exact fraction string, keys
    // are in corpus order, and the modifier is carried through.
    let output = food_cli()
        .args([
            "parse-ingredient",
            "2/3 cup chopped onion",
            "--emit-corpus-row",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let line = String::from_utf8(output.stdout).unwrap();
    let trimmed = line.trim();
    // The row must be valid JSON and carry the exact fraction string for ⅔.
    let row: serde_json::Value = serde_json::from_str(trimmed).expect("emitted row is JSON");
    assert_eq!(row["input"], "2/3 cup chopped onion");
    assert_eq!(row["name"], "onion");
    assert_eq!(row["modifier"], "chopped");
    assert_eq!(row["amounts"][0]["unit"], "cup");
    assert_eq!(row["amounts"][0]["value"], "2/3");
    // Exactly one line of output.
    assert_eq!(line.lines().count(), 1);
}

#[test]
fn emit_corpus_row_refuses_fallback() {
    // A line that falls back to a name-only parse must be refused (non-zero exit,
    // stderr message) so a garbage row can't be appended blindly.
    let output = food_cli()
        .args(["parse-ingredient", "1+1 vitamins", "--emit-corpus-row"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty(), "must not print a row on refusal");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("refusing to emit"),
        "stderr should explain the refusal"
    );
}

#[test]
fn corpus_lint_report_stages_runs() {
    // The default corpus path resolves relative to the crate manifest, so a bare
    // `corpus lint --report-stages` produces the coverage report and exits 0.
    let output = food_cli()
        .args(["corpus", "lint", "--report-stages"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("Pass-coverage report"));
    assert!(stdout.contains("normalize"));
    assert!(stdout.contains("recognize"));
    assert!(stdout.contains("refine"));
    assert!(stdout.contains("ZERO CORPUS COVERAGE"));
    // A known high-frequency refine pass must appear in the report.
    assert!(stdout.contains("extract_adjectives_from_name"));
}

#[test]
fn parse_rich_text_json() {
    let output = food_cli()
        .args([
            "parse-rich-text",
            "Add 2 cups flour and mix",
            "--ingredients",
            "flour",
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let chunks = json.as_array().expect("rich text output is a chunk array");
    assert!(
        chunks
            .iter()
            .any(|c| c.get("kind") == Some(&serde_json::json!("Ing")))
    );
    assert!(
        chunks
            .iter()
            .any(|c| c.get("kind") == Some(&serde_json::json!("Measure")))
    );
}
