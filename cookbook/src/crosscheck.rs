//! Book-level cross-check of extracted titles against the table of contents.
//!
//! The nav is the publisher's own list of what the book contains. A nav recipe
//! title with no extracted match marks the chunk that holds it (the second
//! opinion is told which line is the title). An extracted recipe that the nav
//! does not know and that has almost no content is a likely phantom (a caption
//! or a running head read as a title).

use std::collections::HashSet;

use unicode_normalization::UnicodeNormalization;

use crate::chunk::Chunk;
use crate::contract::Kind;
use crate::epub::nav::Nav;
use crate::lines::BookLines;
use crate::report::{CrossCheck, Flag};

/// Fewer nav titles than this and recall is not judged.
pub const MIN_NAV_TITLES: usize = 5;
const SIMILARITY: f64 = 0.85;

/// A title as extracted, with where it came from.
#[derive(Debug, Clone, PartialEq)]
pub struct ExtractedTitle {
    pub chunk: usize,
    pub title: String,
    pub line: Option<usize>,
    pub kind: Kind,
    pub ingredients: usize,
    pub steps: usize,
}

/// Lowercase, compatibility-normalized, punctuation collapsed to spaces.
pub fn normalize_title(title: &str) -> String {
    let folded: String = title.nfkc().collect::<String>().to_lowercase();
    folded
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Same title, allowing for a dropped subtitle or a typo or two.
pub fn titles_match(a: &str, b: &str) -> bool {
    let (a, b) = (normalize_title(a), normalize_title(b));
    if a.is_empty() || b.is_empty() {
        return false;
    }
    if a == b {
        return true;
    }
    let (short, long) = if a.len() <= b.len() {
        (&a, &b)
    } else {
        (&b, &a)
    };
    if short.split(' ').count() >= 2 && long.starts_with(short.as_str()) {
        return true;
    }
    strsim::normalized_levenshtein(&a, &b) >= SIMILARITY
}

/// The nav entries that name recipes, as `(title, line)`.
pub fn nav_recipe_titles(book: &BookLines, nav: &Nav) -> Vec<(String, usize)> {
    let max_depth = nav.max_depth();
    let mut out = Vec::new();
    for entry in &nav.entries {
        let Some(&(_, line)) = book
            .nav_targets
            .iter()
            .find(|(order, _)| *order == entry.order)
        else {
            continue;
        };
        let doc = book.lines.get(line).map(|l| l.doc);
        let doc_has_run = doc
            .and_then(|d| book.docs.get(d))
            .is_some_and(|d| book.next_ingredient_run(d.first_line, d.len).is_some());
        // A nested contents lists recipes under chapters; a flat one lists
        // whatever it likes, so fall back to "does its document cook".
        let is_recipe = if max_depth >= 2 {
            entry.depth >= 2
        } else {
            doc_has_run
        };
        if is_recipe {
            out.push((entry.label.clone(), line));
        }
    }
    out
}

pub fn crosscheck(
    book: &BookLines,
    nav: &Nav,
    chunks: &[Chunk],
    extracted: &[ExtractedTitle],
) -> (CrossCheck, Vec<(usize, Flag)>) {
    let nav_titles = nav_recipe_titles(book, nav);
    let mut flags = Vec::new();
    let mut matched_nav: HashSet<usize> = HashSet::new();
    let mut matched_extracted: HashSet<usize> = HashSet::new();
    for (ni, (title, _)) in nav_titles.iter().enumerate() {
        if let Some((ei, _)) = extracted
            .iter()
            .enumerate()
            .find(|(ei, e)| !matched_extracted.contains(ei) && titles_match(title, &e.title))
        {
            matched_nav.insert(ni);
            matched_extracted.insert(ei);
        }
    }
    let mut missing = Vec::new();
    for (ni, (title, line)) in nav_titles.iter().enumerate() {
        if matched_nav.contains(&ni) {
            continue;
        }
        missing.push(title.clone());
        if let Some(chunk) = chunks.iter().find(|c| c.contains(*line)) {
            flags.push((
                chunk.index,
                Flag::MissingNavTitle {
                    title: title.clone(),
                    line: *line,
                },
            ));
        }
    }
    let usable = nav_titles.len() >= MIN_NAV_TITLES;
    let mut phantom = Vec::new();
    for (ei, e) in extracted.iter().enumerate() {
        if e.kind != Kind::Recipe || matched_extracted.contains(&ei) {
            continue;
        }
        let in_figure = e
            .line
            .and_then(|l| book.lines.get(l))
            .is_some_and(|l| l.clean.in_figure);
        let thin = e.ingredients <= 3 && e.steps == 0;
        if in_figure || (usable && thin) {
            phantom.push(e.title.clone());
            flags.push((
                e.chunk,
                Flag::PhantomTitle {
                    title: e.title.clone(),
                },
            ));
        }
    }
    let recall = usable.then(|| matched_nav.len() as f32 / nav_titles.len() as f32);
    (
        CrossCheck {
            nav_titles: nav_titles.len(),
            matched: matched_nav.len(),
            missing,
            phantom,
            recall,
        },
        flags,
    )
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::chunk::{Boundary, ChunkOptions, chunk as make_chunks};
    use crate::epub::nav::NavEntry;
    use crate::epub::open::SpineDoc;
    use rstest::rstest;

    #[rstest]
    #[case(
        "Cranberry-Pomegranate Mousse Pie",
        "cranberry pomegranate mousse pie",
        true
    )]
    #[case("Polenta", "POLENTA", true)]
    #[case("Sour Cherry Pie", "Sour Cherry Pie: A Summer Classic", true)]
    #[case("Sour Cherry Pie", "Sour Cherry Pei", true)]
    #[case("Polenta", "Polenta with Fresh Corn", false)]
    #[case("Bread", "Broth", false)]
    #[case("", "Bread", false)]
    fn matching(#[case] a: &str, #[case] b: &str, #[case] expected: bool) {
        assert_eq!(titles_match(a, b), expected);
    }

    fn book() -> (BookLines, Nav) {
        let mut html = String::from("<h1 id=\"ch\">Pies</h1>");
        for (i, title) in [
            "Apple Pie",
            "Cherry Pie",
            "Peach Pie",
            "Plum Pie",
            "Pear Pie",
            "Fig Pie",
        ]
        .iter()
        .enumerate()
        {
            html.push_str(&format!(
                "<h2 id=\"r{i}\">{title}</h2><p>2 cups fruit</p><p>1 cup sugar</p><p>Bake it.</p>"
            ));
        }
        html.push_str("<div class=\"cap\"><p id=\"cap\">Quince Pie, this page</p></div>");
        let doc = SpineDoc {
            index: 0,
            path: "c.xhtml".into(),
            xhtml: format!("<html><body>{html}</body></html>"),
        };
        let mut entries = vec![NavEntry {
            label: "Pies".into(),
            doc_path: "c.xhtml".into(),
            fragment: Some("ch".into()),
            depth: 1,
            order: 0,
        }];
        for (i, title) in [
            "Apple Pie",
            "Cherry Pie",
            "Peach Pie",
            "Plum Pie",
            "Pear Pie",
            "Fig Pie",
        ]
        .iter()
        .enumerate()
        {
            entries.push(NavEntry {
                label: title.to_string(),
                doc_path: "c.xhtml".into(),
                fragment: Some(format!("r{i}")),
                depth: 2,
                order: i + 1,
            });
        }
        let nav = Nav {
            entries,
            page_list: vec![],
        };
        (BookLines::build(&[doc], &nav), nav)
    }

    fn extracted(
        chunk: usize,
        title: &str,
        line: Option<usize>,
        ingredients: usize,
        steps: usize,
    ) -> ExtractedTitle {
        ExtractedTitle {
            chunk,
            title: title.into(),
            line,
            kind: Kind::Recipe,
            ingredients,
            steps,
        }
    }

    #[test]
    fn finds_missing_and_phantom_titles() {
        let (book, nav) = book();
        let chunks = make_chunks(&book, &ChunkOptions::default());
        assert_eq!(chunks.len(), 1);
        let nav_titles = nav_recipe_titles(&book, &nav);
        assert_eq!(nav_titles.len(), 6, "the depth-1 chapter is not a recipe");
        let cap_line = (0..book.len())
            .find(|&i| book.text(i).starts_with("Quince"))
            .unwrap();
        let got = vec![
            extracted(0, "Apple Pie", Some(1), 2, 1),
            extracted(0, "Cherry Pie", Some(5), 2, 1),
            extracted(0, "Peach pie", Some(9), 2, 1),
            extracted(0, "Plum Pie", Some(13), 2, 1),
            extracted(0, "Pear Pie", Some(17), 2, 1),
            // Fig Pie missing; a caption came back as a recipe.
            extracted(0, "Quince Pie, this page", Some(cap_line), 0, 0),
            // A real unlisted recipe with content is not a phantom.
            extracted(0, "Bonus Pie", None, 6, 4),
        ];
        let (check, flags) = crosscheck(&book, &nav, &chunks, &got);
        assert_eq!(check.nav_titles, 6);
        assert_eq!(check.matched, 5);
        assert_eq!(check.missing, ["Fig Pie"]);
        assert_eq!(check.phantom, ["Quince Pie, this page"]);
        assert!((check.recall.unwrap() - 5.0 / 6.0).abs() < 1e-6);
        let fig_line = (0..book.len())
            .find(|&i| book.text(i) == "Fig Pie")
            .unwrap();
        assert!(flags.contains(&(
            0,
            Flag::MissingNavTitle {
                title: "Fig Pie".into(),
                line: fig_line
            }
        )));
        assert!(flags.contains(&(
            0,
            Flag::PhantomTitle {
                title: "Quince Pie, this page".into()
            }
        )));
        assert_eq!(flags.len(), 2);
    }

    #[test]
    fn thin_nav_disables_recall_but_keeps_caption_phantoms() {
        let (book, _) = book();
        let nav = Nav::default();
        let chunk = Chunk {
            id: "k000".into(),
            index: 0,
            start: 0,
            end: book.len(),
            chars: 0,
            title_hint: None,
            boundary: Boundary::Start,
        };
        let cap_line = (0..book.len())
            .find(|&i| book.text(i).starts_with("Quince"))
            .unwrap();
        let got = vec![
            extracted(0, "Thin", None, 1, 0),
            extracted(0, "Quince Pie, this page", Some(cap_line), 0, 0),
        ];
        let (check, flags) = crosscheck(&book, &nav, &[chunk], &got);
        assert_eq!(check.recall, None);
        assert_eq!(
            check.phantom,
            ["Quince Pie, this page"],
            "thin recipes are not phantoms without a nav to contradict them"
        );
        assert_eq!(flags.len(), 1);
    }
}
