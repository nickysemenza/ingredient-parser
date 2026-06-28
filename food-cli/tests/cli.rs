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
