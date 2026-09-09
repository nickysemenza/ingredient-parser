//! Reproduce frozen cookbook samples using source markup only.
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashSet},
    error::Error,
    io::Read,
    path::Path,
};
use unicode_casefold::UnicodeCaseFold;
use xml::reader::{EventReader, XmlEvent};

fn hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
struct Element {
    index: usize,
    id: Option<String>,
    selected: bool,
    heading: bool,
    text: String,
}

/// Returns source records and an updated manifest. The frozen manifest owns
/// source identities/classes/seeds; no parser output influences sampling.
pub fn sample(library: &Path, corpus: &Path) -> Result<(Vec<Value>, Value), Box<dyn Error>> {
    let mut manifest: Value =
        serde_json::from_slice(&std::fs::read(corpus.join("manifest.json"))?)?;
    let excluded: HashSet<String> =
        serde_json::from_slice(&std::fs::read(corpus.join("excluded-inputs.json"))?)?;
    let seed = manifest["seed"].as_str().ok_or("missing seed")?.to_owned();
    let holdout_seed = manifest["holdout_seed"]
        .as_str()
        .ok_or("missing holdout seed")?
        .to_owned();
    let mut records = Vec::new();
    for book in manifest["books"].as_array_mut().ok_or("missing books")? {
        let id = book["book_id"]
            .as_str()
            .ok_or("missing book id")?
            .to_owned();
        let split = book["split"].as_str().ok_or("missing split")?.to_owned();
        let path = library.join(book["epub"].as_str().ok_or("missing epub")?);
        let bytes = std::fs::read(path)?;
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(&bytes))?;
        let mut names: Vec<_> = archive.file_names().map(str::to_owned).collect();
        names.sort();
        let classes: HashSet<String> = serde_json::from_value(book["ingredient_classes"].clone())?;
        let mut candidates = BTreeMap::new();
        for href in names {
            if ![".html", ".htm", ".xhtml"]
                .iter()
                .any(|suffix| href.ends_with(suffix))
            {
                continue;
            }
            let mut raw = Vec::new();
            archive.by_name(&href)?.read_to_end(&mut raw)?;
            let mut stack: Vec<Element> = Vec::new();
            let mut index = 0;
            let mut found = Vec::new();
            let mut valid = true;
            for event in EventReader::new(raw.as_slice()) {
                match event {
                    Ok(XmlEvent::StartElement {
                        name, attributes, ..
                    }) => {
                        let attr = |key: &str| {
                            attributes
                                .iter()
                                .find(|a| a.name.local_name == key)
                                .map(|a| a.value.clone())
                        };
                        let class = attr("class").unwrap_or_default();
                        let heading = class.split_whitespace().any(|c| c == "underline");
                        if heading {
                            for e in &mut stack {
                                e.heading = true;
                            }
                        }
                        stack.push(Element {
                            index,
                            id: attr("id"),
                            selected: matches!(name.local_name.as_str(), "p" | "li")
                                && class.split_whitespace().any(|c| classes.contains(c)),
                            heading,
                            text: String::new(),
                        });
                        index += 1;
                    }
                    Ok(
                        XmlEvent::Characters(text)
                        | XmlEvent::CData(text)
                        | XmlEvent::Whitespace(text),
                    ) => {
                        for e in &mut stack {
                            e.text.push_str(&text);
                        }
                    }
                    Ok(XmlEvent::EndElement { .. }) => {
                        if let Some(e) = stack.pop()
                            && e.selected
                            && !e.heading
                        {
                            found.push(e);
                        }
                    }
                    Err(_) => {
                        valid = false;
                        break;
                    }
                    _ => {}
                }
            }
            if !valid {
                continue;
            }
            found.sort_by_key(|e| e.index);
            for e in found {
                let text = e.text.split_whitespace().collect::<Vec<_>>().join(" ");
                let key: String = text.case_fold().collect();
                if text.is_empty() || text.ends_with(':') || excluded.contains(&key) {
                    continue;
                }
                candidates.entry(key).or_insert_with(|| json!({"book_id":id,"split":split,"input":text,"source":{"href":href,"element_index":e.index,"element_id":e.id}}));
            }
        }
        book["candidate_count"] = json!(candidates.len());
        book["sha256"] = json!(hash(&bytes));
        let mut selected: Vec<_> = candidates.into_values().collect();
        let seed = if split == "holdout" {
            &holdout_seed
        } else {
            &seed
        };
        selected.sort_by_key(|row| {
            hash(format!("{seed}\0{id}\0{}", row["input"].as_str().unwrap_or("")).as_bytes())
        });
        if selected.len() < 50 {
            return Err(format!("{id}: fewer than 50 candidate lines").into());
        }
        selected.truncate(50);
        book["sample_size"] = json!(50);
        for (i, mut row) in selected.into_iter().enumerate() {
            row["id"] = json!(format!("{id}-{:02}", i + 1));
            records.push(row);
        }
    }
    Ok((records, manifest))
}

pub fn verify(library: &Path, corpus: &Path) -> Result<Value, Box<dyn Error>> {
    let (rows, manifest) = sample(library, corpus)?;
    let expected: Vec<Value> = std::fs::read_to_string(corpus.join("sources.jsonl"))?
        .lines()
        .map(serde_json::from_str)
        .collect::<Result<_, _>>()?;
    let original: Value = serde_json::from_slice(&std::fs::read(corpus.join("manifest.json"))?)?;
    if rows != expected || manifest != original {
        return Err("source sample or manifest differs".into());
    }
    let cohort = [
        "wok",
        "arabiyya",
        "charred",
        "home-kitchen",
        "bakers-companion",
        "nopalito",
    ];
    let benchmark: String = rows
        .iter()
        .filter(|r| cohort.contains(&r["book_id"].as_str().unwrap_or("")))
        .map(|r| format!("{}\n", r["input"].as_str().unwrap_or("")))
        .collect();
    let bench = corpus.join("../../../benches/cookbook-lines.txt");
    if std::fs::read_to_string(bench)? != benchmark {
        return Err("benchmark sample differs".into());
    }
    Ok(
        json!({"verified":true,"rows":rows.len(),"books":manifest["books"].as_array().map(Vec::len)}),
    )
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use std::io::Write;

    #[test]
    fn frozen_python_sampling_parity_on_synthetic_source() {
        let dir = std::env::temp_dir().join(format!("sampling-parity-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut archive =
            zip::ZipWriter::new(std::fs::File::create(dir.join("book.epub")).unwrap());
        let options = zip::write::SimpleFileOptions::default();
        // Reverse archive insertion order: source identity must use sorted paths.
        archive.start_file("z.xhtml", options).unwrap();
        archive
            .write_all(b"<html><body><p class='ingredient'>STRASSE</p></body></html>")
            .unwrap();
        archive.start_file("a.xhtml", options).unwrap();
        let mut source = "<html><body><p class='ingredient'>Straße</p><p class='ingredient'><span class='underline'>Heading</span></p><p class='ingredient'>excluded</p>".to_owned();
        for i in 0..60 {
            source.push_str(&format!(
                "<p class='other ingredient' id='b{i}'> {i}\n g <em>beans</em> </p>"
            ));
        }
        source.push_str("</body></html>");
        archive.write_all(source.as_bytes()).unwrap();
        archive.finish().unwrap();
        std::fs::write(dir.join("excluded-inputs.json"), r#"["excluded"]"#).unwrap();
        // Golden digests generated with the retired Python SHA-256 sampling
        // protocol, independently of this implementation. Both seeds are frozen.
        for (split, digest) in [
            (
                "development",
                "c37eff570dcaea00e467e09c310a1f8379d231bf902c548d2fe8f810faecc071",
            ),
            (
                "holdout",
                "bf08123bc6cc37879550a0bc3cf4e70866cb3bea220991aa9941a2d377c9df7b",
            ),
        ] {
            let manifest = json!({"seed":"development-seed","holdout_seed":"holdout-seed","books":[{"book_id":"synthetic","split":split,"epub":"book.epub","ingredient_classes":["ingredient"]}]});
            std::fs::write(
                dir.join("manifest.json"),
                serde_json::to_vec(&manifest).unwrap(),
            )
            .unwrap();
            let (rows, manifest) = sample(&dir, &dir).unwrap();
            assert_eq!(manifest["books"][0]["candidate_count"], 61);
            assert_eq!(rows.len(), 50);
            let inputs = rows
                .iter()
                .map(|r| r["input"].as_str().unwrap())
                .collect::<Vec<_>>()
                .join("\n");
            assert_eq!(hash(inputs.as_bytes()), digest);
            for (i, row) in rows.iter().enumerate() {
                assert_eq!(row["id"], format!("synthetic-{:02}", i + 1));
                assert_eq!(row["source"]["href"], "a.xhtml");
                let input = row["input"].as_str().unwrap();
                if input == "Straße" {
                    assert_eq!(row["source"]["element_index"], 2);
                    assert!(row["source"]["element_id"].is_null());
                } else {
                    let n: usize = input.split_whitespace().next().unwrap().parse().unwrap();
                    assert_eq!(row["source"]["element_index"], 6 + 2 * n);
                    assert_eq!(row["source"]["element_id"], format!("b{n}"));
                }
            }
        }
        std::fs::remove_dir_all(dir).unwrap();
    }
}
