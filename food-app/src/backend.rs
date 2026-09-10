//! Application services for the native desktop shell. Library models stay unchanged.
//! Loading and rendering never start extraction; every write has an explicit path.
use base64::Engine;
use ingredient::{IngredientParser, ParseOptions, TraceDetail};
use recipe_epub::review::{ReviewDecision, ReviewDecisions, ReviewRun, RunOptions};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;
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
    pub sections: Vec<CookbookSection>,
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
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct LibraryBook {
    #[serde(default)]
    pub runs: Vec<SavedRun>,
    pub path: String,
    pub title: String,
    pub authors: Vec<String>,
    pub subjects: Vec<String>,
    pub cookbook: bool,
    pub error: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ReviewNote {
    pub document: String,
    pub status: String,
    pub note: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SourceBlock {
    pub id: String,
    pub tag: String,
    pub text: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SourceDocument {
    pub path: String,
    pub blocks: Vec<SourceBlock>,
    pub images: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CookbookSection {
    pub name: Option<String>,
    pub ingredients: Vec<String>,
    pub instructions: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CookbookRecipe {
    pub index: usize,
    pub title: String,
    pub source_document: String,
    pub recipe_yield: Option<String>,
    pub sections: Vec<CookbookSection>,
    pub description: Option<String>,
    pub image: Option<String>,
    pub references: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CookbookChunk {
    pub id: String,
    pub document: String,
    pub complete: bool,
    pub cached: bool,
    pub error: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct QualityIssue {
    pub kind: String,
    pub source: String,
    pub chunk: Option<String>,
    pub recipe: Option<usize>,
    pub message: String,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ExtractionFeedback {
    pub phase: String,
    pub stop_reason: Option<String>,
    pub policy: Vec<String>,
    pub extraction_usd: f64,
    pub verification_usd: f64,
    pub unresolved_usd: f64,
    pub findings: Vec<ExtractionFinding>,
    pub checks: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ExtractionFinding {
    pub category: String,
    pub message: String,
    pub source: String,
    #[ts(optional)]
    pub chunk: Option<String>,
    pub lines: Vec<usize>,
    pub model: String,
    pub resolved: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CookbookResult {
    #[ts(optional)]
    pub feedback: Option<ExtractionFeedback>,
    pub quality_issues: Vec<QualityIssue>,
    pub status: String,
    pub path: Option<String>,
    pub source: String,
    pub model: String,
    pub source_hash: String,
    pub incomplete: bool,
    pub reserved_usd: f64,
    pub documents: Vec<SourceDocument>,
    pub recipes: Vec<CookbookRecipe>,
    pub chunks: Vec<CookbookChunk>,
    pub review: Vec<ReviewNote>,
    /// Original library payload, preserved for source inspection and export.
    pub run: Value,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ExtractionRequest {
    pub book: String,
    pub out: String,
    pub model: String,
    pub resume: bool,
    pub from: Option<String>,
    pub allow_network: bool,
    pub refresh: bool,
    pub cache_dir: Option<String>,
    pub chunks: Vec<String>,
    pub budget_usd: f64,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ExtractionProgress {
    #[serde(default)]
    #[ts(optional)]
    pub active_models: Option<Vec<String>>,
    #[serde(default)]
    #[ts(optional)]
    pub unresolved_usd: Option<f64>,
    #[ts(optional)]
    pub phase: Option<String>,
    pub active: usize,
    pub failed: usize,
    pub elapsed_seconds: f64,
    pub estimated_usd: Option<f64>,
    pub stopping: bool,
    pub path: String,
    pub completed: usize,
    pub total: usize,
    pub recipes: usize,
    pub reserved_usd: f64,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct BookImage {
    pub path: String,
    pub data_url: String,
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
            .map(|section| CookbookSection {
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

pub fn scan_library(directory: String) -> AppResult<Vec<LibraryBook>> {
    let directory = if directory.trim().is_empty() {
        std::env::var_os("HOME")
            .map(std::path::PathBuf::from)
            .ok_or("Home directory is unavailable")?
            .join("Library/Mobile Documents/com~apple~CloudDocs/Calibre")
    } else {
        std::path::PathBuf::from(directory)
    };
    let directory = directory.as_path();
    if !directory.is_dir() {
        return Err(format!("{} is not a directory", directory.display()));
    }
    let history = recipe_epub::review::store::list(None).map_err(|e| e.to_string())?;
    let mut paths = recipe_epub::find_epubs(directory);
    paths.sort();
    let mut books: Vec<_> = paths
        .iter()
        .map(|path| match recipe_epub::book_metadata(path) {
            Ok(meta) => LibraryBook {
                runs: if !history.is_empty() {
                    let digest = recipe_epub::review::store::source_hash(path).ok();
                    history
                        .iter()
                        .filter(|r| digest.as_deref() == Some(r.epub_sha256.as_str()))
                        .cloned()
                        .map(SavedRun::from)
                        .collect()
                } else {
                    vec![]
                },
                cookbook: recipe_epub::classify_by_tags(&meta) == recipe_epub::CookbookGuess::Yes,
                path: path.to_string_lossy().into(),
                title: meta.title,
                authors: meta.authors,
                subjects: meta.subjects,
                error: None,
            },
            Err(error) => LibraryBook {
                runs: vec![],
                path: path.to_string_lossy().into(),
                title: path
                    .file_stem()
                    .map(|s| s.to_string_lossy().into())
                    .unwrap_or_default(),
                authors: Vec::new(),
                subjects: Vec::new(),
                cookbook: false,
                error: Some(error.to_string()),
            },
        })
        .collect();
    books.sort_by(|a, b| {
        (!a.cookbook, a.title.to_lowercase()).cmp(&(!b.cookbook, b.title.to_lowercase()))
    });
    Ok(books)
}

fn existing_run_path(path: &str) -> AppResult<String> {
    Path::new(path)
        .canonicalize()
        .map(|path| path.to_string_lossy().into_owned())
        .map_err(|e| format!("Cannot resolve {path}: {e}"))
}

/// The human-review sidecar belongs to the actual run, not an alias used to open it.
/// Exposed so the desktop shell can lock the same file the backend will write.
pub fn review_path(path: &str) -> AppResult<String> {
    Ok(Path::new(&existing_run_path(path)?)
        .with_extension("review.json")
        .to_string_lossy()
        .into_owned())
}

fn result(run: &ReviewRun, path: Option<&str>) -> AppResult<CookbookResult> {
    let path = path.map(existing_run_path).transpose()?;
    let decisions = if let Some(path) = &path {
        ReviewDecisions::read(Path::new(&review_path(path)?), run)
            .map_err(|e| e.to_string())?
            .documents
    } else {
        Default::default()
    };
    Ok(CookbookResult {
        feedback: run.recovery.as_ref().map(|s| ExtractionFeedback {
            checks: s
                .groups
                .iter()
                .enumerate()
                .flat_map(|(i, g)| {
                    g.candidates.iter().filter(|c| c.verified).map(move |c| {
                        format!(
                            "Group {} · {} · {}",
                            i + 1,
                            if c.model == "gemini-2.5-flash" {
                                "GLM 5.3 verifier"
                            } else {
                                "Gemini 2.5 Flash verifier"
                            },
                            if c.feedback.is_empty() {
                                "All automated group checks passed"
                            } else {
                                "Source issues detected"
                            }
                        )
                    })
                })
                .collect(),
            phase: s.phase.clone(),
            stop_reason: s.stop_reason.clone(),
            policy: s.models.clone(),
            extraction_usd: s
                .attempts
                .iter()
                .filter(|a| !a.verification)
                .filter_map(|a| a.estimated_usd)
                .sum(),
            verification_usd: s
                .attempts
                .iter()
                .filter(|a| a.verification)
                .filter_map(|a| a.estimated_usd)
                .sum(),
            unresolved_usd: s
                .attempts
                .iter()
                .filter(|a| a.estimated_usd.is_none())
                .map(|a| a.reservation_usd)
                .sum(),
            findings: s
                .feedback()
                .into_iter()
                .map(|f| ExtractionFinding {
                    source: s
                        .source
                        .get(f.chunk)
                        .map(|c| c.doc_path.clone())
                        .unwrap_or_default(),
                    chunk: run.chunks.get(f.chunk).map(|c| c.id.clone()),
                    category: f.category,
                    message: f.message,
                    lines: f.lines,
                    model: f.model,
                    resolved: f.resolved,
                })
                .collect(),
        }),
        quality_issues: recipe_epub::review::quality::issues(run)
            .into_iter()
            .map(|issue| QualityIssue {
                kind: issue.kind,
                source: issue.source,
                chunk: issue.chunk,
                recipe: issue.recipe,
                message: issue.message,
                detail: issue.detail,
            })
            .collect(),
        path,
        source: run.source.clone(),
        model: run.model.clone(),
        source_hash: run.epub_sha256.clone(),
        status: run.status().into(),
        incomplete: run.incomplete(),
        reserved_usd: run.reserved_usd,
        documents: run
            .documents
            .iter()
            .map(|d| SourceDocument {
                path: d.path.clone(),
                blocks: d
                    .blocks
                    .iter()
                    .map(|b| SourceBlock {
                        id: b.id.clone(),
                        tag: b.tag.clone(),
                        text: b.text.clone(),
                    })
                    .collect(),
                images: d.images.iter().map(|i| i.path.clone()).collect(),
            })
            .collect(),
        recipes: run
            .recipes
            .iter()
            .enumerate()
            .map(|(index, r)| CookbookRecipe {
                index,
                title: r.meta.title.clone(),
                source_document: r
                    .url
                    .rsplit_once('#')
                    .map(|(_, p)| p.to_owned())
                    .unwrap_or_default(),
                recipe_yield: r.meta.recipe_yield.clone(),
                description: r.meta.description.clone(),
                image: r.image.as_ref().map(|i| i.path.clone()),
                references: r.references.iter().map(|r| r.title.clone()).collect(),
                sections: r
                    .sections
                    .iter()
                    .map(|s| CookbookSection {
                        name: s.name.clone(),
                        ingredients: s.ingredients.clone(),
                        instructions: s.instructions.clone(),
                    })
                    .collect(),
            })
            .collect(),
        chunks: run
            .chunks
            .iter()
            .map(|c| CookbookChunk {
                id: c.id.clone(),
                document: c.source.doc_path.clone(),
                complete: c.output.is_some(),
                cached: c.cached,
                error: c.error.clone(),
            })
            .collect(),
        review: decisions
            .into_iter()
            .map(|(document, d)| ReviewNote {
                document,
                status: d.status,
                note: d.note,
            })
            .collect(),
        run: serde_json::to_value(run).map_err(|e| e.to_string())?,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ModelChoice {
    pub id: String,
    pub label: String,
    pub enabled: bool,
    pub status: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ExtractionPreview {
    #[ts(optional)]
    pub policy: Option<Vec<String>>,
    #[ts(optional)]
    pub extraction_remaining_usd: Option<f64>,
    #[ts(optional)]
    pub verification_remaining_usd: Option<f64>,
    pub total: usize,
    pub cached: usize,
    pub pending: usize,
    pub low_usd: Option<f64>,
    pub high_usd: Option<f64>,
    pub reservation_usd: Option<f64>,
    pub basis: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SavedRun {
    pub quality_flags: Option<usize>,
    pub status: String,
    pub epub_sha256: String,
    pub path: String,
    pub title: String,
    pub model: String,
    pub prompt_version: String,
    pub configurations: Vec<String>,
    pub created_at: Option<f64>,
    pub recipes: usize,
    pub completed: usize,
    pub total: usize,
    pub incomplete: bool,
    pub reserved_usd: f64,
    pub new_spend_usd: Option<f64>,
    pub inherited_reserved_usd: Option<f64>,
    pub unresolved_usd: f64,
}
pub fn cookbook_models() -> Vec<ModelChoice> {
    std::iter::once(ModelChoice {
        id: recipe_epub::recovery::AUTOMATIC.into(),
        label: "Automatic".into(),
        enabled: true,
        status: "Source verification and bounded recovery".into(),
    })
    .chain(
        recipe_epub::models::catalog()
            .into_iter()
            .map(|m| ModelChoice {
                id: m.id.into(),
                label: m.label.into(),
                enabled: m.enabled,
                status: m.status.into(),
            }),
    )
    .collect()
}
impl From<recipe_epub::review::store::RunSummary> for SavedRun {
    fn from(r: recipe_epub::review::store::RunSummary) -> Self {
        Self {
            quality_flags: r.quality_flags,
            status: r.status,
            epub_sha256: r.epub_sha256,
            path: r.path.to_string_lossy().into_owned(),
            title: r.title,
            model: r.model,
            prompt_version: r.prompt_version,
            configurations: r.configurations,
            created_at: r.created_at.map(|v| v as f64),
            recipes: r.recipes,
            completed: r.completed,
            total: r.total,
            incomplete: r.incomplete,
            reserved_usd: r.reserved_usd,
            new_spend_usd: r.new_spend_usd,
            inherited_reserved_usd: r.inherited_reserved_usd,
            unresolved_usd: r.unresolved_usd,
        }
    }
}
pub fn cookbook_runs(book: Option<String>) -> AppResult<Vec<SavedRun>> {
    recipe_epub::review::store::list(book.as_deref().map(Path::new))
        .map_err(|e| e.to_string())
        .map(|rows| rows.into_iter().map(SavedRun::from).collect())
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ModelBookResult {
    pub latest: SavedRun,
    pub runs: usize,
    pub failed_chunks: usize,
    pub pending_chunks: usize,
    pub content_review_flags: usize,
    pub processing_success_rate: Option<f64>,
    pub attempts: Option<usize>,
    pub failed_attempts: Option<usize>,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ModelBookResults {
    pub rows: Vec<ModelBookResult>,
    pub unreadable: Vec<String>,
}
pub fn cookbook_results(book: Option<String>) -> AppResult<ModelBookResults> {
    let report = recipe_epub::review::results::list(book.as_deref().map(Path::new))
        .map_err(|e| e.to_string())?;
    Ok(ModelBookResults {
        unreadable: report.unreadable,
        rows: report
            .rows
            .into_iter()
            .map(|r| ModelBookResult {
                latest: r.latest.into(),
                runs: r.runs,
                failed_chunks: r.failed_chunks,
                pending_chunks: r.pending_chunks,
                content_review_flags: r.content_review_flags,
                processing_success_rate: r.processing_success_rate,
                attempts: r.attempts,
                failed_attempts: r.failed_attempts,
            })
            .collect(),
    })
}
fn shared_request(request: ExtractionRequest) -> recipe_epub::review::ExtractionRequest {
    recipe_epub::review::ExtractionRequest {
        book: request.book.into(),
        out: request.out.into(),
        model: request.model,
        resume: request.resume,
        from: request.from.map(Into::into),
        options: RunOptions {
            allow_network: request.allow_network,
            refresh: request.refresh,
            cache_dir: request.cache_dir.map(Into::into),
            chunks: request.chunks,
            budget_usd: request.budget_usd,
            concurrency: 4,
        },
    }
}
pub fn extraction_preview(request: ExtractionRequest) -> AppResult<ExtractionPreview> {
    let request = shared_request(request);
    let run = recipe_epub::review::prepare(&request).map_err(|e| e.to_string())?;
    let plan =
        recipe_epub::review::preflight::plan(&run, &request.options).map_err(|e| e.to_string())?;
    Ok(ExtractionPreview {
        policy: plan.recovery.as_ref().map(|p| p.models.clone()),
        extraction_remaining_usd: plan.recovery.as_ref().map(|p| p.extraction_remaining_usd),
        verification_remaining_usd: plan.recovery.as_ref().map(|p| p.verification_remaining_usd),
        total: plan.total,
        cached: plan.cached,
        pending: plan.pending,
        low_usd: plan.estimated_low_usd,
        high_usd: plan.estimated_high_usd,
        reservation_usd: plan.reservation_usd,
        basis: plan.basis.into(),
    })
}
pub fn export_run(path: String, out: String) -> AppResult<()> {
    recipe_epub::review::export_run(Path::new(&path), Path::new(&out)).map_err(|e| e.to_string())
}

pub fn inspect_book(path: String, model: Option<String>) -> AppResult<CookbookResult> {
    let run = recipe_epub::review::store::inspect(
        Path::new(&path),
        model
            .as_deref()
            .unwrap_or(recipe_epub::models::DEFAULT_MODEL),
    )
    .map_err(|e| e.to_string())?;
    result(&run, None)
}
pub fn open_run(path: String) -> AppResult<CookbookResult> {
    let path = existing_run_path(&path)?;
    let run = ReviewRun::read(Path::new(&path)).map_err(|e| e.to_string())?;
    recipe_epub::review::store::register(&run, Path::new(&path)).map_err(|e| e.to_string())?;
    result(&run, Some(&path))
}

pub async fn extract_run(
    request: ExtractionRequest,
    progress: impl Fn(ExtractionProgress),
) -> AppResult<CookbookResult> {
    extract_run_controlled(
        request,
        &recipe_epub::review::ExtractionControl::default(),
        progress,
    )
    .await
}

pub async fn extract_run_controlled(
    mut request: ExtractionRequest,
    control: &recipe_epub::review::ExtractionControl,
    progress: impl Fn(ExtractionProgress),
) -> AppResult<CookbookResult> {
    if request.out.is_empty() {
        if request.resume {
            return Err("Resume requires an existing extraction".into());
        }
        request.out =
            recipe_epub::review::store::destination(Path::new(&request.book), &request.model)
                .map_err(|e| e.to_string())?
                .to_string_lossy()
                .into_owned();
    }
    let progress_path = request.out.clone();
    let outcome = recipe_epub::review::extract_to_run_controlled(
        shared_request(request),
        control,
        |update| {
            progress(ExtractionProgress {
                active_models: Some(update.active_models),
                unresolved_usd: Some(update.unresolved_usd),
                phase: Some(update.phase.into()),
                active: update.active,
                failed: update.failed,
                elapsed_seconds: update.elapsed_seconds as f64,
                estimated_usd: update.estimated_usd,
                stopping: update.stopping,
                path: progress_path.clone(),
                completed: update.completed,
                total: update.total,
                recipes: update.recipes,
                reserved_usd: update.reserved_usd,
            })
        },
    )
    .await
    .map_err(|error| error.to_string())?;
    result(&outcome.run, Some(&outcome.path.to_string_lossy()))
}

pub fn replay_run(path: String, out: String) -> AppResult<CookbookResult> {
    let outcome = recipe_epub::review::replay_to_run(recipe_epub::review::ReplayRequest {
        run: path.into(),
        out: out.into(),
        source: None,
        image_text: None,
    })
    .map_err(|error| error.to_string())?;
    result(&outcome.run, Some(&outcome.path.to_string_lossy()))
}

pub fn save_review(
    path: String,
    document: String,
    status: String,
    note: String,
) -> AppResult<CookbookResult> {
    if !["Unreviewed", "Accepted", "Incorrect", "Uncertain"].contains(&status.as_str()) {
        return Err("Unknown review status".into());
    }
    let path = existing_run_path(&path)?;
    let run = ReviewRun::read(Path::new(&path)).map_err(|e| e.to_string())?;
    if !run.documents.iter().any(|d| d.path == document) {
        return Err("Review document is not part of this run".into());
    }
    let sidecar = std::path::PathBuf::from(review_path(&path)?);
    let mut decisions = ReviewDecisions::read(&sidecar, &run).map_err(|e| e.to_string())?;
    decisions
        .documents
        .insert(document, ReviewDecision { status, note });
    decisions.save(&sidecar).map_err(|e| e.to_string())?;
    result(&run, Some(&path))
}
pub fn run_stats(path: String) -> AppResult<Value> {
    let run = ReviewRun::read(Path::new(&path)).map_err(|e| e.to_string())?;
    serde_json::to_value(
        recipe_epub::review::stats::ingredient_stats(&run).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}
pub fn evaluate_run(path: String, expectations: String) -> AppResult<Value> {
    let run = ReviewRun::read(Path::new(&path)).map_err(|e| e.to_string())?;
    let expected = serde_json::from_slice(&std::fs::read(expectations).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    recipe_epub::review::evaluate_document(&run, expected).map_err(|e| e.to_string())
}
pub fn run_audit(path: String) -> AppResult<Value> {
    Ok(recipe_epub::review::source_audit(
        &ReviewRun::read(Path::new(&path)).map_err(|e| e.to_string())?,
    ))
}
pub fn run_diff(before: String, after: String) -> AppResult<Value> {
    recipe_epub::review::diff(
        &ReviewRun::read(Path::new(&before)).map_err(|e| e.to_string())?,
        &ReviewRun::read(Path::new(&after)).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}
pub fn load_images(run_path: Option<String>, book_path: String) -> AppResult<Vec<BookImage>> {
    let bytes = std::fs::read(&book_path).map_err(|e| e.to_string())?;
    let run = match run_path {
        Some(path) => ReviewRun::read(Path::new(&path)).map_err(|e| e.to_string())?,
        None => ReviewRun::inspect(&bytes, &book_path, recipe_epub::models::DEFAULT_MODEL)
            .map_err(|e| e.to_string())?,
    };
    let images = recipe_epub::review::source_images(&run, &bytes).map_err(|e| e.to_string())?;
    Ok(images
        .into_iter()
        .map(|(path, bytes)| {
            let mime = run
                .documents
                .iter()
                .flat_map(|d| &d.images)
                .find(|i| i.path == path)
                .map(|i| i.mime.as_str())
                .unwrap_or("application/octet-stream");
            BookImage {
                data_url: format!(
                    "data:{mime};base64,{}",
                    base64::engine::general_purpose::STANDARD.encode(bytes)
                ),
                path,
            }
        })
        .collect())
}

/// Single-source declarations for the checked-in frontend contract.
pub fn bindings_source() -> String {
    let declarations = [
        ModelChoice::decl(&ts_rs::Config::default()),
        ExtractionPreview::decl(&ts_rs::Config::default()),
        SavedRun::decl(&ts_rs::Config::default()),
        ModelBookResult::decl(&ts_rs::Config::default()),
        ModelBookResults::decl(&ts_rs::Config::default()),
        Value::decl(&ts_rs::Config::default()),
        IngredientResult::decl(&ts_rs::Config::default()),
        TraceNode::decl(&ts_rs::Config::default()),
        IngredientInspection::decl(&ts_rs::Config::default()),
        RecipeResult::decl(&ts_rs::Config::default()),
        CorpusField::decl(&ts_rs::Config::default()),
        CorpusCase::decl(&ts_rs::Config::default()),
        CorpusResult::decl(&ts_rs::Config::default()),
        LibraryBook::decl(&ts_rs::Config::default()),
        ReviewNote::decl(&ts_rs::Config::default()),
        SourceBlock::decl(&ts_rs::Config::default()),
        SourceDocument::decl(&ts_rs::Config::default()),
        CookbookSection::decl(&ts_rs::Config::default()),
        CookbookRecipe::decl(&ts_rs::Config::default()),
        CookbookChunk::decl(&ts_rs::Config::default()),
        QualityIssue::decl(&ts_rs::Config::default()),
        ExtractionFeedback::decl(&ts_rs::Config::default()),
        ExtractionFinding::decl(&ts_rs::Config::default()),
        CookbookResult::decl(&ts_rs::Config::default()),
        ExtractionRequest::decl(&ts_rs::Config::default()),
        ExtractionProgress::decl(&ts_rs::Config::default()),
        BookImage::decl(&ts_rs::Config::default()),
    ];
    format!(
        "// Generated from Rust application DTOs. Do not edit.\n{}\n",
        declarations
            .into_iter()
            .map(|d| format!(
                "export {}",
                d.lines().map(str::trim_end).collect::<Vec<_>>().join("\n")
            ))
            .collect::<Vec<_>>()
            .join("\n")
    )
}

/// Explicit development command; tests never regenerate checked-in frontend files.
pub fn export_bindings(path: &Path) -> AppResult<()> {
    std::fs::write(path, bindings_source()).map_err(|e| e.to_string())
}

/// Read a library thumbnail on demand; scanning does not decode every book cover.
pub fn load_cover(path: String) -> AppResult<Option<BookImage>> {
    Ok(
        recipe_epub::book_cover(Path::new(&path)).map(|(bytes, mime)| BookImage {
            path,
            data_url: format!(
                "data:{mime};base64,{}",
                base64::engine::general_purpose::STANDARD.encode(bytes)
            ),
        }),
    )
}

/// Presentation-only scaling, using the parser's amount semantics for temperatures
/// and dimensions as well as ingredient alternatives. The saved run is untouched.
pub fn scale_recipe(path: String, index: usize, factor: f64) -> AppResult<CookbookRecipe> {
    use recipe_epub::CookbookRecipeExt;
    validate_scale(factor)?;
    let run = ReviewRun::read(Path::new(&path)).map_err(|e| e.to_string())?;
    let recipe = run
        .recipes
        .get(index)
        .ok_or("Recipe index is out of range")?;
    let mut parsed = recipe.parse();
    scale_sections(&mut parsed.sections, factor);
    let mut result = result(&run, Some(&path))?
        .recipes
        .into_iter()
        .nth(index)
        .ok_or("Recipe index is out of range")?;
    result.sections = rendered_sections(&parsed.sections);
    Ok(result)
}

fn rendered_sections(sections: &[recipe_parsing::ParsedSection]) -> Vec<CookbookSection> {
    sections
        .iter()
        .map(|section| CookbookSection {
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

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    struct Fixture(std::path::PathBuf);
    impl Fixture {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "food-app-backend-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
        fn path(&self, name: &str) -> String {
            self.0.join(name).to_string_lossy().into()
        }
        fn book(&self) -> String {
            let path = self.path("book.epub");
            std::fs::write(&path, recipe_epub_fixtures::cookbook_epub().unwrap()).unwrap();
            path
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

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
        let fixture = Fixture::new();
        let path = fixture.path("corpus.jsonl");
        std::fs::write(&path,"{\"input\":\"1 cup flour\",\"name\":\"flour\",\"amounts\":[{\"unit\":\"cup\",\"value\":1}]}\ninvalid\n{\"input\":\"salt\",\"name\":\"wrong\",\"xfail\":\"known gap\"}\n").unwrap();
        let result = load_corpus(Some(path)).unwrap();
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
    fn inspect_is_source_only_and_reviews_round_trip_with_source_validation() {
        let fixture = Fixture::new();
        let book = fixture.book();
        let inspected = inspect_book(book, None).unwrap();
        assert!(inspected.path.is_none());
        assert!(inspected.recipes.is_empty());
        assert!(inspected.chunks.iter().all(|c| !c.complete));
        assert_eq!(std::fs::read_dir(&fixture.0).unwrap().count(), 1);
        assert!(
            load_images(None, fixture.path("book.epub"))
                .unwrap()
                .is_empty()
        );
        assert_eq!(std::fs::read_dir(&fixture.0).unwrap().count(), 1);
        let path = fixture.path("run.json");
        let run: ReviewRun = serde_json::from_value(inspected.run).unwrap();
        run.save(Path::new(&path)).unwrap();
        let document = run.documents[0].path.clone();
        let before = std::fs::read(&path).unwrap();
        save_review(
            path.clone(),
            document.clone(),
            "Accepted".into(),
            "Source checked".into(),
        )
        .unwrap();
        let reopened = open_run(path.clone()).unwrap();
        assert_eq!(reopened.review[0].note, "Source checked");
        assert_eq!(std::fs::read(&path).unwrap(), before);
        assert!(
            save_review(
                path.clone(),
                "missing.xhtml".into(),
                "Accepted".into(),
                String::new()
            )
            .is_err()
        );
        assert!(save_review(path, document, "invalid".into(), String::new()).is_err());
    }
    #[test]
    fn cache_only_extraction_is_durable_without_credentials_and_cannot_overwrite() {
        let fixture = Fixture::new();
        let book = fixture.book();
        let out = fixture.path("run.json");
        let request = ExtractionRequest {
            book,
            out: out.clone(),
            model: "gemini-2.5-flash".into(),
            resume: false,
            from: None,
            allow_network: false,
            refresh: false,
            cache_dir: Some(fixture.path("empty-cache")),
            chunks: vec![],
            budget_usd: 0.0,
        };
        let rt = tokio::runtime::Runtime::new().unwrap();
        let run = rt.block_on(extract_run(request.clone(), |_| {})).unwrap();
        assert!(run.incomplete);
        assert!(Path::new(&out).exists());
        let before = std::fs::read(&out).unwrap();
        assert!(rt.block_on(extract_run(request, |_| {})).is_err());
        assert_eq!(std::fs::read(&out).unwrap(), before);
        assert_eq!(open_run(out.clone()).unwrap().source_hash, run.source_hash);
        assert!(replay_run(out.clone(), out).is_err());
    }
    #[cfg(unix)]
    #[test]
    fn symlink_run_uses_original_review_sidecar_and_preserves_alias_files() {
        let fixture = Fixture::new();
        let inspected = inspect_book(fixture.book(), None).unwrap();
        let run: ReviewRun = serde_json::from_value(inspected.run).unwrap();
        let path = fixture.path("run.json");
        let alias = fixture.path("alias.json");
        run.save(Path::new(&path)).unwrap();
        std::os::unix::fs::symlink(&path, &alias).unwrap();
        let document = run.documents[0].path.clone();
        save_review(
            path.clone(),
            document.clone(),
            "Accepted".into(),
            "Original decision".into(),
        )
        .unwrap();
        let unrelated = fixture.path("alias.review.json");
        std::fs::write(&unrelated, "unrelated sidecar: do not touch").unwrap();
        let opened = open_run(alias.clone()).unwrap();
        assert_eq!(opened.path, Some(existing_run_path(&path).unwrap()));
        assert_eq!(opened.review[0].note, "Original decision");
        assert_eq!(review_path(&alias).unwrap(), review_path(&path).unwrap());
        save_review(
            alias.clone(),
            document,
            "Uncertain".into(),
            "Updated through alias".into(),
        )
        .unwrap();
        assert_eq!(
            open_run(path.clone()).unwrap().review[0].note,
            "Updated through alias"
        );
        assert_eq!(
            std::fs::read_to_string(unrelated).unwrap(),
            "unrelated sidecar: do not touch"
        );
        assert!(
            std::fs::symlink_metadata(&alias)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        let replayed = replay_run(alias, fixture.path("replay.json")).unwrap();
        assert_eq!(replayed.run["parent"], existing_run_path(&path).unwrap());
    }

    #[test]
    fn incompatible_existing_review_is_never_overwritten() {
        let fixture = Fixture::new();
        let inspected = inspect_book(fixture.book(), None).unwrap();
        let run: ReviewRun = serde_json::from_value(inspected.run).unwrap();
        let path = fixture.path("run.json");
        run.save(Path::new(&path)).unwrap();
        let sidecar = review_path(&path).unwrap();
        let other = r#"{"version":1,"epub_sha256":"different-book","documents":{}}"#;
        std::fs::write(&sidecar, other).unwrap();
        assert!(
            save_review(
                path,
                run.documents[0].path.clone(),
                "Accepted".into(),
                "Do not save".into()
            )
            .is_err()
        );
        assert_eq!(std::fs::read_to_string(sidecar).unwrap(), other);
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
