//! Native application services. Portable commands (parsing, recipes, corpus,
//! opened books and runs) are `food-core`'s, answered from the filesystem by
//! [`FsHost`]; this module adds what only a desktop can do: the Calibre
//! library, the run store, model backends, extraction, and bundle export.
//! Loading and rendering never start extraction; a run is one explicit call
//! that ends in the shared run store.
pub use cookbook::RunSummary;
use cookbook::cache::ChunkCache;
use cookbook::classify::subject_hint;
use cookbook::epub::open::Package;
use cookbook::library::ShaCache;
use cookbook::native::runs;
use cookbook::native::{FsChunkCache, ReqwestTransport};
use cookbook::{Book, CancelToken, Estimate, ExtractOptions, Progress, Transport};
use food_core::Host;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};
use ts_rs::TS;

type AppResult<T> = Result<T, String>;

/// Files are paths on disk; runs come from the shared run store.
pub struct FsHost;

impl Host for FsHost {
    fn read(&self, path: &str) -> AppResult<Vec<u8>> {
        std::fs::read(path).map_err(|e| format!("Cannot read {path}: {e}"))
    }
    fn stamp(&self, path: &str) -> AppResult<String> {
        let metadata =
            std::fs::metadata(path).map_err(|e| format!("Cannot inspect {path}: {e}"))?;
        let modified = metadata
            .modified()
            .ok()
            .and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_nanos())
            .unwrap_or_default();
        Ok(format!("{}:{modified}", metadata.len()))
    }
    fn runs_for(&self, sha256: &str) -> AppResult<Vec<RunSummary>> {
        runs::for_sha(sha256).map_err(|e| e.to_string())
    }
}

/// Run a portable command against the filesystem.
pub fn core(command: &str, args: Value) -> AppResult<Value> {
    food_core::dispatch(&FsHost, command, args)
}

/// Fetch a recipe page the way the CLI does (browser-like user agent, cache).
pub async fn fetch_html(url: String) -> AppResult<String> {
    recipe_scraper_fetcher::Fetcher::new()
        .fetch_html(url.trim())
        .await
        .map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// Cookbooks

/// One EPUB found in the library directory. Metadata only: opening the text
/// waits for `open_book`.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct LibraryBook {
    pub path: String,
    pub title: String,
    pub authors: Vec<String>,
    pub subjects: Vec<String>,
    /// The catalog subjects mention cooking. The structural verdict comes
    /// with `open_book`.
    pub cookbook_hint: bool,
    /// Saved runs of this exact file (matched by its sha256), newest first.
    pub runs: Vec<RunSummary>,
    pub error: Option<String>,
}

/// Whether the gateway credentials are available, and where they came from.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct GatewayStatus {
    pub configured: bool,
    pub base_url: Option<String>,
    /// The file the app reads when launched outside a shell.
    pub config_path: Option<String>,
    pub cache_dir: Option<String>,
    pub runs_dir: Option<String>,
    pub ladder: Vec<String>,
    pub error: Option<String>,
}

fn default_library() -> AppResult<PathBuf> {
    Ok(std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or("Home directory is unavailable")?
        .join("Library/Mobile Documents/com~apple~CloudDocs/Calibre"))
}

pub fn scan_library(directory: String) -> AppResult<Vec<LibraryBook>> {
    let directory = if directory.trim().is_empty() {
        default_library()?
    } else {
        PathBuf::from(directory)
    };
    if !directory.is_dir() {
        return Err(format!("{} is not a directory", directory.display()));
    }
    let history = runs::list().map_err(|e| e.to_string())?;
    // Hashing 191 files reads them once; the cache keeps later scans to
    // metadata reads.
    let mut shas = ShaCache::open_default();
    let mut books: Vec<LibraryBook> = cookbook::library::find_epubs(&directory)
        .iter()
        .map(|path| {
            let display = path.to_string_lossy().into_owned();
            let sha = shas.sha_for(path).ok();
            match Package::parse_file(path) {
                Ok(package) => LibraryBook {
                    runs: history
                        .iter()
                        .filter(|r| sha.as_deref() == Some(r.sha256.as_str()))
                        .cloned()
                        .collect(),
                    cookbook_hint: subject_hint(&package.subjects),
                    path: display,
                    title: if package.title.is_empty() {
                        file_stem(path)
                    } else {
                        package.title
                    },
                    authors: package.authors,
                    subjects: package.subjects,
                    error: None,
                },
                Err(error) => LibraryBook {
                    runs: vec![],
                    cookbook_hint: false,
                    path: display,
                    title: file_stem(path),
                    authors: vec![],
                    subjects: vec![],
                    error: Some(error.to_string()),
                },
            }
        })
        .collect();
    books.sort_by(|a, b| {
        (!a.cookbook_hint, a.title.to_lowercase()).cmp(&(!b.cookbook_hint, b.title.to_lowercase()))
    });
    // A cache that fails to persist only costs the next scan its hashing.
    let _ = shas.save_default();
    Ok(books)
}

fn file_stem(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn chunk_cache() -> AppResult<FsChunkCache> {
    FsChunkCache::open_default().map_err(|e| e.to_string())
}

pub fn gateway_status() -> GatewayStatus {
    let (configured, base_url, error) = match ReqwestTransport::from_env() {
        Ok(t) => (true, Some(t.base().to_string()), None),
        Err(e) => (false, None, Some(e.to_string())),
    };
    GatewayStatus {
        configured,
        base_url,
        config_path: cookbook::native::gateway_config_path()
            .map(|p| p.to_string_lossy().into_owned()),
        cache_dir: FsChunkCache::default_dir().map(|p| p.to_string_lossy().into_owned()),
        runs_dir: runs::root().map(|p| p.to_string_lossy().into_owned()),
        ladder: cookbook::models::DEFAULT_LADDER
            .iter()
            .map(|s| s.to_string())
            .collect(),
        error,
    }
}

/// Extract with any transport and cache (tests use the fixture oracle).
/// A cancelled run is discarded, not saved.
pub async fn extract_book_with(
    path: String,
    transport: &(impl Transport + Sync),
    cache: &(impl ChunkCache + Sync),
    cancel: CancelToken,
    progress: impl FnMut(Progress) + Send,
) -> AppResult<RunSummary> {
    let (book, _) = food_core::books::book(&FsHost, &path)?;
    let options = ExtractOptions {
        label: book.source().label.clone(),
        ..ExtractOptions::default()
    };
    let extraction = book
        .extract(&options, transport, cache, &cancel, progress)
        .await
        .map_err(|e| match e {
            cookbook::Error::Cancelled(_) => "Extraction cancelled".to_string(),
            other => other.to_string(),
        })?;
    let saved = runs::save(&extraction, None).map_err(|e| e.to_string())?;
    Ok(runs::summarize(&saved, &extraction))
}

pub fn list_runs() -> AppResult<Vec<RunSummary>> {
    runs::list().map_err(|e| e.to_string())
}

pub fn export_bundle(run: String, book: String) -> AppResult<cookbook::bundle::BundleExport> {
    cookbook::bundle::export(Path::new(&run), Path::new(&book), None)
        .map_err(|error| error.to_string())
}

/// Delete a saved run. Only files the run store lists can be deleted.
pub fn delete_run(path: String) -> AppResult<()> {
    let listed = list_runs()?.into_iter().any(|r| r.path == path);
    if !listed {
        return Err("Only saved runs from the run store can be deleted".into());
    }
    std::fs::remove_file(&path).map_err(|e| format!("Cannot delete {path}: {e}"))
}

/// Model readiness without creating a model session.
pub async fn backend_statuses() -> Vec<cookbook::harness::BackendStatus> {
    cookbook::harness::statuses().await
}

fn configured_book(
    path: &str,
    config: &cookbook::harness::BackendOptions,
    options: &mut ExtractOptions,
) -> AppResult<Book> {
    let mut book = Book::open(
        std::fs::read(path).map_err(|e| format!("Cannot read {path}: {e}"))?,
        file_stem(Path::new(path)),
    )
    .map_err(|e| e.to_string())?;
    if config.use_catalog {
        cookbook::catalog::apply(&mut book)?;
    }
    options.label = book.source().label.clone();
    config.apply(options)?;
    Ok(book)
}

pub fn estimate_book(
    path: String,
    config: cookbook::harness::BackendOptions,
) -> AppResult<Estimate> {
    let mut options = ExtractOptions::default();
    let book = configured_book(&path, &config, &mut options)?;
    let mut estimate = book
        .estimate(&options, &chunk_cache()?)
        .map_err(|e| e.to_string())?;
    if options.ladder.iter().any(|m| m.contains("-cli/")) {
        estimate.assumptions.push("Subscription usage; dollar estimates do not represent billed cost. Uses the same allowance as your other CLI sessions.".into());
    }
    Ok(estimate)
}

pub async fn extract_book(
    path: String,
    config: cookbook::harness::BackendOptions,
    cancel: CancelToken,
    progress: impl FnMut(Progress) + Send,
) -> AppResult<RunSummary> {
    let mut options = ExtractOptions::default();
    let book = configured_book(&path, &config, &mut options)?;
    let executor = cookbook::harness::NativeExecutor::new(&options.ladder).await?;
    let extraction = book
        .extract_with_executor(&options, &executor, &chunk_cache()?, &cancel, progress)
        .await
        .map_err(|e| e.to_string())?;
    let saved = runs::save(&extraction, None).map_err(|e| e.to_string())?;
    Ok(runs::summarize(&saved, &extraction))
}

pub fn catalog_status(path: String) -> AppResult<Option<cookbook::catalog::Catalog>> {
    let (book, _) = food_core::books::book(&FsHost, &path)?;
    cookbook::catalog::latest(&book)
}

pub async fn catalog_books(
    paths: Vec<String>,
    options: cookbook::catalog::CatalogOptions,
    cancel: CancelToken,
    mut progress: impl FnMut(cookbook::catalog::CatalogProgress) + Send,
) -> AppResult<Vec<cookbook::catalog::Catalog>> {
    let mut ids = vec![options.reader.clone()];
    ids.extend(options.auditor.clone());
    let executor = cookbook::harness::NativeExecutor::new(&ids).await?;
    let cache = chunk_cache()?;
    let mut seen = std::collections::HashSet::new();
    let mut results = Vec::new();
    for path in paths {
        if cancel.is_cancelled() {
            return Err(cancel
                .failure()
                .unwrap_or_else(|| "Catalog cancelled".into()));
        }
        let (book, _) = food_core::books::book(&FsHost, &path)?;
        if seen.insert(book.source().sha256.clone()) {
            results.push(
                cookbook::catalog::build(
                    &book,
                    &options,
                    &executor,
                    &cache,
                    &cancel,
                    &mut progress,
                )
                .await?,
            );
        }
    }
    Ok(results)
}

// ---------------------------------------------------------------------------
// The frontend contract

/// The portable declarations plus every native-only type the desktop commands
/// carry: one file, so the frontend has a single type source.
fn bindings() -> Vec<food_core::bindings::Declaration> {
    use food_core::bindings::entry;
    let cfg = food_core::bindings::config();
    let mut all = food_core::bindings::declarations(&cfg);
    all.extend([
        entry::<LibraryBook>(&cfg),
        entry::<GatewayStatus>(&cfg),
        entry::<cookbook::harness::BackendOptions>(&cfg),
        entry::<cookbook::harness::BackendStatus>(&cfg),
        entry::<cookbook::catalog::Catalog>(&cfg),
        entry::<cookbook::catalog::CatalogOptions>(&cfg),
        entry::<cookbook::catalog::CatalogProgress>(&cfg),
        entry::<cookbook::catalog::WindowRecord>(&cfg),
        entry::<cookbook::catalog::WindowMap>(&cfg),
        entry::<cookbook::catalog::Entry>(&cfg),
        entry::<cookbook::contract::Kind>(&cfg),
        entry::<cookbook::bundle::BundleExport>(&cfg),
        entry::<cookbook::bundle::BundleManifest>(&cfg),
        entry::<cookbook::bundle::BundleImage>(&cfg),
    ]);
    all
}

pub fn bindings_source() -> String {
    food_core::bindings::source(
        "// Generated from Rust (food-core + food-app). Do not edit.\n// Regenerate: cargo run -p food-app --example export_bindings",
        bindings(),
    )
}

/// The checked-in contract the frontend imports.
pub fn bindings_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../ui/src/api/generated.ts")
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    /// Every type a declaration references is itself declared.
    #[test]
    fn frontend_bindings_are_closed_over_their_dependencies() {
        assert_eq!(food_core::bindings::undeclared(&bindings()), None);
        let source = bindings_source();
        assert!(!source.contains("bigint"), "{source}");
        assert!(source.contains("export type LibraryBook = "));
    }

    #[test]
    fn library_scan_reads_metadata_without_opening_text() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("book.epub"),
            cookbook_fixtures::epub3_nav_pagebreaks().unwrap(),
        )
        .unwrap();
        std::fs::write(dir.path().join("broken.epub"), b"not a zip").unwrap();
        let books = scan_library(dir.path().to_string_lossy().into_owned()).unwrap();
        assert_eq!(books.len(), 2);
        assert!(books[0].error.is_none(), "{books:?}");
        assert!(!books[0].title.is_empty());
        assert_eq!(books[1].title, "broken");
        assert!(books[1].error.is_some());
    }

    #[test]
    fn portable_commands_read_the_filesystem() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("book.epub");
        std::fs::write(&path, cookbook_fixtures::epub3_nav_pagebreaks().unwrap()).unwrap();
        let path = path.to_string_lossy().into_owned();
        let opened = core("open_book", serde_json::json!({ "path": path })).unwrap();
        assert!(opened["outline"]["chunks"].as_u64().unwrap() >= 1);
        let estimate = estimate_book(path, Default::default()).unwrap();
        assert!(estimate.cost_usd_high >= estimate.cost_usd_low);
    }
}
