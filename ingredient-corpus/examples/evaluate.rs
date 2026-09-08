//! Evaluate independently labeled cookbook samples. Does not modify labels.
//! cargo run -p ingredient-corpus --example evaluate -- <cookbooks-dir> [development|holdout]
use ingredient_corpus::{CorpusRow, Tally, score};
use serde_json::{Value, json};
use std::{collections::BTreeMap, error::Error, path::PathBuf};

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let dir = PathBuf::from(args.next().ok_or("provide cookbook corpus directory")?);
    let split_filter = args.next();
    let manifest: Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("manifest.json"))?)?;
    let books = manifest["books"].as_array().ok_or("manifest lacks books")?;
    let mut totals: BTreeMap<String, Tally> = BTreeMap::new();
    let mut records = Vec::new();
    for book in books {
        let id = book["book_id"].as_str().ok_or("missing book id")?;
        let split = book["split"].as_str().ok_or("missing split")?;
        if split_filter
            .as_deref()
            .is_some_and(|wanted| wanted != split)
        {
            continue;
        }
        let labels = std::fs::read_to_string(dir.join(format!("{id}.jsonl")))?;
        for line in labels.lines().filter(|line| !line.trim().is_empty()) {
            let raw: Value = serde_json::from_str(line)?;
            let row: CorpusRow = serde_json::from_value(raw.clone())?;
            let scored = score(&row);
            totals.entry(split.to_owned()).or_default().add(&scored);
            let fields: BTreeMap<_, _> = scored
                .fields
                .iter()
                .map(|f| (f.field.as_str(), f.ok))
                .collect();
            let mismatches: Vec<_> = scored
                .mismatches()
                .map(|f| json!({"field": f.field.as_str(), "want": f.want, "got": f.got}))
                .collect();
            records.push(
                json!({"id":raw["id"], "split":split, "fields":fields, "mismatches":mismatches}),
            );
        }
    }
    let summary: BTreeMap<_, _> = totals
        .iter()
        .map(|(split, tally)| {
            (
                split,
                json!({"rows":tally.total, "exact":tally.matched(),
                      "per_field":tally.per_field}),
            )
        })
        .collect();
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({"summary":summary,"rows":records}))?
    );
    Ok(())
}
