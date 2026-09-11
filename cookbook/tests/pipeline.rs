//! End-to-end: fixture EPUB bytes → `Book::extract` with the class oracle
//! standing in for the model → the book tree, checked against each fixture's
//! ground truth.

#![cfg(not(target_arch = "wasm32"))]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use cookbook::cache::NoCache;
use cookbook::chunk::ChunkOptions;
use cookbook::test_support::{
    Oracle, RoleMap, epub3_roles, oracle_transport, split_spine_roles, typographic_roles,
};
use cookbook::{Book, CancelToken, ExtractOptions, Extraction, Item, Recipe, RefKind, RefMethod};
use cookbook_fixtures::{Expected, ExpectedRecipe};

fn options() -> ExtractOptions {
    ExtractOptions {
        label: "fixture".into(),
        concurrency: 4,
        ladder: vec!["gemini-2.5-flash".into(), "claude-haiku-4-5".into()],
        ..ExtractOptions::default()
    }
}

async fn extract(
    bytes: Vec<u8>,
    roles: RoleMap,
    chunking: &ChunkOptions,
) -> (Book, Extraction, usize) {
    let book = Book::open_with(bytes, "fixture", chunking).unwrap();
    let transport = oracle_transport(
        Oracle::new(roles),
        book.lines().clone(),
        book.chunks().to_vec(),
    );
    let extraction = book
        .extract(
            &options(),
            &transport,
            &NoCache,
            &CancelToken::new(),
            |_| {},
        )
        .await
        .unwrap();
    let calls = transport.calls();
    (book, extraction, calls)
}

fn recipes(extraction: &Extraction) -> Vec<&Recipe> {
    extraction.cookbook.recipes().collect()
}

fn find<'a>(extraction: &'a Extraction, title: &str) -> &'a Recipe {
    recipes(extraction)
        .into_iter()
        .find(|r| r.title.eq_ignore_ascii_case(title))
        .unwrap_or_else(|| {
            panic!(
                "no recipe titled {title:?}; have {:?}",
                recipes(extraction)
                    .iter()
                    .map(|r| &r.title)
                    .collect::<Vec<_>>()
            )
        })
}

fn ingredient_count(r: &Recipe) -> usize {
    r.sections.iter().map(|s| s.ingredients.len()).sum()
}

fn step_count(r: &Recipe) -> usize {
    r.sections.iter().map(|s| s.steps.len()).sum()
}

/// Every standalone recipe (and every variation with its own ingredients)
/// comes out with the expected counts; essays come out as essays.
fn check_expected(extraction: &Extraction, expected: Expected) {
    let items: Vec<&Item> = extraction.cookbook.items().collect();
    for e in expected.recipes {
        if e.is_essay {
            assert!(
                items.iter().any(|i| matches!(i, Item::Essay(_)) && i.title().eq_ignore_ascii_case(e.title)),
                "{:?} should be an essay; items: {:?}",
                e.title,
                items.iter().map(|i| (i.title(), std::mem::discriminant(*i))).collect::<Vec<_>>()
            );
            continue;
        }
        if e.is_variation && e.ingredient_lines == 0 {
            continue; // folded into the parent as a note; checked per fixture
        }
        let r = find(extraction, e.title);
        assert_eq!(
            ingredient_count(r),
            e.ingredient_lines,
            "{}: ingredients",
            e.title
        );
        assert_eq!(step_count(r), e.steps, "{}: steps", e.title);
        assert_eq!(
            r.variant_of.is_some(),
            e.is_variation,
            "{}: variation",
            e.title
        );
    }
    let expected_recipes = expected
        .recipes
        .iter()
        .filter(|e: &&ExpectedRecipe| !(e.is_essay || (e.is_variation && e.ingredient_lines == 0)))
        .count();
    assert_eq!(
        recipes(extraction).len(),
        expected_recipes,
        "recipe count; got {:?}",
        recipes(extraction)
            .iter()
            .map(|r| &r.title)
            .collect::<Vec<_>>()
    );
    let invalid: Vec<String> = extraction
        .report
        .calls
        .iter()
        .filter_map(|c| match &c.outcome {
            cookbook::CallOutcome::Invalid { faults } => Some(format!(
                "{} {}: {}",
                c.chunk_id,
                c.model,
                faults.join(" | ")
            )),
            _ => None,
        })
        .collect();
    assert!(
        !extraction.report.incomplete,
        "incomplete; invalid answers: {invalid:#?}; crosscheck {:?}; escalation {:?}",
        extraction.report.crosscheck, extraction.report.escalation
    );
    assert!(extraction.report.total_cost_usd > 0.0);
    assert!(extraction.report.cost_complete);
    assert!(items.iter().all(|i| !i.name().is_empty()));
    let mut names: Vec<&str> = items.iter().map(|i| i.name()).collect();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), items.len(), "names are unique");
}

#[tokio::test]
async fn split_spine_recipes_survive_the_page_split() {
    let (book, extraction, calls) = extract(
        cookbook_fixtures::split_spine().unwrap(),
        split_spine_roles(),
        &ChunkOptions::default(),
    )
    .await;
    assert_eq!(calls, book.chunks().len());
    check_expected(&extraction, cookbook_fixtures::split_spine_expected());
    let mushrooms = find(&extraction, "Tangy Roasted Mushrooms");
    assert_eq!(
        mushrooms.span.doc_path, "OEBPS/index_split_047.html",
        "the span starts at the title's document"
    );
    assert!(
        mushrooms.span.end > book.lines().docs[3].first_line,
        "…and runs into the next one"
    );
    assert_eq!(mushrooms.photos.len(), 1);
    assert_eq!(mushrooms.photos[0].path, "OEBPS/images/00036.jpg");
    assert_eq!(
        mushrooms.meta.recipe_yield.as_deref(),
        Some("serves 4 to 8")
    );
    assert_eq!(
        mushrooms
            .notes
            .iter()
            .map(|n| n.label.as_deref())
            .collect::<Vec<_>>(),
        [Some("DO AHEAD"), Some("NOTE")]
    );
    assert_eq!(mushrooms.meta.description.len(), 1);
    let chicken = find(&extraction, "Sheet-Pan Chicken with Crispy Potatoes");
    let reference = chicken
        .sections
        .iter()
        .flat_map(|s| s.ingredients.iter())
        .find_map(|l| l.reference.as_ref())
        .expect("chicken references the mushrooms");
    assert_eq!(reference.target_id, mushrooms.id);
    assert_eq!(
        (reference.kind, reference.method),
        (RefKind::Ingredient, RefMethod::Anchor)
    );
    assert_eq!(reference.text, "this page");
    assert!(
        extraction
            .cookbook
            .edges
            .iter()
            .any(|e| e.from == chicken.id && e.to == mushrooms.id && e.kind == RefKind::Ingredient)
    );
    assert_eq!(
        extraction.cookbook.dependency_order()[..2],
        [mushrooms.id.clone(), chicken.id.clone()]
    );
    let chapters: Vec<Option<&str>> = extraction
        .cookbook
        .chapters
        .iter()
        .map(|c| c.title.as_deref())
        .collect();
    assert!(chapters.contains(&Some("Vegetables")), "{chapters:?}");
    assert!(
        book.read_image(&mushrooms.photos[0].path)
            .is_some_and(|(bytes, mime)| bytes.starts_with(&[0xFF, 0xD8]) && mime == "image/jpeg")
    );
}

#[tokio::test]
async fn split_spine_survives_tiny_chunks_and_continuations() {
    let (book, extraction, _) = extract(
        cookbook_fixtures::split_spine().unwrap(),
        split_spine_roles(),
        &ChunkOptions {
            budget: 300,
            slack: 150,
        },
    )
    .await;
    assert!(book.chunks().len() >= 3, "{}", book.chunks().len());
    assert!(
        book.chunks().iter().any(|c| c.title_hint.is_some()),
        "at least one hard split"
    );
    check_expected(&extraction, cookbook_fixtures::split_spine_expected());
}

#[tokio::test]
async fn epub3_nav_pagebreaks_and_captions() {
    let (_, extraction, _) = extract(
        cookbook_fixtures::epub3_nav_pagebreaks().unwrap(),
        epub3_roles(),
        &ChunkOptions::default(),
    )
    .await;
    check_expected(
        &extraction,
        cookbook_fixtures::epub3_nav_pagebreaks_expected(),
    );
    let pie = find(&extraction, "Cranberry-Pomegranate Mousse Pie");
    let crust = find(&extraction, "Graham Cracker Crust");
    let cherry = find(&extraction, "Sour Cherry Pie");
    assert_eq!(pie.meta.page.as_deref(), Some("79"));
    assert_eq!(crust.meta.page.as_deref(), Some("327"));
    let reference = pie
        .sections
        .iter()
        .flat_map(|s| s.ingredients.iter())
        .find_map(|l| l.reference.as_ref())
        .unwrap();
    assert_eq!(reference.target_id, crust.id);
    assert_eq!(reference.method, RefMethod::Anchor);
    // The ingredient-less variation is a labelled note on its parent.
    assert!(
        crust
            .notes
            .iter()
            .any(|n| n.label.as_deref() == Some("Speculoos Variation")),
        "{:?}",
        crust.notes
    );
    // The caption naming Sour Cherry Pie is not a title, and its photo goes
    // to Sour Cherry Pie; the mousse pie keeps its own hero.
    assert!(
        !extraction
            .cookbook
            .items()
            .any(|i| i.title().starts_with("Sour Cherry Pie,"))
    );
    assert!(
        cherry
            .photos
            .iter()
            .any(|p| p.caption.as_deref() == Some("Sour Cherry Pie, this page")),
        "{:?}",
        cherry.photos
    );
    assert!(
        pie.photos.iter().all(|p| p.caption.is_none()),
        "{:?}",
        pie.photos
    );
    assert_eq!(pie.photos.len(), 1);
    assert!(
        pie.meta
            .equipment
            .iter()
            .any(|e| e.contains("9-inch pie plate"))
    );
    let chapters: Vec<Option<&str>> = extraction
        .cookbook
        .chapters
        .iter()
        .map(|c| c.title.as_deref())
        .collect();
    assert_eq!(
        chapters,
        [Some("Pies and Tarts"), Some("Foundational Recipes")]
    );
    assert_eq!(extraction.report.crosscheck.nav_titles, 4);
    assert_eq!(
        extraction.report.crosscheck.missing.len(),
        0,
        "{:?}",
        extraction.report.crosscheck
    );
}

#[tokio::test]
async fn typographic_variations_and_plain_page_references() {
    let (_, extraction, _) = extract(
        cookbook_fixtures::typographic_variations().unwrap(),
        typographic_roles(),
        &ChunkOptions::default(),
    )
    .await;
    check_expected(
        &extraction,
        cookbook_fixtures::typographic_variations_expected(),
    );
    let polenta = find(&extraction, "Polenta");
    let corn_polenta = find(&extraction, "Polenta with Fresh Corn");
    let soft = find(&extraction, "Soft Polenta with Braised Greens");
    let corn = find(&extraction, "Corn");
    assert_eq!(
        corn_polenta.variant_of.as_deref(),
        Some(polenta.id.as_str())
    );
    let refs: Vec<(&str, RefMethod)> = corn_polenta
        .sections
        .iter()
        .flat_map(|s| s.ingredients.iter())
        .filter_map(|l| l.reference.as_ref())
        .map(|r| (r.target_id.as_str(), r.method))
        .collect();
    assert!(
        refs.contains(&(polenta.id.as_str(), RefMethod::Title)),
        "{refs:?}"
    );
    assert!(
        refs.contains(&(corn.id.as_str(), RefMethod::Anchor)),
        "{refs:?}"
    );
    let page_ref = soft
        .sections
        .iter()
        .flat_map(|s| s.ingredients.iter())
        .find_map(|l| l.reference.as_ref())
        .unwrap();
    assert_eq!(
        (page_ref.target_id.as_str(), page_ref.method),
        (polenta.id.as_str(), RefMethod::Page)
    );
    assert!(
        extraction
            .cookbook
            .edges
            .iter()
            .any(|e| e.from == corn_polenta.id
                && e.to == polenta.id
                && e.kind == RefKind::Variation)
    );
    assert_ne!(polenta.name, corn_polenta.name);
}
