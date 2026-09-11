#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::process::Command;

fn food_cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_food-cli"))
}

#[test]
fn parse_ingredient_emits_json() {
    let output = food_cli()
        .args([
            "ingredient",
            "parse",
            "1 cup flour, sifted",
            "--format",
            "json",
        ])
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
fn cookbook_extract_exposes_opt_in_hybrid_strategy() {
    let output = food_cli()
        .args(["cookbook", "extract", "--help"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let help = String::from_utf8_lossy(&output.stdout);
    assert!(help.contains("--strategy"));
    assert!(help.contains("indexed"));
    assert!(help.contains("hybrid"));
}

#[test]
fn cookbook_ai_audit_requires_child_output_and_network() {
    for args in [
        ["cookbook", "audit", "missing.json", "--ai"].as_slice(),
        [
            "cookbook",
            "audit",
            "missing.json",
            "--ai",
            "--out",
            "child.json",
        ]
        .as_slice(),
    ] {
        let output = food_cli().args(args).output().unwrap();
        assert!(!output.status.success());
    }
}

#[test]
fn experiment_audit_operation_exposes_child_run_inputs() {
    let prepare = food_cli()
        .args(["cookbook", "experiment", "prepare", "--help"])
        .output()
        .unwrap();
    assert!(prepare.status.success());
    let help = String::from_utf8_lossy(&prepare.stdout);
    for flag in [
        "--operation",
        "--parent-run",
        "--group",
        "--reviewer-model",
        "--budget-usd",
        "--correct",
    ] {
        assert!(help.contains(flag), "prepare help missing {flag}: {help}");
    }

    let run = food_cli()
        .args(["cookbook", "experiment", "run", "--help"])
        .output()
        .unwrap();
    assert!(run.status.success());
    let help = String::from_utf8_lossy(&run.stdout);
    assert!(help.contains("--parent-run"));
    assert!(help.contains("--child-run"));
}

#[test]
fn experiment_audit_prepare_requires_parent_and_finite_budget() {
    let missing_parent = food_cli()
        .args([
            "cookbook",
            "experiment",
            "prepare",
            "--operation",
            "audit",
            "--expectations",
            "expectations.json",
            "--output-dir",
            "out",
            "--budget-usd",
            "1",
        ])
        .output()
        .unwrap();
    assert!(!missing_parent.status.success());

    let invalid_budget = food_cli()
        .args([
            "cookbook",
            "experiment",
            "prepare",
            "--operation",
            "audit",
            "--parent-run",
            "parent.json",
            "--expectations",
            "expectations.json",
            "--output-dir",
            "out",
            "--budget-usd",
            "-1",
        ])
        .output()
        .unwrap();
    assert!(!invalid_budget.status.success());
}

#[test]
fn experiment_run_rejects_unknown_manifest_operation() {
    let dir = std::env::temp_dir().join(format!(
        "cli-experiment-unknown-operation-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let manifest = dir.join("manifest.json");
    std::fs::write(&manifest, r#"{"operation":"future_operation"}"#).unwrap();
    let output = food_cli()
        .args([
            "cookbook",
            "experiment",
            "run",
            "--manifest",
            manifest.to_str().unwrap(),
            "--output-dir",
            dir.join("evidence").to_str().unwrap(),
            "--ledger",
            dir.join("ledger.json").to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("unsupported experiment manifest operation")
    );
}

#[test]
fn parse_amount_success_json() {
    let output = food_cli()
        .args(["amount", "parse", "2 cups", "--format", "json"])
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
        .args(["amount", "parse", "not an amount", "--format", "json"])
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
            "ingredient",
            "parse",
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
        .args(["ingredient", "parse", "1+1 vitamins", "--emit-corpus-row"])
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
fn scrape_epub_missing_path_exits_cleanly() {
    // A missing/unreadable EPUB path must produce a clean error + non-zero
    // exit, not a raw panic — the path is read (and can fail) before any
    // network call, so this is exercisable offline.
    let output = food_cli()
        .args([
            "cookbook",
            "inspect",
            "/tmp/does-not-exist-food-cli-test.epub",
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("panicked"),
        "must be a clean error, not a raw panic: {stderr}"
    );
    assert!(
        stderr.contains("No such file"),
        "stderr should explain the read failure"
    );
}

#[test]
fn scan_cookbooks_nonexistent_dir_errors() {
    let output = food_cli()
        .args(["cookbook", "scan", "/tmp/does-not-exist-food-cli-test-dir"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("not a directory"),
        "stderr should explain the directory doesn't exist"
    );
}

#[test]
fn emit_corpus_row_refuses_empty_name() {
    // A parse that keeps a digit (so it isn't a fallback and isn't low-
    // confidence) but leaves nothing for the name must still be refused —
    // an empty-name row would violate tests/accuracy.rs::never_empty_name.
    let output = food_cli()
        .args([
            "ingredient",
            "parse",
            "1 (5½-ounce) piece",
            "--emit-corpus-row",
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty(), "must not print a row on refusal");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("empty name"),
        "stderr should explain the refusal"
    );
}

#[test]
fn validate_unit_valid_and_invalid() {
    let output = food_cli()
        .args(["amount", "validate", "cup"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "valid");

    let output = food_cli()
        .args(["amount", "validate", "banana"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "invalid");
}

#[test]
fn parse_lines_emits_jsonl_per_line() {
    let path = std::env::temp_dir().join(format!(
        "food-cli-parse-lines-test-{}.txt",
        std::process::id()
    ));
    std::fs::write(&path, "1 cup flour\n2 tbsp sugar\n").unwrap();

    let output = food_cli()
        .args([
            "ingredient",
            "batch",
            path.to_str().unwrap(),
            "--format",
            "jsonl",
        ])
        .output()
        .unwrap();
    let _ = std::fs::remove_file(&path);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), 2);
    for line in lines {
        let json: serde_json::Value = serde_json::from_str(line).expect("line should be JSON");
        assert!(json.get("name").is_some());
        assert!(json.get("amounts").is_some());
    }
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
            "text",
            "parse",
            "Add 2 cups flour and mix",
            "--ingredients",
            "flour",
            "--format",
            "json",
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

#[test]
fn legacy_commands_are_removed() {
    for command in [
        "parse-ingredient",
        "parse-amount",
        "validate-unit",
        "parse-lines",
        "epub",
        "library",
        "scrape-epub",
        "scrape",
        "debug-epub",
        "scan-cookbooks",
        "corpus-table",
        "parse-rich-text",
    ] {
        assert_eq!(
            food_cli().arg(command).output().unwrap().status.code(),
            Some(2)
        );
    }
}

#[test]
fn refresh_requires_explicit_network_authorization() {
    let output = food_cli()
        .args([
            "cookbook",
            "extract",
            "missing.epub",
            "--out",
            "unused.json",
            "--refresh",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn ingredient_stats_json_jsonl_filters_and_invalid_saved_parses() {
    use recipe_epub::review::{ReviewRun, stats::ingredient_stats};
    let dir = std::env::temp_dir().join(format!("cli-ingredient-stats-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("run.json");
    let mut run: ReviewRun = serde_json::from_value(serde_json::json!({
        "version":1,"epub_sha256":"synthetic","source":"fixture.epub","model":"no-network","prompt_version":"old-saved-version",
        "parent":null,"chunks":[],"documents":[],"reserved_usd":0,
        "recipes":[{"meta":{"title":"Soup"},"sections":[{"ingredients":["1 tsp salt","2 tsp salt","1 cup water"]}],"source":"fixture.epub","url":"fixture.epub#soup.xhtml"}],
        "parsed":[{"sections":[{"ingredients":[{"name":"salt"},{"name":"salt"},{"name":"water"}]}]}]
    })).unwrap();
    run.save(&path).unwrap();
    let output = food_cli()
        .args(["cookbook", "stats", "--format", "json"])
        .arg(&path)
        .args(["--limit", "1", "--examples", "1"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let shared = ingredient_stats(&run).unwrap();
    assert_eq!(result["unique_names"], shared.unique_names);
    assert_eq!(result["total_occurrences"], shared.total_occurrences);
    assert_eq!(result["names"][0]["occurrences"], 2);
    assert_eq!(result["names"][0]["recipes"], 1);
    assert_eq!(result["names"][0]["examples"].as_array().unwrap().len(), 1);
    assert_eq!(result["matching_names"], 2);
    let output = food_cli()
        .args(["cookbook", "stats", "--format", "jsonl"])
        .arg(&path)
        .args(["--max-count", "1", "--sort", "name"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout).lines().count(), 1);
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap()["name"],
        "water"
    );
    let output = food_cli()
        .args(["cookbook", "stats", "--format", "json"])
        .arg(&path)
        .args(["--name", "missing"])
        .output()
        .unwrap();
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap()["matching_names"],
        0
    );
    let overview = food_cli()
        .args(["cookbook", "show"])
        .arg(&path)
        .output()
        .unwrap();
    assert!(overview.status.success());
    let overview = String::from_utf8(overview.stdout).unwrap();
    assert!(overview.contains("0 chunks · 1 recipes"));
    assert!(!overview.contains("prompt_version"));
    let full = food_cli()
        .args(["cookbook", "show", "--format", "json"])
        .arg(&path)
        .output()
        .unwrap();
    let full: serde_json::Value = serde_json::from_slice(&full.stdout).unwrap();
    assert_eq!(full["source"], "fixture.epub");
    let expected_path = dir.join("expected.json");
    std::fs::write(
        &expected_path,
        serde_json::to_vec(
            &serde_json::json!({"epub_sha256":run.epub_sha256,"recipes":run.recipes}),
        )
        .unwrap(),
    )
    .unwrap();
    let evaluated = food_cli()
        .args(["cookbook", "evaluate", "--format", "json"])
        .arg(&path)
        .arg("--expectations")
        .arg(&expected_path)
        .output()
        .unwrap();
    assert_eq!(evaluated.status.code(), Some(3));
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&evaluated.stdout).unwrap()["complete"],
        false
    );
    run.parsed = serde_json::json!([]);
    run.save(&path).unwrap();
    let output = food_cli()
        .args(["cookbook", "stats", "--format", "json"])
        .arg(&path)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("replay"));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn stdin_batch_preserves_order_and_reports_blank_and_review_lines() {
    use std::io::Write;
    use std::process::Stdio;
    let mut child = food_cli()
        .args(["ingredient", "batch", "-", "--format", "jsonl"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"1 cup flour\n\n1+1 vitamins\n2 tbsp sugar\n")
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    let rows: Vec<serde_json::Value> = stdout
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(rows.len(), 4);
    assert_eq!(rows[0]["line"], "1 cup flour");
    assert_eq!(rows[1]["error"], "empty ingredient line");
    assert!(rows[2]["error"].is_string());
    assert_eq!(rows[3]["line_number"], 4);
    assert_eq!(rows[3]["name"], "sugar");
}

#[test]
fn default_is_human_even_when_piped_and_trace_keeps_json_clean() {
    let output = food_cli()
        .args(["ingredient", "parse", "1 cup flour"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(serde_json::from_slice::<serde_json::Value>(&output.stdout).is_err());
    assert!(String::from_utf8_lossy(&output.stdout).contains("flour"));
    let output = food_cli()
        .args([
            "ingredient",
            "parse",
            "1 cup flour",
            "--format",
            "json",
            "--debug",
            "--explain",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap()["name"],
        "flour"
    );
    assert!(!output.stderr.is_empty());
    assert_eq!(
        food_cli()
            .args(["ingredient", "parse", "flour", "--format", "jsonl"])
            .output()
            .unwrap()
            .status
            .code(),
        Some(2)
    );
}

#[test]
fn corpus_table_write_failure_is_operational() {
    let output = food_cli()
        .args([
            "corpus",
            "table",
            "--out",
            "/nonexistent-food-cli-output-directory/table.html",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("failed to write"));
}
