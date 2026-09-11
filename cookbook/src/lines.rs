//! The book as one line stream. Spine documents are cleaned and concatenated in
//! reading order; document and page boundaries become provenance on each line
//! rather than walls. Indexes map anchors, pages, and table-of-contents entries
//! to line numbers, and shape predicates (title-like, quantity-like) drive
//! chunking and validation.

use std::collections::{BTreeMap, HashMap};

use ingredient::Confidence;
use serde::{Deserialize, Serialize};

use crate::epub::clean::{CleanLine, clean_document, page_from_id};
use crate::epub::nav::Nav;
use crate::epub::open::{SpineDoc, resolve_href};

#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    /// Global index in the stream.
    pub idx: usize,
    /// Spine document index.
    pub doc: usize,
    /// Index among the document's cleaned lines.
    pub doc_line: usize,
    /// The printed page in effect at this line (the last page marker seen).
    pub page: Option<String>,
    pub clean: CleanLine,
}

impl Line {
    pub fn text(&self) -> &str {
        &self.clean.text
    }

    /// The deterministic item id for an item whose title is this line:
    /// `"{doc:03}.{doc_line:04}"`.
    pub fn id(&self) -> String {
        format!("{:03}.{:04}", self.doc, self.doc_line)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocSpan {
    pub index: usize,
    pub path: String,
    pub first_line: usize,
    pub len: usize,
}

#[derive(Debug, Clone, Default)]
pub struct BookLines {
    pub lines: Vec<Line>,
    pub docs: Vec<DocSpan>,
    /// `(doc_path, fragment)` → line.
    anchors: HashMap<(String, String), usize>,
    doc_first: HashMap<String, usize>,
    /// Printed page number → first line on that page.
    pages: BTreeMap<u32, usize>,
    /// `(nav entry order, line)` for every table-of-contents entry that
    /// resolved to a line.
    pub nav_targets: Vec<(usize, usize)>,
    /// Per-line `looks_like_quantity_text`, computed once: it runs the
    /// ingredient parser, and the chunker asks about each line many times.
    quantity: Vec<bool>,
}

impl BookLines {
    pub fn build(docs: &[SpineDoc], nav: &Nav) -> BookLines {
        let mut book = BookLines::default();
        let mut page: Option<String> = None;
        for doc in docs {
            let cleaned = clean_document(&doc.xhtml, &doc.path);
            let first_line = book.lines.len();
            book.doc_first.insert(doc.path.clone(), first_line);
            for (doc_line, clean) in cleaned.into_iter().enumerate() {
                let idx = book.lines.len();
                if let Some(marker) = &clean.pagebreak {
                    page = Some(marker.clone());
                    if let Ok(n) = marker.parse::<u32>() {
                        book.pages.entry(n).or_insert(idx);
                    }
                }
                for anchor in &clean.anchors {
                    book.anchors
                        .entry((doc.path.clone(), anchor.clone()))
                        .or_insert(idx);
                }
                book.lines.push(Line {
                    idx,
                    doc: doc.index,
                    doc_line,
                    page: page.clone(),
                    clean,
                });
            }
            book.docs.push(DocSpan {
                index: doc.index,
                path: doc.path.clone(),
                first_line,
                len: book.lines.len() - first_line,
            });
        }
        book.quantity = book
            .lines
            .iter()
            .map(|l| looks_like_quantity_text(l.text()))
            .collect();
        // Page-shaped anchor ids (`page_518`, `pg12`) stand in for markers the
        // book does not have; explicit markers and the nav page list win.
        for entry in &nav.page_list {
            if let Ok(n) = entry.page.parse::<u32>()
                && let Some(line) = book.resolve_target(&entry.doc_path, entry.fragment.as_deref())
            {
                book.pages.entry(n).or_insert(line);
            }
        }
        for ((_, fragment), &line) in &book.anchors {
            if let Some(page) = page_from_id(fragment)
                && let Ok(n) = page.parse::<u32>()
            {
                book.pages.entry(n).or_insert(line);
            }
        }
        // Pages in effect for lines that had no explicit marker but sit after
        // a page-shaped anchor are left as the last explicit marker: an anchor
        // is a hint, a marker is authored.
        book.nav_targets = nav
            .entries
            .iter()
            .filter_map(|e| {
                book.resolve_target(&e.doc_path, e.fragment.as_deref())
                    .map(|line| (e.order, line))
            })
            .collect();
        book
    }

    pub fn len(&self) -> usize {
        self.lines.len()
    }

    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    pub fn text(&self, idx: usize) -> &str {
        self.lines.get(idx).map(Line::text).unwrap_or("")
    }

    pub fn doc_path(&self, idx: usize) -> &str {
        self.lines
            .get(idx)
            .and_then(|l| self.docs.get(l.doc))
            .map(|d| d.path.as_str())
            .unwrap_or("")
    }

    /// The line a `(document, fragment)` target names: the anchor's line, else
    /// the page the fragment spells (`page_79`), else the document's first
    /// line. `None` when the document is not in the spine.
    pub fn resolve_target(&self, doc_path: &str, fragment: Option<&str>) -> Option<usize> {
        if let Some(fragment) = fragment {
            if let Some(&line) = self
                .anchors
                .get(&(doc_path.to_string(), fragment.to_string()))
            {
                return Some(line);
            }
            if let Some(page) = page_from_id(fragment)
                && let Ok(n) = page.parse::<u32>()
                && let Some(line) = self.page_line(n)
            {
                return Some(line);
            }
        }
        self.doc_first.get(doc_path).copied()
    }

    /// Resolve an internal href written in `from_doc` (`../c07.xhtml#page_327`,
    /// `#note1`) to a line.
    pub fn resolve_href(&self, from_doc: &str, href: &str) -> Option<usize> {
        let (path, fragment) = match href.split_once('#') {
            Some((p, f)) => (p, Some(f).filter(|f| !f.is_empty())),
            None => (href, None),
        };
        let doc_path = if path.trim().is_empty() {
            from_doc.to_string()
        } else {
            let dir = from_doc.rfind('/').map(|i| &from_doc[..i]).unwrap_or("");
            resolve_href(dir, path)
        };
        self.resolve_target(&doc_path, fragment)
    }

    /// The first line on printed page `page`, or on the nearest earlier page
    /// the book marks.
    pub fn page_line(&self, page: u32) -> Option<usize> {
        self.pages.range(..=page).next_back().map(|(_, &line)| line)
    }

    /// The first line of the page after the one containing `page`, if any.
    pub fn next_page_line(&self, page: u32) -> Option<usize> {
        self.pages.range(page + 1..).next().map(|(_, &line)| line)
    }

    pub fn has_pages(&self) -> bool {
        !self.pages.is_empty()
    }

    /// `(page, first line)` for every printed page the book marks.
    pub fn pages(&self) -> Vec<(u32, usize)> {
        self.pages.iter().map(|(p, l)| (*p, *l)).collect()
    }

    /// [`looks_like_title`] with context: a line directly after a
    /// quantity-like line is inside an ingredient list ("Kosher salt" after
    /// "¼ cup olive oil"), never a title.
    pub fn title_like(&self, idx: usize) -> bool {
        self.lines.get(idx).is_some_and(|l| {
            looks_like_title_with(l, self.quantity_like(idx))
                && (l.clean.heading.is_some() || !self.in_ingredient_list(idx))
        })
    }

    /// Inside an ingredient list: one of the three lines before `idx` is a
    /// quantity line and none after it reads as a sentence. Trailing
    /// unquantified lines ("Flaky salt, for sprinkling") and cross-reference
    /// lines ("Pastry Cream (this page)") live there, and are never titles.
    fn in_ingredient_list(&self, idx: usize) -> bool {
        let mut any_quantity = false;
        for i in idx.saturating_sub(3)..idx {
            let Some(l) = self.lines.get(i) else { continue };
            let text = l.text();
            if l.clean.heading.is_some() {
                any_quantity = false;
            } else if self.quantity_like(i) {
                // A long quantity line ("One 6- to 7-pound corkscrewed whole
                // leg of lamb {shank and sirloin end attached}") is still a
                // list line, so "Salt" after it is an ingredient.
                any_quantity = true;
            } else if text.len() > 100 || text.ends_with('.') {
                any_quantity = false;
            }
        }
        any_quantity
    }

    /// Cached [`looks_like_quantity_line`].
    pub fn quantity_like(&self, idx: usize) -> bool {
        self.quantity.get(idx).copied().unwrap_or(false)
    }

    /// Whether `idx` starts a run of ingredient lines: a quantity-like line
    /// followed by another, or by a short unpunctuated line (two-line lists).
    pub fn ingredient_run_start(&self, idx: usize) -> bool {
        if !self.quantity_like(idx) || (idx > 0 && self.quantity_like(idx - 1)) {
            return false;
        }
        match self.lines.get(idx + 1) {
            Some(next) => {
                self.quantity_like(idx + 1)
                    || (next.text().len() < 80 && !next.text().ends_with('.'))
            }
            None => false,
        }
    }

    /// The first ingredient run starting in `[from, from + within)`.
    pub fn next_ingredient_run(&self, from: usize, within: usize) -> Option<usize> {
        (from..(from + within).min(self.lines.len())).find(|&i| self.ingredient_run_start(i))
    }
}

/// Leader characters that introduce list lines, never titles.
const LEADERS: &str = "•·*-–—▪◦";

/// A short line that reads as a recipe title: a heading, or plain text that is
/// not a quantity, not a sentence, not a `Section:` label, and not a caption.
pub fn looks_like_title(line: &Line) -> bool {
    looks_like_title_with(line, looks_like_quantity_line(line))
}

fn looks_like_title_with(line: &Line, quantity_like: bool) -> bool {
    let c = &line.clean;
    if c.in_figure || c.transformed {
        return false;
    }
    if c.heading.is_some() {
        return !c.text.is_empty();
    }
    let t = c.text.trim();
    if t.is_empty() || t.len() > 80 || t.ends_with('.') || t.ends_with(':') || t.ends_with(',') {
        return false;
    }
    let lower = t.to_ascii_lowercase();
    if lower.starts_with("serves")
        || lower.starts_with("makes")
        || lower.starts_with("yield")
        || crate::validate::is_label(t)
        || lower.contains("(this page)")
    {
        return false;
    }
    match t.chars().next() {
        Some(ch)
            if ch.is_ascii_digit()
                || ingredient::fraction::is_vulgar(ch)
                || LEADERS.contains(ch) =>
        {
            false
        }
        Some(_) => !quantity_like,
        None => false,
    }
}

/// A line that reads as an ingredient: short, not a sentence, and either
/// starting with a quantity or parsing to one with high confidence.
pub fn looks_like_quantity_line(line: &Line) -> bool {
    looks_like_quantity_text(line.text())
}

pub fn looks_like_quantity_text(text: &str) -> bool {
    let t = text
        .trim()
        .trim_start_matches(|c: char| LEADERS.contains(c))
        .trim_start();
    if t.is_empty() || t.len() > 160 || t.ends_with('.') {
        return false;
    }
    match t.chars().next() {
        Some(ch) if ch.is_ascii_digit() || ingredient::fraction::is_vulgar(ch) => true,
        Some(_) => ingredient::from_str(t).parse_notes.confidence == Confidence::High,
        None => false,
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::epub::nav::{NavEntry, PageEntry};
    use rstest::rstest;

    fn doc(index: usize, path: &str, body: &str) -> SpineDoc {
        SpineDoc {
            index,
            path: path.into(),
            xhtml: format!("<html><body>{body}</body></html>"),
        }
    }

    fn book() -> BookLines {
        let docs = [
            doc(
                0,
                "OEBPS/xhtml/c02.xhtml",
                "<p class=\"rt\"><span epub:type=\"pagebreak\" id=\"page_79\" title=\"79\"/>Mousse Pie</p>\
                 <p>Serves 8</p><p class=\"ril\">2 cups cream</p><p class=\"ril\">1 tsp gelatin (see <a href=\"c07.xhtml#page_327\">this page</a>)</p>\
                 <p class=\"rp\">Whisk the cream until thick.</p>",
            ),
            doc(
                1,
                "OEBPS/xhtml/c07.xhtml",
                "<p class=\"rt\"><span epub:type=\"pagebreak\" id=\"page_327\" title=\"327\"/>Graham Cracker Crust</p>\
                 <p class=\"ril\">200 g graham crackers</p><p class=\"ril\">50 g sugar</p><p id=\"page_330\">All-Butter Pie Dough</p>",
            ),
        ];
        let nav = Nav {
            entries: vec![
                NavEntry {
                    label: "Pies".into(),
                    doc_path: "OEBPS/xhtml/c02.xhtml".into(),
                    fragment: None,
                    depth: 1,
                    order: 0,
                },
                NavEntry {
                    label: "Mousse Pie".into(),
                    doc_path: "OEBPS/xhtml/c02.xhtml".into(),
                    fragment: Some("page_79".into()),
                    depth: 2,
                    order: 1,
                },
                NavEntry {
                    label: "Crust".into(),
                    doc_path: "OEBPS/xhtml/c07.xhtml".into(),
                    fragment: Some("page_327".into()),
                    depth: 2,
                    order: 2,
                },
                NavEntry {
                    label: "Gone".into(),
                    doc_path: "OEBPS/xhtml/c99.xhtml".into(),
                    fragment: None,
                    depth: 2,
                    order: 3,
                },
            ],
            page_list: vec![PageEntry {
                page: "326".into(),
                doc_path: "OEBPS/xhtml/c07.xhtml".into(),
                fragment: None,
            }],
        };
        BookLines::build(&docs, &nav)
    }

    #[test]
    fn concatenates_documents_with_provenance() {
        let b = book();
        assert_eq!(b.len(), 9);
        assert_eq!(b.docs[1].first_line, 5);
        assert_eq!(b.docs[1].len, 4);
        let crust = &b.lines[5];
        assert_eq!(crust.text(), "Graham Cracker Crust");
        assert_eq!(crust.doc, 1);
        assert_eq!(crust.doc_line, 0);
        assert_eq!(crust.id(), "001.0000");
        assert_eq!(crust.page.as_deref(), Some("327"));
        // The page carries forward until the next marker.
        assert_eq!(b.lines[4].page.as_deref(), Some("79"));
        assert_eq!(b.doc_path(6), "OEBPS/xhtml/c07.xhtml");
    }

    #[test]
    fn resolves_anchors_pages_and_documents() {
        let b = book();
        assert_eq!(
            b.resolve_target("OEBPS/xhtml/c07.xhtml", Some("page_327")),
            Some(5)
        );
        assert_eq!(b.resolve_target("OEBPS/xhtml/c07.xhtml", None), Some(5));
        // Unknown fragment but page-shaped: the page index answers.
        assert_eq!(
            b.resolve_target("OEBPS/xhtml/c07.xhtml", Some("page_328")),
            Some(5)
        );
        assert_eq!(b.resolve_target("OEBPS/xhtml/nope.xhtml", None), None);
        assert_eq!(
            b.resolve_href("OEBPS/xhtml/c02.xhtml", "c07.xhtml#page_327"),
            Some(5)
        );
        assert_eq!(b.resolve_href("OEBPS/xhtml/c02.xhtml", "#page_79"), Some(0));
        // A page-shaped anchor id without a marker still maps its page.
        assert_eq!(b.page_line(330), Some(8));
        assert_eq!(b.page_line(329), Some(5));
        // From the nav page list: page 326 begins where c07 begins.
        assert_eq!(b.page_line(326), Some(5));
        assert_eq!(b.next_page_line(79), Some(5));
        assert_eq!(b.nav_targets, [(0, 0), (1, 0), (2, 5)]);
    }

    #[test]
    fn detects_ingredient_runs() {
        let b = book();
        assert!(b.ingredient_run_start(2));
        assert!(!b.ingredient_run_start(3), "inside a run, not its start");
        assert!(!b.ingredient_run_start(4), "a step is not a quantity line");
        assert!(b.ingredient_run_start(6));
        assert_eq!(b.next_ingredient_run(0, 10), Some(2));
        assert_eq!(b.next_ingredient_run(5, 25), Some(6));
        assert!(looks_like_title(&b.lines[0]));
        assert!(!looks_like_title(&b.lines[1]), "Serves 8 is a yield");
        assert!(!looks_like_title(&b.lines[2]));
        assert!(!looks_like_title(&b.lines[4]), "a sentence");
        assert!(looks_like_title(&b.lines[8]));
        // …but it follows "50 g sugar" directly, so in context it is a list line.
        assert!(!b.title_like(8));
    }

    #[test]
    fn unquantified_ingredient_after_a_quantity_is_not_a_title() {
        let docs = [doc(
            0,
            "c.html",
            "<p>A headnote sentence.</p><p>Kosher salt</p><p>¼ cup olive oil</p><p>Black pepper</p><h2>Next Recipe</h2>",
        )];
        let b = BookLines::build(&docs, &Nav::default());
        assert!(
            b.title_like(1),
            "first line of a list still reads as a title without context"
        );
        assert!(!b.title_like(3), "after a quantity line: part of the list");
        assert!(b.title_like(4));
    }

    #[rstest]
    #[case::leading_digit("2 cups cream", true)]
    #[case::vulgar("½ cup olive oil", true)]
    #[case::bullet("• 3 eggs", true)]
    #[case::word_first("About 2 teaspoons salt, or to taste", true)]
    #[case::no_amount("Kosher salt and freshly ground black pepper", false)]
    #[case::sentence("Preheat the oven to 425°F.", false)]
    #[case::numbered_step("1 Preheat the oven and grease the pan.", false)]
    #[case::long(&"2 cups ".repeat(30), false)]
    fn quantity_lines(#[case] text: &str, #[case] expected: bool) {
        assert_eq!(looks_like_quantity_text(text), expected, "{text}");
    }
}
