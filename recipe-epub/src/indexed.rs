//! Source-indexed extraction: the model selects structure, Rust owns all text.
use crate::{Chunk, ChunkRequest, EpubError};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::HashMap;

#[derive(Deserialize)]
struct Payload {
    recipes: Vec<Recipe>,
    ignored: Vec<usize>,
}
#[derive(Deserialize)]
struct Recipe {
    title: Vec<usize>,
    #[serde(default)]
    description: Vec<usize>,
    sections: Vec<Section>,
    #[serde(default)]
    recipe_yield: Vec<usize>,
    #[serde(default)]
    notes: Vec<usize>,
    #[serde(default)]
    equipment: Vec<usize>,
    #[serde(default)]
    category: Vec<usize>,
    #[serde(default)]
    page: Vec<usize>,
    #[serde(default)]
    times: Times,
}
#[derive(Default, Deserialize)]
#[serde(default)]
struct Times {
    prep: Vec<usize>,
    cook: Vec<usize>,
    active: Vec<usize>,
    total: Vec<usize>,
}
#[derive(Deserialize)]
struct Section {
    name: Vec<usize>,
    ingredients: Vec<usize>,
    instructions: Vec<usize>,
}

/// Additive request API. The legacy string-based request remains available to
/// existing browser consumers. Native extraction uses this lossless contract.
pub fn build_indexed_chunk_request(chunk: &Chunk) -> ChunkRequest {
    let maximum = chunk.text.lines().count().saturating_sub(1);
    let index = json!({"type":"integer","minimum":0,"maximum":maximum});
    let indices = json!({"type":"array","items":index});
    let optional = json!({"type":"array","items":index,"maxItems":1});
    let schema = json!({"type":"object","properties":{
        "recipes":{"type":"array","items":{"type":"object","properties":{
            "title":indices,"description":indices,"recipe_yield":indices,
            "notes":indices,"equipment":indices,"category":optional,"page":optional,
            "times":{"type":"object","properties":{"prep":optional,"cook":optional,"active":optional,"total":optional}},
            "sections":{"type":"array","items":{"type":"object","properties":{"name":optional,"ingredients":indices,"instructions":indices},"required":["name","ingredients","instructions"]}}
        },"required":["title","description","sections","notes","equipment"]}},
        "ignored":indices
    },"required":["recipes","ignored"]});
    let user = chunk
        .text
        .lines()
        .enumerate()
        .map(|(i, line)| format!("{i}: {line}"))
        .collect::<Vec<_>>()
        .join("\n");
    ChunkRequest {
        system: "Extract EVERY recipe by selecting zero-based source line numbers. Return indices, never rewritten text. All fields contain arrays of indices: use [] for absent metadata and unnamed section names, never a placeholder 0. Account for EVERY input line exactly once across recipe fields or ignored.\nFor each recipe: title includes its complete name and subtitle/translation, but excludes dietary flags. description contains its introductory headnote. Preserve the authored ingredient groups: an unnamed main section and named components such as Masala, Filling, Batter, Paste, or To serve. Each ingredient line belongs to its source group, including unquantified frying oil and flour used for sealing/dusting. Instructions contain ALL method paragraphs in original order. When the book has one shared method, put ALL method lines in the unnamed main section, even if ingredients have several named groups. Never omit the method. notes includes dietary flags, tips, variations, storage and serving suggestions. recipe_yield selects ALL lines of the complete serves/makes metadata, including parenthetical container yield. Parenthetical ingredient-group preparation or advance-start notes belong in notes, never ingredients. times select explicitly printed timing metadata only. equipment selects explicit equipment lists only.\nA line may not be used twice. Do not split a line or fix apparent source errors. Put non-recipe prose, contents/index entries, page headers, blank lines and photo captions in ignored; never ignore recipe ingredients, steps, headnotes or notes. A document without recipes returns recipes=[] and all lines in ignored. Recipe continuations may use title=[] when a continuation title is supplied; do not invent a title.".into(),
        user: match &chunk.title_hint { Some(title) => format!("Continuation title (only use when the source starts mid-recipe): {title}\n\n{user}"), None => user },
        tool_name: "emit_recipes".into(), tool_schema: schema,
    }
}

/// Lower indexed fields to the existing string payload; reject omissions,
/// overlapping ownership, and invalid indices before anything enters the cache.
pub fn lower_indexed_payload(chunk: &Chunk, payload: Value) -> Result<Value, EpubError> {
    let raw_payload = payload.clone();
    let payload: Payload = serde_json::from_value(payload)?;
    let lines: Vec<_> = chunk.text.lines().collect();
    let mut used = HashMap::new();
    let fail = |message: String| {
        EpubError::Proxy(format!(
            "indexed source coverage: {message}; selection={raw_payload}"
        ))
    };
    let mut take = |indices: &[usize], field: &str| -> Result<Vec<String>, EpubError> {
        let mut indices = indices.to_vec();
        indices.sort_unstable();
        indices
            .into_iter()
            .map(|i| {
                let line = lines
                    .get(i)
                    .ok_or_else(|| fail(format!("line {i} is out of bounds")))?;
                if let Some(previous) = used.insert(i, field.to_owned()) {
                    return Err(fail(format!(
                        "line {i} is assigned to both {previous} and {field}"
                    )));
                }
                Ok((*line).to_owned())
            })
            .collect()
    };
    let mut recipes = Vec::new();
    for mut recipe in payload.recipes {
        if let (Some(start), Some(end)) = (
            recipe.title.iter().min(),
            recipe.sections.iter().flat_map(|s| &s.ingredients).min(),
        ) {
            for i in &payload.ignored {
                if i > start
                    && i < end
                    && lines.get(*i).is_some_and(|line| {
                        line.trim().starts_with('(') && line.trim().ends_with(')')
                    })
                {
                    return Err(fail(format!(
                        "parenthetical recipe metadata on line {i} must be preserved in notes, not ignored"
                    )));
                }
            }
        }
        let title = if recipe.title.is_empty() {
            chunk
                .title_hint
                .clone()
                .ok_or_else(|| fail("recipe has no title".into()))?
        } else {
            take(&recipe.title, "title")?.join(" ")
        };
        let description = take(&recipe.description, "description")?.join("\n\n");
        let notes = take(&recipe.notes, "notes")?;
        let equipment = take(&recipe.equipment, "equipment")?;
        let yield_lines = take(&recipe.recipe_yield, "recipe_yield")?;
        let recipe_yield = if yield_lines.is_empty() {
            Value::Null
        } else {
            json!(yield_lines.join(" "))
        };
        let mut optional_line = |indices: &[usize]| -> Result<Value, EpubError> {
            if indices.len() > 1 {
                return Err(fail("optional metadata selects more than one line".into()));
            }
            Ok(json!(take(indices, "optional metadata")?.first()))
        };
        let category = optional_line(&recipe.category)?;
        let page = optional_line(&recipe.page)?;
        let mut times = serde_json::Map::new();
        for (key, index) in [
            ("prep", recipe.times.prep),
            ("cook", recipe.times.cook),
            ("active", recipe.times.active),
            ("total", recipe.times.total),
        ] {
            if !index.is_empty() {
                times.insert(key.into(), optional_line(&index)?);
            }
        }
        // Authored headings partition the ingredient list. An unlabelled model
        // section cannot invent a new boundary after a printed component heading.
        let mut ingredients: Vec<_> = recipe
            .sections
            .iter_mut()
            .flat_map(|s| std::mem::take(&mut s.ingredients))
            .collect();
        ingredients.sort_unstable();
        if !recipe.sections.iter().any(|s| s.name.is_empty()) {
            recipe.sections.push(Section {
                name: vec![],
                ingredients: vec![],
                instructions: vec![],
            });
        }
        for line in ingredients {
            let target = recipe
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
                .or_else(|| recipe.sections.iter().position(|s| s.name.is_empty()));
            if let Some(target) = target {
                recipe.sections[target].ingredients.push(line);
            }
        }
        // Component headings before a single shared method describe ingredients,
        // not separate timelines. Restore that method before sorting components.
        let first_step = recipe
            .sections
            .iter()
            .flat_map(|s| s.instructions.iter())
            .min()
            .copied();
        let last_ingredient = recipe
            .sections
            .iter()
            .flat_map(|s| s.ingredients.iter())
            .max()
            .copied();
        if let (Some(first), Some(last)) = (first_step, last_ingredient)
            && first > last
            && recipe
                .sections
                .iter()
                .all(|s| s.name.iter().all(|name| *name < first))
        {
            let mut instructions: Vec<_> = recipe
                .sections
                .iter_mut()
                .flat_map(|s| std::mem::take(&mut s.instructions))
                .collect();
            instructions.sort_unstable();
            if let Some(main) = recipe.sections.iter_mut().find(|s| s.name.is_empty()) {
                main.instructions = instructions;
            } else {
                recipe.sections.insert(
                    0,
                    Section {
                        name: vec![],
                        ingredients: vec![],
                        instructions,
                    },
                );
            }
        }
        // Ingredients retain source order regardless of the model's grouping order.
        recipe.sections.sort_by_key(|s| {
            s.ingredients
                .iter()
                .min()
                .copied()
                .or_else(|| s.name.first().copied())
                .or_else(|| s.instructions.iter().min().copied())
                .unwrap_or(usize::MAX)
        });
        let mut sections = Vec::new();
        for section in recipe.sections {
            if section.name.len() > 1 {
                return Err(fail("section name selects more than one line".into()));
            }
            let name = json!(take(&section.name, "section name")?.first());
            sections.push(json!({"name":name,"ingredients":take(&section.ingredients, "ingredients")?,"instructions":take(&section.instructions, "instructions")?}));
        }
        let mut output =
            json!({"title":title,"sections":sections,"notes":notes,"equipment":equipment});
        if !description.is_empty() {
            output["description"] = json!(description);
        }
        if !recipe_yield.is_null() {
            output["recipe_yield"] = recipe_yield;
        }
        if !category.is_null() {
            output["category"] = category;
        }
        if !page.is_null() {
            output["page"] = page;
        }
        if !times.is_empty() {
            output["times"] = Value::Object(times);
        }
        recipes.push(output);
    }
    take(&payload.ignored, "ignored")?;
    if used.len() != lines.len() {
        let missing: Vec<_> = (0..lines.len()).filter(|i| !used.contains_key(i)).collect();
        return Err(fail(format!("unassigned source lines {missing:?}")));
    }
    Ok(json!({"recipes":recipes}))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    fn chunk() -> Chunk {
        Chunk {
            title_hint: None,
            text:
                "Soup –\nA translation\n1 cup water\nPaste\n2 cloves garlic\nMix everything.\n(V)"
                    .into(),
            doc_path: "soup.xhtml".into(),
            links: vec![],
            images: vec![],
        }
    }
    fn payload() -> Value {
        json!({"recipes":[{"title":[0,1],"description":[],"sections":[{"name":[3],"ingredients":[4],"instructions":[]},{"name":[],"ingredients":[2],"instructions":[5]}],"notes":[6],"equipment":[]}],"ignored":[]})
    }
    #[test]
    fn copies_source_and_restores_component_order() {
        let out = lower_indexed_payload(&chunk(), payload()).unwrap();
        assert_eq!(out["recipes"][0]["title"], "Soup – A translation");
        assert_eq!(
            out["recipes"][0]["sections"][0]["ingredients"],
            json!(["1 cup water"])
        );
        assert_eq!(out["recipes"][0]["sections"][1]["name"], "Paste");
    }
    #[test]
    fn preserves_multiline_yield_and_bounds_schema_indices() {
        let mut source = chunk();
        source.text.push_str("\nMAKES 12\n(enough for one jar)");
        let mut value = payload();
        value["recipes"][0]["recipe_yield"] = json!([8, 7]);
        let out = lower_indexed_payload(&source, value).unwrap();
        assert_eq!(
            out["recipes"][0]["recipe_yield"],
            "MAKES 12 (enough for one jar)"
        );
        let req = build_indexed_chunk_request(&source);
        assert_eq!(
            req.tool_schema["properties"]["ignored"]["items"]["maximum"],
            8
        );
    }
    #[test]
    fn rejects_discarded_header_metadata_and_preserves_printed_groups() {
        let mut source = chunk();
        source.text = "Soup\n(V)\nSauce\n1g salt\n2g oil\nMix.".into();
        let mut value = json!({"recipes":[{"title":[0],"notes":[],"sections":[{"name":[2],"ingredients":[3],"instructions":[]},{"name":[],"ingredients":[4],"instructions":[5]}]}],"ignored":[1]});
        assert!(
            lower_indexed_payload(&source, value.clone())
                .unwrap_err()
                .to_string()
                .contains("metadata")
        );
        value["recipes"][0]["notes"] = json!([1]);
        value["ignored"] = json!([]);
        let out = lower_indexed_payload(&source, value).unwrap();
        let sections = out["recipes"][0]["sections"].as_array().unwrap();
        assert_eq!(
            sections.iter().find(|s| s["name"] == "Sauce").unwrap()["ingredients"],
            json!(["1g salt", "2g oil"])
        );
    }
    #[rstest::rstest]
    #[case::missing(json!([]))]
    #[case::duplicate(json!([6,6]))]
    #[case::out_of_bounds(json!([99]))]
    fn rejects_lost_or_reassigned_lines(#[case] notes: Value) {
        let mut value = payload();
        value["recipes"][0]["notes"] = notes;
        assert!(lower_indexed_payload(&chunk(), value).is_err());
    }
}
