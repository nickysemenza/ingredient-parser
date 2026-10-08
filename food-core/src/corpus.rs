//! The regression corpus, scored with the same comparisons as `tests/accuracy.rs`.
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::{AppResult, Host};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct CorpusField {
    pub field: String,
    pub matches: bool,
    pub expected: String,
    pub actual: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CorpusCase {
    pub line_number: usize,
    pub section: String,
    pub input: String,
    /// `EXACT`, `REGRESSION`, `XFAIL`, `PROMOTE`, or `INVALID`.
    pub status: String,
    pub reason: Option<String>,
    pub fields: Vec<CorpusField>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct CorpusResult {
    /// `None` for the corpus built into the parser.
    pub path: Option<String>,
    pub cases: Vec<CorpusCase>,
}

/// Score the corpus at `path`, or the built-in one.
pub fn score(host: &dyn Host, path: Option<String>) -> AppResult<CorpusResult> {
    let source = match &path {
        Some(path) => {
            String::from_utf8(host.read(path)?).map_err(|_| format!("{path} is not UTF-8 text"))?
        }
        None => ingredient_corpus::embedded().to_owned(),
    };
    let corpus = ingredient_corpus::parse(&source);
    let cases = corpus
        .entries
        .iter()
        .map(|entry| {
            let section = corpus
                .sections
                .get(entry.section)
                .cloned()
                .unwrap_or_default();
            match &entry.parsed {
                Ok(row) => {
                    let score = ingredient_corpus::score(row);
                    CorpusCase {
                        line_number: entry.line_no,
                        section,
                        input: row.input.clone(),
                        status: score.status.label().into(),
                        reason: row.xfail.clone(),
                        fields: score
                            .fields
                            .iter()
                            .map(|d| CorpusField {
                                field: d.field.as_str().into(),
                                matches: d.ok,
                                expected: d.want.clone(),
                                actual: d.got.clone(),
                            })
                            .collect(),
                    }
                }
                Err(error) => CorpusCase {
                    line_number: entry.line_no,
                    section,
                    input: error.line.clone(),
                    status: "INVALID".into(),
                    reason: Some(error.message.clone()),
                    fields: Vec::new(),
                },
            }
        })
        .collect();
    Ok(CorpusResult { path, cases })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::test_host::MemoryHost;

    #[test]
    fn corpus_keeps_invalid_and_known_gap_rows() {
        let mut host = MemoryHost::default();
        host.files.insert(
            "corpus.jsonl".into(),
            b"{\"input\":\"1 cup flour\",\"name\":\"flour\",\"amounts\":[{\"unit\":\"cup\",\"value\":1}]}\ninvalid\n{\"input\":\"salt\",\"name\":\"wrong\",\"xfail\":\"known gap\"}\n".to_vec(),
        );
        let result = score(&host, Some("corpus.jsonl".into())).unwrap();
        let statuses: Vec<_> = result.cases.iter().map(|r| r.status.as_str()).collect();
        assert_eq!(statuses, vec!["EXACT", "INVALID", "XFAIL"]);
        assert_eq!(result.cases[1].line_number, 2);
        assert!(!score(&host, None).unwrap().cases.is_empty());
    }
}
