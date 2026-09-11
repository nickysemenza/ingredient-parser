//! Application services for the native desktop shell: ingredient parsing,
//! corpus scoring, web recipes, and cookbook extraction over the `cookbook`
//! crate. Loading and rendering never start extraction; a run is one explicit
//! call that ends in the shared run store.
use base64::Engine;
use cookbook::cache::ChunkCache;
use cookbook::classify::{Classified, classify_structure, subject_hint};
use cookbook::epub::open::Package;
use cookbook::native::runs;
pub use cookbook::native::runs::RunSummary;
use cookbook::native::{FsChunkCache, ReqwestTransport};
use cookbook::{
    Book, BookOutline, CancelToken, Estimate, ExtractOptions, Extraction, Progress, Transport,
};
use ingredient::{IngredientParser, ParseOptions, TraceDetail};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock},
};
use ts_rs::TS;

type AppResult<T> = Result<T, String>;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct IngredientResult {
    pub line_number: usize,
    pub input: String,
    pub name: String,
    pub amounts: Vec<String>,
    pub modifier: Option<String>,
    pub optional: bool,
    pub usage: String,
    pub confidence: String,
    pub review_reasons: Vec<String>,
    pub json: Value,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct TraceNode {
    pub name: String,
    pub input: String,
    pub outcome: String,
    pub detail: String,
    pub children: Vec<TraceNode>,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct IngredientInspection {
    pub result: IngredientResult,
    pub stages: String,
    pub trace: Option<TraceNode>,
    pub trace_text: String,
    pub jaeger_json: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RecipeResult {
    pub title: String,
    pub url: String,
    pub ingredients: Vec<IngredientResult>,
    pub sections: Vec<WebSection>,
    pub source: Value,
    pub parsed: Value,
    pub diagnostics: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CorpusField {
    pub field: String,
    pub matches: bool,
    pub expected: String,
    pub actual: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CorpusCase {
    pub line_number: usize,
    pub section: String,
    pub input: String,
    pub status: String,
    pub reason: Option<String>,
    pub fields: Vec<CorpusField>,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CorpusResult {
    pub path: Option<String>,
    pub cases: Vec<CorpusCase>,
}
/// A scraped web recipe's section, rendered for display.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct WebSection {
    pub name: Option<String>,
    pub ingredients: Vec<String>,
    pub instructions: Vec<String>,
}

fn ingredient_result(
    input: &str,
    line_number: usize,
    parsed: &ingredient::ingredient::Ingredient,
) -> IngredientResult {
    let mut review_reasons: Vec<_> = parsed
        .parse_notes
        .review_reasons()
        .iter()
        .map(ToString::to_string)
        .collect();
    if input.trim().is_empty() {
        review_reasons.push("Empty ingredient line".into());
    }
    IngredientResult {
        line_number,
        input: input.into(),
        name: parsed.name.clone(),
        amounts: parsed.amounts.iter().map(ToString::to_string).collect(),
        modifier: parsed.modifier.clone(),
        optional: parsed.optional,
        usage: serde_json::to_value(parsed.usage)
            .ok()
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_else(|| format!("{:?}", parsed.usage)),
        confidence: format!("{:?}", parsed.parse_notes.confidence),
        review_reasons,
        json: serde_json::to_value(parsed).unwrap_or(Value::Null),
    }
}

pub fn parse_batch(input: String) -> AppResult<Vec<IngredientResult>> {
    let parser = IngredientParser::new();
    Ok(input
        .lines()
        .enumerate()
        .map(|(index, line)| ingredient_result(line, index + 1, &parser.from_str(line)))
        .collect())
}

pub fn inspect_ingredient(input: String) -> AppResult<IngredientInspection> {
    let execution = IngredientParser::new().parse_line(
        &input,
        ParseOptions {
            decomposition: false,
            trace: TraceDetail::Full,
        },
    );
    fn node(source: &ingredient::trace::TraceNode) -> TraceNode {
        use ingredient::trace::TraceOutcome;
        let (outcome, detail) = match &source.outcome {
            TraceOutcome::Success {
                consumed,
                output_preview,
            } => (
                "success",
                format!("{consumed} characters · {output_preview}"),
            ),
            TraceOutcome::Failure { error } => ("failure", error.clone()),
            TraceOutcome::Incomplete => ("incomplete", String::new()),
        };
        TraceNode {
            name: source.name.clone(),
            input: source.input.clone(),
            outcome: outcome.into(),
            detail,
            children: source.children.iter().map(node).collect(),
        }
    }
    Ok(IngredientInspection {
        result: ingredient_result(&input, 1, &execution.ingredient),
        stages: execution
            .stages
            .as_ref()
            .map(|s| s.format(false))
            .unwrap_or_default(),
        trace: execution.trace.as_ref().map(|t| node(&t.root)),
        trace_text: execution
            .trace
            .as_ref()
            .map(|t| t.format_tree(false))
            .unwrap_or_default(),
        jaeger_json: execution
            .trace
            .as_ref()
            .map(|t| t.to_jaeger_json())
            .unwrap_or_default(),
    })
}

pub async fn load_recipe(url: String) -> AppResult<RecipeResult> {
    let recipe = recipe_scraper_fetcher::Fetcher::new()
        .scrape_url(url.trim())
        .await
        .map_err(|e| e.to_string())?;
    recipe_result(recipe)
}
fn recipe_result(recipe: recipe_scraper::ScrapedRecipe) -> AppResult<RecipeResult> {
    let parser = IngredientParser::new();
    let execution =
        recipe_parsing::execute_sections(&recipe.sections, &parser, ParseOptions::default());
    let ingredients = recipe
        .ingredients()
        .enumerate()
        .map(|(index, line)| ingredient_result(line, index + 1, &parser.from_str(line)))
        .collect();
    Ok(RecipeResult {
        title: recipe.name.clone(),
        url: recipe.url.clone(),
        ingredients,
        sections: recipe
            .sections
            .iter()
            .map(|section| WebSection {
                name: section.name.clone(),
                ingredients: section.ingredients.clone(),
                instructions: section.instructions.clone(),
            })
            .collect(),
        source: serde_json::to_value(&recipe).map_err(|e| e.to_string())?,
        parsed: serde_json::to_value(&execution.recipe).map_err(|e| e.to_string())?,
        diagnostics: execution
            .instruction_diagnostics
            .iter()
            .map(|d| {
                format!(
                    "Section {}, instruction {}: {}",
                    d.section + 1,
                    d.instruction + 1,
                    d.message
                )
            })
            .collect(),
    })
}

pub fn load_corpus(path: Option<String>) -> AppResult<CorpusResult> {
    let source = match &path {
        Some(path) => {
            std::fs::read_to_string(path).map_err(|e| format!("Cannot read {path}: {e}"))?
        }
        None => ingredient_corpus::embedded().to_owned(),
    };
    let corpus = ingredient_corpus::parse(&source);
    let cases = corpus
        .entries
        .iter()
        .map(|entry| {
            let section = corpus
                .sections
                .get(entry.section)
                .cloned()
                .unwrap_or_default();
            match &entry.parsed {
                Ok(row) => {
                    let score = ingredient_corpus::score(row);
                    CorpusCase {
                        line_number: entry.line_no,
                        section,
                        input: row.input.clone(),
                        status: score.status.label().into(),
                        reason: row.xfail.clone(),
                        fields: score
                            .fields
                            .iter()
                            .map(|d| CorpusField {
                                field: d.field.as_str().into(),
                                matches: d.ok,
                                expected: d.want.clone(),
                                actual: d.got.clone(),
                            })
                            .collect(),
                    }
                }
                Err(error) => CorpusCase {
                    line_number: entry.line_no,
                    section,
                    input: error.line.clone(),
                    status: "INVALID".into(),
                    reason: Some(error.message.clone()),
                    fields: Vec::new(),
                },
            }
        })
        .collect();
    Ok(CorpusResult { path, cases })
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
    /// Saved runs of this exact file, newest first (matched by file name until
    /// the book is opened and hashed).
    pub runs: Vec<RunSummary>,
    pub error: Option<String>,
}

/// What `open_book` returns: the outline plus the offline cookbook verdict.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct OpenedBook {
    pub path: String,
    pub outline: BookOutline,
    pub classified: Classified,
    /// Runs of this exact file (by content hash), newest first.
    pub runs: Vec<RunSummary>,
    pub open_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct BookImage {
    pub path: String,
    pub data_url: String,
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
    let mut books: Vec<LibraryBook> = cookbook::library::find_epubs(&directory)
        .iter()
        .map(|path| {
            let display = path.to_string_lossy().into_owned();
            match Package::parse_file(path) {
                Ok(package) => LibraryBook {
                    runs: history
                        .iter()
                        .filter(|r| r.book == package.title)
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
    Ok(books)
}

fn file_stem(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Opened books, keyed by canonical path. A book holds its whole archive, so
/// only the two most recent stay resident.
struct OpenBooks(Vec<(PathBuf, BookIdentity, Arc<Book>)>);

#[derive(Debug, Clone, PartialEq, Eq)]
struct BookIdentity {
    length: u64,
    modified_nanos: u128,
}

static BOOKS: OnceLock<Mutex<OpenBooks>> = OnceLock::new();
const RESIDENT_BOOKS: usize = 2;

fn identity(path: &Path) -> AppResult<BookIdentity> {
    let metadata =
        std::fs::metadata(path).map_err(|e| format!("Cannot inspect {}: {e}", path.display()))?;
    Ok(BookIdentity {
        length: metadata.len(),
        modified_nanos: metadata
            .modified()
            .ok()
            .and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_nanos())
            .unwrap_or_default(),
    })
}

/// The opened book for `path`, reusing a resident handle when the file is
/// unchanged. Returns the open time in milliseconds (0 when reused).
fn book(path: &str) -> AppResult<(Arc<Book>, u64)> {
    let canonical = Path::new(path)
        .canonicalize()
        .map_err(|e| format!("Cannot read {path}: {e}"))?;
    let identity = identity(&canonical)?;
    let cache = BOOKS.get_or_init(|| Mutex::new(OpenBooks(Vec::new())));
    {
        let books = cache.lock().map_err(|_| "Book cache is unavailable")?;
        if let Some((_, _, book)) = books
            .0
            .iter()
            .find(|(p, id, _)| *p == canonical && *id == identity)
        {
            return Ok((book.clone(), 0));
        }
    }
    let started = std::time::Instant::now();
    let bytes = std::fs::read(&canonical).map_err(|e| format!("Cannot read {path}: {e}"))?;
    let label = file_stem(&canonical);
    let book = Arc::new(Book::open(bytes, label).map_err(|e| e.to_string())?);
    let open_ms = started.elapsed().as_millis() as u64;
    let mut books = cache.lock().map_err(|_| "Book cache is unavailable")?;
    books.0.retain(|(p, _, _)| *p != canonical);
    if books.0.len() >= RESIDENT_BOOKS {
        books.0.remove(0);
    }
    books.0.push((canonical, identity, book.clone()));
    Ok((book, open_ms))
}

fn runs_of(sha256: &str) -> AppResult<Vec<RunSummary>> {
    Ok(runs::list()
        .map_err(|e| e.to_string())?
        .into_iter()
        .filter(|r| r.sha256 == sha256)
        .collect())
}

pub fn open_book(path: String) -> AppResult<OpenedBook> {
    let (book, open_ms) = book(&path)?;
    Ok(OpenedBook {
        runs: runs_of(&book.source().sha256)?,
        outline: book.outline(),
        classified: classify_structure(&book),
        path,
        open_ms,
    })
}

fn chunk_cache() -> AppResult<FsChunkCache> {
    FsChunkCache::open_default().map_err(|e| e.to_string())
}

pub fn estimate_book(path: String) -> AppResult<Estimate> {
    let (book, _) = book(&path)?;
    book.estimate(&ExtractOptions::default(), &chunk_cache()?)
        .map_err(|e| e.to_string())
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

/// Extract with the live gateway and the shared file cache, then save the run.
pub async fn extract_book(
    path: String,
    cancel: CancelToken,
    progress: impl FnMut(Progress) + Send,
) -> AppResult<RunSummary> {
    let transport = ReqwestTransport::from_env().map_err(|e| e.to_string())?;
    let cache = chunk_cache()?;
    extract_book_with(path, &transport, &cache, cancel, progress).await
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
    let (book, _) = book(&path)?;
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

pub fn open_run(path: String) -> AppResult<Extraction> {
    runs::load(Path::new(&path)).map_err(|e| format!("Cannot open run {path}: {e}"))
}

/// Delete a saved run. Only files the run store lists can be deleted.
pub fn delete_run(path: String) -> AppResult<()> {
    let listed = list_runs()?.into_iter().any(|r| r.path == path);
    if !listed {
        return Err("Only saved runs from the run store can be deleted".into());
    }
    std::fs::remove_file(&path).map_err(|e| format!("Cannot delete {path}: {e}"))
}

fn data_url(mime: &str, bytes: &[u8]) -> String {
    format!(
        "data:{mime};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    )
}

/// One image from the archive, as a data URL.
pub fn book_image(book_path: String, image: String) -> AppResult<BookImage> {
    let (book, _) = book(&book_path)?;
    let (bytes, mime) = book
        .read_image(&image)
        .ok_or_else(|| format!("{image} is not in the book"))?;
    Ok(BookImage {
        data_url: data_url(&mime, &bytes),
        path: image,
    })
}

/// Read a library thumbnail without keeping the book resident.
pub fn load_cover(path: String) -> AppResult<Option<BookImage>> {
    let package = Package::parse_file(Path::new(&path)).map_err(|e| e.to_string())?;
    let Some(cover) = package.cover_ref() else {
        return Ok(None);
    };
    let bytes = std::fs::read(&path).map_err(|e| format!("Cannot read {path}: {e}"))?;
    Ok(
        cookbook::epub::open::read_image(&bytes, &cover.path).map(|(bytes, mime)| BookImage {
            path,
            data_url: data_url(&mime, &bytes),
        }),
    )
}

/// Single-source declarations for the checked-in frontend contract: the
/// application DTOs plus every `cookbook` type they carry. Each entry lists
/// the type's direct dependencies so a test can prove the file is closed.
fn bindings() -> Vec<(String, Vec<String>)> {
    let cfg = ts_rs::Config::new().with_large_int("number");
    fn entry<T: TS + 'static>(cfg: &ts_rs::Config) -> (String, Vec<String>) {
        (
            T::decl(cfg),
            T::dependencies(cfg)
                .into_iter()
                .map(|d| d.ts_name)
                .collect(),
        )
    }
    vec![
        (Value::decl(&cfg), vec![]),
        entry::<IngredientResult>(&cfg),
        entry::<TraceNode>(&cfg),
        entry::<IngredientInspection>(&cfg),
        entry::<WebSection>(&cfg),
        entry::<RecipeResult>(&cfg),
        entry::<CorpusField>(&cfg),
        entry::<CorpusCase>(&cfg),
        entry::<CorpusResult>(&cfg),
        entry::<LibraryBook>(&cfg),
        entry::<OpenedBook>(&cfg),
        entry::<BookImage>(&cfg),
        entry::<GatewayStatus>(&cfg),
        entry::<RunSummary>(&cfg),
        entry::<Classified>(&cfg),
        entry::<cookbook::classify::Classification>(&cfg),
        entry::<BookOutline>(&cfg),
        entry::<Estimate>(&cfg),
        entry::<Progress>(&cfg),
        entry::<cookbook::Phase>(&cfg),
        entry::<cookbook::Eta>(&cfg),
        entry::<Extraction>(&cfg),
        entry::<cookbook::Cookbook>(&cfg),
        entry::<cookbook::BookSource>(&cfg),
        entry::<cookbook::Chapter>(&cfg),
        entry::<cookbook::Item>(&cfg),
        entry::<cookbook::Recipe>(&cfg),
        entry::<cookbook::Technique>(&cfg),
        entry::<cookbook::Essay>(&cfg),
        entry::<cookbook::RecipeMeta>(&cfg),
        entry::<recipe_types::RecipeTimes>(&cfg),
        entry::<cookbook::Section>(&cfg),
        entry::<cookbook::IngredientLine>(&cfg),
        entry::<ingredient::ingredient::Ingredient>(&cfg),
        entry::<ingredient::unit::Measure>(&cfg),
        entry::<ingredient::IngredientUsage>(&cfg),
        entry::<ingredient::Confidence>(&cfg),
        entry::<cookbook::Step>(&cfg),
        entry::<cookbook::Note>(&cfg),
        entry::<cookbook::RecipeRef>(&cfg),
        entry::<cookbook::RefKind>(&cfg),
        entry::<cookbook::RefMethod>(&cfg),
        entry::<cookbook::Edge>(&cfg),
        entry::<cookbook::ImageRef>(&cfg),
        entry::<cookbook::Span>(&cfg),
        entry::<cookbook::RunReport>(&cfg),
        entry::<ExtractOptions>(&cfg),
        entry::<cookbook::StageTiming>(&cfg),
        entry::<cookbook::CallRecord>(&cfg),
        entry::<cookbook::CallPurpose>(&cfg),
        entry::<cookbook::CallOutcome>(&cfg),
        entry::<cookbook::Usage>(&cfg),
        entry::<cookbook::ChunkReport>(&cfg),
        entry::<cookbook::ChunkStatus>(&cfg),
        entry::<cookbook::Flag>(&cfg),
        entry::<cookbook::SecondOpinion>(&cfg),
        entry::<cookbook::Chosen>(&cfg),
        entry::<cookbook::CrossCheck>(&cfg),
        entry::<cookbook::Escalation>(&cfg),
        entry::<cookbook::UnresolvedRef>(&cfg),
        entry::<cookbook::ModelUsage>(&cfg),
        entry::<cookbook::EtaSample>(&cfg),
    ]
}

pub fn bindings_source() -> String {
    format!(
        "// Generated from Rust DTOs (food-app + cookbook). Do not edit.\n{}\n",
        bindings()
            .into_iter()
            .map(|(decl, _)| format!(
                "export {}",
                decl.lines()
                    .map(str::trim_end)
                    .collect::<Vec<_>>()
                    .join("\n")
            ))
            .collect::<Vec<_>>()
            .join("\n")
    )
}

/// Explicit development command; tests never regenerate checked-in frontend files.
pub fn export_bindings(path: &Path) -> AppResult<()> {
    std::fs::write(path, bindings_source()).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    #[test]
    fn batch_and_inspector_share_results_and_keep_failed_source() {
        let input = "1 cup flour\n\n1+1 vitamins";
        let rows = parse_batch(input.into()).unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[1].line_number, 2);
        assert!(!rows[1].review_reasons.is_empty());
        for row in rows {
            let inspection = inspect_ingredient(row.input.clone()).unwrap();
            assert_eq!(inspection.result.json, row.json);
            assert_eq!(inspection.result.input, row.input);
            assert!(inspection.trace.is_some());
            assert!(serde_json::from_str::<Value>(&inspection.jaeger_json).is_ok());
        }
    }

    #[test]
    fn corpus_keeps_invalid_and_known_gap_rows() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("corpus.jsonl");
        std::fs::write(&path,"{\"input\":\"1 cup flour\",\"name\":\"flour\",\"amounts\":[{\"unit\":\"cup\",\"value\":1}]}\ninvalid\n{\"input\":\"salt\",\"name\":\"wrong\",\"xfail\":\"known gap\"}\n").unwrap();
        let result = load_corpus(Some(path.to_string_lossy().into_owned())).unwrap();
        assert_eq!(
            result
                .cases
                .iter()
                .map(|r| r.status.as_str())
                .collect::<Vec<_>>(),
            vec!["EXACT", "INVALID", "XFAIL"]
        );
        assert_eq!(result.cases[1].line_number, 2);
    }

    #[test]
    fn recipe_source_parses_without_a_network_transport() {
        let html = r#"<script type="application/ld+json">{"name":"Soup","recipeIngredient":["1 tsp salt"],"recipeInstructions":[{"@type":"HowToStep","text":"Mix the salt."}]}</script>"#;
        let result =
            recipe_result(recipe_scraper::scrape(html, "https://example.com/soup").unwrap())
                .unwrap();
        assert_eq!(result.title, "Soup");
        assert_eq!(result.ingredients[0].name, "salt");
    }

    /// Every type a declaration references is itself declared, and large
    /// integers come out as `number` (Tauri sends JSON numbers, not bigint).
    #[test]
    fn frontend_bindings_are_closed_over_their_dependencies() {
        let entries = bindings();
        let source = bindings_source();
        for (decl, deps) in &entries {
            for dep in deps {
                assert!(
                    source.contains(&format!("export type {dep} ")),
                    "{dep} (needed by {}) is not declared",
                    decl.lines().next().unwrap_or("")
                );
            }
        }
        assert!(!source.contains("bigint"), "{source}");
        assert!(source.contains("export type Extraction = "));
        assert!(source.contains("export type Ingredient = "));
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
        let cover = load_cover(books[0].path.clone()).unwrap();
        assert!(cover.is_none() || cover.unwrap().data_url.starts_with("data:image/"));
    }

    #[test]
    fn opening_a_book_is_offline_and_reuses_the_handle() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("book.epub");
        std::fs::write(&path, cookbook_fixtures::epub3_nav_pagebreaks().unwrap()).unwrap();
        let path = path.to_string_lossy().into_owned();
        let first = open_book(path.clone()).unwrap();
        assert!(first.outline.chunks >= 1);
        assert!(first.outline.lines > 10);
        let second = open_book(path.clone()).unwrap();
        assert_eq!(second.open_ms, 0, "second open should reuse the handle");
        assert_eq!(first.outline, second.outline);
        let estimate = estimate_book(path).unwrap();
        assert_eq!(estimate.chunks, first.outline.chunks);
        assert!(estimate.cost_usd_high >= estimate.cost_usd_low);
    }
}

fn rendered_sections(sections: &[recipe_parsing::ParsedSection]) -> Vec<WebSection> {
    sections
        .iter()
        .map(|section| WebSection {
            name: section.name.clone(),
            ingredients: section
                .ingredients
                .iter()
                .map(ToString::to_string)
                .collect(),
            instructions: section
                .instructions
                .iter()
                .map(|chunks| {
                    chunks
                        .iter()
                        .map(|chunk| match chunk {
                            ingredient::rich_text::Chunk::Measure(amounts) => amounts
                                .iter()
                                .map(ToString::to_string)
                                .collect::<Vec<_>>()
                                .join(" / "),
                            ingredient::rich_text::Chunk::Text(text)
                            | ingredient::rich_text::Chunk::Ing(text) => text.clone(),
                        })
                        .collect::<String>()
                })
                .collect(),
        })
        .collect()
}

fn validate_scale(factor: f64) -> AppResult<()> {
    if !factor.is_finite() || !(0.01..=100.0).contains(&factor) {
        return Err("Scale must be between 0.01 and 100".into());
    }
    Ok(())
}

fn scale_sections(sections: &mut [recipe_parsing::ParsedSection], factor: f64) {
    for section in sections {
        for ingredient in &mut section.ingredients {
            ingredient.amounts = ingredient
                .amounts
                .iter()
                .map(|amount| amount.scale(factor))
                .collect();
        }
        for chunks in &mut section.instructions {
            for chunk in chunks {
                if let ingredient::rich_text::Chunk::Measure(amounts) = chunk {
                    *amounts = amounts.iter().map(|amount| amount.scale(factor)).collect();
                }
            }
        }
    }
}

/// Scale from the original scraped source every time, so repeated presentation
/// changes never compound rounding or change the ingredient's source context.
pub fn scale_web_recipe(source: Value, factor: f64) -> AppResult<RecipeResult> {
    validate_scale(factor)?;
    let recipe: recipe_scraper::ScrapedRecipe =
        serde_json::from_value(source).map_err(|e| e.to_string())?;
    let mut result = recipe_result(recipe)?;
    let mut parsed: recipe_parsing::ParsedRecipe =
        serde_json::from_value(result.parsed).map_err(|e| e.to_string())?;
    scale_sections(&mut parsed.sections, factor);
    for (row, ingredient) in result.ingredients.iter_mut().zip(
        parsed
            .sections
            .iter()
            .flat_map(|section| &section.ingredients),
    ) {
        *row = ingredient_result(&row.input, row.line_number, ingredient);
    }
    result.sections = rendered_sections(&parsed.sections);
    result.parsed = serde_json::to_value(parsed).map_err(|e| e.to_string())?;
    Ok(result)
}

#[cfg(test)]
mod scaling_tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    #[test]
    fn web_scaling_changes_quantities_once_and_preserves_source_and_constraints() {
        use ingredient::unit::{MeasureKind, Unit};
        let html = r#"<script type="application/ld+json">{"name":"Soup","recipeIngredient":["1 cup (240 g) water"],"recipeInstructions":[{"@type":"HowToStep","text":"Add 1 cup (240 g) water; cut into 3cm cubes, then bake at 365 degrees F for 20 minutes."}]}</script>"#;
        let original =
            recipe_result(recipe_scraper::scrape(html, "https://example.com/soup").unwrap())
                .unwrap();
        let scaled = scale_web_recipe(original.source.clone(), 2.0).unwrap();
        assert_eq!(scaled.source, original.source);
        assert_eq!(scaled.ingredients[0].input, original.ingredients[0].input);
        let parsed: recipe_parsing::ParsedRecipe = serde_json::from_value(scaled.parsed).unwrap();
        let ingredient = &parsed.sections[0].ingredients[0];
        assert!(
            ingredient
                .amounts
                .iter()
                .any(|m| *m.unit() == Unit::Cup && m.value() == 2.0)
        );
        assert!(
            ingredient
                .amounts
                .iter()
                .any(|m| *m.unit() == Unit::Gram && m.value() == 480.0)
        );
        let original_parsed: recipe_parsing::ParsedRecipe =
            serde_json::from_value(original.parsed).unwrap();
        let measure_values = |recipe: &recipe_parsing::ParsedRecipe| {
            recipe.sections[0]
                .instructions
                .iter()
                .flatten()
                .filter_map(|chunk| match chunk {
                    ingredient::rich_text::Chunk::Measure(m) => Some(m),
                    _ => None,
                })
                .flatten()
                .cloned()
                .collect::<Vec<_>>()
        };
        let before = measure_values(&original_parsed);
        let after = measure_values(&parsed);
        for kind in [
            MeasureKind::Length,
            MeasureKind::Temperature,
            MeasureKind::Time,
        ] {
            assert_eq!(
                before.iter().find(|m| m.kind() == kind).unwrap(),
                after.iter().find(|m| m.kind() == kind).unwrap()
            );
        }
        let reset = scale_web_recipe(scaled.source, 1.0).unwrap();
        assert_eq!(reset.ingredients[0].json, original.ingredients[0].json);
        assert!(scale_web_recipe(original.source, 0.0).is_err());
    }
}
