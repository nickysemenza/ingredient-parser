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
pub const MIN_NAV_TITLES: usize = 12;
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
/// Phrases a printed title carries that are not part of its name.
pub const PHOTO_POINTERS: &[&str] = &[
    "pictured here",
    "pictured opposite",
    "pictured on page",
    "photograph here",
    "photographs here",
    "shown here",
    "see photograph",
];

/// `title` without photo pointers such as `{Pictured here}`.
pub fn strip_photo_pointers(title: &str) -> String {
    let mut out = title.to_string();
    for pointer in PHOTO_POINTERS {
        loop {
            let lower = out.to_lowercase();
            let Some(at) = lower.find(pointer) else { break };
            let mut start = at;
            let mut end = at + pointer.len();
            // Take the surrounding brackets or parentheses with it.
            let bytes = out.as_bytes();
            let open = out[..start].rfind(['{', '(', '[']);
            if let Some(o) = open
                && out[o..start].trim_matches(['{', '(', '[', ' ']).is_empty()
            {
                start = o;
                if let Some(c) = out[end..].find(['}', ')', ']']) {
                    end += c + 1;
                }
            }
            let _ = bytes;
            out.replace_range(start..end, " ");
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn normalize_title(title: &str) -> String {
    let folded: String = strip_photo_pointers(title)
        .nfkc()
        .collect::<String>()
        .to_lowercase();
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
    let words = short.split(' ').count();
    // A dropped subtitle ("Sour Cherry Pie" / "Sour Cherry Pie: A Summer
    // Classic"), or a name printed inside a longer title ("Campagne Boule" /
    // "Pain de Campagne Campagne Boule", "Central Thai–style papaya salad" /
    // "Som Tam Thai Central Thai–style papaya salad"). A one-word name takes
    // a subtitle of two or more words ("Ratatouille" / "Ratatouille Provençal
    // vegetable stew"); callers match exact titles first, so "Polenta" is
    // only taken for "Polenta with Fresh Corn" when no plain "Polenta" is
    // left.
    if long.starts_with(short.as_str())
        && (words >= 2 || long.split(' ').count() >= 3)
        && long[short.len()..].starts_with(' ')
    {
        return true;
    }
    if words >= 2 && short.len() >= 12 && contains_tokens(long, short) {
        return true;
    }
    strsim::normalized_levenshtein(&a, &b) >= SIMILARITY
}

/// `needle` appears in `hay` as whole space-separated tokens.
fn contains_tokens(hay: &str, needle: &str) -> bool {
    hay == needle
        || hay.starts_with(&format!("{needle} "))
        || hay.ends_with(&format!(" {needle}"))
        || hay.contains(&format!(" {needle} "))
}

/// Lines after a contents target within which the first ingredient run must
/// start for the entry to be a recipe (a title page plus headnote fits).
const NAV_RUN_WINDOW: usize = 60;
/// A contents section holding more ingredient runs than this is a chapter or
/// part, not a recipe with a few sub-recipes.
const NAV_MAX_RUNS_PER_RECIPE: usize = 6;

/// Contents entries that name recipes: `(entry order, label, line)`. Whatever
/// the nesting, an entry is a recipe when the text between its target and the
/// next entry's target holds one to a few ingredient runs, the first of them
/// close to the target. Chapter entries hold many; essay entries hold none.
fn nav_recipe_entries(book: &BookLines, nav: &Nav) -> Vec<(usize, String, usize)> {
    let mut targets: Vec<usize> = book.nav_targets.iter().map(|(_, l)| *l).collect();
    targets.sort_unstable();
    let line_of = |order: usize| {
        book.nav_targets
            .iter()
            .find(|(o, _)| *o == order)
            .map(|(_, l)| *l)
    };
    let mut out = Vec::new();
    for (index, entry) in nav.entries.iter().enumerate() {
        let Some(line) = line_of(entry.order) else {
            continue;
        };
        if is_front_matter(&entry.label) {
            continue;
        }
        // An entry with entries nested under it is a chapter or a section,
        // not a recipe, whether or not the first of them shares its target
        // line (an entry without a fragment).
        let container = nav
            .entries
            .get(index + 1)
            .is_some_and(|next| next.depth > entry.depth)
            || nav
                .entries
                .iter()
                .any(|e| e.depth > entry.depth && line_of(e.order) == Some(line));
        if container {
            continue;
        }
        let next_target = targets
            .iter()
            .copied()
            .find(|&l| l > line)
            .unwrap_or(book.len());
        let window = next_target.saturating_sub(line).clamp(1, NAV_RUN_WINDOW);
        let run_start = |i: usize| book.ingredient_run_start(i) && !is_contents_run(book, i);
        let runs = (line..next_target).filter(|&i| run_start(i)).count();
        let is_recipe = (1..=NAV_MAX_RUNS_PER_RECIPE).contains(&runs)
            && (line..(line + window).min(book.len())).any(run_start);
        if is_recipe {
            out.push((entry.order, entry.label.clone(), line));
        }
    }
    out
}

/// Whether the line at `idx` sits in a chapter's own little table of contents
/// rather than an ingredient list. Each such line is nothing but one link to
/// elsewhere in the book, and a column of short noun phrases ("A Note on
/// Baking Materials", "Essential Bakeware", "Ten Tips For Better Baking")
/// parses as an ingredient run exactly like the real thing. A neighbour of
/// the same shape settles it: an ingredient line that links to a sub-recipe
/// stands among lines that are not links.
fn is_contents_run(book: &BookLines, idx: usize) -> bool {
    let whole_line_link = |i: usize| {
        book.lines.get(i).is_some_and(|l| {
            let text = l.clean.text.trim();
            !text.is_empty() && l.clean.links.len() == 1 && l.clean.links[0].text.trim() == text
        })
    };
    whole_line_link(idx)
        && (whole_line_link(idx + 1) || (idx > 0 && whole_line_link(idx - 1)))
}

/// Contents entries that are never recipes whatever follows them.
fn is_front_matter(label: &str) -> bool {
    let l = label.trim().to_ascii_lowercase();
    let l = l.trim_end_matches(['.', ':']);
    matches!(
        l,
        "cover"
            | "title page"
            | "half title"
            | "copyright"
            | "copyright page"
            | "contents"
            | "table of contents"
            | "dedication"
            | "epigraph"
            | "foreword"
            | "preface"
            | "introduction"
            | "acknowledgments"
            | "acknowledgements"
            | "index"
            | "glossary"
            | "resources"
            | "sources"
            | "bibliography"
            | "notes"
            | "about the author"
            | "about the authors"
            | "also by the author"
            | "conversion chart"
            | "conversion charts"
            | "measurement conversions"
    ) || l.starts_with("also by ")
        || l.starts_with("praise for ")
}

/// The nav entries that name recipes, as `(title, line)`.
pub fn nav_recipe_titles(book: &BookLines, nav: &Nav) -> Vec<(String, usize)> {
    nav_recipe_entries(book, nav)
        .into_iter()
        .map(|(_, label, line)| (label, line))
        .collect()
}

/// Nav entries that head chapters: depth-1 entries, minus the ones that are
/// recipes in a flat contents.
pub fn nav_chapter_entries(book: &BookLines, nav: &Nav) -> Vec<(String, usize)> {
    let recipes: HashSet<usize> = nav_recipe_entries(book, nav)
        .iter()
        .map(|(order, _, _)| *order)
        .collect();
    nav.entries
        .iter()
        .filter(|e| e.depth == 1 && !recipes.contains(&e.order))
        .filter_map(|e| {
            book.nav_targets
                .iter()
                .find(|(o, _)| *o == e.order)
                .map(|(_, line)| (e.label.clone(), *line))
        })
        .collect()
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
    // Exact titles pair off first so a lenient match never steals a recipe
    // that another contents entry names exactly.
    let exact = |a: &str, b: &str| normalize_title(a) == normalize_title(b);
    for lenient in [false, true] {
        for (ni, (title, _)) in nav_titles.iter().enumerate() {
            if matched_nav.contains(&ni) {
                continue;
            }
            if let Some((ei, _)) = extracted.iter().enumerate().find(|(ei, e)| {
                !matched_extracted.contains(ei)
                    && if lenient {
                        titles_match(title, &e.title)
                    } else {
                        exact(title, &e.title)
                    }
            }) {
                matched_nav.insert(ni);
                matched_extracted.insert(ei);
            }
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
    #[case("Polenta", "Polenta with Fresh Corn", true)]
    #[case("Ratatouille", "Ratatouille Provençal vegetable stew", true)]
    #[case("Campagne Boule", "Pain de Campagne Campagne Boule", true)]
    #[case(
        "CENTRAL THAI–STYLE PAPAYA SALAD",
        "Som Tam Thai (Central Thai–Style Papaya Salad)",
        true
    )]
    #[case("Salt", "Salted Caramel", false)]
    #[case(
        "Kaeng Khanun {Pictured here} NORTHERN THAI YOUNG JACKFRUIT CURRY",
        "Kaeng Khanun (Northern Thai Young Jackfruit Curry)",
        true
    )]
    #[case("Bread", "Broth", false)]
    #[case("", "Bread", false)]
    fn matching(#[case] a: &str, #[case] b: &str, #[case] expected: bool) {
        assert_eq!(titles_match(a, b), expected);
    }

    /// Twelve recipes: enough contents entries for recall to be judged.
    const PIES: [&str; 12] = [
        "Apple Pie",
        "Cherry Pie",
        "Peach Pie",
        "Plum Pie",
        "Pear Pie",
        "Fig Pie",
        "Quince Tart",
        "Rhubarb Pie",
        "Blueberry Pie",
        "Pecan Pie",
        "Pumpkin Pie",
        "Lemon Pie",
    ];

    fn book() -> (BookLines, Nav) {
        let mut html = String::from("<h1 id=\"ch\">Pies</h1>");
        for (i, title) in PIES.iter().enumerate() {
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
        for (i, title) in PIES.iter().enumerate() {
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
        assert_eq!(nav_titles.len(), 12, "the depth-1 chapter is not a recipe");
        let cap_line = (0..book.len())
            .find(|&i| book.text(i).starts_with("Quince Pie"))
            .unwrap();
        // Every pie but Fig, one of them with a case difference; each recipe
        // occupies four lines after the chapter heading.
        let mut got: Vec<ExtractedTitle> = PIES
            .iter()
            .enumerate()
            .filter(|(_, t)| **t != "Fig Pie")
            .map(|(i, t)| {
                let title = if i == 2 { "Peach pie" } else { t };
                extracted(0, title, Some(1 + 4 * i), 2, 1)
            })
            .collect();
        // A caption came back as a recipe.
        got.push(extracted(0, "Quince Pie, this page", Some(cap_line), 0, 0));
        // A real unlisted recipe with content is not a phantom.
        got.push(extracted(0, "Bonus Pie", None, 6, 4));
        let (check, flags) = crosscheck(&book, &nav, &chunks, &got);
        assert_eq!(check.nav_titles, 12);
        assert_eq!(check.matched, 11);
        assert_eq!(check.missing, ["Fig Pie"]);
        assert_eq!(check.phantom, ["Quince Pie, this page"]);
        assert!((check.recall.unwrap() - 11.0 / 12.0).abs() < 1e-6);
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

    /// A chapter whose first page is its own little table of contents
    /// ("Flour", "Leaveners", "Sugar"…, each a bare link) is not a recipe:
    /// those lines parse as an ingredient run, which used to make the
    /// contents entry a recipe that nothing could ever match. The Cook's
    /// Illustrated Baking Book's "Baking Basics" is this shape.
    #[rstest]
    #[case(true, false)]
    #[case(false, true)]
    fn a_chapter_toc_is_not_an_ingredient_run(#[case] linked: bool, #[case] expected: bool) {
        const ENTRIES: [&str; 3] = [
            "A Note on Baking Materials",
            "Essential Bakeware",
            "Ten Tips For Better Baking",
        ];
        let mut html = String::from("<h2 id=\"basics\">Baking Basics</h2>");
        for entry in ENTRIES {
            let cell = if linked {
                format!("<a href=\"#f\">{entry}</a>")
            } else {
                entry.to_string()
            };
            html.push_str(&format!("<p>{cell}</p>"));
        }
        html.push_str("<p id=\"f\">Flour is a powder.</p>");
        let doc = SpineDoc {
            index: 0,
            path: "c.xhtml".into(),
            xhtml: format!("<html><body>{html}</body></html>"),
        };
        let nav = Nav {
            entries: vec![NavEntry {
                label: "Baking Basics".into(),
                doc_path: "c.xhtml".into(),
                fragment: Some("basics".into()),
                depth: 1,
                order: 0,
            }],
            page_list: vec![],
        };
        let book = BookLines::build(&[doc], &nav);
        assert_eq!(
            nav_recipe_titles(&book, &nav).len() == 1,
            expected,
            "linked={linked}"
        );
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
            .find(|&i| book.text(i).starts_with("Quince Pie"))
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
