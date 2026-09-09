use crate::{CorpusRow, Tally, score};
use serde_json::{Value, json};
use std::{collections::BTreeMap, error::Error, path::Path};

pub fn evaluate(dir: &Path, split_filter: Option<&str>) -> Result<Value, Box<dyn Error>> {
    let manifest: Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("manifest.json"))?)?;
    let books = manifest["books"].as_array().ok_or("manifest lacks books")?;
    let mut totals: BTreeMap<String, Tally> = BTreeMap::new();
    let mut records = Vec::new();
    for book in books {
        let id = book["book_id"].as_str().ok_or("missing book id")?;
        let split = book["split"].as_str().ok_or("missing split")?;
        if split_filter.is_some_and(|wanted| wanted != split) {
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
    Ok(json!({"summary":summary,"rows":records}))
}

const FIELDS: [&str; 5] = ["name", "amounts", "modifier", "optional", "usage"];

/// Compare frozen evaluations without reopening labels or running the parser.
pub fn compare(
    before: &Value,
    after: &Value,
    book_ids: &[String],
) -> Result<Value, Box<dyn Error>> {
    fn rows(
        value: &Value,
        books: &[String],
    ) -> Result<BTreeMap<String, BTreeMap<String, bool>>, Box<dyn Error>> {
        let mut rows: BTreeMap<String, BTreeMap<String, bool>> = BTreeMap::new();
        for row in value["rows"]
            .as_array()
            .ok_or("evaluator JSON lacks rows")?
        {
            let id = row["id"].as_str().ok_or("every row needs a string id")?;
            let fields: BTreeMap<String, bool> = serde_json::from_value(row["fields"].clone())?;
            if fields.len() != FIELDS.len() || FIELDS.iter().any(|f| !fields.contains_key(*f)) {
                return Err("expected exactly five field scores".into());
            }
            if rows.insert(id.into(), fields).is_some() {
                return Err("duplicate row id".into());
            }
        }
        for book in books {
            if !rows
                .keys()
                .any(|id| id.rsplit_once('-').is_some_and(|(b, _)| b == book))
            {
                return Err(format!("requested books absent: {book}").into());
            }
        }
        if !books.is_empty() {
            rows.retain(|id, _| {
                id.rsplit_once('-')
                    .is_some_and(|(b, _)| books.iter().any(|book| book == b))
            });
        }
        Ok(rows)
    }
    let before = rows(before, book_ids)?;
    let after = rows(after, book_ids)?;
    if before.keys().ne(after.keys()) {
        return Err("evaluator row IDs differ".into());
    }
    let transition = |field: Option<&str>| {
        let ok = |values: &BTreeMap<String, bool>| {
            field.map_or_else(|| values.values().all(|v| *v), |f| values[f])
        };
        let mut improved = Vec::new();
        let mut regressed = Vec::new();
        let mut b = 0i64;
        let mut a = 0i64;
        for (id, old) in &before {
            let was = ok(old);
            let now = ok(&after[id]);
            b += i64::from(was);
            a += i64::from(now);
            if now && !was {
                improved.push(id);
            }
            if was && !now {
                regressed.push(id);
            }
        }
        json!({"before":b,"after":a,"delta":a-b,"improved":improved,"regressed":regressed})
    };
    let per_field: BTreeMap<_, _> = FIELDS.iter().map(|f| (*f, transition(Some(f)))).collect();
    let books: std::collections::BTreeSet<_> = book_ids.iter().collect();
    Ok(json!({"rows":before.len(),"exact":transition(None),"per_field":per_field,"book_ids":books}))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    fn row(id: &str, bad: Option<&str>) -> Value {
        let fields: BTreeMap<_, _> = FIELDS.iter().map(|f| (*f, Some(*f) != bad)).collect();
        json!({"id":id,"fields":fields})
    }
    #[test]
    fn reports_transitions_and_rejects_different_cohorts() {
        let before = json!({"rows":[row("wok-01",Some("name")),row("wok-02",None)]});
        let after = json!({"rows":[row("wok-01",None),row("wok-02",Some("modifier"))]});
        let result = compare(&before, &after, &[]).unwrap();
        assert_eq!(
            result["exact"],
            json!({"before":1,"after":1,"delta":0,"improved":["wok-01"],"regressed":["wok-02"]})
        );
        assert!(compare(&before, &json!({"rows":[row("other-01",None)]}), &[]).is_err());
        assert!(compare(&before, &after, &["missing".into()]).is_err());
        assert!(
            compare(
                &json!({"rows":[row("same",None),row("same",None)]}),
                &after,
                &[]
            )
            .is_err()
        );
    }
}
