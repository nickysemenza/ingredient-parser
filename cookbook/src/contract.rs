//! The model contract: what a chunk request looks like and how an answer is
//! lowered to text.
//!
//! The model never writes prose. It answers with zero-based *line numbers* of
//! the chunk it was shown, grouped into items and fields; Rust copies the
//! source lines. Every line must be claimed exactly once across all items,
//! `captions`, `chapter_headings`, and `ignored`, so nothing can be dropped or
//! duplicated silently. The tool schema is derived from [`Payload`] and bounded
//! to the chunk's line count on every request.

use std::collections::{BTreeSet, HashMap, HashSet};

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

/// A list of line numbers. Deserializes from a bare integer or `null` too,
/// because some models answer `"page": 33` where the schema says `[33]`.
#[derive(Debug, Default, Clone, PartialEq, Eq, JsonSchema)]
#[schemars(transparent)]
pub struct IndexList(pub Vec<usize>);

impl<'de> Deserialize<'de> for IndexList {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let raw = Value::deserialize(d)?;
        let one = |v: &Value| -> Option<usize> {
            match v {
                Value::Number(n) => n.as_u64().map(|n| n as usize).or_else(|| {
                    n.as_f64()
                        .filter(|f| f.fract() == 0.0 && *f >= 0.0)
                        .map(|f| f as usize)
                }),
                Value::String(s) => s.trim().parse::<usize>().ok(),
                _ => None,
            }
        };
        let bad = |v: &Value| {
            serde::de::Error::custom(format!(
                "expected a line number or a list of line numbers, got {v}"
            ))
        };
        Ok(match &raw {
            Value::Null => IndexList(Vec::new()),
            Value::Array(items) => IndexList(
                items
                    .iter()
                    .map(|v| one(v).ok_or_else(|| bad(v)))
                    .collect::<Result<Vec<_>, _>>()?,
            ),
            v => IndexList(vec![one(v).ok_or_else(|| bad(v))?]),
        })
    }
}

impl std::ops::Deref for IndexList {
    type Target = Vec<usize>;

    fn deref(&self) -> &Vec<usize> {
        &self.0
    }
}

impl std::ops::DerefMut for IndexList {
    fn deref_mut(&mut self) -> &mut Vec<usize> {
        &mut self.0
    }
}

impl<'a> IntoIterator for &'a IndexList {
    type Item = &'a usize;
    type IntoIter = std::slice::Iter<'a, usize>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

impl<'a> IntoIterator for &'a mut IndexList {
    type Item = &'a mut usize;
    type IntoIter = std::slice::IterMut<'a, usize>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter_mut()
    }
}

impl IntoIterator for IndexList {
    type Item = usize;
    type IntoIter = std::vec::IntoIter<usize>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

/// The tool input. Every leaf is a list of zero-based line numbers of the
/// chunk shown to the model. Unknown top-level fields are ignored: any lines
/// they carried show up as unassigned and are handled there.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(default)]
pub struct Payload {
    /// Every titled item in the chunk, in source order.
    pub items: Vec<PayloadItem>,
    /// Photo captions that belong to no item. A caption naming another recipe
    /// is a caption, never a title.
    pub captions: IndexList,
    /// Chapter or part headings.
    pub chapter_headings: IndexList,
    /// Navigation, running heads, filler, and unrelated prose.
    pub ignored: IndexList,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(default)]
#[schemars(deny_unknown_fields)]
pub struct PayloadItem {
    pub kind: Kind,
    /// Models sometimes nest `ignored` inside an item; accepted (and folded
    /// into the chunk's ignored lines) but not part of the schema.
    #[schemars(skip)]
    pub ignored: IndexList,
    /// The item's complete printed name and subtitle. Empty only for the
    /// first item when it continues a recipe cut at the previous chunk.
    pub title: IndexList,
    /// For a variation: the title line(s) of the recipe it varies.
    pub variation_of: IndexList,
    /// Headnote paragraphs.
    pub description: IndexList,
    /// Every line of the serves/makes statement.
    pub recipe_yield: IndexList,
    pub times: PayloadTimes,
    /// Equipment lists and their headings.
    pub equipment: IndexList,
    /// A printed category line.
    #[schemars(length(max = 1))]
    pub category: IndexList,
    /// A printed page number line.
    #[schemars(length(max = 1))]
    pub page: IndexList,
    /// The authored ingredient groups and their steps.
    pub sections: Vec<PayloadSection>,
    /// Tips, do-ahead, storage, serving suggestions, dietary flags,
    /// parenthetical group notes, and combined metadata lines.
    pub notes: IndexList,
    /// Caption lines of this item's photos.
    pub photos: IndexList,
    /// Some models put ingredient lines at item level; they fold into the
    /// unnamed main section.
    #[schemars(skip)]
    pub ingredients: IndexList,
    /// Some models put steps at item level; they fold into the main section.
    #[schemars(skip)]
    pub steps: IndexList,
}

/// Explicitly printed timing lines, one line per field. A line that combines
/// several times, or a time with other metadata, goes to `notes` instead.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(default)]
#[schemars(deny_unknown_fields)]
pub struct PayloadTimes {
    #[schemars(length(max = 1))]
    pub prep: IndexList,
    #[schemars(length(max = 1))]
    pub cook: IndexList,
    #[schemars(length(max = 1))]
    pub active: IndexList,
    #[schemars(length(max = 1))]
    pub total: IndexList,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(default)]
#[schemars(deny_unknown_fields)]
pub struct PayloadSection {
    /// The printed component heading ("For the filling"); empty for the main
    /// or only group.
    pub name: IndexList,
    pub ingredients: IndexList,
    /// Method paragraphs in order. A shared method belongs to the unnamed
    /// main section.
    pub steps: IndexList,
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

Fields. title: the complete printed name and subtitle or translation, never dietary flags; \
a title printed over consecutive lines (name, then translation or subtitle, then perhaps \
the native script) is one title, so select every one of those lines. When a heading is \
followed by a lower-level heading before the first ingredient line, the lower heading \
names the recipe and the higher one is a chapter heading or an essay over its own paragraphs. \
description: the headnote paragraphs. recipe_yield: every line of the serves/makes statement. \
times: explicitly printed timing lines, one per field; a line combining several times or a \
time with other metadata goes in notes and the times fields stay empty. equipment: equipment \
lists and their headings; non-food wrappers listed there stay in equipment. category and \
page: a printed category or page line. sections: keep the authored ingredient groups, an \
unnamed main group plus named components; a short heading over an ingredient list such as \
\"Paste\", \"Fish\", or \"For the sauce\" is a section name, never an item; each ingredient \
line stays in its printed group, \
including unquantified ones such as frying oil or salt; steps are the method paragraphs in \
order and belong to the group they prepare, with a shared method in the unnamed main group. A \
required procedure is always a step, even when printed before the ingredients, labelled as a \
note, or ending with a serving or storage aside. Do not split a line or move part of it. \
notes: tips, do-ahead, storage, serving suggestions, dietary flags, parenthetical group notes. \
photos: caption lines of this item's photos.

Outside items. captions: photo captions belonging to no item; lines marked [caption] sit \
inside a figure and go in photos or captions, never in title, even when they name the dish. \
chapter_headings: chapter or part titles. ignored: \
navigation, running heads, filler, and prose that belongs to no titled item.

Only the first item may continue a recipe cut before this chunk: then its title is [] and \
the continuation title given in the message applies. Every other item needs its own title \
line. Never invent a title from a caption, a link, or a neighbouring recipe. A line naming \
another recipe with \"this page\" or a page number is an ingredient, step, or note, never a \
title; a bare label such as \"Do Ahead\", \"Note\", or \"Special Equipment:\" is not a title \
either. If nothing here \
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
    /// Lines the model left out that were quietly added to `ignored`.
    pub auto_ignored: Vec<usize>,
}

/// An answer that cannot be lowered. The message is written for the model: it
/// is appended to the retry request as feedback.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct Invalid(pub String);

/// Some models hand the tool input back as a JSON string: the whole object,
/// or (Claude Sonnet 5) the whole object or just the item list as a string
/// under `items`. Decode those into the object the schema describes.
fn unwrap_string_payload(payload: Value) -> Result<Value, Invalid> {
    fn parse(text: &str) -> Result<Value, Invalid> {
        serde_json::from_str::<Value>(text)
            .map_err(|e| Invalid(format!("the answer is a string, not a tool object: {e}")))
    }
    let mut payload = match payload {
        Value::String(text) => parse(&text)?,
        other => other,
    };
    let Some(Value::String(text)) = payload.get("items") else {
        return Ok(payload);
    };
    match parse(text)? {
        items @ Value::Array(_) => {
            payload["items"] = items;
            Ok(payload)
        }
        Value::Object(mut inner) => {
            // The outer object's other fields stand unless the inner one
            // repeats them.
            if let Value::Object(outer) = payload {
                for (key, value) in outer {
                    if key != "items" {
                        inner.entry(key).or_insert(value);
                    }
                }
            }
            Ok(Value::Object(inner))
        }
        _ => Err(Invalid(
            "`items` must be a list of items, not a string".into(),
        )),
    }
}

/// Lower a tool answer for `chunk` into text with global line indices.
pub fn lower(chunk: &Chunk, book: &BookLines, payload: Value) -> Result<Lowered, Invalid> {
    let mut payload: Payload = serde_json::from_value(unwrap_string_payload(payload)?)
        .map_err(|e| Invalid(format!("the answer does not match the tool schema: {e}")))?;
    for item in &mut payload.items {
        payload.ignored.extend(std::mem::take(&mut item.ignored));
    }
    let n = chunk.lines();
    let mut used: HashMap<usize, (&'static str, usize)> = HashMap::new();
    let mut take = |indices: &[usize],
                    field: &'static str,
                    owner: usize|
     -> Result<Vec<Text>, Invalid> {
        let mut sorted = indices.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        sorted
            .into_iter()
            .map(|i| {
                if i >= n {
                    return Err(Invalid(format!("line {i} is out of range (0..{n}) in {field}")));
                }
                if let Some(&(previous, previous_owner)) = used.get(&i) {
                    if previous == field && previous_owner == owner {
                        // The same line listed twice under one field of one
                        // item (two sections' ingredients): keep the first.
                        return Ok(None);
                    }
                    if owner == usize::MAX && previous_owner != usize::MAX {
                        // Also listed under captions or ignored: the item's
                        // use wins.
                        return Ok(None);
                    }
                    return Err(Invalid(format!(
                        "line {i} is assigned to both {previous} and {field}; every line belongs to exactly one field"
                    )));
                }
                used.insert(i, (field, owner));
                Ok(Some(Text {
                    line: chunk.global(i),
                    text: book.text(chunk.global(i)).to_string(),
                }))
            })
            .filter_map(Result::transpose)
            .collect()
    };

    let mut payload_items = payload.items;
    payload_items.sort_by_key(first_index);
    fold_untitled_items(&mut payload_items);
    fold_prose_variations(&mut payload_items, chunk, book);

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
            let lines = take(&item.title, "title", index)?;
            let mut seen = BTreeSet::new();
            let text = lines
                .iter()
                .filter(|t| seen.insert(t.text.to_lowercase()))
                .map(|t| t.text.as_str())
                .collect::<Vec<_>>()
                .join(" ");
            (text, lines.into_iter().map(|t| t.line).collect(), false)
        };
        for &i in item.variation_of.iter() {
            if i >= n {
                return Err(Invalid(format!(
                    "variation_of line {i} is out of range (0..{n})"
                )));
            }
        }
        move_combined_metadata_to_notes(&mut item);
        dedupe_within_item(&mut item);
        normalize_sections(&mut item);

        let description = take(&item.description, "description", index)?;
        let notes = take(&item.notes, "notes", index)?;
        let equipment = take(&item.equipment, "equipment", index)?;
        let yield_lines = take(&item.recipe_yield, "recipe_yield", index)?;
        let recipe_yield = (!yield_lines.is_empty()).then(|| {
            yield_lines
                .iter()
                .map(|t| t.text.as_str())
                .collect::<Vec<_>>()
                .join(" ")
        });
        let category = take(&item.category, "category", index)?.into_iter().next();
        let page = take(&item.page, "page", index)?.into_iter().next();
        let mut times = RecipeTimes::default();
        for (field, indices, slot) in [
            ("times.prep", &item.times.prep, &mut times.prep),
            ("times.cook", &item.times.cook, &mut times.cook),
            ("times.active", &item.times.active, &mut times.active),
            ("times.total", &item.times.total, &mut times.total),
        ] {
            *slot = take(indices, field, index)?
                .into_iter()
                .next()
                .map(|t| t.text);
        }
        fill_time_minutes(&mut times);
        let times = (!times.is_empty()).then_some(times);
        let mut sections = Vec::with_capacity(item.sections.len());
        for section in &item.sections {
            let name_lines = take(&section.name, "section name", index)?;
            sections.push(ChunkSection {
                name: (!name_lines.is_empty()).then(|| {
                    name_lines
                        .iter()
                        .map(|t| t.text.as_str())
                        .collect::<Vec<_>>()
                        .join(" ")
                }),
                name_lines: name_lines.into_iter().map(|t| t.line).collect(),
                ingredients: take(&section.ingredients, "ingredients", index)?,
                steps: take(&section.steps, "steps", index)?,
            });
        }
        let photos: Vec<usize> = take(&item.photos, "photos", index)?
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
    // A recipe with no ingredient list and no quantity anywhere in its own
    // text is prose the model mislabelled; keep it as what it is. When it
    // does hold quantities the model misplaced an ingredient list and
    // validation asks again.
    for item in &mut items {
        if item.kind == Kind::Recipe && !item.continues && item.ingredient_count() == 0 {
            let any_quantity = item
                .description
                .iter()
                .chain(item.notes.iter())
                .chain(item.equipment.iter())
                .chain(item.sections.iter().flat_map(|s| s.steps.iter()))
                .any(|t| crate::lines::looks_like_quantity_text(&t.text));
            if !any_quantity {
                item.kind = if item.step_count() > 0 {
                    Kind::Technique
                } else {
                    Kind::Essay
                };
            }
        }
    }
    let mut captions: Vec<usize> = take(&payload.captions, "captions", usize::MAX)?
        .into_iter()
        .map(|t| t.line)
        .collect();
    let chapter_headings: Vec<usize> =
        take(&payload.chapter_headings, "chapter_headings", usize::MAX)?
            .into_iter()
            .map(|t| t.line)
            .collect();
    let mut ignored: Vec<usize> = take(&payload.ignored, "ignored", usize::MAX)?
        .into_iter()
        .map(|t| t.line)
        .collect();
    let mut auto_ignored = Vec::new();
    let mut prose_missing = Vec::new();
    for i in (0..n).filter(|i| !used.contains_key(i)) {
        let global = chunk.global(i);
        let line = &book.lines[global];
        let text = book.text(global);
        if line.clean.in_figure {
            // A caption the model skipped is still a caption.
            captions.push(global);
        } else if is_structural_label(text, line.clean.heading.is_some()) {
            // Method sub-headings ("MAKE THE PASTE"), bare labels, and
            // headings carry no recipe text; leaving them out is harmless.
            auto_ignored.push(global);
        } else {
            prose_missing.push(i);
        }
    }
    if !prose_missing.is_empty() {
        let losable = prose_missing.len() <= MAX_AUTO_IGNORED
            && prose_missing
                .iter()
                .all(|&i| !crate::lines::looks_like_quantity_text(book.text(chunk.global(i))));
        if !losable {
            return Err(Invalid(format!(
                "lines {prose_missing:?} are not assigned to any field; put every line in an item field, captions, chapter_headings, or ignored"
            )));
        }
        auto_ignored.extend(prose_missing.iter().map(|&i| chunk.global(i)));
    }
    auto_ignored.sort_unstable();
    ignored.extend(auto_ignored.iter().copied());
    captions.sort_unstable();
    Ok(Lowered {
        items,
        captions,
        chapter_headings,
        ignored,
        auto_ignored,
    })
}

/// Unassigned prose lines tolerated per chunk (they are ignored and flagged);
/// an unassigned quantity line always fails the answer.
pub const MAX_AUTO_IGNORED: usize = 3;

/// A short heading-like line: a heading tag, an upper-case label ("MAKE THE
/// PASTE"), or a label ending in a colon. Never a quantity.
fn is_structural_label(text: &str, heading: bool) -> bool {
    let t = text.trim();
    if t.is_empty() || t.len() > 60 || crate::lines::looks_like_quantity_text(t) {
        return false;
    }
    if heading || t.ends_with(':') || crate::validate::is_label(t) {
        return true;
    }
    t.chars().any(|c| c.is_alphabetic()) && !t.chars().any(|c| c.is_lowercase())
}

/// An untitled item after the first is a component the model split off (a
/// pizza's per-pie blocks, a sauce printed under its dish): fold it into the
/// item before it as extra sections, notes, and photos. Nothing is lost and
/// the first item keeps its continuation meaning.
fn fold_untitled_items(items: &mut Vec<PayloadItem>) {
    let mut i = 1;
    while i < items.len() {
        if !items[i].title.is_empty() {
            i += 1;
            continue;
        }
        let orphan = items.remove(i);
        let Some(prev) = items.get_mut(i - 1) else {
            continue;
        };
        prev.sections.extend(orphan.sections);
        prev.ingredients.extend(orphan.ingredients.iter().copied());
        prev.steps.extend(orphan.steps.iter().copied());
        prev.notes.extend(orphan.notes.iter().copied());
        prev.notes.extend(orphan.description.iter().copied());
        prev.equipment.extend(orphan.equipment.iter().copied());
        prev.photos.extend(orphan.photos.iter().copied());
        if prev.recipe_yield.is_empty() {
            prev.recipe_yield = orphan.recipe_yield;
        } else {
            prev.notes.extend(orphan.recipe_yield.iter().copied());
        }
        prev.notes.extend(orphan.category.iter().copied());
        prev.notes.extend(orphan.page.iter().copied());
        for t in [
            orphan.times.prep,
            orphan.times.cook,
            orphan.times.active,
            orphan.times.total,
        ] {
            prev.notes.extend(t.iter().copied());
        }
    }
}

/// A line listed in two fields of the same item is a lossless duplicate:
/// keep it in the field with higher precedence and drop the other.
fn dedupe_within_item(item: &mut PayloadItem) {
    let mut seen: HashSet<usize> = HashSet::new();
    let mut keep = |list: &mut IndexList| list.retain(|i| seen.insert(*i));
    keep(&mut item.title);
    for s in &mut item.sections {
        keep(&mut s.name);
    }
    for s in &mut item.sections {
        keep(&mut s.ingredients);
    }
    for s in &mut item.sections {
        keep(&mut s.steps);
    }
    keep(&mut item.recipe_yield);
    keep(&mut item.times.prep);
    keep(&mut item.times.cook);
    keep(&mut item.times.active);
    keep(&mut item.times.total);
    keep(&mut item.equipment);
    keep(&mut item.category);
    keep(&mut item.page);
    keep(&mut item.description);
    keep(&mut item.notes);
    keep(&mut item.photos);
    keep(&mut item.ingredients);
    keep(&mut item.steps);
}

/// A variation whose "title" is a whole paragraph ("Mint: Omit the malted
/// milk powder and add ½ teaspoon peppermint…") is a note on the recipe
/// before it, not an item; fold its lines into that recipe's notes.
fn fold_prose_variations(items: &mut Vec<PayloadItem>, chunk: &Chunk, book: &BookLines) {
    let mut i = 1;
    while i < items.len() {
        let prose = items[i].kind == Kind::Variation
            && items[i].title.iter().any(|&t| {
                let text = book.text(chunk.global(t));
                text.len() > 120 || (text.len() > 60 && text.contains(": "))
            });
        if !prose {
            i += 1;
            continue;
        }
        let mut orphan = items.remove(i);
        let Some(prev) = items.get_mut(i - 1) else {
            continue;
        };
        prev.notes.extend(orphan.title.drain(..));
        orphan.title.clear();
        prev.notes.extend(orphan.description.iter().copied());
        prev.notes.extend(orphan.notes.iter().copied());
        prev.notes.extend(orphan.recipe_yield.iter().copied());
        prev.notes.extend(orphan.equipment.iter().copied());
        prev.notes.extend(orphan.ingredients.iter().copied());
        prev.notes.extend(orphan.steps.iter().copied());
        for section in orphan.sections {
            prev.notes.extend(section.name.iter().copied());
            prev.notes.extend(section.ingredients.iter().copied());
            prev.notes.extend(section.steps.iter().copied());
        }
        prev.photos.extend(orphan.photos.iter().copied());
    }
}

/// Items sort by their title line; an untitled continuation sorts by its
/// first claimed line.
fn first_index(item: &PayloadItem) -> usize {
    if let Some(&t) = item.title.iter().min() {
        return t;
    }
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
    if !item.ingredients.is_empty() || !item.steps.is_empty() {
        item.sections.push(PayloadSection {
            name: IndexList::default(),
            ingredients: std::mem::take(&mut item.ingredients),
            steps: std::mem::take(&mut item.steps),
        });
    }
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
            main.steps = IndexList(steps);
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

    /// A tool input handed back as a JSON string (the whole object, or the
    /// object or item list as a string under `items`) still lowers, and a
    /// paragraph-titled variation folds into the recipe's notes.
    #[rstest]
    #[case::whole_object(|p: Value| Value::String(p.to_string()))]
    #[case::object_under_items(|p: Value| json!({"items": p.to_string()}))]
    #[case::items_under_items(|p: Value| json!({"items": p["items"].to_string(), "ignored": []}))]
    fn string_payloads_and_prose_variations_lower(#[case] encode: fn(Value) -> Value) {
        let (book, chunk) = book_and_chunk(
            &[
                "Malted Brownies",
                "2 cups flour",
                "Bake.",
                "Mint: Omit the malted milk powder and add ½ teaspoon peppermint extract with the vanilla, then top with crushed candy canes.",
            ],
            0,
        );
        let payload = json!({"items":[
            {"title":[0],"sections":[{"ingredients":[1],"steps":[2]}],"ignored":[]},
            {"kind":"variation","title":[3],"variation_of":[0]}]});
        let lowered = lower(&chunk, &book, encode(payload)).unwrap();
        assert_eq!(lowered.items.len(), 1);
        assert_eq!(lowered.items[0].notes.len(), 1);
        assert!(lowered.items[0].notes[0].text.starts_with("Mint:"));
        assert!(crate::validate::validate(&chunk, &book, &lowered).is_ok());
    }

    #[rstest]
    #[case::unassigned_quantity(json!({"items":[{"title":[0],"sections":[{"ingredients":[2]}]}],"ignored":[1,3,5,6]}), "not assigned")]
    #[case::too_many_unassigned(json!({"items":[{"title":[0],"sections":[{"ingredients":[2,4]}]}],"ignored":[]}), "not assigned")]
    #[case::double_across_items(json!({"items":[{"title":[0],"sections":[{"ingredients":[2]}]},{"title":[3],"sections":[{"ingredients":[2,4]}]}],"ignored":[1,5,6]}), "assigned to both")]
    #[case::out_of_range(json!({"items":[{"title":[0],"sections":[{"ingredients":[9]}]}],"ignored":[1,2,3,4,5,6]}), "out of range")]
    #[case::continuation_without_hint(json!({"items":[{"title":[],"sections":[{"ingredients":[2]}]}],"ignored":[0,1,3,4,5,6]}), "no continuation title")]
    fn rejects_bad_coverage(#[case] payload: Value, #[case] message: &str) {
        let (book, chunk) = book_and_chunk(SOUP, 0);
        let err = lower(&chunk, &book, payload).unwrap_err();
        assert!(err.0.contains(message), "{err}");
    }

    #[test]
    fn untitled_later_items_fold_into_their_predecessor() {
        let (book, chunk) = book_and_chunk(SOUP, 0);
        let payload = json!({"items":[
            {"title":[0],"sections":[{"ingredients":[2]}]},
            {"title":[],"description":[1],"sections":[{"name":[3],"ingredients":[4],"steps":[5]}]}],
            "ignored":[6]});
        let lowered = lower(&chunk, &book, payload).unwrap();
        assert_eq!(lowered.items.len(), 1);
        let item = &lowered.items[0];
        assert_eq!(item.ingredient_count(), 2);
        assert_eq!(
            item.sections
                .iter()
                .map(|s| s.name.as_deref())
                .collect::<Vec<_>>(),
            [None, Some("Paste")]
        );
        assert_eq!(item.notes[0].text, "A translation");
    }

    #[test]
    fn index_lists_accept_strings_and_whole_floats() {
        let (book, chunk) = book_and_chunk(SOUP, 0);
        let payload = json!({"items":[{"title":["0"],"page":1.0,"sections":[{"name":[3],"ingredients":["2",4],"steps":[5]}],"notes":[6]}]});
        let lowered = lower(&chunk, &book, payload).unwrap();
        assert_eq!(lowered.items[0].ingredient_count(), 2);
        let bad = json!({"items":[{"title":[0.5],"sections":[{"ingredients":[2]}]}]});
        assert!(
            lower(&chunk, &book, bad)
                .unwrap_err()
                .0
                .contains("line number")
        );
    }

    #[test]
    fn a_few_dropped_prose_lines_are_ignored_and_reported() {
        let (book, chunk) = book_and_chunk(SOUP, 0);
        // Lines 3 ("Paste") and 6 ("(V)") are left out. ("A translation"
        // parses as a quantity — "a" is one — so it would not be losable.)
        let payload =
            json!({"items":[{"title":[0,1],"sections":[{"ingredients":[2,4],"steps":[5]}]}]});
        let lowered = lower(&chunk, &book, payload).unwrap();
        assert_eq!(lowered.auto_ignored, [3, 6]);
        assert!(lowered.ignored.contains(&3) && lowered.ignored.contains(&6));
    }

    #[test]
    fn duplicates_within_an_item_keep_the_stronger_field() {
        let (book, chunk) = book_and_chunk(SOUP, 0);
        let payload = json!({"items":[{"title":[0,1],"notes":[1,6],"sections":[{"ingredients":[2,4],"steps":[5]}],"photos":[5]}],"ignored":[3]});
        let item = lower(&chunk, &book, payload).unwrap().items.remove(0);
        assert_eq!(item.title, "Soup – A translation");
        assert_eq!(item.notes.iter().map(|t| t.line).collect::<Vec<_>>(), [6]);
        assert!(item.photos.is_empty(), "a step beats a photo caption claim");
    }

    #[test]
    fn item_level_ingredients_and_steps_fold_into_the_main_section() {
        let (book, chunk) = book_and_chunk(SOUP, 0);
        let payload = json!({"items":[{"title":[0],"ingredients":[2,4],"steps":[5],"sections":[{"name":[3]}]}],"ignored":[1,6]});
        let item = lower(&chunk, &book, payload).unwrap().items.remove(0);
        assert_eq!(item.ingredient_count(), 2);
        assert_eq!(item.step_count(), 1);
        let schema = tool_schema(7).to_string();
        assert!(
            !schema.contains("\"steps\":{\"type\":\"array\"},\"title\""),
            "item-level fields stay out of the schema"
        );
    }

    #[test]
    fn bare_integers_and_stray_root_fields_are_tolerated() {
        let (book, chunk) = book_and_chunk(SOUP, 0);
        let payload = json!({"items":[{"title":0,"page":1,"sections":[{"name":3,"ingredients":[2,4],"steps":5}],"notes":6}],"stray":[1]});
        let lowered = lower(&chunk, &book, payload).unwrap();
        assert_eq!(lowered.items[0].page.as_ref().map(|t| t.line), Some(1));
        assert_eq!(lowered.items[0].step_count(), 1);
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
