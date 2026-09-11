//! Unique display names. `title` stays verbatim; `name` is the title made
//! unique within the book: a variation qualifier first, then the chapter,
//! then a counter.

use std::collections::HashMap;

use crate::crosscheck::normalize_title;
use crate::model::{Chapter, Item};

pub fn assign_names(chapters: &mut [Chapter]) {
    // Pass 0: everything starts as its title.
    let mut counts: HashMap<String, usize> = HashMap::new();
    for item in chapters.iter().flat_map(|c| c.items.iter()) {
        *counts.entry(normalize_title(item.title())).or_default() += 1;
    }
    let parent_names: HashMap<String, String> = chapters
        .iter()
        .flat_map(|c| c.items.iter())
        .map(|i| (i.id().to_string(), i.title().to_string()))
        .collect();
    let mut taken: HashMap<String, usize> = HashMap::new();
    for chapter in chapters.iter_mut() {
        let chapter_title = chapter.title.clone();
        for item in &mut chapter.items {
            let title = item.title().to_string();
            let mut candidates = vec![title.clone()];
            if counts.get(&normalize_title(&title)).copied().unwrap_or(0) > 1 {
                if let Item::Recipe(r) = item
                    && let Some(parent) = r.variant_of.as_deref().and_then(|p| parent_names.get(p))
                {
                    candidates.push(format!("{title} ({parent} variation)"));
                }
                if let Some(ch) = &chapter_title {
                    candidates.push(format!("{title} ({ch})"));
                }
            }
            let mut chosen = None;
            for candidate in &candidates {
                let key = normalize_title(candidate);
                if !taken.contains_key(&key) {
                    chosen = Some(candidate.clone());
                    break;
                }
            }
            let name = chosen.unwrap_or_else(|| {
                let mut n = 2;
                loop {
                    let candidate = format!("{title} ({n})");
                    if !taken.contains_key(&normalize_title(&candidate)) {
                        break candidate;
                    }
                    n += 1;
                }
            });
            *taken.entry(normalize_title(&name)).or_default() += 1;
            *taken.entry(normalize_title(&title)).or_default() += 1;
            set_name(item, name);
        }
    }
}

fn set_name(item: &mut Item, name: String) {
    match item {
        Item::Recipe(r) => r.name = name,
        Item::Technique(t) => t.name = name,
        Item::Essay(e) => e.name = name,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Essay, Recipe, RecipeMeta, Span};

    fn recipe(id: &str, title: &str, variant_of: Option<&str>) -> Item {
        Item::Recipe(Box::new(Recipe {
            id: id.into(),
            title: title.into(),
            name: String::new(),
            meta: RecipeMeta::default(),
            sections: vec![],
            photos: vec![],
            notes: vec![],
            variant_of: variant_of.map(str::to_string),
            span: Span {
                start: 0,
                end: 0,
                doc_path: String::new(),
                page: None,
            },
        }))
    }

    fn chapter(title: Option<&str>, items: Vec<Item>) -> Chapter {
        Chapter {
            id: "ch".into(),
            title: title.map(str::to_string),
            span: Span {
                start: 0,
                end: 0,
                doc_path: String::new(),
                page: None,
            },
            intro: vec![],
            items,
        }
    }

    fn names(chapters: &[Chapter]) -> Vec<String> {
        chapters
            .iter()
            .flat_map(|c| c.items.iter().map(|i| i.name().to_string()))
            .collect()
    }

    #[test]
    fn unique_titles_keep_their_name() {
        let mut ch = vec![chapter(
            Some("Starches"),
            vec![recipe("a", "Polenta", None), recipe("b", "Risotto", None)],
        )];
        assign_names(&mut ch);
        assert_eq!(names(&ch), ["Polenta", "Risotto"]);
    }

    #[test]
    fn duplicates_take_variation_then_chapter_then_counter() {
        let mut ch = vec![
            chapter(
                Some("Starches"),
                vec![
                    recipe("a", "Polenta", None),
                    recipe("b", "Polenta", Some("a")),
                ],
            ),
            chapter(
                Some("Sides"),
                vec![recipe("c", "Polenta", None), recipe("d", "polenta", None)],
            ),
        ];
        assign_names(&mut ch);
        assert_eq!(
            names(&ch),
            [
                "Polenta",
                "Polenta (Polenta variation)",
                "Polenta (Sides)",
                "polenta (2)"
            ]
        );
    }

    #[test]
    fn essays_and_recipes_share_one_namespace() {
        let essay = Item::Essay(Essay {
            id: "e".into(),
            title: "Bread".into(),
            name: String::new(),
            text: vec![],
            photos: vec![],
            span: Span {
                start: 0,
                end: 0,
                doc_path: String::new(),
                page: None,
            },
        });
        let mut ch = vec![chapter(None, vec![recipe("a", "Bread", None), essay])];
        assign_names(&mut ch);
        assert_eq!(names(&ch), ["Bread", "Bread (2)"]);
    }
}
