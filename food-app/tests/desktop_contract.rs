//! Durable application-command contracts exercised without a webview or a
//! model: the fixture oracle answers every chunk, and the run store lives in
//! a temporary directory.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use cookbook::cache::NoCache;
use cookbook::test_support::{Oracle, epub3_roles, oracle_transport};
use cookbook::{Book, CancelToken};
use food_app::backend;
use std::path::PathBuf;

type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;

struct Fixture {
    dir: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Result<Self> {
        let dir = tempfile::tempdir()?;
        std::fs::write(
            dir.path().join("book.epub"),
            cookbook_fixtures::epub3_nav_pagebreaks()?,
        )?;
        Ok(Self { dir })
    }
    fn book(&self) -> String {
        self.dir
            .path()
            .join("book.epub")
            .to_string_lossy()
            .into_owned()
    }
    fn runs_dir(&self) -> PathBuf {
        self.dir.path().join("runs")
    }
    fn oracle(&self) -> Result<cookbook::test_support::ScriptedTransport> {
        let bytes = std::fs::read(self.book())?;
        let opened = Book::open(bytes, "fixture")?;
        Ok(oracle_transport(
            Oracle::new(epub3_roles()),
            opened.lines().clone(),
            opened.chunks().to_vec(),
        ))
    }
}

/// The run store is process-global; one test owns it.
#[test]
fn extract_saves_a_run_that_lists_opens_and_deletes() -> Result {
    let fixture = Fixture::new()?;
    // SAFETY: this is the only test in the binary that touches the process
    // environment, and it does so before any other thread reads it.
    unsafe { std::env::set_var(cookbook::native::runs::RUNS_DIR_VAR, fixture.runs_dir()) };

    let open_book = |path: String| -> Result<serde_json::Value> {
        Ok(backend::core(
            "open_book",
            serde_json::json!({ "path": path }),
        )?)
    };
    let opened = open_book(fixture.book())?;
    let chunks = opened["outline"]["chunks"].as_u64().ok_or("no chunks")? as usize;
    assert!(chunks >= 1);
    assert_eq!(opened["runs"], serde_json::json!([]));
    assert!(backend::list_runs()?.is_empty());

    let transport = fixture.oracle()?;
    let runtime = tokio::runtime::Builder::new_current_thread().build()?;
    let mut progress = Vec::new();
    let summary = runtime.block_on(backend::extract_book_with(
        fixture.book(),
        &transport,
        &NoCache,
        CancelToken::new(),
        |p| progress.push(p.done),
    ))?;
    assert_eq!(progress.last(), Some(&chunks));
    assert!(!summary.incomplete);
    assert!(summary.recipes >= 3, "{summary:?}");
    assert!(
        summary
            .path
            .starts_with(&fixture.runs_dir().to_string_lossy().into_owned())
    );

    let listed = backend::list_runs()?;
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].run_id, summary.run_id);
    assert_eq!(
        open_book(fixture.book())?["runs"].as_array().map(Vec::len),
        Some(1)
    );

    let opened_run = backend::core("open_run", serde_json::json!({ "path": summary.path }))?;
    assert_eq!(opened_run["summary"]["run_id"], summary.run_id);
    let extraction: cookbook::Extraction =
        serde_json::from_value(opened_run["extraction"].clone())?;
    assert_eq!(extraction.report.run_id, summary.run_id);
    assert_eq!(extraction.cookbook.recipes().count(), summary.recipes);
    assert_eq!(extraction.report.calls.len(), transport.calls());
    let recipe = extraction.cookbook.recipes().next().unwrap();
    assert!(!recipe.sections.is_empty());
    if let Some(photo) = recipe.photos.first() {
        let image = backend::core(
            "book_image",
            serde_json::json!({ "book": fixture.book(), "image": photo.path }),
        )?;
        assert!(
            image["dataUrl"]
                .as_str()
                .is_some_and(|u| u.starts_with("data:image/"))
        );
    }

    // A cancelled run is discarded, never saved.
    let cancelled = CancelToken::new();
    cancelled.cancel();
    let outcome = runtime.block_on(backend::extract_book_with(
        fixture.book(),
        &transport,
        &NoCache,
        cancelled,
        |_| {},
    ));
    assert!(outcome.is_err());
    assert_eq!(backend::list_runs()?.len(), 1);

    // Only listed runs can be deleted.
    let stray = fixture.dir.path().join("stray.json");
    std::fs::write(&stray, "{}")?;
    assert!(backend::delete_run(stray.to_string_lossy().into_owned()).is_err());
    assert!(stray.exists());
    backend::delete_run(summary.path.clone())?;
    assert!(backend::list_runs()?.is_empty());
    Ok(())
}
