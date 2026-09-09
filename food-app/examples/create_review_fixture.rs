//! Generate real, offline cookbook review inputs for native and browser QA.
//! Run: cargo run -p food-app --example create_review_fixture -- /tmp/food-app-qa
use recipe_epub::review::{ReviewDecision, ReviewDecisions, ReviewRun, SourceDocument};
use recipe_epub::{ExtractedRecipe, MockExtractor, RecipeMeta, RecipeSection};
use std::path::PathBuf;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

// These are authored fixture labels, not a general recipe extraction heuristic.
// Read the actual inspected XHTML classes so output cannot drift from the EPUB.
fn expected_recipe(document: &SourceDocument) -> Result<ExtractedRecipe> {
    let title = document
        .blocks
        .iter()
        .find(|b| b.tag == "h1")
        .ok_or("fixture document is missing its h1")?
        .text
        .clone();
    let lines = |class: &str| {
        document
            .blocks
            .iter()
            .filter(|b| b.classes.split_whitespace().any(|c| c == class))
            .map(|b| b.text.clone())
            .collect::<Vec<_>>()
    };
    let ingredients = lines("ingredient");
    let instructions = lines("instruction");
    if ingredients.is_empty() || instructions.is_empty() {
        return Err(format!("fixture labels missing from {}", document.path).into());
    }
    let prose = document
        .blocks
        .iter()
        .filter(|b| b.tag == "p" && b.classes.is_empty())
        .map(|b| b.text.clone())
        .collect::<Vec<_>>();
    Ok(ExtractedRecipe {
        meta: RecipeMeta {
            title,
            description: prose.first().cloned(),
            recipe_yield: prose.iter().find(|p| p.ends_with(" servings")).cloned(),
            ..Default::default()
        },
        sections: vec![RecipeSection::new(ingredients, instructions)],
    })
}

fn main() -> Result<()> {
    let mut args = std::env::args_os().skip(1);
    let output = PathBuf::from(
        args.next()
            .ok_or("usage: create_review_fixture OUTPUT_DIRECTORY")?,
    );
    if args.next().is_some() {
        return Err("usage: create_review_fixture OUTPUT_DIRECTORY".into());
    }
    std::fs::create_dir_all(&output)?;
    let output = output.canonicalize()?;
    let epub_path = output.join("cookbook.epub");
    let run_path = output.join("cookbook-run.json");
    let review_path = run_path.with_extension("review.json");
    let bytes = recipe_epub_fixtures::cookbook_epub()?;
    std::fs::write(&epub_path, &bytes)?;
    let mut run = ReviewRun::inspect(&bytes, &epub_path.to_string_lossy(), "offline-fixture-mock")?;
    if run.documents.len() != 3 {
        return Err("expected three source documents in cookbook fixture".into());
    }
    let rules = run
        .documents
        .iter()
        .map(|document| {
            let recipe = expected_recipe(document)?;
            Ok((recipe.meta.title.clone(), vec![recipe]))
        })
        .collect::<Result<Vec<_>>>()?;
    let extractor = MockExtractor::new(rules);
    let runtime = tokio::runtime::Builder::new_current_thread().build()?;
    runtime.block_on(recipe_epub::review::extract_run_with_extractor(
        &mut run,
        &output.join("extraction-cache"),
        &run_path,
        &extractor,
        |_| {},
    ))?;
    if run.recipes.len() != 3 || run.parsed.as_array().map(Vec::len) != Some(3) || run.incomplete()
    {
        return Err("fixture replay did not produce three complete parsed recipes".into());
    }
    let decisions = ReviewDecisions {
        version: 1,
        epub_sha256: run.epub_sha256.clone(),
        documents: run.documents.iter().enumerate().map(|(index, document)| {
            (document.path.clone(), ReviewDecision {
                status: ["Unreviewed", "Accepted", "Uncertain"][index].into(),
                note: if index == 2 { "Synthetic QA review note; verify the lemon quantity against the source.".into() } else { String::new() },
            })
        }).collect(),
    };
    decisions.save(&review_path)?;
    // Check the same persisted boundaries used by both app transports.
    let restored = ReviewRun::read(&run_path)?;
    ReviewDecisions::read(&review_path, &restored)?;
    let inputs = ["2 cups flour", "salt and pepper to taste", "???", ""];
    let ingredients = food_app::backend::parse_batch(inputs.join("\n") + "\n")?;
    // Keep the real corpus and add one intentionally mislabeled QA case so the
    // failure workflow remains exercised when the repository corpus is all exact.
    let corpus_path = output.join("corpus.jsonl");
    let mismatch = serde_json::json!({
        "input": "1 cup fixture flour", "name": "intentionally mismatched label",
        "xfail": "Synthetic QA mismatch for field comparison"
    });
    std::fs::write(
        &corpus_path,
        format!("{}\n{}\n", ingredient_corpus::embedded(), mismatch),
    )?;
    let corpus = food_app::backend::load_corpus(Some(corpus_path.to_string_lossy().into_owned()))?;
    let html = r#"<script type="application/ld+json">{"name":"Weeknight soup","recipeIngredient":["1 cup (240 g) water","1 tsp salt"],"recipeInstructions":[{"@type":"HowToStep","text":"Add 1 cup (240 g) water; cut into 3cm cubes, then bake at 365 degrees F for 20 minutes."}]}</script>"#;
    let source = serde_json::to_value(recipe_scraper::scrape(
        html,
        "https://example.com/weeknight-soup",
    )?)?;
    let web_recipe = food_app::backend::scale_web_recipe(source.clone(), 1.0)?;
    let scaled_web_recipe = food_app::backend::scale_web_recipe(source, 2.0)?;
    let all_inputs: std::collections::BTreeSet<_> = inputs
        .into_iter()
        .map(str::to_owned)
        .chain(
            run.recipes
                .iter()
                .flat_map(|recipe| &recipe.sections)
                .flat_map(|section| section.ingredients.iter().cloned()),
        )
        // Only trace the corpus rows exercised by browser QA; scoring the large
        // table needs every row, but serializing hundreds of trace trees does not.
        .chain(
            corpus
                .cases
                .iter()
                .filter(|case| case.status != "EXACT")
                .map(|case| case.input.clone()),
        )
        .chain(web_recipe.ingredients.iter().map(|row| row.input.clone()))
        .collect();
    let inspections = all_inputs
        .into_iter()
        .map(|input| Ok((input.clone(), food_app::backend::inspect_ingredient(input)?)))
        .collect::<Result<std::collections::BTreeMap<_, _>>>()?;
    let frontend_path = output.join("frontend-fixture.json");
    let run_string = run_path.to_string_lossy().into_owned();
    let frontend = serde_json::json!({
        "cookbook": food_app::backend::open_run(run_string.clone())?,
        "sourceOnly": food_app::backend::inspect_book(epub_path.to_string_lossy().into_owned(), None)?,
        "ingredients": ingredients,
        "inspections": inspections,
        "corpus": corpus,
        "library": food_app::backend::scan_library(output.to_string_lossy().into_owned())?,
        "stats": food_app::backend::run_stats(run_string.clone())?,
        "audit": food_app::backend::run_audit(run_string.clone())?,
        "scaledCookbook": food_app::backend::scale_recipe(run_string,0,2.0)?,
        "webRecipe": web_recipe,
        "scaledWebRecipe": scaled_web_recipe,
    });
    std::fs::write(&frontend_path, serde_json::to_vec_pretty(&frontend)?)?;
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "epub": epub_path, "run": run_path, "review": review_path,
            "frontend": frontend_path,
            "documents": restored.documents.len(), "recipes": restored.recipes.len(),
        }))?
    );
    Ok(())
}
