//! Source integrity and field-level ratchet for independently labeled samples.
#![allow(clippy::unwrap_used, clippy::panic)]
use ingredient_corpus::{CorpusRow, score};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};

#[test]
fn cookbook_sources_labels_and_accepted_fields_are_consistent() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../ingredient-parser/tests/corpus/cookbooks");
    let read_json = |name: &str| -> Value {
        serde_json::from_str(&std::fs::read_to_string(dir.join(name)).unwrap()).unwrap()
    };
    let manifest = read_json("manifest.json");
    let accepted: BTreeMap<String, BTreeSet<String>> =
        serde_json::from_value(read_json("accepted-mismatches.json")).unwrap();
    let source_text = std::fs::read_to_string(dir.join("sources.jsonl")).unwrap();
    let sources: BTreeMap<String, Value> = source_text
        .lines()
        .map(|line| {
            let row: Value = serde_json::from_str(line).unwrap();
            (row["id"].as_str().unwrap().to_owned(), row)
        })
        .collect();
    assert_eq!(sources.len(), 500);
    let mut labeled = BTreeSet::new();
    let mut split_counts = BTreeMap::new();
    let mut regressions = Vec::new();
    for book in manifest["books"].as_array().unwrap() {
        let id = book["book_id"].as_str().unwrap();
        let split = book["split"].as_str().unwrap();
        let text = std::fs::read_to_string(dir.join(format!("{id}.jsonl"))).unwrap();
        let mut book_count = 0;
        for line in text.lines() {
            let value: Value = serde_json::from_str(line).unwrap();
            let row_id = value["id"].as_str().unwrap().to_owned();
            assert!(
                labeled.insert(row_id.to_owned()),
                "duplicate label {row_id}"
            );
            let source = sources.get(&row_id).unwrap();
            assert_eq!(
                source["input"], value["input"],
                "source changed for {row_id}"
            );
            assert_eq!(source["book_id"], id);
            assert_eq!(source["split"], split);
            assert!(source["source"]["href"].as_str().is_some());
            assert!(source["source"]["element_index"].as_u64().is_some());
            let row: CorpusRow = serde_json::from_value(value).unwrap();
            assert!(
                row.xfail.is_none(),
                "independent desired labels do not encode implementation status"
            );
            let allowed = accepted.get(&row_id).unwrap();
            for field in allowed {
                assert!(
                    ["name", "amounts", "modifier", "optional", "usage"].contains(&field.as_str())
                );
            }
            for mismatch in score(&row).mismatches() {
                if !allowed.contains(mismatch.field.as_str()) {
                    regressions.push(format!(
                        "{row_id} {}: got {}, want {}",
                        mismatch.field.as_str(),
                        mismatch.got,
                        mismatch.want
                    ));
                }
            }
            book_count += 1;
        }
        assert_eq!(book_count, 50, "{id}");
        *split_counts.entry(split.to_owned()).or_insert(0) += book_count;
    }
    assert_eq!(split_counts.get("development"), Some(&300));
    assert_eq!(split_counts.get("holdout"), Some(&200));
    assert_eq!(labeled, sources.keys().cloned().collect());
    assert_eq!(labeled, accepted.keys().cloned().collect());
    assert!(
        regressions.is_empty(),
        "cookbook field regressions:\n{}",
        regressions.join("\n")
    );
}
