//! Turn chunk answers into the book tree: merge recipes cut across chunks,
//! attach variations, demote phantoms, bind photos, and group items into
//! chapters from the table of contents.

use std::collections::{BTreeSet, HashSet};

use crate::contract::{ChunkItem, Kind, Lowered, Text};
use crate::crosscheck::titles_match;
use crate::epub::nav::Nav;
use crate::lines::BookLines;
use crate::model::{
    Chapter, Essay, ImageRef, IngredientLine, Item, Note, Recipe, RecipeMeta, Section, Span, Step,
    Technique,
};
use crate::parse::LineParser;
use crate::run::ChunkResult;

/// Lines above a title within which an unclaimed image is the item's hero.
const HERO_REACH: usize = 6;
/// Lines after an image within which a figure line is its caption.
const CAPTION_REACH: usize = 3;

/// An item after cross-chunk merging, before conversion.
#[derive(Debug, Clone)]
struct Merged {
    item: ChunkItem,
    /// Ingredient-less variations folded into this item as labelled notes.
    variation_notes: Vec<(String, Vec<Text>)>,
}

/// The tree plus captioned photos that no item's span covered; the reference
/// pass places those by what their caption names.
pub struct Assembled {
    pub chapters: Vec<Chapter>,
    pub orphan_photos: Vec<ImageRef>,
}

pub fn assemble(
    book: &BookLines,
    nav: &Nav,
    results: &[ChunkResult],
    phantoms: &[String],
    parser: &LineParser,
) -> Assembled {
    let mut merged: Vec<Merged> = Vec::new();
    let mut ignored: BTreeSet<usize> = BTreeSet::new();
    let mut chapter_headings: BTreeSet<usize> = BTreeSet::new();
    let mut captions: BTreeSet<usize> = BTreeSet::new();
    let mut previous_chunk_ok = false;

    for result in results {
        let Some(lowered) = &result.lowered else {
            previous_chunk_ok = false;
            continue;
        };
        collect_outside(lowered, &mut ignored, &mut chapter_headings, &mut captions);
        for (index, item) in lowered.items.iter().enumerate() {
            let mut item = item.clone();
            if item.kind == Kind::Recipe && strip_variation_label(&item.title) != item.title.trim()
            {
                // The book's own marker outranks the model's kind.
                item.kind = Kind::Variation;
            }
            if item.continues
                && index == 0
                && previous_chunk_ok
                && let Some(last) = merged.last_mut().filter(|m| m.item.kind == Kind::Recipe)
            {
                merge_into(&mut last.item, item);
                continue;
            }
            item.title = crate::crosscheck::strip_photo_pointers(&item.title);
            if let Some((section, recipe)) = split_group_heading(book, &item) {
                merged.push(Merged {
                    item: section,
                    variation_notes: Vec::new(),
                });
                item = recipe;
            }
            if item.kind == Kind::Recipe && is_formula_table(book, &item) {
                // A baker's formula printed for comparison, with no method:
                // kept as prose, not offered as a recipe.
                item.kind = Kind::Essay;
            }
            if item.continues && item.kind == Kind::Recipe && item.ingredient_count() == 0 {
                // Nothing to continue into and no ingredient list of its own:
                // the hint named a sidebar, not a recipe.
                item.kind = if item.step_count() > 0 {
                    Kind::Technique
                } else {
                    Kind::Essay
                };
            }
            match item.kind {
                Kind::Variation if item.ingredient_count() == 0 => {
                    let parent = parent_index(&merged, &item);
                    match parent {
                        Some(p) => {
                            let mut text = item.description.clone();
                            text.extend(item.notes.iter().cloned());
                            text.extend(item.sections.iter().flat_map(|s| s.steps.iter().cloned()));
                            text.sort_by_key(|t| t.line);
                            merged[p].variation_notes.push((item.title.clone(), text));
                            merged[p].item.last = merged[p].item.last.max(item.last);
                        }
                        None => merged.push(Merged {
                            item,
                            variation_notes: Vec::new(),
                        }),
                    }
                }
                Kind::Variation => {
                    let mut item = item;
                    item.title = strip_variation_label(&item.title);
                    merged.push(Merged {
                        item,
                        variation_notes: Vec::new(),
                    });
                }
                _ => merged.push(Merged {
                    item,
                    variation_notes: Vec::new(),
                }),
            }
        }
        previous_chunk_ok = true;
    }

    // Spans, in book order; every item owns its lines up to the next item.
    let mut spans: Vec<Span> = merged
        .iter()
        .map(|m| {
            // The page is read at the title, not at a photo pulled in above it.
            let anchor = m.item.title_lines.first().copied().unwrap_or(m.item.first);
            Span {
                start: m.item.first,
                end: m.item.last + 1,
                doc_path: book.doc_path(m.item.first).to_string(),
                page: book.lines.get(anchor).and_then(|l| l.page.clone()),
            }
        })
        .collect();
    let title_lines: Vec<Option<usize>> = merged
        .iter()
        .map(|m| m.item.title_lines.first().copied())
        .collect();
    let parents: Vec<Option<usize>> = merged
        .iter()
        .map(|m| {
            (m.item.kind == Kind::Variation)
                .then(|| parent_index(&merged, &m.item))
                .flatten()
        })
        .collect();
    let ids: Vec<String> = merged
        .iter()
        .map(|m| {
            book.lines
                .get(m.item.title_lines.first().copied().unwrap_or(m.item.first))
                .map(|l| l.id())
                .unwrap_or_default()
        })
        .collect();
    let photos = bind_photos(book, &spans, &title_lines, &merged);
    let claimed: HashSet<&str> = photos.iter().flatten().map(|p| p.path.as_str()).collect();
    let orphan_photos: Vec<ImageRef> = book
        .lines
        .iter()
        .filter(|l| l.clean.in_figure && (!l.clean.links.is_empty() || !l.text().is_empty()))
        .flat_map(|l| l.clean.images.iter().map(move |img| (l.idx, img)))
        .filter(|(_, img)| !claimed.contains(img.path.as_str()))
        .map(|(line, img)| {
            let mut img = img.clone();
            img.line = Some(line);
            img.caption = caption_for(book, line);
            img
        })
        .collect();
    for (span, photos) in spans.iter_mut().zip(&photos) {
        for p in photos {
            if let Some(line) = p.line {
                span.start = span.start.min(line);
            }
        }
    }

    let mut items: Vec<Item> = Vec::new();
    for (i, m) in merged.into_iter().enumerate() {
        let is_phantom = m.item.kind == Kind::Recipe
            && phantoms.iter().any(|p| titles_match(p, &m.item.title))
            && m.item.ingredient_count() <= 3
            && m.item.step_count() == 0;
        let kind = if is_phantom { Kind::Essay } else { m.item.kind };
        let id = ids[i].clone();
        let span = spans[i].clone();
        let photos = photos[i].clone();
        let item = m.item;
        match kind {
            Kind::Recipe | Kind::Variation => {
                let mut notes: Vec<Note> = item.notes.iter().map(note_from).collect();
                for (label, text) in m.variation_notes {
                    for t in text {
                        notes.push(Note {
                            label: Some(label.clone()),
                            text: t.text,
                            line: t.line,
                            refs: Vec::new(),
                        });
                    }
                }
                notes.sort_by_key(|n| n.line);
                items.push(Item::Recipe(Box::new(Recipe {
                    id,
                    title: item.title,
                    name: String::new(),
                    meta: RecipeMeta {
                        description: item.description.into_iter().map(|t| t.text).collect(),
                        recipe_yield: item.recipe_yield,
                        times: item.times,
                        equipment: item.equipment.into_iter().map(|t| t.text).collect(),
                        category: item.category.map(|t| t.text),
                        page: item.page.map(|t| t.text).or_else(|| span.page.clone()),
                    },
                    sections: item
                        .sections
                        .into_iter()
                        .map(|s| Section {
                            name: s.name,
                            ingredients: s
                                .ingredients
                                .into_iter()
                                .map(|t| {
                                    let (parsed, confidence) = parser.parse(&t.text);
                                    IngredientLine {
                                        raw: t.text,
                                        line: t.line,
                                        parsed,
                                        confidence,
                                        reference: None,
                                    }
                                })
                                .collect(),
                            steps: s.steps.into_iter().map(step_from).collect(),
                        })
                        .collect(),
                    photos,
                    notes,
                    variant_of: parents[i].and_then(|p| ids.get(p).cloned()),
                    span,
                })));
            }
            Kind::Technique => items.push(Item::Technique(Technique {
                id,
                title: item.title,
                name: String::new(),
                description: item
                    .description
                    .into_iter()
                    .chain(item.notes)
                    .map(|t| t.text)
                    .collect(),
                steps: item
                    .sections
                    .into_iter()
                    .flat_map(|s| s.steps)
                    .map(step_from)
                    .collect(),
                photos,
                span,
            })),
            Kind::Essay => {
                let mut text: Vec<Text> = item.description;
                text.extend(item.notes);
                text.extend(item.equipment);
                text.extend(
                    item.sections
                        .into_iter()
                        .flat_map(|s| s.ingredients.into_iter().chain(s.steps)),
                );
                text.sort_by_key(|t| t.line);
                items.push(Item::Essay(Essay {
                    id,
                    title: item.title,
                    name: String::new(),
                    text: text.into_iter().map(|t| t.text).collect(),
                    photos,
                    span,
                }));
            }
        }
    }
    Assembled {
        chapters: into_chapters(book, nav, items, &ignored, &chapter_headings),
        orphan_photos,
    }
}

fn collect_outside(
    lowered: &Lowered,
    ignored: &mut BTreeSet<usize>,
    headings: &mut BTreeSet<usize>,
    captions: &mut BTreeSet<usize>,
) {
    ignored.extend(lowered.ignored.iter().copied());
    headings.extend(lowered.chapter_headings.iter().copied());
    captions.extend(lowered.captions.iter().copied());
}

fn note_from(t: &Text) -> Note {
    let (label, text) = split_note_label(&t.text);
    Note {
        label,
        text,
        line: t.line,
        refs: Vec::new(),
    }
}

fn step_from(t: Text) -> Step {
    Step {
        text: t.text,
        line: t.line,
        refs: Vec::new(),
    }
}

/// `Variation Polenta with Fresh Corn` → `Polenta with Fresh Corn`. The word
/// is the book's marker for the item's kind, not part of its name.
pub fn strip_variation_label(title: &str) -> String {
    let trimmed = title.trim();
    for label in ["variations:", "variation:", "variations", "variation"] {
        let boundary = trimmed
            .get(label.len()..)
            .and_then(|r| r.chars().next())
            .is_some_and(|c| !c.is_alphanumeric());
        if trimmed.len() > label.len()
            && boundary
            && trimmed[..label.len()].eq_ignore_ascii_case(label)
        {
            let rest = trimmed[label.len()..]
                .trim_start_matches([' ', ':', '-', '–', '—'])
                .trim();
            if !rest.is_empty() {
                return rest.to_string();
            }
        }
    }
    trimmed.to_string()
}

/// `DO AHEAD Mushrooms can be…` → (`DO AHEAD`, `Mushrooms can be…`). A label
/// is up to three leading all-caps words followed by more text.
pub fn split_note_label(text: &str) -> (Option<String>, String) {
    let words: Vec<&str> = text.split(' ').collect();
    let caps = words
        .iter()
        .take(3)
        .take_while(|w| {
            let letters: Vec<char> = w.chars().filter(|c| c.is_alphabetic()).collect();
            letters.len() >= 2
                && letters.iter().all(|c| c.is_uppercase())
                && w.trim_end_matches(':')
                    .chars()
                    .all(|c| c.is_alphabetic() || c == '-')
        })
        .count();
    let rest_is_prose = words
        .get(caps)
        .is_some_and(|w| w.chars().next().is_some_and(char::is_alphabetic));
    if caps == 0 || caps == words.len() || !rest_is_prose {
        return (None, text.to_string());
    }
    let label = words[..caps].join(" ").trim_end_matches(':').to_string();
    (Some(label), words[caps..].join(" "))
}

/// The recipe a variation belongs to: the one whose title line it names,
/// else the nearest preceding recipe.
/// `<h1>Doughnuts</h1>` + two paragraphs + `<h2>Sugared Doughnuts</h2>` +
/// ingredients: the model titled the recipe by the section heading. The
/// heading one level down, sitting between the title and the first
/// ingredient with only description lines before it, is the recipe's real
/// title; the higher heading becomes an essay over those paragraphs.
fn split_group_heading(book: &BookLines, item: &ChunkItem) -> Option<(ChunkItem, ChunkItem)> {
    if item.kind != Kind::Recipe || item.continues {
        return None;
    }
    let &title = item.title_lines.first()?;
    let level = book.lines.get(title)?.clean.heading?;
    let first_ingredient = item
        .sections
        .iter()
        .flat_map(|s| s.ingredients.iter().map(|t| t.line))
        .min()?;
    let named: HashSet<usize> = item
        .sections
        .iter()
        .flat_map(|s| s.name_lines.iter().copied())
        .collect();
    let sub = (title + 1..first_ingredient).find(|&i| {
        book.lines.get(i).and_then(|l| l.clean.heading) == Some(level + 1) && !named.contains(&i)
    })?;
    let described: HashSet<usize> = item.description.iter().map(|t| t.line).collect();
    if !(title + 1..sub).all(|i| described.contains(&i)) {
        return None;
    }
    let mut section = ChunkItem {
        kind: Kind::Essay,
        title: book.text(title).to_string(),
        title_lines: vec![title],
        continues: false,
        variation_of: Vec::new(),
        description: item
            .description
            .iter()
            .filter(|t| t.line < sub)
            .cloned()
            .collect(),
        recipe_yield: None,
        times: None,
        equipment: Vec::new(),
        category: None,
        page: None,
        sections: Vec::new(),
        notes: Vec::new(),
        photos: Vec::new(),
        first: title,
        last: sub - 1,
    };
    section.last = section
        .description
        .iter()
        .map(|t| t.line)
        .max()
        .unwrap_or(title)
        .max(title);
    let mut recipe = item.clone();
    recipe.title = book.text(sub).to_string();
    recipe.title_lines = vec![sub];
    recipe.description.retain(|t| t.line > sub);
    recipe.first = sub;
    Some((section, recipe))
}

/// A recipe with no method, no yield, and only table rows as ingredients is a
/// formula listed for comparison, not something to cook from.
fn is_formula_table(book: &BookLines, item: &ChunkItem) -> bool {
    item.kind == Kind::Recipe
        && !item.continues
        && item.step_count() == 0
        && item.recipe_yield.is_none()
        && item.ingredient_count() > 0
        && item
            .sections
            .iter()
            .flat_map(|s| s.ingredients.iter())
            .all(|t| book.lines.get(t.line).is_some_and(|l| l.clean.transformed))
}

fn parent_index(merged: &[Merged], item: &ChunkItem) -> Option<usize> {
    if let Some(&line) = item.variation_of.first()
        && let Some(p) = merged
            .iter()
            .rposition(|m| m.item.title_lines.contains(&line))
    {
        return Some(p);
    }
    merged.iter().rposition(|m| m.item.kind == Kind::Recipe)
}

/// Fold a continuation into the recipe it continues: unnamed sections merge
/// into the main one, named sections append, everything else concatenates.
fn merge_into(target: &mut ChunkItem, continuation: ChunkItem) {
    for section in continuation.sections {
        match section.name {
            None => {
                if let Some(main) = target.sections.iter_mut().find(|s| s.name.is_none()) {
                    main.ingredients.extend(section.ingredients);
                    main.steps.extend(section.steps);
                } else {
                    target.sections.push(section);
                }
            }
            Some(_) => target.sections.push(section),
        }
    }
    target.description.extend(continuation.description);
    target.notes.extend(continuation.notes);
    target.equipment.extend(continuation.equipment);
    target.photos.extend(continuation.photos);
    if target.recipe_yield.is_none() {
        target.recipe_yield = continuation.recipe_yield;
    }
    if target.times.is_none() {
        target.times = continuation.times;
    }
    if target.category.is_none() {
        target.category = continuation.category;
    }
    if target.page.is_none() {
        target.page = continuation.page;
    }
    target.last = target.last.max(continuation.last);
}

/// Images inside each span, plus an unclaimed image just above the title as
/// the hero (first).
fn bind_photos(
    book: &BookLines,
    spans: &[Span],
    title_lines: &[Option<usize>],
    merged: &[Merged],
) -> Vec<Vec<ImageRef>> {
    let owner = |line: usize| spans.iter().position(|s| s.contains(line));
    let mut out = Vec::with_capacity(spans.len());
    for (i, span) in spans.iter().enumerate() {
        let mut photos: Vec<ImageRef> = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();
        let mut push = |line: usize, photos: &mut Vec<ImageRef>| {
            if let Some(l) = book.lines.get(line) {
                for img in &l.clean.images {
                    if seen.insert(img.path.clone()) {
                        let mut img = img.clone();
                        img.line = Some(line);
                        img.caption = caption_for(book, line);
                        photos.push(img);
                    }
                }
            }
        };
        // Hero: nearest image above the title not owned by another item.
        let title = title_lines[i].unwrap_or(span.start);
        for line in (title.saturating_sub(HERO_REACH)..title).rev() {
            if owner(line).is_some_and(|o| o != i) {
                break;
            }
            if let Some(l) = book.lines.get(line)
                && !l.clean.images.is_empty()
            {
                // A captioned figure with a link shows some other recipe; the
                // reference pass will hand it to that recipe.
                if !l.clean.in_figure || l.clean.links.is_empty() {
                    push(line, &mut photos);
                }
                break;
            }
        }
        for line in span.start..span.end {
            push(line, &mut photos);
        }
        // Captions the model assigned to this item pull their images in too.
        for &line in &merged[i].item.photos {
            for l in line.saturating_sub(CAPTION_REACH)..=line {
                push(l, &mut photos);
            }
        }
        out.push(photos);
    }
    out
}

fn caption_for(book: &BookLines, image_line: usize) -> Option<String> {
    (image_line..=(image_line + CAPTION_REACH).min(book.len().saturating_sub(1)))
        .filter_map(|l| book.lines.get(l))
        .find(|l| l.clean.in_figure && !l.text().is_empty())
        .map(|l| l.text().to_string())
}

/// Group items into chapters from depth-1 nav entries. Items before the first
/// entry form a leading untitled chapter. Without a nav, each spine document
/// with items is a chapter.
fn into_chapters(
    book: &BookLines,
    nav: &Nav,
    items: Vec<Item>,
    ignored: &BTreeSet<usize>,
    headings: &BTreeSet<usize>,
) -> Vec<Chapter> {
    let mut starts: Vec<(Option<String>, usize)> =
        crate::crosscheck::nav_chapter_entries(book, nav)
            .into_iter()
            .map(|(label, line)| (Some(label), line))
            .collect();
    if starts.is_empty() {
        for doc in &book.docs {
            let has_item = items.iter().any(|i| {
                doc.first_line <= i.span().start && i.span().start < doc.first_line + doc.len
            });
            if has_item {
                let title = (doc.first_line..doc.first_line + doc.len)
                    .find(|&l| headings.contains(&l) || book.lines[l].clean.heading.is_some())
                    .map(|l| book.text(l).to_string());
                starts.push((title, doc.first_line));
            }
        }
    }
    starts.sort_by_key(|(_, line)| *line);
    starts.dedup_by_key(|(_, line)| *line);
    if starts
        .first()
        .is_none_or(|(_, line)| *line > items.first().map(|i| i.span().start).unwrap_or(0))
    {
        starts.insert(0, (None, 0));
    }
    let mut chapters: Vec<Chapter> = starts
        .iter()
        .enumerate()
        .map(|(i, (title, line))| {
            let end = starts.get(i + 1).map(|(_, l)| *l).unwrap_or(book.len());
            Chapter {
                id: format!("ch{i:02}"),
                title: title.clone(),
                span: Span {
                    start: *line,
                    end,
                    doc_path: book.doc_path(*line).to_string(),
                    page: book.lines.get(*line).and_then(|l| l.page.clone()),
                },
                intro: Vec::new(),
                items: Vec::new(),
            }
        })
        .collect();
    for item in items {
        let start = item.span().start;
        let index = chapters
            .iter()
            .rposition(|c| c.span.start <= start)
            .unwrap_or(0);
        chapters[index].items.push(item);
    }
    for chapter in &mut chapters {
        let first_item = chapter
            .items
            .first()
            .map(|i| i.span().start)
            .unwrap_or(chapter.span.end);
        chapter.intro = (chapter.span.start..first_item)
            .filter(|l| ignored.contains(l) && !headings.contains(l))
            .map(|l| book.text(l).to_string())
            .filter(|t| !t.is_empty())
            .collect();
    }
    chapters.retain(|c| !c.items.is_empty() || c.title.is_some());
    chapters
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[rstest]
    #[case("Variation POLENTA with FRESH CORN", "POLENTA with FRESH CORN")]
    #[case("VARIATION: Brown Butter", "Brown Butter")]
    #[case("Variations", "Variations")]
    #[case("Polenta", "Polenta")]
    fn variation_labels(#[case] title: &str, #[case] expected: &str) {
        assert_eq!(strip_variation_label(title), expected);
    }

    #[rstest]
    #[case(
        "DO AHEAD Mushrooms can be roasted ahead.",
        Some("DO AHEAD"),
        "Mushrooms can be roasted ahead."
    )]
    #[case("NOTE: Any mushroom works.", Some("NOTE"), "Any mushroom works.")]
    #[case("Wine: Merlot delle Venezie", None, "Wine: Merlot delle Venezie")]
    #[case("SERVES 4", None, "SERVES 4")]
    #[case("A plain note.", None, "A plain note.")]
    fn note_labels(#[case] text: &str, #[case] label: Option<&str>, #[case] rest: &str) {
        let (l, t) = split_note_label(text);
        assert_eq!(l.as_deref(), label);
        assert_eq!(t, rest);
    }
}
