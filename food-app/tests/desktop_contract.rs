//! Durable application-command contracts exercised without a webview or model.
use food_app::backend;
use recipe_epub::review::ReviewRun;
use recipe_epub::{ExtractedRecipe, MockExtractor, RecipeMeta, RecipeSection};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};

type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;
static NEXT: AtomicUsize = AtomicUsize::new(0);

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Result<Self> {
        let path = std::env::temp_dir().join(format!(
            "food-app-contract-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path)?;
        Ok(Self(path))
    }
    fn path(&self, name: &str) -> String {
        self.0.join(name).to_string_lossy().into_owned()
    }
    fn complete_run(&self) -> Result<ReviewRun> {
        let book = self.path("book.epub");
        std::fs::write(&book, recipe_epub_fixtures::cookbook_epub()?)?;
        let inspected = backend::inspect_book(book, None)?;
        let mut run: ReviewRun = serde_json::from_value(inspected.run)?;
        let rules = run
            .documents
            .iter()
            .map(|document| {
                let title = document
                    .blocks
                    .iter()
                    .find(|block| block.tag == "h1")
                    .ok_or("fixture needs a title")?
                    .text
                    .clone();
                let lines = |class: &str| {
                    document
                        .blocks
                        .iter()
                        .filter(|block| {
                            block.classes.split_whitespace().any(|value| value == class)
                        })
                        .map(|block| block.text.clone())
                        .collect()
                };
                Ok((
                    title.clone(),
                    vec![ExtractedRecipe {
                        meta: RecipeMeta {
                            title,
                            ..Default::default()
                        },
                        sections: vec![RecipeSection::new(
                            lines("ingredient"),
                            lines("instruction"),
                        )],
                    }],
                ))
            })
            .collect::<Result<Vec<_>>>()?;
        let extractor = MockExtractor::new(rules);
        let mut progress = Vec::new();
        tokio::runtime::Builder::new_current_thread()
            .build()?
            .block_on(recipe_epub::review::extract_run_with_extractor(
                &mut run,
                &self.0.join("extraction-cache"),
                &self.0.join("run.json"),
                &extractor,
                |run| progress.push(run.chunks.iter().filter(|c| c.output.is_some()).count()),
            ))?;
        assert_eq!(progress.first(), Some(&0));
        assert_eq!(progress.last(), Some(&run.chunks.len()));
        assert!(run.chunks.iter().all(|chunk| chunk.cached));
        Ok(run)
    }
    fn request(&self, model: String) -> backend::ExtractionRequest {
        backend::ExtractionRequest {
            book: self.path("book.epub"),
            out: self.path("run.json"),
            model,
            resume: true,
            from: None,
            allow_network: false,
            refresh: false,
            cache_dir: Some(self.path("empty-cache")),
            chunks: Vec::new(),
            budget_usd: 0.0,
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn complete_mock_run_survives_application_review_and_offline_replay() -> Result {
    let fixture = Fixture::new()?;
    let original = fixture.complete_run()?;
    let path = fixture.path("run.json");
    let before = std::fs::read(&path)?;
    let opened = backend::open_run(path.clone())?;
    assert_eq!(opened.documents.len(), 3);
    assert_eq!(opened.recipes.len(), 3);
    assert!(!opened.incomplete);
    assert_eq!(opened.run["parsed"].as_array().map(Vec::len), Some(3));
    assert_eq!(
        opened
            .recipes
            .iter()
            .map(|recipe| recipe
                .sections
                .iter()
                .map(|s| s.ingredients.len())
                .sum::<usize>())
            .collect::<Vec<_>>(),
        vec![12, 3, 3]
    );
    for recipe in &opened.recipes {
        assert!(
            opened
                .documents
                .iter()
                .any(|document| document.path == recipe.source_document)
        );
    }
    for (document, status) in original
        .documents
        .iter()
        .zip(["Accepted", "Incorrect", "Uncertain"])
    {
        backend::save_review(
            path.clone(),
            document.path.clone(),
            status.into(),
            format!("Checked {}", document.path),
        )?;
    }
    let reviewed = backend::open_run(path.clone())?;
    assert_eq!(reviewed.review.len(), 3);
    for decision in &reviewed.review {
        assert_eq!(decision.note, format!("Checked {}", decision.document));
    }
    let sidecar = fixture.0.join("run.review.json");
    let sidecar_before = std::fs::read(&sidecar)?;
    let replay_path = fixture.path("replayed.json");
    let replayed = backend::replay_run(path.clone(), replay_path.clone())?;
    assert_eq!(replayed.source_hash, opened.source_hash);
    assert_eq!(replayed.run["documents"], opened.run["documents"]);
    assert_eq!(replayed.run["recipes"], opened.run["recipes"]);
    assert_eq!(replayed.run["parsed"], opened.run["parsed"]);
    assert_eq!(
        replayed.run["parent"],
        std::fs::canonicalize(&path)?.to_string_lossy().as_ref()
    );
    assert_eq!(std::fs::read(&path)?, before);
    assert_eq!(std::fs::read(&sidecar)?, sidecar_before);
    assert_eq!(backend::open_run(replay_path)?.recipes.len(), 3);
    // Presentation-only scaling must not rewrite ingredient source or review data.
    assert!(
        !backend::scale_recipe(path.clone(), 0, 2.0)?
            .sections
            .is_empty()
    );
    assert_eq!(std::fs::read(path)?, before);
    Ok(())
}

#[test]
fn invalid_extraction_requests_preserve_the_complete_checkpoint() -> Result {
    let fixture = Fixture::new()?;
    let run = fixture.complete_run()?;
    let before = std::fs::read(fixture.path("run.json"))?;
    let base = fixture.request(run.model);
    let mut cases = Vec::new();
    for budget in [-1.0, f64::NAN, f64::INFINITY] {
        let mut request = base.clone();
        request.budget_usd = budget;
        cases.push(request);
    }
    let mut wrong_model = base.clone();
    wrong_model.model = "different-model".into();
    cases.push(wrong_model);
    let mut parent_and_resume = base.clone();
    parent_and_resume.from = Some(fixture.path("run.json"));
    cases.push(parent_and_resume);
    let mut refresh_offline = base.clone();
    refresh_offline.refresh = true;
    cases.push(refresh_offline);
    let mut unknown_chunk = base;
    unknown_chunk.chunks = vec!["missing-chunk".into()];
    cases.push(unknown_chunk);
    let runtime = tokio::runtime::Builder::new_current_thread().build()?;
    for request in cases {
        assert!(!request.allow_network);
        let progress = std::cell::Cell::new(0);
        let result = runtime.block_on(backend::extract_run(request, |_| {
            progress.set(progress.get() + 1);
        }));
        assert!(result.is_err());
        assert_eq!(progress.get(), 0);
        assert_eq!(std::fs::read(fixture.path("run.json"))?, before);
    }
    Ok(())
}

#[test]
fn image_loading_checks_epub_identity_even_when_fixture_has_no_images() -> Result {
    let fixture = Fixture::new()?;
    fixture.complete_run()?;
    let path = fixture.path("run.json");
    assert!(backend::load_images(Some(path.clone()), fixture.path("book.epub"))?.is_empty());
    let mut different = std::fs::read(fixture.path("book.epub"))?;
    different.extend_from_slice(b"different source bytes");
    std::fs::write(fixture.path("other.epub"), different)?;
    let error = backend::load_images(Some(path), fixture.path("other.epub"))
        .err()
        .ok_or("different EPUB was accepted")?;
    assert!(error.contains("different EPUB"), "{error}");
    Ok(())
}

#[test]
fn cache_only_progress_reports_initial_and_final_checkpoint_for_hits_and_misses() -> Result {
    let fixture = Fixture::new()?;
    let completed = fixture.complete_run()?;
    let bytes = std::fs::read(fixture.path("book.epub"))?;
    let runtime = tokio::runtime::Builder::new_current_thread().build()?;
    let mut cached = ReviewRun::inspect(&bytes, &completed.source, &completed.model)?;
    runtime.block_on(recipe_epub::review::extract_run_with_extractor(
        &mut cached,
        &fixture.0.join("extraction-cache"),
        &fixture.0.join("cached-run.json"),
        &MockExtractor::new(Vec::new()),
        |_| {},
    ))?;
    assert_eq!(
        cached.recipes, completed.recipes,
        "existing cache must not be replaced by new mock responses"
    );
    for (cache, should_complete) in [("extraction-cache", true), ("missing-cache", false)] {
        let mut run = ReviewRun::inspect(&bytes, &completed.source, &completed.model)?;
        let checkpoint = fixture.0.join(format!("{cache}-run.json"));
        let mut snapshots = Vec::new();
        runtime.block_on(recipe_epub::review::extract_run_with_progress(
            &mut run,
            &recipe_epub::review::RunOptions {
                cache_dir: Some(fixture.0.join(cache)),
                ..Default::default()
            },
            &checkpoint,
            |run| {
                snapshots.push((
                    run.chunks
                        .iter()
                        .filter(|chunk| chunk.output.is_some())
                        .count(),
                    run.chunks
                        .iter()
                        .filter(|chunk| chunk.error.is_some())
                        .count(),
                ))
            },
        ))?;
        assert_eq!(snapshots.first(), Some(&(0, 0)));
        assert!(snapshots.len() >= 2);
        let expected = if should_complete {
            (run.chunks.len(), 0)
        } else {
            (0, run.chunks.len())
        };
        assert_eq!(snapshots.last(), Some(&expected));
        assert_eq!(!ReviewRun::read(&checkpoint)?.incomplete(), should_complete);
    }
    Ok(())
}
