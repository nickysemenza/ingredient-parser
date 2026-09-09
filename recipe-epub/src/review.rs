//! Durable local extraction runs shared by command-line and desktop review.

use crate::{
    Chunk, CookbookRecipe, CookbookRecipeExt, EpubError, ExtractedRecipe, RecipeExtractor, Usage,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    io::Write,
    path::{Path, PathBuf},
};

pub mod preflight;
pub mod quality;
pub mod stats;
pub mod store;
mod workflow;
pub use workflow::{
    ExtractionRequest, ReplayRequest, RunOutcome, RunProgress, WorkflowError, export_run,
    extract_to_run, extract_to_run_controlled, prepare, replay_to_run,
};

pub const RUN_VERSION: u32 = 1;

pub use crate::source::{SourceBlock, SourceDocument};

/// Source coordinates refer to the stored, cleaned chunk, never guessed DOM offsets.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunChunk {
    pub id: String,
    pub source: Chunk,
    pub output: Option<Vec<ExtractedRecipe>>,
    pub error: Option<String>,
    pub cached: bool,
    pub usage: Usage,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub prompt_version: Option<String>,
    #[serde(default)]
    pub request_identity: Option<crate::cache_contract::CacheIdentity>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewRun {
    #[serde(default)]
    pub execution_status: Option<String>,
    #[serde(default)]
    pub metadata: Option<store::RunMetadata>,
    #[serde(default)]
    pub charges: Vec<store::Charge>,
    pub version: u32,
    pub epub_sha256: String,
    pub source: String,
    pub model: String,
    pub prompt_version: String,
    pub parent: Option<String>,
    pub chunks: Vec<RunChunk>,
    pub documents: Vec<SourceDocument>,
    pub recipes: Vec<CookbookRecipe>,
    pub parsed: serde_json::Value,
    /// Conservative reservations include interrupted requests with unknown billing.
    pub reserved_usd: f64,
    /// Optional transcribed/OCR source captions, distinct from desired output labels.
    #[serde(default)]
    pub image_text: Option<SourceImageText>,
}

pub(crate) fn hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
fn error(message: impl Into<String>) -> EpubError {
    EpubError::Cache(message.into())
}

/// Cooperative cancellation stops admission and drains requests already in flight.
#[derive(Clone, Default)]
pub struct ExtractionControl(std::sync::Arc<std::sync::atomic::AtomicBool>);
impl ExtractionControl {
    pub fn cancel(&self) {
        self.0.store(true, std::sync::atomic::Ordering::Relaxed);
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(std::sync::atomic::Ordering::Relaxed)
    }
}

impl ReviewRun {
    pub fn status(&self) -> &str {
        if !self.incomplete() {
            return "complete";
        }
        match self.execution_status.as_deref() {
            Some("running" | "stopping") => "interrupted",
            Some("cancelled") => "cancelled",
            Some("failed") => "failed",
            _ if self.charges.iter().any(|c| c.status == "pending") => "interrupted",
            _ if self.charges.iter().any(|c| c.status == "failed") => "failed",
            _ => "incomplete",
        }
    }

    pub fn inspect(bytes: &[u8], source: &str, model: &str) -> Result<Self, EpubError> {
        let chunks = crate::chunk_epub(bytes)?
            .into_iter()
            .enumerate()
            .map(|(i, source)| RunChunk {
                id: format!("chunk-{i:04}-{}", &hash(source.text.as_bytes())[..12]),
                source,
                output: None,
                error: None,
                cached: false,
                usage: Usage::default(),
                model: None,
                prompt_version: None,
                request_identity: None,
            })
            .collect();
        Ok(Self {
            execution_status: None,
            metadata: None,
            charges: vec![],
            version: RUN_VERSION,
            epub_sha256: hash(bytes),
            source: source.into(),
            model: model.into(),
            prompt_version: crate::cache::PROMPT_VERSION.into(),
            parent: None,
            chunks,
            documents: crate::source::inspect_source(bytes)?,
            recipes: vec![],
            parsed: serde_json::json!([]),
            reserved_usd: 0.0,
            image_text: None,
        })
    }

    /// Reuse only unchanged source chunks when starting a new configuration/run.
    pub fn inherit_outputs(&self, parent: &Self, parent_path: &Path) -> Result<Self, EpubError> {
        if self.epub_sha256 != parent.epub_sha256 {
            return Err(error("parent belongs to a different EPUB"));
        }
        let mut run = self.clone();
        run.parent = Some(parent_path.to_string_lossy().into_owned());
        run.reserved_usd = parent.reserved_usd;
        run.image_text = parent.image_text.clone();
        for chunk in &mut run.chunks {
            if let Some(old) = parent.chunks.iter().find(|old| {
                old.id == chunk.id
                    && old.source.text == chunk.source.text
                    && old.source.doc_path == chunk.source.doc_path
                    && old.source.title_hint == chunk.source.title_hint
            }) {
                let source = chunk.source.clone();
                *chunk = old.clone();
                chunk.source = source;
                if chunk.output.is_some() {
                    chunk.model.get_or_insert_with(|| parent.model.clone());
                    chunk
                        .prompt_version
                        .get_or_insert_with(|| parent.prompt_version.clone());
                }
            }
        }
        run.replay()?;
        Ok(run)
    }

    pub fn read(path: &Path) -> Result<Self, EpubError> {
        let run: Self =
            serde_json::from_slice(&std::fs::read(path).map_err(|e| error(e.to_string()))?)?;
        if run.version != RUN_VERSION {
            return Err(error(format!("unsupported run version {}", run.version)));
        }
        let mut ids = std::collections::HashSet::new();
        if run.chunks.iter().any(|c| !ids.insert(&c.id)) {
            return Err(error("duplicate chunk IDs"));
        }
        Ok(run)
    }

    /// Atomically checkpoint a run. The caller owns the destination and overwrite policy.
    pub fn save(&self, path: &Path) -> Result<(), EpubError> {
        let tmp = path.with_extension(format!("{}.tmp", std::process::id()));
        let result = (|| {
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&tmp)
                .map_err(|e| error(e.to_string()))?;
            file.write_all(&serde_json::to_vec_pretty(self)?)
                .map_err(|e| error(e.to_string()))?;
            file.sync_all().map_err(|e| error(e.to_string()))?;
            std::fs::rename(&tmp, path).map_err(|e| error(e.to_string()))
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(tmp);
        }
        result
    }

    /// Re-run assembly and the current ingredient parser without a transport or credentials.
    pub fn replay(&mut self) -> Result<(), EpubError> {
        self.recipes = crate::assemble_recipes(
            self.chunks
                .iter()
                .map(|c| (c.source.clone(), c.output.clone().unwrap_or_default()))
                .collect(),
            self.chunks
                .iter()
                .flat_map(|c| c.source.links.clone())
                .collect(),
            &self.source,
        );
        crate::source::enrich_from_source(&mut self.recipes, &self.documents);
        if let Some(text) = &self.image_text {
            apply_image_text(&mut self.recipes, &self.documents, &self.epub_sha256, text)?;
        }
        self.parsed = serde_json::to_value(
            self.recipes
                .iter()
                .map(CookbookRecipeExt::parse)
                .collect::<Vec<_>>(),
        )?;
        Ok(())
    }

    pub fn incomplete(&self) -> bool {
        self.chunks.is_empty() || self.chunks.iter().any(|c| c.output.is_none())
    }
}

/// Network is opt-in; a missing cache entry never silently initializes a backend.
#[derive(Debug, Clone, Default)]
pub struct RunOptions {
    pub allow_network: bool,
    pub refresh: bool,
    pub cache_dir: Option<PathBuf>,
    pub chunks: Vec<String>,
    pub budget_usd: f64,
}

/// Bounded calls reserve budget and checkpoint before requests and after each result.
pub async fn extract_run(
    run: &mut ReviewRun,
    options: &RunOptions,
    checkpoint: &Path,
) -> Result<(), EpubError> {
    extract_run_with_progress(run, options, checkpoint, |_| {}).await
}

/// Build a durable run using a caller-supplied extractor and the shared cache engine.
///
/// This is the fixture/custom-extractor path: the caller owns transport authorization
/// and cost policy. It does not construct the native network backend or apply its
/// reservation policy. Existing completed chunks and valid cache entries are reused;
/// newly produced outputs are cached, then the ordinary cache-only run engine owns
/// replay, completion metadata, progress, and checkpointing.
pub async fn extract_run_with_extractor<E: RecipeExtractor>(
    run: &mut ReviewRun,
    cache_dir: &Path,
    checkpoint: &Path,
    extractor: &E,
    progress: impl FnMut(&ReviewRun),
) -> Result<(), EpubError> {
    if run.prompt_version != crate::cache::PROMPT_VERSION {
        return Err(error(
            "prompt changed; create a new extraction run (offline replay remains available)",
        ));
    }
    if !extractor.model().is_empty() && extractor.model() != run.model {
        return Err(error("extractor model differs from the run model"));
    }
    for chunk in &run.chunks {
        let key = crate::cache::identity(&run.model, &chunk.source)?;
        if chunk.output.is_some() || crate::cache::read_entry(cache_dir, &key).is_some() {
            continue;
        }
        let output = extractor.extract(&chunk.source).await?;
        if output.truncated {
            return Err(error(format!(
                "truncated custom extractor output for {}",
                chunk.id
            )));
        }
        crate::cache::write_entry(cache_dir, &key, &output.recipes, Some(output.usage))?;
    }
    extract_run_with_progress(
        run,
        &RunOptions {
            cache_dir: Some(cache_dir.to_owned()),
            ..Default::default()
        },
        checkpoint,
        progress,
    )
    .await
}

/// Same execution engine with frontend-owned progress reporting. Reports the
/// validated initial state and each checkpoint, including cache-only completion.
pub async fn extract_run_with_progress(
    run: &mut ReviewRun,
    options: &RunOptions,
    checkpoint: &Path,
    progress: impl FnMut(&ReviewRun),
) -> Result<(), EpubError> {
    extract_run_controlled(
        run,
        options,
        checkpoint,
        &ExtractionControl::default(),
        progress,
    )
    .await
}

pub async fn extract_run_controlled(
    run: &mut ReviewRun,
    options: &RunOptions,
    checkpoint: &Path,
    control: &ExtractionControl,
    progress: impl FnMut(&ReviewRun),
) -> Result<(), EpubError> {
    let model = run.model.clone();
    let source = run.source.clone();
    extract_run_with_transport(
        run,
        options,
        checkpoint,
        control,
        progress,
        || {
            crate::backend::Backend::from_env(
                &crate::Options {
                    model: Some(model),
                    ..Default::default()
                },
                &source,
            )
        },
        |transport, chunk| transport.take_calls(chunk),
    )
    .await
}

async fn extract_run_with_transport<E: RecipeExtractor>(
    run: &mut ReviewRun,
    options: &RunOptions,
    checkpoint: &Path,
    control: &ExtractionControl,
    mut progress: impl FnMut(&ReviewRun),
    create_transport: impl FnOnce() -> Result<E, EpubError>,
    take_calls: impl Fn(&E, &Chunk) -> Vec<store::CallRecord>,
) -> Result<(), EpubError> {
    preflight::plan(run, options)?;
    progress(run);
    let cache_dir = options
        .cache_dir
        .clone()
        .unwrap_or_else(crate::cache::default_dir);
    let mut pending = Vec::new();
    for i in 0..run.chunks.len() {
        if !options.chunks.is_empty() && !options.chunks.contains(&run.chunks[i].id) {
            continue;
        }
        if run.chunks[i].output.is_some() && !options.refresh {
            continue;
        }
        let source = &run.chunks[i].source;
        let key = crate::cache::identity(&run.model, source)?;
        if !options.refresh
            && let Some(output) = crate::cache::read_entry(&cache_dir, &key)
        {
            run.chunks[i].request_identity = Some(key.clone());
            run.chunks[i].model = Some(run.model.clone());
            run.chunks[i].prompt_version = Some(run.prompt_version.clone());
            run.chunks[i].output = Some(output);
            run.chunks[i].cached = true;
            run.chunks[i].error = None;
        } else if options.allow_network {
            pending.push((i, key));
        } else {
            run.chunks[i].error = Some("cache miss (network disabled)".into());
        }
    }
    run.replay()?;
    if let Some(metadata) = &mut run.metadata {
        metadata.updated_at = Some(store::now());
    }
    run.save(checkpoint)?;
    progress(run);
    if pending.is_empty() || control.is_cancelled() {
        return Ok(());
    }
    // Validate/reserve before constructing a transport. At most four requests
    // are in flight; every reservation is durable before its request begins.
    let reservation = |source: &Chunk| preflight::reservation(&run.model, source);
    let reservations: Vec<_> = pending
        .iter()
        .map(|(i, _)| reservation(&run.chunks[*i].source))
        .collect::<Result<_, _>>()?;
    if reservations
        .iter()
        .all(|reserved| run.reserved_usd + reserved > options.budget_usd)
    {
        for (i, _) in pending {
            run.chunks[i].error = Some("budget exhausted before request".into());
        }
        run.save(checkpoint)?;
        progress(run);
        return Ok(());
    }
    let transport = create_transport()?;
    use futures::{StreamExt, stream::FuturesUnordered};
    let mut waiting: std::collections::VecDeque<_> =
        pending.into_iter().zip(reservations).collect();
    let mut results = FuturesUnordered::new();
    loop {
        // Revisit budget-blocked work after each completion: resolved usage may
        // release enough of an earlier reservation to admit another request.
        if control.is_cancelled() {
            run.execution_status = Some("stopping".into());
        }
        let candidates = if control.is_cancelled() {
            0
        } else {
            waiting.len()
        };
        for _ in 0..candidates {
            if results.len() >= 4 || control.is_cancelled() {
                break;
            }
            let Some(((i, key), reserved)) = waiting.pop_front() else {
                break;
            };
            if run.reserved_usd + reserved > options.budget_usd {
                run.chunks[i].error = Some("budget exhausted before request".into());
                waiting.push_back(((i, key), reserved));
                continue;
            }
            run.reserved_usd += reserved;
            run.charges.push(store::Charge {
                attempts: vec![],
                chunk: run.chunks[i].id.clone(),
                model: run.model.clone(),
                started_at: store::now(),
                reservation_usd: reserved,
                usage: None,
                estimated_usd: None,
                input_rate: crate::accounting::price_per_mtok(&run.model).map(|r| r.0),
                output_rate: crate::accounting::price_per_mtok(&run.model).map(|r| r.1),
                cache_read_rate: crate::accounting::cost_for_usage(
                    &run.model,
                    &Usage {
                        cache_read_input_tokens: 1_000_000,
                        ..Usage::default()
                    },
                ),
                cache_creation_rate: crate::accounting::cost_for_usage(
                    &run.model,
                    &Usage {
                        cache_creation_input_tokens: 1_000_000,
                        ..Usage::default()
                    },
                ),
                rate_date: "2026-09-09".into(),
                rate_source: crate::models::pricing_source(&run.model).map(str::to_owned),
                status: "pending".into(),
            });
            if run.chunks[i].model.as_deref() != Some(run.model.as_str()) {
                run.chunks[i].usage = Usage::default();
            }
            run.chunks[i].request_identity = Some(key.clone());
            run.chunks[i].model = Some(run.model.clone());
            run.chunks[i].prompt_version = Some(run.prompt_version.clone());
            run.chunks[i].output = None;
            if options.refresh {
                run.chunks[i].usage = Usage::default();
            }
            run.chunks[i].error =
                Some("request pending; reservation retained if interrupted".into());
            // Persist before the future can be polled and send a request.
            run.replay()?;
            if let Some(metadata) = &mut run.metadata {
                metadata.updated_at = Some(store::now());
            }
            run.save(checkpoint)?;
            progress(run);
            let source = run.chunks[i].source.clone();
            let transport = &transport;
            results
                .push(async move { (i, key, reserved, transport.extract_detailed(&source).await) });
        }
        let next = tokio::select! {
            result = results.next() => result,
            _ = tokio::time::sleep(std::time::Duration::from_secs(1)) => {
                progress(run);
                continue;
            }
        };
        let Some((i, key, reserved, result)) = next else {
            break;
        };
        if let Some(charge) = run
            .charges
            .iter_mut()
            .rev()
            .find(|c| c.chunk == run.chunks[i].id && c.status == "pending")
        {
            charge.attempts = take_calls(&transport, &run.chunks[i].source);
            let (usage, status) = match &result {
                Ok(o) => (
                    &o.usage,
                    if o.truncated {
                        "truncated"
                    } else {
                        "completed"
                    },
                ),
                Err(e) => (&e.usage, "failed"),
            };
            charge.status = status.into();
            if *usage != Usage::default() {
                charge.usage = Some(usage.clone());
                charge.estimated_usd = crate::accounting::cost_for_usage(&charge.model, usage);
            }
        }
        match result {
            Ok(outcome) => {
                if let Some(actual) = crate::accounting::cost_for_usage(&run.model, &outcome.usage)
                    && outcome.usage != Usage::default()
                    && run
                        .charges
                        .iter()
                        .rev()
                        .find(|c| c.chunk == run.chunks[i].id)
                        .is_none_or(|c| c.attempts.iter().all(|a| a.usage.is_some()))
                {
                    run.reserved_usd += actual - reserved;
                }
                run.chunks[i].usage.add(&outcome.usage);
                run.chunks[i].cached = false;
                if outcome.truncated {
                    run.chunks[i].error = Some("truncated response".into());
                } else {
                    std::fs::create_dir_all(&cache_dir).map_err(|e| error(e.to_string()))?;
                    crate::cache::write_entry(
                        &cache_dir,
                        &key,
                        &outcome.recipes,
                        Some(outcome.usage.clone()),
                    )?;
                    run.chunks[i].output = Some(outcome.recipes);
                    run.chunks[i].error = None;
                }
            }
            Err(failure) => {
                run.chunks[i].usage.add(&failure.usage);
                run.chunks[i].error = Some(failure.error.to_string());
            }
        }
        run.replay()?;
        if let Some(metadata) = &mut run.metadata {
            metadata.updated_at = Some(store::now());
        }
        run.save(checkpoint)?;
        progress(run);
    }
    run.replay()?;
    if let Some(metadata) = &mut run.metadata {
        metadata.updated_at = Some(store::now());
    }
    run.save(checkpoint)?;
    progress(run);
    Ok(())
}

/// Exact, source-authored recipe expectations; never derived from parser confidence.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExpectedIngredient {
    pub id: String,
    pub recipe_index: usize,
    pub section_index: usize,
    pub line_index: usize,
    pub input: String,
    pub name: String,
    pub amounts: Vec<ingredient::unit::Measure>,
    pub modifier: Option<String>,
    pub optional: bool,
    pub usage: ingredient::IngredientUsage,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Expectations {
    pub epub_sha256: String,
    pub recipes: Vec<CookbookRecipe>,
    #[serde(default)]
    pub ingredients: Vec<ExpectedIngredient>,
}

pub fn evaluate(run: &ReviewRun, expected: &Expectations) -> Result<serde_json::Value, EpubError> {
    if run.epub_sha256 != expected.epub_sha256 {
        return Err(error("expectations belong to a different EPUB"));
    }
    let mut mismatches = Vec::new();
    fn compare_fields(
        path: &str,
        want: &serde_json::Value,
        got: &serde_json::Value,
        rows: &mut Vec<serde_json::Value>,
    ) {
        if want == got {
            return;
        }
        if let (Some(w), Some(g)) = (want.as_object(), got.as_object()) {
            let keys: std::collections::BTreeSet<_> = w.keys().chain(g.keys()).collect();
            for key in keys {
                compare_fields(&format!("{path}/{key}"), &want[key], &got[key], rows);
            }
        } else {
            rows.push(serde_json::json!({"path":path,"want":want,"got":got}));
        }
    }
    let count = run.recipes.len().max(expected.recipes.len());
    for i in 0..count {
        let got = run
            .recipes
            .get(i)
            .map(serde_json::to_value)
            .transpose()?
            .unwrap_or_default();
        let want = expected
            .recipes
            .get(i)
            .map(serde_json::to_value)
            .transpose()?
            .unwrap_or_default();
        compare_fields(&format!("recipes/{i}"), &want, &got, &mut mismatches);
    }
    let ingredients = evaluate_ingredients(run, &expected.ingredients)?;
    Ok(
        serde_json::json!({"complete": !run.incomplete(), "expected_recipes":expected.recipes.len(),
        "actual_recipes":run.recipes.len(), "mismatches":mismatches,
        "ingredients":ingredients}),
    )
}

pub fn evaluate_ingredients(
    run: &ReviewRun,
    labels: &[ExpectedIngredient],
) -> Result<serde_json::Value, EpubError> {
    let mut ingredient_rows = Vec::new();
    let mut per_field = [0usize; 5];
    let mut exact = 0;
    let mut ids = std::collections::HashSet::new();
    for label in labels {
        if !ids.insert(&label.id) {
            return Err(error("duplicate ingredient expectation ID"));
        }
        let raw = run
            .recipes
            .get(label.recipe_index)
            .and_then(|r| r.sections.get(label.section_index))
            .and_then(|s| s.ingredients.get(label.line_index));
        let parsed = run
            .parsed
            .get(label.recipe_index)
            .and_then(|r| r.get("sections"))
            .and_then(|s| s.get(label.section_index))
            .and_then(|s| s.get("ingredients"))
            .and_then(|i| i.get(label.line_index));
        let got: Option<ingredient::ingredient::Ingredient> = parsed
            .map(|p| serde_json::from_value(p.clone()))
            .transpose()?;
        let source_matches = raw.is_some_and(|raw| normalized(raw) == normalized(&label.input));
        let fields = got
            .as_ref()
            .map(|got| {
                [
                    got.name == label.name,
                    got.amounts == label.amounts,
                    got.modifier == label.modifier,
                    got.optional == label.optional,
                    got.usage == label.usage,
                ]
            })
            .unwrap_or([false; 5])
            .map(|ok| ok && source_matches);
        for (i, ok) in fields.iter().enumerate() {
            per_field[i] += usize::from(*ok);
        }
        exact += usize::from(fields.iter().all(|ok| *ok));
        ingredient_rows.push(serde_json::json!({"id":label.id,"input":label.input,"source_matches":source_matches,"fields":fields,"want":label,"got":got}));
    }
    Ok(
        serde_json::json!({"available":!labels.is_empty(),"rows":ingredient_rows,"exact":exact,"per_field":per_field,"field_names":["name","amounts","modifier","optional","usage"]}),
    )
}

pub fn diff(before: &ReviewRun, after: &ReviewRun) -> Result<serde_json::Value, EpubError> {
    if before.epub_sha256 != after.epub_sha256 {
        return Err(error("cannot compare different EPUBs"));
    }
    let group = |run: &ReviewRun| {
        let mut docs: std::collections::BTreeMap<String, Vec<serde_json::Value>> =
            Default::default();
        for (i, recipe) in run.recipes.iter().enumerate() {
            docs.entry(recipe.url.clone())
                .or_default()
                .push(serde_json::json!({"recipe":recipe,"parsed":run.parsed.get(i)}));
        }
        docs
    };
    let old = group(before);
    let new = group(after);
    let keys: std::collections::BTreeSet<_> = old.keys().chain(new.keys()).collect();
    let changed: Vec<_> = keys
        .into_iter()
        .filter(|key| old.get(*key) != new.get(*key))
        .map(|key| serde_json::json!({"source":key,"before":old.get(key),"after":new.get(key)}))
        .collect();
    Ok(
        serde_json::json!({"before_status":before.status(),"after_status":after.status(),"before_model":before.model,"after_model":after.model,"before_cost":store::summary(before,Path::new("")).new_spend_usd,"after_cost":store::summary(after,Path::new("")).new_spend_usd,"before_incomplete":before.incomplete(),"after_incomplete":after.incomplete(),
        "extraction_changed":before.recipes != after.recipes, "parsing_changed":before.parsed != after.parsed,
        "before_recipes":before.recipes.len(),"after_recipes":after.recipes.len(),
        "before_prompt":before.prompt_version,"after_prompt":after.prompt_version,"changed_sources":changed}),
    )
}

/// Load review images only from bytes supplied by the caller, with source identity checked.
pub fn source_images(run: &ReviewRun, bytes: &[u8]) -> Result<Vec<(String, Vec<u8>)>, EpubError> {
    if hash(bytes) != run.epub_sha256 {
        return Err(error("image source is a different EPUB"));
    }
    let mut doc = epub::doc::EpubDoc::from_reader(std::io::Cursor::new(bytes))
        .map_err(|e| EpubError::Open(e.to_string()))?;
    let paths: std::collections::BTreeSet<_> = run
        .documents
        .iter()
        .flat_map(|d| d.images.iter().map(|i| &i.path))
        .collect();
    Ok(paths
        .into_iter()
        .filter_map(|p| doc.get_resource_by_path(p).map(|bytes| (p.clone(), bytes)))
        .collect())
}

/// Source-markup coverage expectations can be authored before an extraction exists.
/// They complement exact recipe/ingredient labels; coverage is not a semantic accuracy score.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceExpectations {
    pub epub_sha256: String,
    pub recipes: Vec<SourceRecipe>,
    #[serde(default)]
    pub ingredients: Vec<ExpectedIngredient>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceRecipe {
    pub document: String,
    pub title: String,
    pub description: String,
    #[serde(rename = "yield")]
    pub recipe_yield: Option<String>,
    pub sections: Vec<SourceSection>,
    pub methods: Vec<String>,
    #[serde(default)]
    pub notes: Vec<String>,
    #[serde(default)]
    pub check_image: bool,
    #[serde(default)]
    pub image: Option<String>,
    #[serde(default)]
    pub references: Option<Vec<SourceReference>>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceReference {
    pub title: String,
    pub line: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceSection {
    pub name: Option<String>,
    pub ingredients: Vec<String>,
}

fn normalized(text: &str) -> String {
    // Inline XHTML boundaries can insert spaces next to punctuation. Match the
    // extraction contract's permitted whitespace normalization, not wording.
    crate::extractor::normalize_source_whitespace(text)
}

pub fn evaluate_source(
    run: &ReviewRun,
    expected: &SourceExpectations,
) -> Result<serde_json::Value, EpubError> {
    if run.epub_sha256 != expected.epub_sha256 {
        return Err(error("expectations belong to a different EPUB"));
    }
    let mut rows = Vec::new();
    let mut matched = std::collections::HashSet::new();
    for source in &expected.recipes {
        let candidates: Vec<_> = run
            .recipes
            .iter()
            .enumerate()
            .filter(|(_, r)| {
                r.url
                    .rsplit_once('#')
                    .is_some_and(|(_, d)| d == source.document)
                    && normalized(&r.meta.title) == normalized(&source.title)
            })
            .collect();
        let mut issues = Vec::new();
        if candidates.len() != 1 {
            issues.push(serde_json::json!({"field":"recipe_identity","expected":1,"actual":candidates.len()}));
        }
        if let [(index, recipe)] = candidates.as_slice() {
            matched.insert(*index);
            let expected_lines: Vec<_> = source
                .sections
                .iter()
                .flat_map(|s| &s.ingredients)
                .map(|s| normalized(s))
                .collect();
            let actual_lines: Vec<_> = recipe
                .sections
                .iter()
                .flat_map(|s| &s.ingredients)
                .map(|s| normalized(s))
                .collect();
            if expected_lines != actual_lines {
                issues.push(serde_json::json!({"field":"ingredients","expected":source.sections.iter().flat_map(|s| &s.ingredients).collect::<Vec<_>>(),"actual":recipe.sections.iter().flat_map(|s| &s.ingredients).collect::<Vec<_>>()}));
            }
            let expected_groups: Vec<_> = source
                .sections
                .iter()
                .filter(|s| !s.ingredients.is_empty())
                .map(|s| (s.name.as_deref().map(normalized), s.ingredients.len()))
                .collect();
            let actual_groups: Vec<_> = recipe
                .sections
                .iter()
                .filter(|s| !s.ingredients.is_empty())
                .map(|s| (s.name.as_deref().map(normalized), s.ingredients.len()))
                .collect();
            if expected_groups != actual_groups {
                issues.push(serde_json::json!({"field":"ingredient_groups","expected":expected_groups,"actual":actual_groups}));
            }
            let content: Vec<_> = recipe
                .sections
                .iter()
                .flat_map(|s| &s.instructions)
                .chain(&recipe.meta.notes)
                .map(|s| normalized(s))
                .collect();
            for (i, paragraph) in source.methods.iter().enumerate() {
                let wanted = normalized(paragraph);
                if !content.iter().any(|text| text.contains(&wanted)) {
                    issues.push(serde_json::json!({"field":"method_or_note","paragraph":i,"expected":paragraph}));
                }
            }
            let method_positions: Vec<_> = recipe
                .sections
                .iter()
                .flat_map(|s| &s.instructions)
                .filter_map(|step| {
                    source
                        .methods
                        .iter()
                        .position(|m| normalized(m) == normalized(step))
                })
                .collect();
            if method_positions.windows(2).any(|pair| pair[0] >= pair[1]) {
                issues.push(
                    serde_json::json!({"field":"method_order","source_positions":method_positions}),
                );
            }
            if source.check_image
                && recipe.image.as_ref().map(|i| i.path.as_str()) != source.image.as_deref()
            {
                issues.push(serde_json::json!({"field":"image","expected":source.image,"actual":recipe.image}));
            }
            if let Some(references) = &source.references {
                let want: std::collections::BTreeSet<_> = references
                    .iter()
                    .map(|r| (normalized(&r.title), normalized(&r.line)))
                    .collect();
                let got: std::collections::BTreeSet<_> = recipe
                    .references
                    .iter()
                    .map(|r| (normalized(&r.title), normalized(&r.line)))
                    .collect();
                if want != got {
                    issues.push(serde_json::json!({"field":"references","expected":references,"actual":recipe.references}));
                }
            }
            for (i, note) in source.notes.iter().enumerate() {
                let wanted = normalized(note);
                if !recipe
                    .meta
                    .notes
                    .iter()
                    .any(|text| normalized(text).contains(&wanted))
                {
                    issues.push(serde_json::json!({"field":"note","paragraph":i,"expected":note}));
                }
            }
            if normalized(recipe.meta.description.as_deref().unwrap_or(""))
                != normalized(&source.description)
            {
                issues.push(serde_json::json!({"field":"description","expected":source.description,"actual":recipe.meta.description}));
            }
            if recipe.meta.recipe_yield.as_deref().map(normalized)
                != source.recipe_yield.as_deref().map(normalized)
            {
                issues.push(serde_json::json!({"field":"yield","expected":source.recipe_yield,"actual":recipe.meta.recipe_yield}));
            }
        }
        rows.push(
            serde_json::json!({"source":source.document,"title":source.title,"issues":issues}),
        );
    }
    let extra: Vec<_> = run
        .recipes
        .iter()
        .enumerate()
        .filter(|(i, _)| !matched.contains(i))
        .map(|(i, r)| serde_json::json!({"index":i,"title":r.meta.title,"source":r.url}))
        .collect();
    let failing = rows
        .iter()
        .filter(|r| r["issues"].as_array().is_some_and(|a| !a.is_empty()))
        .count();
    Ok(
        serde_json::json!({"kind":"source_coverage","complete":!run.incomplete(),"expected_recipes":expected.recipes.len(),"matched_recipes":matched.len(),"passing_recipes":expected.recipes.len()-failing,"extra_recipes":extra,"rows":rows,"ingredients":evaluate_ingredients(run, &expected.ingredients)?}),
    )
}

pub fn evaluate_document(
    run: &ReviewRun,
    document: serde_json::Value,
) -> Result<serde_json::Value, EpubError> {
    if document.get("kind").and_then(|v| v.as_str()) == Some("source_coverage") {
        evaluate_source(run, &serde_json::from_value(document)?)
    } else {
        evaluate(run, &serde_json::from_value(document)?)
    }
}

pub fn evaluation_failed(value: &serde_json::Value) -> bool {
    if value["kind"] == "source_coverage" {
        value["passing_recipes"] != value["expected_recipes"]
            || value["ingredients"]["rows"].as_array().is_some_and(|rows| {
                rows.len() as u64 != value["ingredients"]["exact"].as_u64().unwrap_or(0)
            })
            || value["extra_recipes"]
                .as_array()
                .is_some_and(|a| !a.is_empty())
    } else {
        value["mismatches"]
            .as_array()
            .is_some_and(|a| !a.is_empty())
            || value["ingredients"]["rows"].as_array().is_some_and(|a| {
                a.len() as u64 != value["ingredients"]["exact"].as_u64().unwrap_or(0)
            })
    }
}

/// Source navigation for either expectation format, including ingredient-only
/// failures and extra recipes. Frontends share this mapping with headless review.
pub fn discrepancies_by_source(
    run: &ReviewRun,
    evaluation: &serde_json::Value,
) -> std::collections::BTreeMap<String, Vec<serde_json::Value>> {
    let mut result = std::collections::BTreeMap::<String, Vec<serde_json::Value>>::new();
    let source_at = |index: usize| {
        run.recipes
            .get(index)
            .and_then(|r| r.url.rsplit_once('#').map(|(_, p)| p.to_owned()))
    };
    for row in evaluation["rows"].as_array().into_iter().flatten() {
        if let (Some(source), Some(issues)) = (row["source"].as_str(), row["issues"].as_array())
            && !issues.is_empty()
        {
            result
                .entry(source.into())
                .or_default()
                .extend(issues.clone());
        }
    }
    for row in evaluation["extra_recipes"].as_array().into_iter().flatten() {
        if let Some(source) = row["source"]
            .as_str()
            .and_then(|url| url.rsplit_once('#').map(|(_, p)| p))
        {
            result
                .entry(source.into())
                .or_default()
                .push(serde_json::json!({"field":"extra_recipe","detail":row}));
        }
    }
    for row in evaluation["mismatches"].as_array().into_iter().flatten() {
        let source = row["path"]
            .as_str()
            .and_then(|p| p.split('/').nth(1))
            .and_then(|i| i.parse().ok())
            .and_then(source_at)
            .or_else(|| {
                row["want"]["url"]
                    .as_str()
                    .and_then(|url| url.rsplit_once('#').map(|(_, p)| p.to_owned()))
            });
        if let Some(source) = source {
            result.entry(source).or_default().push(row.clone());
        }
    }
    for row in evaluation["ingredients"]["rows"]
        .as_array()
        .into_iter()
        .flatten()
    {
        if row["fields"]
            .as_array()
            .is_some_and(|fields| fields.iter().any(|ok| ok != true))
            && let Some(source) = row["want"]["recipe_index"]
                .as_u64()
                .and_then(|i| source_at(i as usize))
        {
            result
                .entry(source)
                .or_default()
                .push(serde_json::json!({"field":"ingredient","detail":row}));
        }
    }
    result
}

/// Human decisions live separately from reproducible extraction outputs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewDecision {
    pub status: String,
    pub note: String,
}
impl Default for ReviewDecision {
    fn default() -> Self {
        Self {
            status: "Unreviewed".into(),
            note: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewDecisions {
    pub version: u32,
    pub epub_sha256: String,
    pub documents: std::collections::BTreeMap<String, ReviewDecision>,
}
impl ReviewDecisions {
    pub fn read(path: &Path, run: &ReviewRun) -> Result<Self, EpubError> {
        let decisions = if path.exists() {
            serde_json::from_slice::<Self>(&std::fs::read(path).map_err(|e| error(e.to_string()))?)?
        } else {
            Self {
                version: 1,
                epub_sha256: run.epub_sha256.clone(),
                documents: Default::default(),
            }
        };
        if decisions.version != 1 || decisions.epub_sha256 != run.epub_sha256 {
            return Err(error(
                "review decisions have an incompatible version or source",
            ));
        }
        Ok(decisions)
    }
    pub fn save(&self, path: &Path) -> Result<(), EpubError> {
        let tmp = path.with_extension(format!("{}.tmp", std::process::id()));
        std::fs::write(&tmp, serde_json::to_vec_pretty(self)?).map_err(|e| error(e.to_string()))?;
        std::fs::rename(tmp, path).map_err(|e| error(e.to_string()))
    }
}

/// Compare every retained source block against result fields without treating
/// unassigned prose or image adjacency as an automatic recipe association.
pub fn source_audit(run: &ReviewRun) -> serde_json::Value {
    let norm = crate::extractor::normalize_source_whitespace;
    let documents: Vec<_> = run.documents.iter().map(|doc| {
        let mut fields = Vec::new();
        for (ri, recipe) in run.recipes.iter().enumerate().filter(|(_,r)|r.url.rsplit_once('#').is_some_and(|(_,p)|p==doc.path)) {
            fields.push((format!("recipes/{ri}/meta/title"), recipe.meta.title.clone()));
            if let Some(text) = &recipe.meta.description { fields.push((format!("recipes/{ri}/meta/description"),text.clone())); }
            if let Some(text) = &recipe.meta.recipe_yield { fields.push((format!("recipes/{ri}/meta/recipe_yield"),text.clone())); }
            for (i,text) in recipe.meta.notes.iter().enumerate() { fields.push((format!("recipes/{ri}/meta/notes/{i}"),text.clone())); }
            for (si,section) in recipe.sections.iter().enumerate() {
                if let Some(name) = &section.name { fields.push((format!("recipes/{ri}/sections/{si}/name"),name.clone())); }
                for (kind, lines) in [("ingredients",&section.ingredients),("instructions",&section.instructions)] {
                    for (i,text) in lines.iter().enumerate() { fields.push((format!("recipes/{ri}/sections/{si}/{kind}/{i}"),text.clone())); }
                }
            }
        }
        let blocks: Vec<_> = doc.blocks.iter().map(|block| {
            let needle = norm(&block.text);
            let matches: Vec<_> = fields.iter().filter(|(_,text)|!needle.is_empty() && norm(text).contains(&needle)).map(|(path,_)|path).collect();
            serde_json::json!({"id":block.id,"text":block.text,"fields":matches,"links":block.links})
        }).collect();
        serde_json::json!({"source":doc.path,"blocks":blocks,"images":doc.images})
    }).collect();
    serde_json::json!({"epub_sha256":run.epub_sha256,"quality_issues":quality::issues(run),"documents":documents})
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceImageText {
    pub epub_sha256: String,
    /// For example: visual transcription, or an identified OCR engine/version.
    pub method: String,
    pub images: Vec<ImageText>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageText {
    pub path: String,
    /// One printed recipe caption per entry; empty means no recipe caption.
    pub captions: Vec<String>,
}
fn apply_image_text(
    recipes: &mut [CookbookRecipe],
    documents: &[SourceDocument],
    source_hash: &str,
    text: &SourceImageText,
) -> Result<(), EpubError> {
    if text.epub_sha256 != source_hash {
        return Err(error("image text belongs to another EPUB"));
    }
    let images: std::collections::BTreeMap<_, _> = documents
        .iter()
        .flat_map(|d| d.images.iter())
        .map(|i| (i.path.as_str(), i))
        .collect();
    let mut seen = std::collections::HashSet::new();
    for annotation in &text.images {
        if !images.contains_key(annotation.path.as_str()) || !seen.insert(&annotation.path) {
            return Err(error("unknown or duplicated source image text"));
        }
    }
    for recipe in recipes {
        let title = crate::normalize_title(&recipe.meta.title);
        let matches: Vec<_> = text
            .images
            .iter()
            .filter(|image| {
                image.captions.iter().any(|caption| {
                    let caption = crate::normalize_title(caption);
                    !caption.is_empty()
                        && (title == caption || title.ends_with(&format!(" {caption}")))
                })
            })
            .collect();
        if let [image] = matches.as_slice() {
            recipe.image = images.get(image.path.as_str()).map(|i| (*i).clone());
        } else if recipe
            .image
            .as_ref()
            .is_some_and(|i| seen.contains(&i.path))
        {
            recipe.image = None;
        }
    }
    Ok(())
}

#[cfg(test)]
mod pool_tests {
    use super::*;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    struct Probe {
        checkpoint: PathBuf,
        started: AtomicUsize,
        active: AtomicUsize,
        peak: AtomicUsize,
        fifth: tokio::sync::Notify,
        hold_first: bool,
        cancel: Option<ExtractionControl>,
    }
    impl RecipeExtractor for Arc<Probe> {
        async fn extract(&self, chunk: &Chunk) -> Result<crate::ChunkOutcome, EpubError> {
            let saved = ReviewRun::read(&self.checkpoint)?;
            assert!(
                saved
                    .charges
                    .iter()
                    .any(|c| c.chunk == chunk.doc_path && c.status == "pending")
            );
            let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
            self.peak.fetch_max(active, Ordering::SeqCst);
            let started = self.started.fetch_add(1, Ordering::SeqCst) + 1;
            if started == 1
                && let Some(control) = &self.cancel
            {
                control.cancel();
            }
            if started == 5 {
                self.fifth.notify_one();
            }
            if started == 1 && self.hold_first {
                self.fifth.notified().await;
            } else {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
            self.active.fetch_sub(1, Ordering::SeqCst);
            Ok(crate::ChunkOutcome {
                recipes: vec![],
                usage: Usage {
                    input_tokens: 1,
                    output_tokens: 1,
                    ..Default::default()
                },
                cached: false,
                truncated: false,
            })
        }
    }

    #[tokio::test]
    async fn exhausted_budget_does_not_construct_transport()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = std::env::temp_dir().join(store::new_id());
        std::fs::create_dir_all(&root)?;
        let checkpoint = root.join("run.json");
        let mut run = ReviewRun::inspect(
            &recipe_epub_fixtures::cookbook_epub()?,
            "test",
            "gemini-2.5-flash",
        )?;
        extract_run_with_transport::<Arc<Probe>>(
            &mut run,
            &RunOptions {
                allow_network: true,
                budget_usd: 0.0,
                cache_dir: Some(root.join("cache")),
                ..Default::default()
            },
            &checkpoint,
            &ExtractionControl::default(),
            |_| {},
            || Err(EpubError::MissingBaseUrl),
            |_, _| vec![],
        )
        .await?;
        assert!(run.charges.is_empty());
        assert_eq!(run.reserved_usd, 0.0);
        assert!(
            ReviewRun::read(&checkpoint)?
                .chunks
                .iter()
                .all(|c| c.error.as_deref() == Some("budget exhausted before request"))
        );
        std::fs::remove_dir_all(root)?;
        Ok(())
    }

    #[tokio::test]
    async fn replenishes_before_slow_request_finishes_and_reconsiders_budget()
    -> Result<(), Box<dyn std::error::Error>> {
        for mode in 0..3 {
            let hold_first = mode == 0;
            let control = ExtractionControl::default();
            let root = std::env::temp_dir().join(store::new_id());
            std::fs::create_dir_all(&root)?;
            let checkpoint = root.join("run.json");
            let mut run = ReviewRun::inspect(
                &recipe_epub_fixtures::cookbook_epub()?,
                "test",
                "gemini-2.5-flash",
            )?;
            let template = run.chunks[0].clone();
            run.chunks = (0..6)
                .map(|i| {
                    let mut c = template.clone();
                    c.id = format!("chunk-{i}");
                    c.source.doc_path = c.id.clone();
                    c.source.text.push_str(&format!("\nSample {i}"));
                    c
                })
                .collect();
            let reservation = preflight::reservation(&run.model, &run.chunks[0].source)?;
            let options = RunOptions {
                allow_network: true,
                budget_usd: if mode != 1 { 10.0 } else { reservation * 1.1 },
                cache_dir: Some(root.join("cache")),
                ..Default::default()
            };
            let probe = Arc::new(Probe {
                checkpoint: checkpoint.clone(),
                started: AtomicUsize::new(0),
                active: AtomicUsize::new(0),
                peak: AtomicUsize::new(0),
                fifth: tokio::sync::Notify::new(),
                hold_first,
                cancel: (mode == 2).then(|| control.clone()),
            });
            tokio::time::timeout(
                std::time::Duration::from_secs(3),
                extract_run_with_transport(
                    &mut run,
                    &options,
                    &checkpoint,
                    &control,
                    |_| {},
                    || Ok(probe.clone()),
                    |_, _| vec![],
                ),
            )
            .await??;
            assert_eq!(
                probe.started.load(Ordering::SeqCst),
                if mode == 2 { 4 } else { 6 }
            );
            assert_eq!(
                probe.peak.load(Ordering::SeqCst),
                if mode != 1 { 4 } else { 1 }
            );
            assert_eq!(
                run.chunks.iter().filter(|c| c.output.is_some()).count(),
                if mode == 2 { 4 } else { 6 }
            );
            if mode == 2 {
                assert!(run.charges.iter().all(|c| c.status != "pending"));
                extract_run_with_transport(
                    &mut run,
                    &options,
                    &checkpoint,
                    &ExtractionControl::default(),
                    |_| {},
                    || Ok(probe.clone()),
                    |_, _| vec![],
                )
                .await?;
                assert_eq!(probe.started.load(Ordering::SeqCst), 6);
                assert!(run.chunks.iter().all(|c| c.output.is_some()));
            }
            assert!(run.reserved_usd <= options.budget_usd);
            assert_eq!(ReviewRun::read(&checkpoint)?.charges.len(), 6);
            std::fs::remove_dir_all(root)?;
        }
        Ok(())
    }
}
