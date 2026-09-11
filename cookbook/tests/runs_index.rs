//! The run store's index: saved runs are listed without parsing every file,
//! vanished files drop out, unknown files are picked up, and a broken index
//! is rebuilt.

#![cfg(not(target_arch = "wasm32"))]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use cookbook::cache::NoCache;
use cookbook::native::runs;
use cookbook::test_support::{Oracle, epub3_roles, oracle_transport};
use cookbook::{Book, CancelToken, ExtractOptions, Extraction};

async fn extraction(label: &str) -> Extraction {
    let bytes = cookbook_fixtures::epub3_nav_pagebreaks().unwrap();
    let book = Book::open(bytes, label).unwrap();
    let transport = oracle_transport(
        Oracle::new(epub3_roles()),
        book.lines().clone(),
        book.chunks().to_vec(),
    );
    let options = ExtractOptions {
        label: label.into(),
        concurrency: 4,
        ladder: vec!["gemini-2.5-flash".into(), "claude-haiku-4-5".into()],
        ..ExtractOptions::default()
    };
    book.extract(&options, &transport, &NoCache, &CancelToken::new(), |_| {})
        .await
        .unwrap()
}

#[tokio::test]
async fn index_follows_the_directory() {
    let dir = tempfile::tempdir().unwrap();
    // SAFETY: nextest runs each test in its own process; nothing else reads
    // the variable concurrently.
    unsafe { std::env::set_var(runs::RUNS_DIR_VAR, dir.path()) };
    let first = extraction("first").await;
    let mut second = extraction("second").await;
    second.report.run_id = format!("{}-b", second.report.run_id);
    second.report.started_at = "2030-01-01T00:00:00Z".into();
    let first_path = runs::save(&first, None).unwrap();
    let second_path = runs::save(&second, None).unwrap();

    let index_path = dir.path().join("index.json");
    assert!(index_path.exists(), "save writes the index");
    let listed = runs::list().unwrap();
    assert_eq!(listed.len(), 2);
    assert_eq!(
        listed[0].path,
        second_path.to_string_lossy(),
        "newest first"
    );
    let sha = &first.cookbook.source.sha256;
    assert_eq!(
        runs::for_sha(sha).unwrap().len(),
        2,
        "same fixture, same sha"
    );
    assert_eq!(
        runs::latest_for_sha(sha).unwrap().unwrap().run_id,
        second.report.run_id
    );
    assert!(runs::for_sha("nope").unwrap().is_empty());

    // A file deleted behind the index's back drops out of the listing.
    std::fs::remove_file(&second_path).unwrap();
    let listed = runs::list().unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].path, first_path.to_string_lossy());

    // A file copied in behind the index's back is picked up.
    let copied = dir.path().join("copied--x.json");
    std::fs::copy(&first_path, &copied).unwrap();
    assert_eq!(runs::list().unwrap().len(), 2);

    // A corrupt index is rebuilt from the files.
    std::fs::write(&index_path, "{not json").unwrap();
    assert_eq!(runs::list().unwrap().len(), 2);
    let rebuilt = std::fs::read_to_string(&index_path).unwrap();
    assert!(rebuilt.contains("\"version\":1"));
}
