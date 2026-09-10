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
                    let classes = e.value().attr("class").unwrap_or("");
                    let styled_div = tag == "div"
                        && classes.split_whitespace().any(|class| {
                            is_ingredient_class(class)
                                || matches!(class, "method_step" | "headnote" | "headnote1")
                        });
                    if !styled_div
                        && !matches!(tag, "p" | "li" | "h1" | "h2" | "h3" | "h4" | "figcaption")
                    {
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
                                scraper::Node::Element(e) if e.name() == "br" => Some(" "),
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
    // Older indexed outputs joined a repeated caption and heading. Repair only
    // an exact doubled source heading, including during offline replay.
    let repeated_titles: std::collections::HashMap<_, std::collections::HashMap<_, _>> = documents
        .iter()
        .map(|doc| {
            let titles = doc
                .blocks
                .iter()
                .filter(|block| matches!(block.tag.as_str(), "h1" | "h2"))
                .filter_map(|block| {
                    let title = block.text.split_whitespace().collect::<Vec<_>>().join(" ");
                    (!title.is_empty()).then(|| (format!("{title} {title}").to_lowercase(), title))
                })
                .collect();
            (doc.path.as_str(), titles)
        })
        .collect();
    for recipe in recipes.iter_mut() {
        let key = recipe
            .meta
            .title
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase();
        if let Some(title) = recipe
            .url
            .rsplit_once('#')
            .and_then(|(_, doc)| repeated_titles.get(doc))
            .and_then(|titles| titles.get(&key))
        {
            recipe.meta.title.clone_from(title);
        }
    }
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
    let normalized: std::collections::HashMap<_, _> = documents
        .iter()
        .map(|doc| {
            let mut blocks: std::collections::HashMap<String, Vec<usize>> =
                std::collections::HashMap::new();
            for (i, block) in doc.blocks.iter().enumerate() {
                blocks.entry(normalize(&block.text)).or_default().push(i);
            }
            (doc.path.as_str(), blocks)
        })
        .collect();
    for (index, recipe) in recipes.iter_mut().enumerate() {
        let Some((_, doc_path)) = recipe.url.rsplit_once('#') else {
            continue;
        };
        let Some(doc) = documents.iter().find(|d| d.path == doc_path) else {
            continue;
        };
        let positions = &normalized[doc_path];
        restore_preparation_ingredients(recipe, doc, positions);
        restore_empty_ingredient_sections(recipe, doc, positions);
        restore_shared_method(recipe, positions);
        for line in recipe.sections.iter().flat_map(|s| &s.ingredients) {
            let Some(indices) = positions.get(&normalize(line)) else {
                continue;
            };
            // Repeated source wording with different links has no unique ownership.
            let [i] = indices.as_slice() else {
                continue;
            };
            let block = &doc.blocks[*i];
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
pub(crate) fn is_ingredient_block(block: &SourceBlock) -> bool {
    block.classes.split_whitespace().any(is_ingredient_class)
}
fn is_ingredient_class(class: &str) -> bool {
    matches!(class, "ril" | "rilf") || class == "IL_item" || class.starts_with("IL_item_")
}

/// Restore leading ingredient lists classified as preparation notes. Require an
/// unambiguous authored recipe heading and source positions inside that recipe;
/// a matching ingredient elsewhere in the chapter is not sufficient evidence.
fn restore_preparation_ingredients(
    recipe: &mut crate::CookbookRecipe,
    doc: &SourceDocument,
    positions: &std::collections::HashMap<String, Vec<usize>>,
) {
    let compact = |s: &str| {
        s.chars()
            .filter(|c| !c.is_whitespace())
            .flat_map(char::to_lowercase)
            .collect::<String>()
    };
    let title = compact(&recipe.meta.title);
    let heading = |b: &SourceBlock| {
        matches!(b.tag.as_str(), "h1" | "h2") || b.classes.split_whitespace().any(|c| c == "rt")
    };
    let matches: Vec<_> = doc
        .blocks
        .iter()
        .enumerate()
        .filter(|(_, b)| heading(b) && compact(&b.text) == title)
        .map(|(i, _)| i)
        .collect();
    let [start] = matches.as_slice() else {
        return;
    };
    let end = doc
        .blocks
        .iter()
        .enumerate()
        .skip(start + 1)
        .find(|(_, b)| heading(b))
        .map_or(doc.blocks.len(), |(i, _)| i);
    let position = |text: &str| {
        let found: Vec<_> = positions
            .get(&crate::extractor::normalize_source_whitespace(text))?
            .iter()
            .copied()
            .filter(|&i| i > *start && i < end)
            .collect();
        if let [i] = found.as_slice() {
            Some(*i)
        } else {
            None
        }
    };
    let Some(first) = recipe
        .sections
        .iter()
        .flat_map(|s| &s.ingredients)
        .filter_map(|line| position(line))
        .min()
    else {
        return;
    };
    let mut restored: Vec<_> = recipe
        .meta
        .notes
        .iter()
        .enumerate()
        .filter_map(|(note, text)| {
            let i = position(text)?;
            (i < first
                && is_ingredient_block(&doc.blocks[i])
                && !recipe
                    .sections
                    .iter()
                    .flat_map(|s| &s.ingredients)
                    .any(|line| line == text))
            .then(|| (i, note, text.clone()))
        })
        .collect();
    restored.sort_by_key(|(i, _, _)| *i);
    if restored.is_empty() {
        return;
    }
    let mut moved: std::collections::HashSet<_> =
        restored.iter().map(|(_, note, _)| *note).collect();
    let section_name = restored
        .first()
        .and_then(|(i, _, _)| i.checked_sub(1))
        .and_then(|i| {
            recipe
                .meta
                .notes
                .iter()
                .enumerate()
                .find(|(_, text)| position(text) == Some(i))
                .filter(|(_, _)| !is_ingredient_block(&doc.blocks[i]))
                .map(|(note, text)| {
                    moved.insert(note);
                    text.clone()
                })
        });
    recipe.sections.insert(
        0,
        crate::RecipeSection {
            name: section_name,
            ..crate::RecipeSection::new(
                restored.into_iter().map(|(_, _, text)| text).collect(),
                vec![],
            )
        },
    );
    recipe.meta.notes = std::mem::take(&mut recipe.meta.notes)
        .into_iter()
        .enumerate()
        .filter_map(|(i, text)| (!moved.contains(&i)).then_some(text))
        .collect();
}

fn restore_empty_ingredient_sections(
    recipe: &mut crate::CookbookRecipe,
    doc: &SourceDocument,
    positions: &std::collections::HashMap<String, Vec<usize>>,
) {
    let mut i = 1;
    while i < recipe.sections.len() {
        let section = &recipe.sections[i];
        let source_ingredient = section
            .name
            .as_ref()
            .and_then(|name| positions.get(&crate::extractor::normalize_source_whitespace(name)))
            .is_some_and(|indices| {
                !indices.is_empty()
                    && indices
                        .iter()
                        .all(|&index| is_ingredient_block(&doc.blocks[index]))
            });
        let already_owned = section.name.as_ref().is_some_and(|name| {
            recipe
                .sections
                .iter()
                .flat_map(|s| &s.ingredients)
                .any(|line| {
                    crate::extractor::normalize_source_whitespace(line)
                        == crate::extractor::normalize_source_whitespace(name)
                })
        });
        if section.ingredients.is_empty()
            && section.instructions.is_empty()
            && source_ingredient
            && !already_owned
        {
            let removed = recipe.sections.remove(i);
            if let Some(name) = removed.name {
                recipe.sections[i - 1].ingredients.push(name);
            }
        } else {
            i += 1;
        }
    }
}

fn restore_shared_method(
    recipe: &mut crate::CookbookRecipe,
    positions: &std::collections::HashMap<String, Vec<usize>>,
) {
    let norm = crate::extractor::normalize_source_whitespace;
    let position = |text: &str| {
        let matches = positions.get(&norm(text))?;
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
            !positions
                .get(&norm(s))
                .is_some_and(|indices| indices.iter().any(|i| *i < first))
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
    fn source_inspection_includes_styled_divs_without_their_container()
    -> Result<(), Box<dyn std::error::Error>> {
        use std::io::{Cursor, Write};
        let mut original =
            zip::ZipArchive::new(Cursor::new(recipe_epub_fixtures::cookbook_epub()?))?;
        let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for i in 0..original.len() {
            let file = original.by_index(i)?;
            if file.name() == "OEBPS/tomato-soup.xhtml" {
                archive.start_file(file.name(), zip::write::SimpleFileOptions::default())?;
                archive.write_all(br#"<html xmlns="http://www.w3.org/1999/xhtml"><body><div class="recipe"><h1>Tacos<br/>with Sauce</h1><div class="headnote">A quick supper.</div><div class="IL_item"><a href="flatbread.xhtml">dipping sauce</a></div><div class="method_step">Serve with the sauce.</div></div></body></html>"#)?;
            } else {
                archive.raw_copy_file(file)?;
            }
        }
        let docs = inspect_source(&archive.finish()?.into_inner())?;
        let doc = docs
            .iter()
            .find(|doc| doc.path == "OEBPS/tomato-soup.xhtml")
            .ok_or("missing source document")?;
        assert_eq!(
            doc.blocks
                .iter()
                .map(|b| b.text.as_str())
                .collect::<Vec<_>>(),
            [
                "Tacos with Sauce",
                "A quick supper.",
                "dipping sauce",
                "Serve with the sauce."
            ]
        );
        assert_eq!(doc.blocks[2].links[0].text, "dipping sauce");
        Ok(())
    }
    #[rstest::rstest]
    #[case("Tacos", "IL_item", true)]
    #[case("Other recipe", "IL_item", false)]
    #[case("Tacos", "note", false)]
    fn preparation_notes_restore_only_source_proven_ingredients(
        #[case] heading: &str,
        #[case] class: &str,
        #[case] repair: bool,
    ) {
        let mut recipe = crate::CookbookRecipe {
            meta: crate::RecipeMeta {
                title: "Tacos".into(),
                notes: vec![
                    "ADVANCE PREPARATION".into(),
                    "Salsa Roja, for serving".into(),
                ],
                ..Default::default()
            },
            sections: vec![crate::RecipeSection::new(vec!["1 tomato".into()], vec![])],
            source: "book".into(),
            url: "book#chapter".into(),
            references: vec![],
            image: None,
        };
        let doc = SourceDocument {
            path: "chapter".into(),
            images: vec![],
            anchors: vec![],
            blocks: [
                ("h2", "", heading),
                ("p", "heading", "ADVANCE PREPARATION"),
                ("p", class, "Salsa Roja, for serving"),
                ("p", "IL_item", "1 tomato"),
            ]
            .into_iter()
            .enumerate()
            .map(|(i, (tag, classes, text))| SourceBlock {
                id: i.to_string(),
                element_index: i,
                anchor: None,
                tag: tag.into(),
                classes: classes.into(),
                text: text.into(),
                links: vec![],
            })
            .collect(),
        };
        enrich_from_source(
            std::slice::from_mut(&mut recipe),
            std::slice::from_ref(&doc),
        );
        assert_eq!(recipe.sections.len(), if repair { 2 } else { 1 });
        assert_eq!(recipe.meta.notes.len(), if repair { 0 } else { 2 });
        if repair {
            assert_eq!(
                recipe.sections[0].name.as_deref(),
                Some("ADVANCE PREPARATION")
            );
            assert_eq!(recipe.sections[0].ingredients, ["Salsa Roja, for serving"]);
        }
        let once = serde_json::to_value(&recipe).unwrap();
        enrich_from_source(std::slice::from_mut(&mut recipe), &[doc]);
        assert_eq!(serde_json::to_value(recipe).unwrap(), once);
    }
    #[rstest::rstest]
    #[case("IL_item", true)]
    #[case("IL_item_space", true)]
    #[case("rilh", false)]
    #[case("", false)]
    fn empty_section_is_an_ingredient_only_with_source_markup(
        #[case] class: &str,
        #[case] repair: bool,
    ) {
        let mut recipe = crate::CookbookRecipe {
            meta: Default::default(),
            sections: vec![
                crate::RecipeSection::new(vec!["1 tomato".into()], vec![]),
                crate::RecipeSection {
                    name: Some("dipping sauce".into()),
                    ..crate::RecipeSection::new(vec![], vec![])
                },
            ],
            source: "book".into(),
            url: "book#main.xhtml".into(),
            references: vec![],
            image: None,
        };
        let docs = vec![SourceDocument {
            path: "main.xhtml".into(),
            anchors: vec![],
            images: vec![],
            blocks: vec![SourceBlock {
                id: "ingredient".into(),
                element_index: 0,
                anchor: None,
                tag: "p".into(),
                classes: class.into(),
                text: "dipping sauce".into(),
                links: vec![],
            }],
        }];
        enrich_from_source(std::slice::from_mut(&mut recipe), &docs);
        assert_eq!(recipe.sections.len(), if repair { 1 } else { 2 });
        assert_eq!(
            recipe.sections[0].ingredients.len(),
            if repair { 2 } else { 1 }
        );
    }
    #[rstest::rstest]
    #[case("Lemon Cake LEMON CAKE", "Lemon Cake")]
    #[case("Lemon Lemon Cake", "Lemon Lemon Cake")]
    #[case("Other Cake Other Cake", "Other Cake Other Cake")]
    fn replay_repairs_only_exact_doubled_source_headings(
        #[case] title: &str,
        #[case] expected: &str,
    ) {
        let mut recipes = vec![crate::CookbookRecipe {
            meta: crate::RecipeMeta {
                title: title.into(),
                ..Default::default()
            },
            sections: vec![],
            source: "book".into(),
            url: "book#main.xhtml".into(),
            references: vec![],
            image: None,
        }];
        let docs = vec![SourceDocument {
            path: "main.xhtml".into(),
            anchors: vec![],
            images: vec![],
            blocks: vec![SourceBlock {
                id: "title".into(),
                element_index: 0,
                anchor: None,
                tag: "h2".into(),
                classes: String::new(),
                text: "Lemon Cake".into(),
                links: vec![],
            }],
        }];
        enrich_from_source(&mut recipes, &docs);
        assert_eq!(recipes[0].meta.title, expected);
    }
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
