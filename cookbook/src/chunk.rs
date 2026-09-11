//! Cut the book's line stream into model-sized chunks.
//!
//! A chunk is a contiguous range of global lines, about `CHUNK_BUDGET` chars.
//! Breaks prefer lines that start something: a table-of-contents target, a
//! heading, or a title-like line with an ingredient run soon after. A break is
//! never placed between a title-like line and the ingredient run that follows
//! it, so a recipe whose title sits in one spine document and whose body sits
//! in the next (Calibre page-split books) stays whole. Document boundaries are
//! not special. When no clean boundary appears within `CHUNK_SLACK`, the chunk
//! is cut hard and the next chunk carries the interrupted recipe's title as a
//! `title_hint` for the continuation contract.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::lines::{BookLines, looks_like_yield};

/// Target chunk size in characters. Large enough that one long recipe stays
/// whole; small enough that the model's index-only answer stays far below the
/// output cap.
pub const CHUNK_BUDGET: usize = 12_000;
/// Extra characters accepted while looking for a clean boundary.
pub const CHUNK_SLACK: usize = 6_000;
/// How far after a title an ingredient run may start and still belong to it.
const RUN_WINDOW: usize = 60;
/// Lines a heading's ingredient list proper may sit past its title.
const SOLID_RUN_WINDOW: usize = 80;

/// Why a chunk begins where it does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Boundary {
    /// The first chunk.
    Start,
    /// A table-of-contents or internal-link target.
    NavTarget,
    Heading,
    TitleLike,
    /// Over budget with no clean boundary; the previous recipe may continue.
    Hard,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Chunk {
    /// `"k000"`, `"k001"`, …
    pub id: String,
    pub index: usize,
    /// First global line.
    pub start: usize,
    /// One past the last global line.
    pub end: usize,
    pub chars: usize,
    /// The title of a recipe cut by a hard split before this chunk.
    pub title_hint: Option<String>,
    pub boundary: Boundary,
}

impl Chunk {
    pub fn lines(&self) -> usize {
        self.end - self.start
    }

    pub fn contains(&self, idx: usize) -> bool {
        self.start <= idx && idx < self.end
    }

    /// Global line index for a chunk-local one.
    pub fn global(&self, local: usize) -> usize {
        self.start + local
    }

    /// The chunk as the model sees it: `"{local}: {text}"` per line.
    /// The numbered lines the model reads. Lines inside a figure carry a
    /// `[caption]` marker so a caption that names a dish is not mistaken for
    /// its title; the marker is never copied into output (Rust copies text
    /// by index).
    pub fn text(&self, book: &BookLines) -> String {
        (self.start..self.end)
            .enumerate()
            .map(|(local, idx)| {
                if book.lines[idx].clean.in_figure {
                    format!("{local}: {CAPTION_MARKER} {}", book.text(idx))
                } else {
                    format!("{local}: {}", book.text(idx))
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// Prefix on figure lines in the chunk text.
pub const CAPTION_MARKER: &str = "[caption]";

#[derive(Debug, Clone, Copy)]
pub struct ChunkOptions {
    pub budget: usize,
    pub slack: usize,
}

impl Default for ChunkOptions {
    fn default() -> Self {
        Self {
            budget: CHUNK_BUDGET,
            slack: CHUNK_SLACK,
        }
    }
}

pub fn chunk(book: &BookLines, opts: &ChunkOptions) -> Vec<Chunk> {
    let n = book.len();
    if n == 0 {
        return Vec::new();
    }
    let title_like: Vec<bool> = (0..n).map(|i| book.title_like(i)).collect();
    // Lines that begin a recipe body soon after a title; the range between a
    // title and its run must not be cut.
    let mut protected = vec![false; n];
    let mut run_after_title = vec![false; n];
    for (i, _) in title_like.iter().enumerate().filter(|(_, t)| **t) {
        if let Some(run) = book.next_ingredient_run(i + 1, RUN_WINDOW) {
            run_after_title[i] = true;
            for p in &mut protected[i + 1..=run] {
                *p = true;
            }
        }
        // A heading's recipe body may be preceded by a plan or an equipment
        // list with a number in it; protect up to the real ingredient list,
        // but never past the next heading (that one starts something else).
        if book.lines[i].clean.heading.is_some()
            && let Some(run) = book.next_solid_run(i + 1, SOLID_RUN_WINDOW)
        {
            let next_heading = (i + 1..run)
                .find(|&j| book.lines[j].clean.heading.is_some())
                .unwrap_or(run);
            if next_heading >= run {
                for p in &mut protected[i + 1..=run] {
                    *p = true;
                }
            }
        }
    }
    let mut nav_lines: HashSet<usize> = book.nav_targets.iter().map(|&(_, line)| line).collect();
    for line in &book.lines {
        let doc_path = book.doc_path(line.idx);
        for link in &line.clean.links {
            if let Some(target) = book.resolve_href(doc_path, &link.href)
                && title_like[target]
            {
                nav_lines.insert(target);
            }
        }
    }
    // A heading straight after an ingredient line or a yield line heads an
    // ingredient group ("PASTE", "FISH"), not a recipe: never cut there.
    let after_list = |i: usize| -> bool {
        i > 0 && (book.quantity_like(i - 1) || looks_like_yield(book.text(i - 1)))
    };
    let candidate = |i: usize| -> Option<Boundary> {
        if protected[i] || after_list(i) {
            return None;
        }
        if nav_lines.contains(&i) {
            return Some(Boundary::NavTarget);
        }
        if run_after_title[i] {
            return Some(if book.lines[i].clean.heading.is_some() {
                Boundary::Heading
            } else {
                Boundary::TitleLike
            });
        }
        None
    };

    let mut chunks = Vec::new();
    let mut start = 0usize;
    let mut len = 0usize;
    let mut boundary = Boundary::Start;
    let mut hint: Option<String> = None;
    let mut last_title: Option<String> = None;
    // The hint names the recipe a hard cut lands inside. A heading (or a
    // contents target) is that name until its ingredient run has started;
    // plain title-like lines in between ("THE PLAN", a wine pairing, a
    // sidebar) are not.
    let mut heading_pending = false;
    for (i, &is_title) in title_like.iter().enumerate() {
        let line_len = book.text(i).len() + 1;
        if i > start && len >= opts.budget {
            // A contents target is a real title; a guessed boundary may sit
            // mid-recipe, so carry the last title in case the model needs it.
            let cut = match candidate(i) {
                Some(Boundary::NavTarget) => Some((Boundary::NavTarget, None)),
                Some(kind) => Some((kind, last_title.clone())),
                None if len >= opts.budget + opts.slack => {
                    Some((Boundary::Hard, last_title.clone()))
                }
                None => None,
            };
            if let Some((kind, next_hint)) = cut {
                chunks.push(Chunk {
                    id: format!("k{:03}", chunks.len()),
                    index: chunks.len(),
                    start,
                    end: i,
                    chars: len,
                    title_hint: hint.take(),
                    boundary,
                });
                start = i;
                len = 0;
                boundary = kind;
                hint = next_hint;
            }
        }
        if book.solid_run_start(i) {
            heading_pending = false;
        }
        if is_title {
            let strong =
                nav_lines.contains(&i) || (book.lines[i].clean.heading.is_some() && !after_list(i));
            if strong {
                last_title = Some(book.text(i).to_string());
                heading_pending = true;
            } else if !heading_pending && !crate::validate::is_label(book.text(i)) {
                last_title = Some(book.text(i).to_string());
            }
        }
        len += line_len;
    }
    chunks.push(Chunk {
        id: format!("k{:03}", chunks.len()),
        index: chunks.len(),
        start,
        end: n,
        chars: len,
        title_hint: hint,
        boundary,
    });
    chunks
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::epub::nav::{Nav, NavEntry};
    use crate::epub::open::SpineDoc;

    fn doc(index: usize, path: &str, body: &str) -> SpineDoc {
        SpineDoc {
            index,
            path: path.into(),
            xhtml: format!("<html><body>{body}</body></html>"),
        }
    }

    fn recipe_html(title: &str, ingredients: usize, steps: usize) -> String {
        let mut s = format!("<h2>{title}</h2><p>Serves 4</p>");
        for i in 0..ingredients {
            s.push_str(&format!(
                "<p>{} cups ingredient number {i} for {title}</p>",
                i + 1
            ));
        }
        for i in 0..steps {
            s.push_str(&format!(
                "<p>Step {i}: do the thing carefully and then do the next thing until it is done for {title}.</p>"
            ));
        }
        s
    }

    fn covers_every_line(chunks: &[Chunk], n: usize) {
        assert_eq!(chunks[0].start, 0);
        assert_eq!(chunks.last().unwrap().end, n);
        for pair in chunks.windows(2) {
            assert_eq!(pair[0].end, pair[1].start);
        }
        for (i, c) in chunks.iter().enumerate() {
            assert_eq!(c.index, i);
            assert_eq!(c.id, format!("k{i:03}"));
        }
    }

    #[test]
    fn breaks_at_titles_and_never_inside_a_title_to_run_gap() {
        // Page-split shape: recipe B's title (and photo) ends doc 1; its body
        // opens doc 2. The budget lands the natural break right there.
        let a = recipe_html("Recipe A", 6, 3);
        let docs = [
            doc(
                0,
                "p1.html",
                &format!("{a}<h2>Recipe B</h2><div><img src=\"b.jpg\"/></div>"),
            ),
            doc(
                1,
                "p2.html",
                &format!(
                    "<p>serves 4 to 8</p><p>A headnote paragraph.</p>{}",
                    &recipe_html("Recipe B body", 6, 3)[..0]
                ),
            ),
            doc(2, "p3.html", &recipe_html("Recipe C", 6, 3)),
        ];
        // Give doc 2 a real body after the headnote.
        let mut docs = docs;
        docs[1].xhtml = docs[1].xhtml.replace(
            "</body>",
            "<p>2 pounds mushrooms</p><p>¼ cup olive oil</p><p>Kosher salt</p><p>Roast until browned and crisped.</p></body>",
        );
        let book = BookLines::build(&docs, &Nav::default());
        let a_chars: usize = (0..book.len())
            .take_while(|&i| book.text(i) != "Recipe B")
            .map(|i| book.text(i).len() + 1)
            .sum();
        // Budget reached exactly at "Recipe B" (a title-like line with an
        // ingredient run soon after) → clean TitleLike break there.
        let chunks = chunk(
            &book,
            &ChunkOptions {
                budget: a_chars,
                slack: 10_000,
            },
        );
        covers_every_line(&chunks, book.len());
        assert_eq!(chunks[1].boundary, Boundary::Heading);
        assert_eq!(book.text(chunks[1].start), "Recipe B");
        // Budget reached one line later, inside the protected gap: no break
        // until the next clean candidate ("Recipe C").
        let chunks = chunk(
            &book,
            &ChunkOptions {
                budget: a_chars + 5,
                slack: 10_000,
            },
        );
        covers_every_line(&chunks, book.len());
        assert_eq!(book.text(chunks[1].start), "Recipe C");
        // Guessed boundaries carry the last title as a hint; the model only
        // uses it when the chunk really starts mid-recipe.
        assert_eq!(chunks[1].title_hint.as_deref(), Some("Recipe B"));
    }

    #[test]
    fn hard_split_carries_the_interrupted_title() {
        let docs = [doc(0, "c.html", &recipe_html("Long Recipe", 8, 30))];
        let book = BookLines::build(&docs, &Nav::default());
        let chunks = chunk(
            &book,
            &ChunkOptions {
                budget: 400,
                slack: 200,
            },
        );
        covers_every_line(&chunks, book.len());
        assert!(chunks.len() >= 2);
        assert_eq!(chunks[1].boundary, Boundary::Hard);
        assert_eq!(chunks[1].title_hint.as_deref(), Some("Long Recipe"));
        assert!(
            chunks[2..]
                .iter()
                .all(|c| c.title_hint.as_deref() == Some("Long Recipe"))
        );
        assert!(chunks[0].title_hint.is_none());
    }

    #[test]
    fn nav_targets_are_boundaries_even_without_an_ingredient_run() {
        let docs = [doc(
            0,
            "c.html",
            &format!(
                "{}<p id=\"essay\">Thoughts on Hosting</p><p>Long prose without any quantities at all here.</p>{}",
                recipe_html("A", 4, 2),
                recipe_html("B", 4, 2)
            ),
        )];
        let nav = Nav {
            entries: vec![NavEntry {
                label: "Thoughts on Hosting".into(),
                doc_path: "c.html".into(),
                fragment: Some("essay".into()),
                depth: 2,
                order: 0,
            }],
            page_list: vec![],
        };
        let book = BookLines::build(&docs, &nav);
        let essay = (0..book.len())
            .find(|&i| book.text(i) == "Thoughts on Hosting")
            .unwrap();
        let before: usize = (0..essay).map(|i| book.text(i).len() + 1).sum();
        let chunks = chunk(
            &book,
            &ChunkOptions {
                budget: before,
                slack: 10_000,
            },
        );
        assert_eq!(chunks[1].start, essay);
        assert_eq!(chunks[1].boundary, Boundary::NavTarget);
    }

    #[test]
    fn renders_local_indices() {
        let docs = [doc(0, "c.html", "<p>alpha</p><p>beta</p><p>gamma</p>")];
        let book = BookLines::build(&docs, &Nav::default());
        let chunks = chunk(&book, &ChunkOptions::default());
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].text(&book), "0: alpha\n1: beta\n2: gamma");
        assert_eq!(chunks[0].global(2), 2);
        assert_eq!(chunks[0].boundary, Boundary::Start);
    }

    #[test]
    fn empty_book_has_no_chunks() {
        assert!(chunk(&BookLines::default(), &ChunkOptions::default()).is_empty());
    }
}
