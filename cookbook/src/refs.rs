//! Cross-references between items: an ingredient line naming another recipe,
//! a step pointing at a technique, a note citing a page. Resolved by the
//! strongest evidence available: an internal link's target, then a printed
//! page number, then the target's title in the text.

use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

use petgraph::algo::{tarjan_scc, toposort};
use petgraph::graphmap::DiGraphMap;
use regex::Regex;

use crate::crosscheck::normalize_title;
use crate::lines::BookLines;
use crate::model::{Chapter, Cookbook, Edge, ImageRef, Item, RecipeRef, RefKind, RefMethod};
use crate::report::UnresolvedRef;

static PAGE_REF: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(?:see\s+)?(?:page|p\.)\s*(\d{1,4})\b")
        .unwrap_or_else(|e| unreachable!("{e}"))
});

/// Words that signal a cross-reference even when the title is short.
const MARKERS: &[&str] = &[
    "recipe",
    "this page",
    "see ",
    "opposite",
    "page",
    "above",
    "below",
    "here",
    "preceding",
    "following",
];

struct Index<'a> {
    book: &'a BookLines,
    ids: Vec<String>,
    /// Line → item ordinal.
    owner: Vec<Option<usize>>,
    spans: Vec<(usize, usize, Option<usize>)>,
    /// Unique normalized titles, longest first.
    titles: Vec<(String, String, usize)>,
}

impl Index<'_> {
    fn owner_of(&self, line: usize) -> Option<usize> {
        self.owner.get(line).copied().flatten()
    }

    /// The item printed on page `n`: the one whose title starts on it, else
    /// the first whose span reaches it.
    fn item_for_page(&self, n: u32) -> Option<usize> {
        let start = self.book.page_line(n)?;
        let end = self.book.next_page_line(n).unwrap_or(self.book.len());
        self.spans
            .iter()
            .position(|(_, _, title)| title.is_some_and(|t| start <= t && t < end))
            .or_else(|| {
                self.spans
                    .iter()
                    .position(|(s, e, _)| *s < end && start < *e)
            })
    }
}

/// Resolve references on every ingredient line, step, note, and caption, and
/// derive the edge list. Returns the edges and what could not be resolved.
pub fn resolve(
    chapters: &mut [Chapter],
    book: &BookLines,
    orphan_photos: Vec<ImageRef>,
) -> (Vec<Edge>, Vec<UnresolvedRef>) {
    let items: Vec<&Item> = chapters.iter().flat_map(|c| c.items.iter()).collect();
    let ids: Vec<String> = items.iter().map(|i| i.id().to_string()).collect();
    let spans: Vec<(usize, usize, Option<usize>)> = items
        .iter()
        .map(|i| {
            let s = i.span();
            let title = (s.start..s.end)
                .find(|&l| book.lines.get(l).is_some_and(|line| line.id() == i.id()));
            (s.start, s.end, title)
        })
        .collect();
    let mut owner = vec![None; book.len()];
    for (ordinal, (s, e, _)) in spans.iter().enumerate() {
        for slot in owner.iter_mut().take((*e).min(book.len())).skip(*s) {
            if slot.is_none() {
                *slot = Some(ordinal);
            }
        }
    }
    let mut counts: HashMap<String, usize> = HashMap::new();
    for i in &items {
        *counts.entry(normalize_title(i.title())).or_default() += 1;
    }
    let mut titles: Vec<(String, String, usize)> = items
        .iter()
        .enumerate()
        .filter(|(_, i)| counts.get(&normalize_title(i.title())) == Some(&1))
        .map(|(o, i)| (normalize_title(i.title()), i.title().to_string(), o))
        .filter(|(n, _, _)| !n.is_empty())
        .collect();
    titles.sort_by_key(|(normalized, _, _)| std::cmp::Reverse(normalized.len()));
    let index = Index {
        book,
        ids,
        owner,
        spans,
        titles,
    };

    let mut edges: Vec<Edge> = Vec::new();
    let mut unresolved: Vec<UnresolvedRef> = Vec::new();
    let mut ordinal = 0usize;
    for chapter in chapters.iter_mut() {
        for item in chapter.items.iter_mut() {
            let self_id = item.id().to_string();
            let mut ctx = Ctx {
                index: &index,
                me: ordinal,
                self_id: &self_id,
                edges: &mut edges,
                unresolved: &mut unresolved,
            };
            match item {
                Item::Recipe(r) => {
                    for section in &mut r.sections {
                        for line in &mut section.ingredients {
                            line.reference = ctx
                                .resolve(&line.raw, line.line, RefKind::Ingredient)
                                .into_iter()
                                .next();
                        }
                        for step in &mut section.steps {
                            step.refs = ctx.resolve(&step.text, step.line, RefKind::Step);
                        }
                    }
                    for note in &mut r.notes {
                        note.refs = ctx.resolve(&note.text, note.line, RefKind::Note);
                    }
                    for photo in &r.photos {
                        if let (Some(caption), Some(line)) = (&photo.caption, photo.line) {
                            ctx.resolve(caption, line, RefKind::Caption);
                        }
                    }
                    if let Some(parent) = &r.variant_of {
                        ctx.edges.push(Edge {
                            from: self_id.clone(),
                            to: parent.clone(),
                            kind: RefKind::Variation,
                            method: RefMethod::Title,
                        });
                    }
                }
                Item::Technique(t) => {
                    for step in &mut t.steps {
                        step.refs = ctx.resolve(&step.text, step.line, RefKind::Step);
                    }
                }
                Item::Essay(_) => {}
            }
            ordinal += 1;
        }
    }
    let mut seen = HashSet::new();
    edges.retain(|e| seen.insert(e.clone()));
    move_captioned_photos(chapters, &edges);
    place_orphan_photos(chapters, &index, orphan_photos);
    (edges, unresolved)
}

/// A captioned photo outside every span goes to the item its caption names:
/// by the caption's link, its printed page, or a title in its text.
fn place_orphan_photos(chapters: &mut [Chapter], index: &Index<'_>, orphans: Vec<ImageRef>) {
    for photo in orphans {
        let Some(line) = photo.line else {
            continue;
        };
        let caption = photo.caption.clone().unwrap_or_default();
        let mut target: Option<usize> = None;
        if let Some(l) = index.book.lines.get(line) {
            let doc_path = index.book.doc_path(line);
            target = l
                .clean
                .links
                .iter()
                .find_map(|link| index.book.resolve_href(doc_path, &link.href))
                .and_then(|t| index.owner_of(t));
        }
        if target.is_none() {
            target = PAGE_REF
                .captures_iter(&caption)
                .find_map(|c| c.get(1).and_then(|m| m.as_str().parse::<u32>().ok()))
                .and_then(|n| index.item_for_page(n));
        }
        if target.is_none() {
            let normalized = normalize_title(&caption);
            target = index
                .titles
                .iter()
                .find(|(n, _, _)| contains_whole_tokens(&normalized, n))
                .map(|(_, _, o)| *o);
        }
        let Some(target) = target else {
            continue;
        };
        if let Some(item) = chapters
            .iter_mut()
            .flat_map(|c| c.items.iter_mut())
            .nth(target)
        {
            item.photos_mut().push(photo);
        }
    }
}

/// A photo whose caption names another recipe belongs to that recipe.
fn move_captioned_photos(chapters: &mut [Chapter], edges: &[Edge]) {
    let moves: Vec<(String, String)> = edges
        .iter()
        .filter(|e| e.kind == RefKind::Caption)
        .map(|e| (e.from.clone(), e.to.clone()))
        .collect();
    for (from, to) in moves {
        let mut taken: Vec<crate::model::ImageRef> = Vec::new();
        for item in chapters.iter_mut().flat_map(|c| c.items.iter_mut()) {
            if item.id() == from
                && let Item::Recipe(r) = item
            {
                let (moved, kept): (Vec<_>, Vec<_>) =
                    r.photos.drain(..).partition(|p| p.caption.is_some());
                r.photos = kept;
                taken = moved;
            }
        }
        for item in chapters.iter_mut().flat_map(|c| c.items.iter_mut()) {
            if item.id() == to
                && let Item::Recipe(r) = item
            {
                r.photos.append(&mut taken);
            }
        }
    }
}

struct Ctx<'a, 'b> {
    index: &'a Index<'b>,
    me: usize,
    self_id: &'a str,
    edges: &'a mut Vec<Edge>,
    unresolved: &'a mut Vec<UnresolvedRef>,
}

impl Ctx<'_, '_> {
    fn resolve(&mut self, text: &str, line: usize, kind: RefKind) -> Vec<RecipeRef> {
        let index = self.index;
        let mut refs: Vec<RecipeRef> = Vec::new();
        let mut attempted: Vec<RefMethod> = Vec::new();
        let push = |target: usize,
                    text: String,
                    method: RefMethod,
                    refs: &mut Vec<RecipeRef>,
                    edges: &mut Vec<Edge>| {
            if target == self.me {
                return;
            }
            let target_id = index.ids[target].clone();
            if refs.iter().any(|r| r.target_id == target_id) {
                return;
            }
            edges.push(Edge {
                from: self.self_id.to_string(),
                to: target_id.clone(),
                kind,
                method,
            });
            refs.push(RecipeRef {
                target_id,
                text,
                kind,
                method,
            });
        };

        // 1. Anchors.
        if let Some(l) = index.book.lines.get(line) {
            let doc_path = index.book.doc_path(line);
            for link in &l.clean.links {
                attempted.push(RefMethod::Anchor);
                if let Some(target_line) = index.book.resolve_href(doc_path, &link.href)
                    && let Some(target) = index.owner_of(target_line)
                {
                    push(
                        target,
                        link.text.clone(),
                        RefMethod::Anchor,
                        &mut refs,
                        self.edges,
                    );
                }
            }
        }
        // 2. Printed page numbers.
        for cap in PAGE_REF.captures_iter(text) {
            attempted.push(RefMethod::Page);
            if let Some(n) = cap.get(1).and_then(|m| m.as_str().parse::<u32>().ok())
                && let Some(target) = index.item_for_page(n)
            {
                push(
                    target,
                    cap.get(0)
                        .map(|m| m.as_str().to_string())
                        .unwrap_or_default(),
                    RefMethod::Page,
                    &mut refs,
                    self.edges,
                );
            }
        }
        // 3. Titles in the text.
        let lower = text.to_lowercase();
        let has_marker = MARKERS.iter().any(|m| lower.contains(m));
        if !refs.is_empty() || kind != RefKind::Ingredient || has_marker || !attempted.is_empty() {
            attempted.push(RefMethod::Title);
        }
        let normalized = normalize_title(&text.replace(['{', '}'], " "));
        for (norm_title, title, target) in &index.titles {
            if *target == self.me {
                continue;
            }
            let short = norm_title.split(' ').count() < 3 && norm_title.len() < 12;
            if short && !has_marker {
                continue;
            }
            if contains_whole_tokens(&normalized, norm_title) {
                if !attempted.contains(&RefMethod::Title) {
                    attempted.push(RefMethod::Title);
                }
                push(
                    *target,
                    title.clone(),
                    RefMethod::Title,
                    &mut refs,
                    self.edges,
                );
                if kind == RefKind::Ingredient {
                    break;
                }
            }
        }
        if refs.is_empty()
            && (attempted.contains(&RefMethod::Anchor) || attempted.contains(&RefMethod::Page))
        {
            self.unresolved.push(UnresolvedRef {
                item_id: self.self_id.to_string(),
                line,
                text: text.to_string(),
                attempted,
            });
        }
        refs
    }
}

/// `needle`'s tokens appear consecutively in `haystack` on token boundaries.
pub fn contains_whole_tokens(haystack: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return false;
    }
    let h: Vec<&str> = haystack.split(' ').collect();
    let n: Vec<&str> = needle.split(' ').collect();
    h.windows(n.len()).any(|w| w == n.as_slice())
}

/// Items in an order where every dependency (ingredient and variation edges)
/// comes before the item that needs it. A cycle is broken by dropping the
/// earlier items' forward references to its latest item, so the later item
/// still links back to what came before it (a consumer then treats the
/// forward reference as a plain ingredient).
pub fn dependency_order(cookbook: &Cookbook) -> Vec<String> {
    let ids: Vec<&str> = cookbook.items().map(Item::id).collect();
    let position: HashMap<&str, usize> = ids.iter().enumerate().map(|(i, id)| (*id, i)).collect();
    let mut graph: DiGraphMap<usize, ()> = DiGraphMap::new();
    for (i, _) in ids.iter().enumerate() {
        graph.add_node(i);
    }
    for edge in &cookbook.edges {
        if !matches!(edge.kind, RefKind::Ingredient | RefKind::Variation) {
            continue;
        }
        if let (Some(&from), Some(&to)) = (
            position.get(edge.from.as_str()),
            position.get(edge.to.as_str()),
        ) && from != to
        {
            graph.add_edge(to, from, ());
        }
    }
    loop {
        match toposort(&graph, None) {
            Ok(order) => {
                // Stable: among independent items keep reading order.
                let mut result: Vec<usize> = Vec::with_capacity(order.len());
                let mut placed = vec![false; ids.len()];
                let mut remaining: Vec<usize> = (0..ids.len()).collect();
                while !remaining.is_empty() {
                    let next = remaining
                        .iter()
                        .copied()
                        .find(|&i| {
                            graph
                                .neighbors_directed(i, petgraph::Direction::Incoming)
                                .all(|d| placed[d])
                        })
                        .unwrap_or(remaining[0]);
                    placed[next] = true;
                    result.push(next);
                    remaining.retain(|&i| i != next);
                }
                return result.into_iter().map(|i| ids[i].to_string()).collect();
            }
            Err(_) => {
                for scc in tarjan_scc(&graph) {
                    if scc.len() < 2 {
                        continue;
                    }
                    let latest = scc.iter().copied().max().unwrap_or(0);
                    // Edges run dependency → item; an earlier item depending
                    // on the latest one is the forward reference to drop.
                    let cut: Vec<usize> = scc
                        .iter()
                        .copied()
                        .filter(|&n| n != latest && graph.contains_edge(latest, n))
                        .collect();
                    for n in cut {
                        graph.remove_edge(latest, n);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::model::{BookSource, Recipe, RecipeMeta, Span};

    fn recipe(id: &str) -> Item {
        Item::Recipe(Box::new(Recipe {
            id: id.into(),
            title: id.into(),
            name: id.into(),
            meta: RecipeMeta::default(),
            sections: vec![],
            photos: vec![],
            notes: vec![],
            variant_of: None,
            span: Span {
                start: 0,
                end: 0,
                doc_path: String::new(),
                page: None,
            },
        }))
    }

    fn cookbook(ids: &[&str], edges: &[(&str, &str)]) -> Cookbook {
        Cookbook {
            contract: String::new(),
            source: BookSource {
                label: String::new(),
                sha256: String::new(),
                title: String::new(),
                authors: vec![],
                identifiers: vec![],
                subjects: vec![],
                spine_docs: 0,
                lines: 0,
            },
            cover: None,
            chapters: vec![Chapter {
                id: "ch00".into(),
                title: None,
                span: Span {
                    start: 0,
                    end: 0,
                    doc_path: String::new(),
                    page: None,
                },
                intro: vec![],
                items: ids.iter().map(|i| recipe(i)).collect(),
            }],
            edges: edges
                .iter()
                .map(|(f, t)| Edge {
                    from: f.to_string(),
                    to: t.to_string(),
                    kind: RefKind::Ingredient,
                    method: RefMethod::Anchor,
                })
                .collect(),
        }
    }

    #[test]
    fn dependencies_come_first_and_cycles_break_at_the_later_item() {
        let c = cookbook(
            &["pie", "crust", "sauce"],
            &[("pie", "crust"), ("pie", "sauce")],
        );
        assert_eq!(dependency_order(&c), ["crust", "sauce", "pie"]);
        let c = cookbook(&["a", "b", "c"], &[("a", "b"), ("b", "a")]);
        assert_eq!(
            dependency_order(&c),
            ["a", "b", "c"],
            "a's forward reference to b is dropped; b still follows a"
        );
        let c = cookbook(&["a", "b"], &[]);
        assert_eq!(dependency_order(&c), ["a", "b"]);
    }

    #[test]
    fn whole_token_containment() {
        assert!(contains_whole_tokens(
            "1 recipe polenta made without butter",
            "polenta"
        ));
        assert!(!contains_whole_tokens("polentas", "polenta"));
        assert!(contains_whole_tokens(
            "graham cracker crust speculoos variation",
            "graham cracker crust"
        ));
        assert!(!contains_whole_tokens("x", ""));
    }
}
