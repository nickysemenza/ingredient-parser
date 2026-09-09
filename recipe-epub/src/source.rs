//! Source inspection and source-backed recipe associations. No transport or filesystem.
use crate::EpubError;
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceBlock {
    pub id: String,
    pub element_index: usize,
    pub anchor: Option<String>,
    pub tag: String,
    pub classes: String,
    pub text: String,
    #[serde(default)]
    pub links: Vec<crate::Link>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceDocument {
    pub path: String,
    pub blocks: Vec<SourceBlock>,
    #[serde(default)]
    pub images: Vec<crate::ImageRef>,
    #[serde(default)]
    pub anchors: Vec<String>,
}

pub fn inspect_source(bytes: &[u8]) -> Result<Vec<SourceDocument>, EpubError> {
    let mut doc = epub::doc::EpubDoc::from_reader(std::io::Cursor::new(bytes))
        .map_err(|e| EpubError::Open(e.to_string()))?;
    let mut result = Vec::new();
    loop {
        let path = doc
            .get_current_path()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        if let Some((raw, mime)) = doc.get_current()
            && mime.contains("html")
        {
            let decoded = String::from_utf8_lossy(&raw);
            let xhtml = crate::epub_text::close_self_closing_rawtext(&decoded);
            let html = scraper::Html::parse_document(&xhtml);
            let blocks = html
                .tree
                .nodes()
                .filter_map(scraper::ElementRef::wrap)
                .enumerate()
                .filter_map(|(i, e)| {
                    let tag = e.value().name();
                    if !matches!(tag, "p" | "li" | "h1" | "h2" | "h3" | "h4" | "figcaption") {
                        return None;
                    }
                    Some(SourceBlock {
                        id: format!("{path}:element-{i}"),
                        element_index: i,
                        anchor: e.value().attr("id").map(str::to_owned),
                        tag: tag.into(),
                        classes: e.value().attr("class").unwrap_or("").into(),
                        links: e
                            .descendants()
                            .filter_map(scraper::ElementRef::wrap)
                            .filter(|a| a.value().name() == "a")
                            .filter_map(|a| {
                                let href = a.value().attr("href")?;
                                let (resource, fragment) =
                                    href.split_once('#').unwrap_or((href, ""));
                                let target = if resource.is_empty() {
                                    path.clone()
                                } else {
                                    crate::epub_text::resolve_relative(&path, resource)?
                                };
                                Some(crate::Link {
                                    text: a.text().collect::<String>(),
                                    href: if fragment.is_empty() {
                                        target
                                    } else {
                                        format!("{target}#{fragment}")
                                    },
                                })
                            })
                            .collect(),
                        text: e
                            .descendants()
                            .filter(|n| {
                                !n.ancestors()
                                    .filter_map(scraper::ElementRef::wrap)
                                    .any(crate::epub_text::is_footnote_marker)
                            })
                            .filter_map(|n| match n.value() {
                                scraper::Node::Text(t) => Some::<&str>(t),
                                _ => None,
                            })
                            .collect::<String>()
                            .split_whitespace()
                            .collect::<Vec<_>>()
                            .join(" "),
                    })
                })
                .collect();
            let images = html
                .tree
                .nodes()
                .filter_map(scraper::ElementRef::wrap)
                .filter(|e| matches!(e.value().name(), "img" | "image"))
                .filter_map(|e| {
                    let resource = e
                        .value()
                        .attr("src")
                        .or_else(|| e.value().attr("href"))
                        // SVG xlink:href has a namespace; attr() only looks up
                        // unqualified attributes, while attrs() exposes local names.
                        .or_else(|| {
                            e.value()
                                .attrs()
                                .find(|(name, _)| *name == "href")
                                .map(|(_, value)| value)
                        })?;
                    let image_path = crate::epub_text::resolve_relative(&path, resource)?;
                    Some(crate::ImageRef {
                        mime: crate::epub_text::mime_from_ext(&image_path)?,
                        path: image_path,
                        alt: e.value().attr("alt").map(str::to_owned),
                    })
                })
                .collect();
            let anchors = html
                .tree
                .nodes()
                .filter_map(scraper::ElementRef::wrap)
                .filter_map(|e| e.value().attr("id").map(str::to_owned))
                .collect();
            result.push(SourceDocument {
                path,
                blocks,
                images,
                anchors,
            });
        }
        if !doc.go_next() {
            break;
        }
    }
    Ok(result)
}

/// Resolve literal ingredient hyperlinks when their target document contains one
/// recipe. Unqualified link text such as "here" is sufficient; ambiguous targets
/// and instruction-only links remain unassigned.
pub fn enrich_from_source(recipes: &mut [crate::CookbookRecipe], documents: &[SourceDocument]) {
    let by_doc: std::collections::HashMap<String, Vec<usize>> =
        recipes
            .iter()
            .enumerate()
            .fold(std::collections::HashMap::new(), |mut map, (i, r)| {
                if let Some((_, doc)) = r.url.rsplit_once('#') {
                    map.entry(doc.to_owned()).or_default().push(i);
                }
                map
            });
    let titles: Vec<_> = recipes.iter().map(|r| r.meta.title.clone()).collect();
    let normalize = crate::extractor::normalize_source_whitespace;
    for (index, recipe) in recipes.iter_mut().enumerate() {
        let Some((_, doc_path)) = recipe.url.rsplit_once('#') else {
            continue;
        };
        let Some(doc) = documents.iter().find(|d| d.path == doc_path) else {
            continue;
        };
        restore_shared_method(recipe, doc);
        for line in recipe.sections.iter().flat_map(|s| &s.ingredients) {
            let blocks: Vec<_> = doc
                .blocks
                .iter()
                .filter(|b| normalize(&b.text) == normalize(line))
                .collect();
            // Repeated source wording with different links has no unique ownership.
            let [block] = blocks.as_slice() else {
                continue;
            };
            for link in &block.links {
                let (target, anchor) = link.href.split_once('#').unwrap_or((&link.href, ""));
                let Some(target_doc) = documents.iter().find(|d| d.path == target) else {
                    continue;
                };
                if !anchor.is_empty() && !target_doc.anchors.iter().any(|a| a == anchor) {
                    continue;
                }
                let Some(indices) = by_doc.get(target) else {
                    continue;
                };
                let [other] = indices.as_slice() else {
                    continue;
                };
                if *other == index {
                    continue;
                }
                if let Some(existing) = recipe
                    .references
                    .iter_mut()
                    .find(|r| r.line == *line && r.title == titles[*other])
                {
                    existing.confidence = crate::RefConfidence::Linked;
                } else {
                    recipe.references.push(crate::RecipeRef {
                        title: titles[*other].clone(),
                        line: line.clone(),
                        confidence: crate::RefConfidence::Linked,
                    });
                }
            }
        }
    }
    // A standalone following photo is a candidate for source review, not enough
    // evidence by itself to attach a hero. Preserve it in SourceDocument.images.
}

/// Replay can restore source ordering even for older string-based model outputs.
fn restore_shared_method(recipe: &mut crate::CookbookRecipe, doc: &SourceDocument) {
    let norm = crate::extractor::normalize_source_whitespace;
    let position = |text: &str| {
        let matches: Vec<_> = doc
            .blocks
            .iter()
            .enumerate()
            .filter(|(_, b)| norm(&b.text) == norm(text))
            .map(|(i, _)| i)
            .collect();
        if let [i] = matches.as_slice() {
            Some(*i)
        } else {
            None
        }
    };
    let steps: Option<Vec<_>> = recipe
        .sections
        .iter()
        .flat_map(|s| &s.instructions)
        .map(|s| position(s).map(|i| (i, s.clone())))
        .collect();
    let Some(mut steps) = steps else {
        return;
    };
    let Some(first) = steps.iter().map(|(i, _)| *i).min() else {
        return;
    };
    if recipe
        .sections
        .iter()
        .flat_map(|s| &s.ingredients)
        .any(|s| {
            !doc.blocks
                .iter()
                .enumerate()
                .any(|(i, b)| i < first && norm(&b.text) == norm(s))
        })
        || recipe
            .sections
            .iter()
            .filter_map(|s| s.name.as_deref())
            .any(|s| position(s).is_none_or(|i| i >= first))
    {
        return;
    }
    steps.sort_by_key(|(i, _)| *i);
    for section in &mut recipe.sections {
        section.instructions.clear();
    }
    let instructions = steps.into_iter().map(|(_, s)| s).collect();
    if let Some(main) = recipe.sections.iter_mut().find(|s| s.name.is_none()) {
        main.instructions = instructions;
    } else {
        recipe
            .sections
            .insert(0, crate::RecipeSection::new(vec![], instructions));
    }
    recipe
        .sections
        .retain(|s| s.name.is_some() || !s.ingredients.is_empty() || !s.instructions.is_empty());
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    #[test]
    fn literal_here_link_resolves_only_a_unique_recipe_target() {
        let recipe = |title: &str, doc: &str, ingredients: Vec<String>| crate::CookbookRecipe {
            meta: crate::RecipeMeta {
                title: title.into(),
                ..Default::default()
            },
            sections: vec![crate::RecipeSection::new(ingredients, vec![])],
            source: "book".into(),
            url: format!("book#{doc}"),
            references: vec![],
            image: None,
        };
        let line = "2 tablespoons sauce (see here)";
        let docs = vec![
            SourceDocument {
                path: "main.xhtml".into(),
                anchors: vec![],
                images: vec![],
                blocks: vec![SourceBlock {
                    id: "line".into(),
                    element_index: 0,
                    anchor: None,
                    tag: "p".into(),
                    classes: String::new(),
                    text: line.into(),
                    links: vec![crate::Link {
                        text: "here".into(),
                        href: "sauce.xhtml#sauce".into(),
                    }],
                }],
            },
            SourceDocument {
                path: "sauce.xhtml".into(),
                anchors: vec!["sauce".into()],
                images: vec![],
                blocks: vec![],
            },
        ];
        let main = recipe("Main dish", "main.xhtml", vec![line.into()]);
        let sauce = recipe(
            "Sauce – full bilingual title",
            "sauce.xhtml",
            vec!["1 tomato".into()],
        );
        let mut recipes = vec![main.clone(), sauce.clone()];
        enrich_from_source(&mut recipes, &docs);
        assert_eq!(recipes[0].references[0].title, sauce.meta.title);
        assert_eq!(
            recipes[0].references[0].confidence,
            crate::RefConfidence::Linked
        );
        let mut ambiguous = vec![main, sauce.clone(), sauce];
        enrich_from_source(&mut ambiguous, &docs);
        assert!(ambiguous[0].references.is_empty());
    }
}
