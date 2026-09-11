//! The model contract: what a chunk request looks like and how an answer is
//! lowered to text.
//!
//! The model never writes prose. It answers with zero-based *line numbers* of
//! the chunk it was shown, grouped into items and fields; Rust copies the
//! source lines. Every line must be claimed exactly once across all items,
//! `captions`, `chapter_headings`, and `ignored`, so nothing can be dropped or
//! duplicated silently. The tool schema is derived from [`Payload`] and bounded
//! to the chunk's line count on every request.

use std::collections::{BTreeSet, HashMap};

use recipe_types::RecipeTimes;
use schemars::{JsonSchema, SchemaGenerator, generate::SchemaSettings};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::chunk::Chunk;
use crate::lines::BookLines;

/// Bump when the prompt, schema, or lowering changes meaning; it is part of
/// every cache key.
pub const CONTRACT_VERSION: &str = "cookbook-indexed-v1";
pub const TOOL_NAME: &str = "emit_items";

/// A fully built model request, provider-neutral. `gateway` turns it into an
/// HTTP body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChunkRequest {
    pub system: String,
    pub user: String,
    pub tool_name: String,
    pub tool_schema: Value,
}

/// What kind of titled item a selection describes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// A titled item with at least one ingredient line.
    #[default]
    Recipe,
    /// A titled variant of the nearest preceding recipe: with its own
    /// ingredient lines when the book prints them, otherwise just notes.
    Variation,
    /// A titled method with steps but no ingredient list.
    Technique,
    /// Titled prose: a chapter introduction, a sidebar, an essay.
    Essay,
}

/// The tool input. Every leaf is a list of zero-based line numbers of the
/// chunk shown to the model.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Payload {
    /// Every titled item in the chunk, in source order.
    pub items: Vec<PayloadItem>,
    /// Photo captions that belong to no item. A caption naming another recipe
    /// is a caption, never a title.
    pub captions: Vec<usize>,
    /// Chapter or part headings.
    pub chapter_headings: Vec<usize>,
    /// Navigation, running heads, filler, and unrelated prose.
    pub ignored: Vec<usize>,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct PayloadItem {
    pub kind: Kind,
    /// The item's complete printed name and subtitle. Empty only for the
    /// first item when it continues a recipe cut at the previous chunk.
    pub title: Vec<usize>,
    /// For a variation: the title line(s) of the recipe it varies.
    pub variation_of: Vec<usize>,
    /// Headnote paragraphs.
    pub description: Vec<usize>,
    /// Every line of the serves/makes statement.
    pub recipe_yield: Vec<usize>,
    pub times: PayloadTimes,
    /// Equipment lists and their headings.
    pub equipment: Vec<usize>,
    /// A printed category line.
    #[schemars(length(max = 1))]
    pub category: Vec<usize>,
    /// A printed page number line.
    #[schemars(length(max = 1))]
    pub page: Vec<usize>,
    /// The authored ingredient groups and their steps.
    pub sections: Vec<PayloadSection>,
    /// Tips, do-ahead, storage, serving suggestions, dietary flags,
    /// parenthetical group notes, and combined metadata lines.
    pub notes: Vec<usize>,
    /// Caption lines of this item's photos.
    pub photos: Vec<usize>,
}

/// Explicitly printed timing lines, one line per field. A line that combines
/// several times, or a time with other metadata, goes to `notes` instead.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct PayloadTimes {
    #[schemars(length(max = 1))]
    pub prep: Vec<usize>,
    #[schemars(length(max = 1))]
    pub cook: Vec<usize>,
    #[schemars(length(max = 1))]
    pub active: Vec<usize>,
    #[schemars(length(max = 1))]
    pub total: Vec<usize>,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct PayloadSection {
    /// The printed component heading ("For the filling"); empty for the main
    /// or only group.
    pub name: Vec<usize>,
    pub ingredients: Vec<usize>,
    /// Method paragraphs in order. A shared method belongs to the unnamed
    /// main section.
    pub steps: Vec<usize>,
}

const SYSTEM_PROMPT: &str = "\
You read numbered source lines from a cookbook and return their structure through the tool. \
Every field is a list of zero-based line numbers; never rewrite text. Account for EVERY line \
exactly once across item fields, captions, chapter_headings, and ignored.

Items, in source order. kind `recipe`: a title plus at least one ingredient line. kind \
`variation`: a titled variant of the nearest preceding recipe; set variation_of to that \
recipe's title line; give it its own sections when the book prints ingredients for it, \
otherwise put its text in notes. kind `technique`: a titled method with steps but no \
ingredient list. kind `essay`: titled prose with neither.

Fields. title: the complete printed name and subtitle or translation, never dietary flags. \
description: the headnote paragraphs. recipe_yield: every line of the serves/makes statement. \
times: explicitly printed timing lines, one per field; a line combining several times or a \
time with other metadata goes in notes and the times fields stay empty. equipment: equipment \
lists and their headings; non-food wrappers listed there stay in equipment. category and \
page: a printed category or page line. sections: keep the authored ingredient groups, an \
unnamed main group plus named components; each ingredient line stays in its printed group, \
including unquantified ones such as frying oil or salt; steps are the method paragraphs in \
order and belong to the group they prepare, with a shared method in the unnamed main group. A \
required procedure is always a step, even when printed before the ingredients, labelled as a \
note, or ending with a serving or storage aside. Do not split a line or move part of it. \
notes: tips, do-ahead, storage, serving suggestions, dietary flags, parenthetical group notes. \
photos: caption lines of this item's photos.

Outside items. captions: photo captions belonging to no item; a caption naming another \
recipe is a caption, not a title. chapter_headings: chapter or part titles. ignored: \
navigation, running heads, filler, and prose that belongs to no titled item.

Only the first item may continue a recipe cut before this chunk: then its title is [] and \
the continuation title given in the message applies. Every other item needs its own title \
line. Never invent a title from a caption, a link, or a neighbouring recipe. If nothing here \
belongs to a titled item, return items=[] and list every line under ignored or \
chapter_headings.";

/// Build the request for one chunk.
pub fn build_request(chunk: &Chunk, book: &BookLines) -> ChunkRequest {
    let text = chunk.text(book);
    let user = match &chunk.title_hint {
        Some(hint) => format!(
            "Continuation title (use only if the source starts mid-recipe): {hint}\n\n{text}"
        ),
        None => text,
    };
    ChunkRequest {
        system: SYSTEM_PROMPT.to_string(),
        user,
        tool_name: TOOL_NAME.to_string(),
        tool_schema: tool_schema(chunk.lines()),
    }
}

/// The tool schema with every index bounded to `[0, line_count)`. Subschemas
/// are inlined and formats stripped because Gemini's function declarations
/// accept neither `$ref` nor `format`.
pub fn tool_schema(line_count: usize) -> Value {
    let settings = SchemaSettings::draft07().with(|s| {
        s.inline_subschemas = true;
        s.meta_schema = None;
    });
    let schema = SchemaGenerator::new(settings).into_root_schema_for::<Payload>();
    let mut value = schema.to_value();
    if let Some(obj) = value.as_object_mut() {
        obj.remove("$schema");
        obj.remove("title");
    }
    bound_integers(&mut value, line_count.saturating_sub(1));
    value
}

fn bound_integers(value: &mut Value, max: usize) {
    match value {
        Value::Object(map) => {
            if map.get("type").and_then(Value::as_str) == Some("integer") {
                map.remove("format");
                map.insert("minimum".into(), Value::from(0));
                map.insert("maximum".into(), Value::from(max));
            }
            // A documented enum comes out as `oneOf: [{const, description}]`;
            // fold it into a plain `enum` with the descriptions merged.
            if let Some(variants) = map.get("oneOf").and_then(Value::as_array)
                && let Some(consts) = variants
                    .iter()
                    .map(|v| v.get("const").and_then(Value::as_str).map(str::to_string))
                    .collect::<Option<Vec<_>>>()
            {
                let described: Vec<String> = variants
                    .iter()
                    .zip(&consts)
                    .filter_map(|(v, name)| {
                        v.get("description")
                            .and_then(Value::as_str)
                            .map(|d| format!("{name}: {d}"))
                    })
                    .collect();
                map.remove("oneOf");
                map.insert("type".into(), Value::from("string"));
                map.insert("enum".into(), Value::from(consts));
                if !described.is_empty() {
                    map.insert("description".into(), Value::from(described.join(" ")));
                }
            }
            for v in map.values_mut() {
                bound_integers(v, max);
            }
        }
        Value::Array(items) => {
            for v in items {
                bound_integers(v, max);
            }
        }
        _ => {}
    }
}

/// A source line copied into an item field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Text {
    /// Global line index.
    pub line: usize,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ChunkSection {
    pub name: Option<String>,
    pub name_lines: Vec<usize>,
    pub ingredients: Vec<Text>,
    pub steps: Vec<Text>,
}

/// One item lowered from a chunk answer. Line indices are global.
#[derive(Debug, Clone, PartialEq)]
pub struct ChunkItem {
    pub kind: Kind,
    pub title: String,
    pub title_lines: Vec<usize>,
    /// The item continues a recipe from the previous chunk; `title` is the
    /// chunk's hint.
    pub continues: bool,
    pub variation_of: Vec<usize>,
    pub description: Vec<Text>,
    pub recipe_yield: Option<String>,
    pub times: Option<RecipeTimes>,
    pub equipment: Vec<Text>,
    pub category: Option<Text>,
    pub page: Option<Text>,
    pub sections: Vec<ChunkSection>,
    pub notes: Vec<Text>,
    pub photos: Vec<usize>,
    /// First and last global line this item claims.
    pub first: usize,
    pub last: usize,
}

impl ChunkItem {
    pub fn ingredient_count(&self) -> usize {
        self.sections.iter().map(|s| s.ingredients.len()).sum()
    }

    pub fn step_count(&self) -> usize {
        self.sections.iter().map(|s| s.steps.len()).sum()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Lowered {
    pub items: Vec<ChunkItem>,
    pub captions: Vec<usize>,
    pub chapter_headings: Vec<usize>,
    pub ignored: Vec<usize>,
}

/// An answer that cannot be lowered. The message is written for the model: it
/// is appended to the retry request as feedback.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct Invalid(pub String);

/// Lower a tool answer for `chunk` into text with global line indices.
pub fn lower(chunk: &Chunk, book: &BookLines, payload: Value) -> Result<Lowered, Invalid> {
    let payload: Payload = serde_json::from_value(payload)
        .map_err(|e| Invalid(format!("the answer does not match the tool schema: {e}")))?;
    let n = chunk.lines();
    let mut used: HashMap<usize, &'static str> = HashMap::new();
    let mut take = |indices: &[usize], field: &'static str| -> Result<Vec<Text>, Invalid> {
        let mut sorted = indices.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        sorted
            .into_iter()
            .map(|i| {
                if i >= n {
                    return Err(Invalid(format!("line {i} is out of range (0..{n}) in {field}")));
                }
                if let Some(previous) = used.insert(i, field) {
                    return Err(Invalid(format!(
                        "line {i} is assigned to both {previous} and {field}; every line belongs to exactly one field"
                    )));
                }
                Ok(Text {
                    line: chunk.global(i),
                    text: book.text(chunk.global(i)).to_string(),
                })
            })
            .collect()
    };

    let mut payload_items = payload.items;
    payload_items.sort_by_key(first_index);

    let mut items = Vec::with_capacity(payload_items.len());
    for (index, mut item) in payload_items.into_iter().enumerate() {
        let (title, title_lines, continues) = if item.title.is_empty() {
            if index != 0 {
                return Err(Invalid(format!(
                    "item {index} has no title; only the first item may continue the previous chunk's recipe"
                )));
            }
            let hint = chunk.title_hint.clone().ok_or_else(|| {
                Invalid("the first item has no title and there is no continuation title; select its title line".into())
            })?;
            (hint, Vec::new(), true)
        } else {
            let lines = take(&item.title, "title")?;
            let mut seen = BTreeSet::new();
            let text = lines
                .iter()
                .filter(|t| seen.insert(t.text.to_lowercase()))
                .map(|t| t.text.as_str())
                .collect::<Vec<_>>()
                .join(" ");
            (text, lines.into_iter().map(|t| t.line).collect(), false)
        };
        for &i in &item.variation_of {
            if i >= n {
                return Err(Invalid(format!(
                    "variation_of line {i} is out of range (0..{n})"
                )));
            }
        }
        move_combined_metadata_to_notes(&mut item);
        normalize_sections(&mut item);

        let description = take(&item.description, "description")?;
        let notes = take(&item.notes, "notes")?;
        let equipment = take(&item.equipment, "equipment")?;
        let yield_lines = take(&item.recipe_yield, "recipe_yield")?;
        let recipe_yield = (!yield_lines.is_empty()).then(|| {
            yield_lines
                .iter()
                .map(|t| t.text.as_str())
                .collect::<Vec<_>>()
                .join(" ")
        });
        let category = take(&item.category, "category")?.into_iter().next();
        let page = take(&item.page, "page")?.into_iter().next();
        let mut times = RecipeTimes::default();
        for (field, indices, slot) in [
            ("times.prep", &item.times.prep, &mut times.prep),
            ("times.cook", &item.times.cook, &mut times.cook),
            ("times.active", &item.times.active, &mut times.active),
            ("times.total", &item.times.total, &mut times.total),
        ] {
            *slot = take(indices, field)?.into_iter().next().map(|t| t.text);
        }
        fill_time_minutes(&mut times);
        let times = (!times.is_empty()).then_some(times);
        let mut sections = Vec::with_capacity(item.sections.len());
        for section in &item.sections {
            let name_lines = take(&section.name, "section name")?;
            sections.push(ChunkSection {
                name: (!name_lines.is_empty()).then(|| {
                    name_lines
                        .iter()
                        .map(|t| t.text.as_str())
                        .collect::<Vec<_>>()
                        .join(" ")
                }),
                name_lines: name_lines.into_iter().map(|t| t.line).collect(),
                ingredients: take(&section.ingredients, "ingredients")?,
                steps: take(&section.steps, "steps")?,
            });
        }
        let photos: Vec<usize> = take(&item.photos, "photos")?
            .into_iter()
            .map(|t| t.line)
            .collect();

        let mut claimed: Vec<usize> = title_lines.clone();
        claimed.extend(description.iter().map(|t| t.line));
        claimed.extend(notes.iter().map(|t| t.line));
        claimed.extend(equipment.iter().map(|t| t.line));
        claimed.extend(yield_lines.iter().map(|t| t.line));
        claimed.extend(category.iter().chain(page.iter()).map(|t| t.line));
        claimed.extend(sections.iter().flat_map(|s| {
            s.name_lines
                .iter()
                .copied()
                .chain(s.ingredients.iter().map(|t| t.line))
                .chain(s.steps.iter().map(|t| t.line))
        }));
        claimed.extend(photos.iter().copied());
        let first = claimed.iter().copied().min().unwrap_or(chunk.start);
        let last = claimed.iter().copied().max().unwrap_or(first);

        items.push(ChunkItem {
            kind: item.kind,
            title,
            title_lines,
            continues,
            variation_of: item.variation_of.iter().map(|&i| chunk.global(i)).collect(),
            description,
            recipe_yield,
            times,
            equipment,
            category,
            page,
            sections,
            notes,
            photos,
            first,
            last,
        });
    }
    let captions: Vec<usize> = take(&payload.captions, "captions")?
        .into_iter()
        .map(|t| t.line)
        .collect();
    let chapter_headings: Vec<usize> = take(&payload.chapter_headings, "chapter_headings")?
        .into_iter()
        .map(|t| t.line)
        .collect();
    let ignored: Vec<usize> = take(&payload.ignored, "ignored")?
        .into_iter()
        .map(|t| t.line)
        .collect();
    if used.len() != n {
        let missing: Vec<usize> = (0..n).filter(|i| !used.contains_key(i)).collect();
        return Err(Invalid(format!(
            "lines {missing:?} are not assigned to any field; put every line in an item field, captions, chapter_headings, or ignored"
        )));
    }
    Ok(Lowered {
        items,
        captions,
        chapter_headings,
        ignored,
    })
}

fn first_index(item: &PayloadItem) -> usize {
    [
        &item.title,
        &item.description,
        &item.recipe_yield,
        &item.notes,
        &item.equipment,
        &item.category,
        &item.page,
        &item.times.prep,
        &item.times.cook,
        &item.times.active,
        &item.times.total,
        &item.photos,
    ]
    .into_iter()
    .flatten()
    .chain(
        item.sections
            .iter()
            .flat_map(|s| [&s.name, &s.ingredients, &s.steps].into_iter().flatten()),
    )
    .copied()
    .min()
    .unwrap_or(usize::MAX)
}

/// A printed line that several metadata fields point at ("Prep 10 min | Cook
/// 20 min") has one owner: it moves to notes and those fields empty out.
fn move_combined_metadata_to_notes(item: &mut PayloadItem) {
    let mut counts: HashMap<usize, usize> = HashMap::new();
    for indices in [
        &item.category,
        &item.times.prep,
        &item.times.cook,
        &item.times.active,
        &item.times.total,
    ] {
        for &i in indices {
            *counts.entry(i).or_default() += 1;
        }
    }
    let combined: BTreeSet<usize> = counts
        .into_iter()
        .filter(|(_, n)| *n > 1)
        .map(|(i, _)| i)
        .collect();
    if combined.is_empty() {
        return;
    }
    for indices in [
        &mut item.category,
        &mut item.times.prep,
        &mut item.times.cook,
        &mut item.times.active,
        &mut item.times.total,
    ] {
        indices.retain(|i| !combined.contains(i));
    }
    for i in combined {
        if !item.notes.contains(&i) {
            item.notes.push(i);
        }
    }
}

/// Authored headings partition the ingredient list: each ingredient line goes
/// to the nearest named section printed before it, else the unnamed main
/// section. When the whole method is printed after every ingredient group, it
/// is one shared method and lives in the main section. Sections are then
/// ordered by source position.
fn normalize_sections(item: &mut PayloadItem) {
    let mut ingredients: Vec<usize> = item
        .sections
        .iter_mut()
        .flat_map(|s| std::mem::take(&mut s.ingredients))
        .collect();
    ingredients.sort_unstable();
    if !item.sections.iter().any(|s| s.name.is_empty()) {
        item.sections.push(PayloadSection::default());
    }
    for line in ingredients {
        let target = item
            .sections
            .iter()
            .enumerate()
            .filter_map(|(i, s)| {
                s.name
                    .first()
                    .filter(|name| **name < line)
                    .map(|name| (*name, i))
            })
            .max()
            .map(|(_, i)| i)
            .or_else(|| item.sections.iter().position(|s| s.name.is_empty()));
        if let Some(target) = target {
            item.sections[target].ingredients.push(line);
        }
    }
    let first_step = item
        .sections
        .iter()
        .flat_map(|s| s.steps.iter())
        .min()
        .copied();
    let last_ingredient = item
        .sections
        .iter()
        .flat_map(|s| s.ingredients.iter())
        .max()
        .copied();
    if let (Some(first), Some(last)) = (first_step, last_ingredient)
        && first > last
        && item
            .sections
            .iter()
            .all(|s| s.name.iter().all(|name| *name < first))
    {
        let mut steps: Vec<usize> = item
            .sections
            .iter_mut()
            .flat_map(|s| std::mem::take(&mut s.steps))
            .collect();
        steps.sort_unstable();
        if let Some(main) = item.sections.iter_mut().find(|s| s.name.is_empty()) {
            main.steps = steps;
        }
    }
    item.sections
        .retain(|s| !(s.name.is_empty() && s.ingredients.is_empty() && s.steps.is_empty()));
    item.sections.sort_by_key(|s| {
        s.ingredients
            .iter()
            .min()
            .copied()
            .or_else(|| s.name.first().copied())
            .or_else(|| s.steps.iter().min().copied())
            .unwrap_or(usize::MAX)
    });
}

/// Parse a printed time ("30 minutes", "1 hr 15 min", "About 2 hours") into
/// whole minutes. Ranges, fractions, qualifiers, and words ("overnight") yield
/// `None`: the display string keeps them, and a wrong number is worse than
/// none.
pub fn parse_freeform_duration(input: &str) -> Option<u32> {
    let lowered = input.trim().to_ascii_lowercase();
    let mut rest = lowered.as_str();
    for approx in [
        "about ",
        "approximately ",
        "approx. ",
        "approx ",
        "around ",
        "roughly ",
        "~",
    ] {
        if let Some(stripped) = rest.strip_prefix(approx) {
            rest = stripped.trim_start();
            break;
        }
    }
    let mut total: u32 = 0;
    let mut saw_component = false;
    let mut chars = rest.chars().peekable();
    while let Some(&c) = chars.peek() {
        if c == ' ' {
            chars.next();
            continue;
        }
        if !c.is_ascii_digit() {
            return None;
        }
        let mut num = String::new();
        while let Some(&d) = chars.peek() {
            if d.is_ascii_digit() {
                num.push(d);
                chars.next();
            } else {
                break;
            }
        }
        while chars.peek() == Some(&' ') {
            chars.next();
        }
        let mut unit = String::new();
        while let Some(&u) = chars.peek() {
            if u.is_ascii_alphabetic() {
                unit.push(u);
                chars.next();
            } else {
                break;
            }
        }
        if chars.peek() == Some(&'.') {
            chars.next();
        }
        let per_unit = match unit.as_str() {
            "h" | "hr" | "hrs" | "hour" | "hours" => 60,
            "m" | "min" | "mins" | "minute" | "minutes" => 1,
            _ => return None,
        };
        total = total.checked_add(num.parse::<u32>().ok()?.checked_mul(per_unit)?)?;
        saw_component = true;
    }
    (saw_component && total > 0).then_some(total)
}

fn fill_time_minutes(times: &mut RecipeTimes) {
    let pairs: [(&Option<String>, &mut Option<u32>); 4] = [
        (&times.active, &mut times.active_minutes),
        (&times.total, &mut times.total_minutes),
        (&times.prep, &mut times.prep_minutes),
        (&times.cook, &mut times.cook_minutes),
    ];
    for (text, minutes) in pairs {
        if minutes.is_none() {
            *minutes = text.as_deref().and_then(parse_freeform_duration);
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::chunk::Boundary;
    use crate::epub::nav::Nav;
    use crate::epub::open::SpineDoc;
    use rstest::rstest;
    use serde_json::json;

    /// A one-document book with one `<p>` per line, plus a chunk that starts
    /// at `offset` and runs to the end (so global = offset + local).
    fn book_and_chunk(lines: &[&str], offset: usize) -> (BookLines, Chunk) {
        let body: String = lines.iter().map(|l| format!("<p>{l}</p>")).collect();
        let doc = SpineDoc {
            index: 0,
            path: "c.xhtml".into(),
            xhtml: format!("<html><body>{body}</body></html>"),
        };
        let book = BookLines::build(&[doc], &Nav::default());
        let chunk = Chunk {
            id: "k000".into(),
            index: 0,
            start: offset,
            end: book.len(),
            chars: 0,
            title_hint: None,
            boundary: Boundary::Start,
        };
        (book, chunk)
    }

    const SOUP: &[&str] = &[
        "Soup –",
        "A translation",
        "1 cup water",
        "Paste",
        "2 cloves garlic",
        "Mix everything.",
        "(V)",
    ];

    fn soup_payload() -> Value {
        json!({"items":[{"kind":"recipe","title":[0,1],
            "sections":[{"name":[3],"ingredients":[4],"steps":[]},{"name":[],"ingredients":[2],"steps":[5]}],
            "notes":[6]}]})
    }

    #[test]
    fn copies_source_and_restores_component_order() {
        let (book, chunk) = book_and_chunk(SOUP, 0);
        let lowered = lower(&chunk, &book, soup_payload()).unwrap();
        let item = &lowered.items[0];
        assert_eq!(item.title, "Soup – A translation");
        assert_eq!(item.title_lines, [0, 1]);
        assert!(!item.continues);
        assert_eq!(item.sections.len(), 2);
        assert_eq!(item.sections[0].name, None);
        assert_eq!(
            item.sections[0].ingredients,
            [Text {
                line: 2,
                text: "1 cup water".into()
            }]
        );
        assert_eq!(item.sections[0].steps[0].text, "Mix everything.");
        assert_eq!(item.sections[1].name.as_deref(), Some("Paste"));
        assert_eq!(item.sections[1].ingredients[0].text, "2 cloves garlic");
        assert_eq!(item.notes[0].text, "(V)");
        assert_eq!((item.first, item.last), (0, 6));
        assert_eq!(item.ingredient_count(), 2);
        assert_eq!(item.step_count(), 1);
    }

    #[test]
    fn indices_are_chunk_local_and_lowered_to_global() {
        let lines = [
            "filler",
            "filler",
            "Soup –",
            "A translation",
            "1 cup water",
            "Paste",
            "2 cloves garlic",
            "Mix everything.",
            "(V)",
        ];
        let (book, chunk) = book_and_chunk(&lines, 2);
        assert_eq!(chunk.text(&book).lines().next().unwrap(), "0: Soup –");
        let lowered = lower(&chunk, &book, soup_payload()).unwrap();
        assert_eq!(lowered.items[0].title_lines, [2, 3]);
        assert_eq!(lowered.items[0].sections[1].ingredients[0].line, 6);
    }

    #[rstest]
    #[case::unassigned(json!({"items":[{"title":[0],"sections":[{"ingredients":[2]}]}],"ignored":[1,3,4,5]}), "not assigned")]
    #[case::double(json!({"items":[{"title":[0],"sections":[{"ingredients":[2],"steps":[2]}]}],"ignored":[1,3,4,5,6]}), "assigned to both")]
    #[case::out_of_range(json!({"items":[{"title":[0],"sections":[{"ingredients":[9]}]}],"ignored":[1,2,3,4,5,6]}), "out of range")]
    #[case::recipe_level_ingredients(json!({"items":[{"title":[0],"ingredients":[2]}],"ignored":[1,3,4,5,6]}), "does not match the tool schema")]
    #[case::second_item_untitled(json!({"items":[{"title":[0],"sections":[{"ingredients":[2]}]},{"title":[],"sections":[{"ingredients":[4]}]}],"ignored":[1,3,5,6]}), "only the first item may continue")]
    #[case::continuation_without_hint(json!({"items":[{"title":[],"sections":[{"ingredients":[2]}]}],"ignored":[0,1,3,4,5,6]}), "no continuation title")]
    fn rejects_bad_coverage(#[case] payload: Value, #[case] message: &str) {
        let (book, chunk) = book_and_chunk(SOUP, 0);
        let err = lower(&chunk, &book, payload).unwrap_err();
        assert!(err.0.contains(message), "{err}");
    }

    #[test]
    fn continuation_uses_the_hint_and_claims_no_title_line() {
        let (book, mut chunk) =
            book_and_chunk(&["2 cups flour", "Knead well.", "Next Recipe", "1 egg"], 0);
        chunk.title_hint = Some("Long Bread".into());
        let payload = json!({"items":[
            {"title":[],"sections":[{"ingredients":[0],"steps":[1]}]},
            {"title":[2],"sections":[{"ingredients":[3]}]}]});
        let lowered = lower(&chunk, &book, payload).unwrap();
        assert!(lowered.items[0].continues);
        assert_eq!(lowered.items[0].title, "Long Bread");
        assert!(lowered.items[0].title_lines.is_empty());
        assert_eq!(lowered.items[0].first, 0);
        assert_eq!(lowered.items[1].title, "Next Recipe");
    }

    #[test]
    fn combined_metadata_is_kept_once_in_notes() {
        let (book, chunk) =
            book_and_chunk(&["Cake", "Prep 10 min | Cook 20 min", "1 egg", "Bake."], 0);
        let payload = json!({"items":[{"title":[0],"times":{"prep":[1],"cook":[1]},"sections":[{"ingredients":[2],"steps":[3]}]}]});
        let item = lower(&chunk, &book, payload).unwrap().items.remove(0);
        assert_eq!(item.times, None);
        assert_eq!(item.notes[0].text, "Prep 10 min | Cook 20 min");
    }

    #[test]
    fn times_get_minute_counts() {
        let (book, chunk) = book_and_chunk(
            &[
                "Cake",
                "Active Time: 1 hour 15 min",
                "Total Time: overnight",
                "1 egg",
                "Bake.",
            ],
            0,
        );
        let payload = json!({"items":[{"title":[0],"times":{"active":[1],"total":[2]},"sections":[{"ingredients":[3],"steps":[4]}]}]});
        let item = lower(&chunk, &book, payload).unwrap().items.remove(0);
        let times = item.times.unwrap();
        assert_eq!(times.active.as_deref(), Some("Active Time: 1 hour 15 min"));
        assert_eq!(
            times.active_minutes, None,
            "prefixed label is not a bare duration"
        );
        assert_eq!(times.total_minutes, None);
    }

    #[rstest]
    #[case("30 minutes", Some(30))]
    #[case("1 hr 15 min", Some(75))]
    #[case("About 2 hours", Some(120))]
    #[case("1 hr. 15 min.", Some(75))]
    #[case("30 to 40 minutes", None)]
    #[case("1 1/2 hours", None)]
    #[case("overnight", None)]
    #[case("30 minutes, plus chilling", None)]
    fn durations(#[case] text: &str, #[case] minutes: Option<u32>) {
        assert_eq!(parse_freeform_duration(text), minutes);
    }

    #[test]
    fn multiline_yield_joins_and_variation_of_is_not_a_claim() {
        let (book, chunk) = book_and_chunk(
            &[
                "Polenta",
                "5 cups water",
                "Stir.",
                "Polenta with Corn",
                "Makes about",
                "5 cups",
                "1 recipe polenta",
                "Fold in corn.",
            ],
            0,
        );
        let payload = json!({"items":[
            {"title":[0],"sections":[{"ingredients":[1],"steps":[2]}]},
            {"kind":"variation","title":[3],"variation_of":[0],"recipe_yield":[4,5],"sections":[{"ingredients":[6],"steps":[7]}]}]});
        let lowered = lower(&chunk, &book, payload).unwrap();
        let variation = &lowered.items[1];
        assert_eq!(variation.kind, Kind::Variation);
        assert_eq!(variation.variation_of, [0]);
        assert_eq!(
            variation.recipe_yield.as_deref(),
            Some("Makes about 5 cups")
        );
    }

    #[test]
    fn items_are_sorted_by_source_position_and_outside_lines_are_claimed() {
        let (book, chunk) = book_and_chunk(
            &[
                "Chapter One",
                "Photo of soup",
                "Soup",
                "1 cup water",
                "Boil.",
                "page 12",
            ],
            0,
        );
        let payload = json!({"items":[{"title":[2],"sections":[{"ingredients":[3],"steps":[4]}]}],
            "captions":[1],"chapter_headings":[0],"ignored":[5]});
        let lowered = lower(&chunk, &book, payload).unwrap();
        assert_eq!(lowered.captions, [1]);
        assert_eq!(lowered.chapter_headings, [0]);
        assert_eq!(lowered.ignored, [5]);
        // Out-of-order items come back in source order.
        let (book, chunk) = book_and_chunk(&["A", "1 egg", "Cook.", "B", "2 eggs", "Fry."], 0);
        let payload = json!({"items":[
            {"title":[3],"sections":[{"ingredients":[4],"steps":[5]}]},
            {"title":[0],"sections":[{"ingredients":[1],"steps":[2]}]}]});
        let lowered = lower(&chunk, &book, payload).unwrap();
        assert_eq!(lowered.items[0].title, "A");
        assert_eq!(lowered.items[1].title, "B");
    }

    #[test]
    fn shared_method_after_named_groups_moves_to_main() {
        let lines = [
            "Tart",
            "For the crust",
            "1 cup flour",
            "For the filling",
            "2 eggs",
            "Make the crust.",
            "Fill it.",
        ];
        let (book, chunk) = book_and_chunk(&lines, 0);
        let payload = json!({"items":[{"title":[0],"sections":[
            {"name":[1],"ingredients":[2],"steps":[5]},
            {"name":[3],"ingredients":[4],"steps":[6]}]}]});
        let item = lower(&chunk, &book, payload).unwrap().items.remove(0);
        let names: Vec<Option<&str>> = item.sections.iter().map(|s| s.name.as_deref()).collect();
        assert_eq!(
            names,
            [Some("For the crust"), Some("For the filling"), None]
        );
        assert_eq!(
            item.sections[2].steps.len(),
            2,
            "one shared method in the main section"
        );
        assert!(item.sections[0].steps.is_empty());
        assert!(item.sections[1].steps.is_empty());
    }

    #[test]
    fn schema_is_bounded_inlined_and_format_free() {
        let schema = tool_schema(7);
        let text = schema.to_string();
        assert!(!text.contains("$ref"), "{text}");
        assert!(!text.contains("$defs"), "{text}");
        assert!(!text.contains("\"format\""), "{text}");
        assert!(!text.contains("$schema"));
        assert!(text.contains("\"maximum\":6"));
        assert!(text.contains("\"maxItems\":1"));
        assert!(text.contains("\"additionalProperties\":false"));
        assert!(!text.contains("oneOf"), "{text}");
        assert!(!text.contains("const"), "{text}");
        assert_eq!(
            schema["properties"]["items"]["items"]["properties"]["kind"]["enum"],
            json!(["recipe", "variation", "technique", "essay"])
        );
    }

    #[test]
    fn request_carries_hint_and_numbered_lines() {
        let (book, mut chunk) = book_and_chunk(&["alpha", "beta"], 0);
        chunk.title_hint = Some("Cut Recipe".into());
        let request = build_request(&chunk, &book);
        assert!(request.user.starts_with("Continuation title (use only if the source starts mid-recipe): Cut Recipe\n\n0: alpha\n1: beta"));
        assert_eq!(request.tool_name, TOOL_NAME);
        assert!(request.system.contains("exactly once"));
    }
}
