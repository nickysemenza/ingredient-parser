//! Generate real, offline cookbook inputs for native and browser QA: a fixture
//! EPUB, a saved run extracted by the fixture oracle (no model), and a JSON
//! bundle the Playwright bridge serves as command answers.
//! Run: cargo run -p food-app --example create_run_fixture -- /tmp/food-app-qa
#![allow(clippy::expect_used)]

use cookbook::cache::NoCache;
use cookbook::test_support::{Oracle, epub3_roles, oracle_transport};
use cookbook::{Book, CancelToken};
use std::path::PathBuf;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn main() -> Result<()> {
    let mut args = std::env::args_os().skip(1);
    let output = PathBuf::from(
        args.next()
            .ok_or("usage: create_run_fixture OUTPUT_DIRECTORY")?,
    );
    if args.next().is_some() {
        return Err("usage: create_run_fixture OUTPUT_DIRECTORY".into());
    }
    std::fs::create_dir_all(&output)?;
    let output = output.canonicalize()?;
    let runs_dir = output.join("runs");
    // SAFETY: single-threaded so far; the run store reads this on first use.
    unsafe { std::env::set_var(cookbook::native::runs::RUNS_DIR_VAR, &runs_dir) };

    let epub_path = output.join("cookbook.epub");
    let bytes = cookbook_fixtures::epub3_nav_pagebreaks()?;
    std::fs::write(&epub_path, &bytes)?;
    let book_string = epub_path.to_string_lossy().into_owned();

    let opened = Book::open(bytes, "fixture")?;
    let transport = oracle_transport(
        Oracle::new(epub3_roles()),
        opened.lines().clone(),
        opened.chunks().to_vec(),
    );
    let runtime = tokio::runtime::Builder::new_current_thread().build()?;
    let summary = runtime.block_on(food_app::backend::extract_book_with(
        book_string.clone(),
        &transport,
        &NoCache,
        CancelToken::new(),
        |_| {},
    ))?;
    if summary.incomplete || summary.recipes < 3 {
        return Err(format!("fixture extraction is not complete: {summary:?}").into());
    }
    let estimate = food_app::backend::estimate_book(book_string.clone(), Default::default())?;
    let runs = food_app::backend::list_runs()?;
    let library = food_app::backend::scan_library(output.to_string_lossy().into_owned())?;
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
    let recipe_page = output.join("weeknight-soup.html");
    std::fs::write(
        &recipe_page,
        r#"<script type="application/ld+json">{"name":"Weeknight soup","recipeIngredient":["1 cup (240 g) water","1 tsp salt"],"recipeInstructions":[{"@type":"HowToStep","text":"Add 1 cup (240 g) water; cut into 3cm cubes, then bake at 365 degrees F for 20 minutes."}]}</script>"#,
    )?;
    let bundle = food_app::backend::export_bundle(summary.path.clone(), book_string.clone())?;
    // Native-only answers. Portable commands run for real in the browser's
    // WASM worker against these files, preloaded under their native paths.
    let files: serde_json::Map<String, serde_json::Value> = [
        (book_string.clone(), epub_path.clone()),
        (summary.path.clone(), PathBuf::from(&summary.path)),
        (
            corpus_path.to_string_lossy().into_owned(),
            corpus_path.clone(),
        ),
    ]
    .into_iter()
    .map(|(key, path)| -> Result<_> {
        use base64::Engine;
        let bytes = std::fs::read(path)?;
        Ok((
            key,
            base64::engine::general_purpose::STANDARD
                .encode(bytes)
                .into(),
        ))
    })
    .collect::<Result<_>>()?;
    // For the tests' expectations only; the app opens the run itself.
    let extraction = food_app::backend::core(
        "open_run",
        serde_json::json!({ "path": summary.path }),
    )?["extraction"]
        .take();
    let frontend = serde_json::json!({
        "extraction": extraction,
        "estimate": estimate,
        "bundle": bundle,
        "runs": runs,
        "library": library,
        "gateway": food_app::backend::gateway_status(),
        "corpusPath": corpus_path,
        "recipeHtml": std::fs::read_to_string(&recipe_page)?,
        "recipeUrl": "https://example.com/weeknight-soup",
        "files": files,
    });
    let frontend_path = output.join("frontend-fixture.json");
    std::fs::write(&frontend_path, serde_json::to_string_pretty(&frontend)?)?;
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "epub": epub_path, "run": summary.path, "frontend": frontend_path,
            "recipes": summary.recipes, "items": summary.items, "bundle": bundle.path,
        }))?
    );
    Ok(())
}
