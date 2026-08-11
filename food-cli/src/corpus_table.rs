//! Render the accuracy corpus as a static HTML table for eyeballing.
//!
//! The *viewer*: shows rows as the file spells them, via
//! [`ingredient_corpus::render_authored`]. Deliberately not `Measure`'s
//! `Display`, which would re-spell them.

use ingredient_corpus::{Corpus, CorpusRow, Entry, render_authored};

const CORPUS_STYLE: &str = "\
body { font-family: -apple-system, system-ui, sans-serif; margin: 2rem; color: #1a1a1a; }
h1 { font-size: 1.4rem; }
h2 { font-size: 1.05rem; margin-top: 2rem; color: #444; border-bottom: 1px solid #ddd; padding-bottom: .2rem; }
.summary { color: #666; }
table { border-collapse: collapse; width: 100%; font-size: .85rem; margin-bottom: 1rem; }
th, td { text-align: left; padding: .3rem .5rem; border-bottom: 1px solid #eee; vertical-align: top; }
thead th { position: sticky; top: 0; background: #fff; border-bottom: 2px solid #ccc; }
tbody tr:nth-child(even) { background: #fafafa; }
td code { font-family: ui-monospace, monospace; white-space: pre-wrap; }
tr.xfail, tr.xfail:nth-child(even) { background: #fff8e1; }
tr.err, tr.err:nth-child(even) { background: #fdecea; }
.opt { text-align: center; color: #2e7d32; }";

/// The cells one row contributes. A malformed line still renders, so the
/// viewer never silently shows a short corpus.
struct Cells<'a> {
    input: &'a str,
    name: &'a str,
    amounts: String,
    modifier: &'a str,
    optional: bool,
    note: String,
    class: Option<&'static str>,
}

fn cells(entry: &Entry<CorpusRow>) -> Cells<'_> {
    match &entry.parsed {
        Ok(row) => {
            let note = row.xfail.clone().unwrap_or_default();
            Cells {
                input: &row.input,
                name: &row.name,
                amounts: render_authored(&row.amounts),
                modifier: row.modifier.as_deref().unwrap_or(""),
                optional: row.optional,
                class: (!note.is_empty()).then_some("xfail"),
                note,
            }
        }
        Err(problem) => Cells {
            input: &problem.line,
            name: "",
            amounts: String::new(),
            modifier: "",
            optional: false,
            note: format!("malformed: {}", problem.message),
            class: Some("err"),
        },
    }
}

/// Render the corpus as a self-contained static HTML doc: one `<h2>` + `<table>`
/// per section. No JS. Returns `(html, row_count)`. `maud` auto-escapes every
/// interpolated value, so no manual escaping is needed.
pub fn render_html(corpus: &Corpus<CorpusRow>) -> (String, usize) {
    use maud::{DOCTYPE, PreEscaped, html};

    let total = corpus.entries.len();
    let xfail = corpus.rows().filter(|r| r.xfail.is_some()).count();
    let committed = total - xfail;

    let markup = html! {
        (DOCTYPE)
        html lang="en" {
            head {
                meta charset="utf-8";
                title { "Ingredient parser corpus" }
                style { (PreEscaped(CORPUS_STYLE)) }
            }
            body {
                h1 { "Ingredient parser corpus" }
                p.summary { (total) " rows · " (committed) " committed · " (xfail) " xfail" }
                @for (name, entries) in corpus.by_section() {
                    h2 { (name) }
                    table {
                        thead { tr {
                            th { "input" } th { "name" } th { "amounts" }
                            th { "modifier" } th { "optional" } th { "xfail" }
                        } }
                        tbody {
                            @for entry in entries {
                                @let c = cells(entry);
                                tr class=[c.class] {
                                    td { code { (c.input) } }
                                    td { (c.name) }
                                    td { (c.amounts) }
                                    td { (c.modifier) }
                                    td.opt { @if c.optional { "✓" } }
                                    td { (c.note) }
                                }
                            }
                        }
                    }
                }
            }
        }
    };
    (markup.into_string(), total)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"// header comment, ignored
//
// --- basics ---
{"input": "2 cups flour", "name": "flour", "amounts": [{"unit": "cup", "value": 2}]}

{"input": "2-3 cups <broth>", "name": "broth", "amounts": [{"unit": "cup", "value": 2, "upper_value": 3}]}
{"input": "2/3 cup milk", "name": "milk", "amounts": [{"unit": "cup", "value": "2/3"}]}
// --- gaps ---
{"input": "1 pint berries", "name": "berries", "amounts": [{"unit": "pint", "value": 1}], "xfail": "pint range"}
not valid json
"#;

    #[test]
    fn render_escapes_and_counts() {
        let (html, rows) = render_html(&ingredient_corpus::parse(SAMPLE));
        assert_eq!(rows, 5);
        assert!(html.contains("<table>"));
        // Summary: 5 entries, 1 has xfail, the malformed one counts as committed.
        assert!(html.contains("5 rows · 4 committed · 1 xfail"));
        // Section headings rendered.
        assert!(html.contains("<h2>basics</h2>"));
        assert!(html.contains("<h2>gaps</h2>"));
        // Angle brackets in input are escaped, not emitted raw.
        assert!(html.contains("&lt;broth&gt;"));
        assert!(!html.contains("<broth>"));
        // Range chip uses an en dash — the authored lens, not Display's " - ".
        assert!(html.contains("2–3 cup"));
        // A fraction-string value renders as the fraction, not a glyph or a blank.
        assert!(html.contains("2/3 cup"));
        assert!(!html.contains("> cup<"));
        // xfail reason surfaces.
        assert!(html.contains("pint range"));
        // The malformed line is tolerated and flagged, not dropped.
        assert!(html.contains(r#"class="err""#));
    }
}
