//! File-backed workflows used by both maintainer tools. No terminal or UI policy.
use super::{ReviewRun, RunOptions, extract_run_controlled};
use crate::EpubError;
use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum WorkflowError {
    #[error("{0}")]
    InvalidRequest(&'static str),
    #[error("output already exists; resume it or choose a new run path: {0}")]
    OutputExists(PathBuf),
    #[error("cannot resume a missing run: {0}")]
    MissingRun(PathBuf),
    #[error("{0}")]
    SourceMismatch(&'static str),
    #[error("cannot access {}: {source}", path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error(transparent)]
    Run(#[from] EpubError),
    #[error("{source}. Any completed work is saved at {}", path.display())]
    Execution {
        path: PathBuf,
        #[source]
        source: EpubError,
    },
    #[error("cannot read image captions: {0}")]
    ImageText(#[from] serde_json::Error),
}

/// Options shared by the desktop and CLI, including validation independent of Clap.
#[derive(Debug, Clone)]
pub struct ExtractionRequest {
    pub book: PathBuf,
    pub out: PathBuf,
    pub model: String,
    pub resume: bool,
    pub from: Option<PathBuf>,
    pub options: RunOptions,
}

#[derive(Debug, Clone)]
pub struct ReplayRequest {
    pub run: PathBuf,
    pub out: PathBuf,
    pub source: Option<PathBuf>,
    pub image_text: Option<PathBuf>,
}

/// The canonical output identity and durable result. Callers choose their own
/// presentation and map `run.incomplete()` to UI state or an exit status.
pub struct RunOutcome {
    pub path: PathBuf,
    pub run: ReviewRun,
}

#[derive(Debug, Clone, Copy)]
pub struct RunProgress {
    pub active: usize,
    pub failed: usize,
    pub elapsed_seconds: u64,
    pub estimated_usd: Option<f64>,
    pub stopping: bool,
    pub completed: usize,
    pub total: usize,
    pub recipes: usize,
    pub reserved_usd: f64,
}

fn read(path: &Path) -> Result<Vec<u8>, WorkflowError> {
    std::fs::read(path).map_err(|source| WorkflowError::Io {
        path: path.into(),
        source,
    })
}

fn canonical(path: &Path) -> Result<PathBuf, WorkflowError> {
    path.canonicalize().map_err(|source| WorkflowError::Io {
        path: path.into(),
        source,
    })
}

fn output_path(path: &Path, resume: bool) -> Result<PathBuf, WorkflowError> {
    // Includes dangling symlinks: a new run must never overwrite an existing entry.
    let exists = match path.symlink_metadata() {
        Ok(_) => true,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(source) => {
            return Err(WorkflowError::Io {
                path: path.into(),
                source,
            });
        }
    };
    match (exists, resume) {
        (true, false) => return Err(WorkflowError::OutputExists(path.into())),
        (false, true) => return Err(WorkflowError::MissingRun(path.into())),
        (true, true) => return canonical(path),
        (false, false) => {}
    }
    let name = path
        .file_name()
        .ok_or(WorkflowError::InvalidRequest("choose a run file path"))?;
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    Ok(canonical(parent)?.join(name))
}

fn lock_output(path: &Path) -> Result<std::fs::File, WorkflowError> {
    use fs2::FileExt;
    let lock_path = path.with_extension("run-lock");
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&lock_path)
        .map_err(|source| WorkflowError::Io {
            path: lock_path,
            source,
        })?;
    file.try_lock_exclusive()
        .map_err(|_| WorkflowError::InvalidRequest("this run is busy in another operation"))?;
    Ok(file)
}

/// Validate and prepare a run without saving or making network requests.
/// Both preflight and execution use this exact source/parent/resume policy.
pub fn prepare(request: &ExtractionRequest) -> Result<ReviewRun, WorkflowError> {
    if !request.options.budget_usd.is_finite() || request.options.budget_usd < 0.0 {
        return Err(WorkflowError::InvalidRequest(
            "budget must be finite and nonnegative",
        ));
    }
    if request.resume && request.from.is_some() {
        return Err(WorkflowError::InvalidRequest(
            "choose resume or a parent run, not both",
        ));
    }
    if request.options.refresh && (!request.options.allow_network || request.resume) {
        return Err(WorkflowError::InvalidRequest(
            "refresh requires network access and a new output run",
        ));
    }
    if request.resume && request.out.as_os_str().is_empty() {
        return Err(WorkflowError::InvalidRequest(
            "resume requires an explicit run path",
        ));
    }
    let fresh = super::store::inspect(&request.book, &request.model)?;
    let mut run = if request.resume {
        let path = output_path(&request.out, true)?;
        ReviewRun::read(&path)?
    } else if let Some(parent) = &request.from {
        let parent = canonical(parent)?;
        fresh.inherit_outputs(&ReviewRun::read(&parent)?, &parent)?
    } else {
        fresh.clone()
    };
    if run.epub_sha256 != fresh.epub_sha256 || run.model != request.model {
        return Err(WorkflowError::SourceMismatch(
            "resume requires the same EPUB and model",
        ));
    }
    run.source = request.book.to_string_lossy().into_owned();
    run.documents = fresh.documents;
    super::preflight::plan(&run, &request.options)?;
    Ok(run)
}

/// Inspect, validate, create/resume/inherit, and checkpoint through the existing
/// extraction engine. Network remains opt-in in `RunOptions`; no output rendering.
pub async fn extract_to_run(
    request: ExtractionRequest,
    progress: impl FnMut(RunProgress),
) -> Result<RunOutcome, WorkflowError> {
    extract_to_run_controlled(request, &super::ExtractionControl::default(), progress).await
}

pub async fn extract_to_run_controlled(
    request: ExtractionRequest,
    control: &super::ExtractionControl,
    mut progress: impl FnMut(RunProgress),
) -> Result<RunOutcome, WorkflowError> {
    let mut run = prepare(&request)?;
    let automatic;
    let out = if request.out.as_os_str().is_empty() {
        if request.resume {
            return Err(WorkflowError::InvalidRequest(
                "resume requires an explicit run path",
            ));
        }
        automatic = super::store::destination(&request.book, &request.model)?;
        &automatic
    } else {
        &request.out
    };
    let path = output_path(out, request.resume)?;
    let _lock = lock_output(&path)?;
    output_path(&path, request.resume)?;
    // Reload after acquiring the cross-process lock.
    if request.resume {
        run = prepare(&request)?;
    }
    if !request.resume {
        run.metadata = Some(super::store::RunMetadata {
            parent_id: run
                .parent
                .as_deref()
                .and_then(|p| ReviewRun::read(Path::new(p)).ok())
                .and_then(|r| r.metadata.map(|m| m.id)),
            prompt_fingerprint: crate::cache::prompt_fingerprint(),
            id: super::store::new_id(),
            title: crate::book_metadata(&request.book)?.title,
            created_at: super::store::now(),
            updated_at: Some(super::store::now()),
            inherited_reserved_usd: run.reserved_usd,
            operation: if request.from.is_some() {
                "refresh"
            } else {
                "extract"
            }
            .into(),
        });
    }
    if let Some(metadata) = &mut run.metadata {
        metadata.updated_at = Some(super::store::now());
    }
    let started = std::time::Instant::now();
    let first_charge = run.charges.len();
    run.execution_status = Some("running".into());
    let extraction = extract_run_controlled(&mut run, &request.options, &path, control, |run| {
        let charges = &run.charges[first_charge..];
        progress(RunProgress {
            active: charges.iter().filter(|c| c.status == "pending").count(),
            failed: charges
                .iter()
                .filter(|c| c.status == "failed" || c.status == "truncated")
                .count(),
            elapsed_seconds: started.elapsed().as_secs(),
            estimated_usd: charges
                .iter()
                .filter(|c| c.status != "pending")
                .map(|c| c.estimated_usd)
                .sum(),
            stopping: control.is_cancelled(),
            completed: run.chunks.iter().filter(|c| c.output.is_some()).count(),
            total: run.chunks.len(),
            recipes: run.recipes.len(),
            reserved_usd: run.reserved_usd,
        });
    })
    .await;
    run.execution_status = Some(
        if !run.incomplete() {
            "complete"
        } else if control.is_cancelled() {
            "cancelled"
        } else if extraction.is_err()
            || run.charges[first_charge..]
                .iter()
                .any(|c| c.status == "failed")
        {
            "failed"
        } else {
            "incomplete"
        }
        .into(),
    );
    if path.exists() {
        run.save(&path)?;
    }
    // Register even partial checkpoints before returning an execution failure.
    if path.exists() {
        super::store::register(&run, &path)?;
    }
    extraction.map_err(|source| WorkflowError::Execution {
        path: path.clone(),
        source,
    })?;
    Ok(RunOutcome { path, run })
}

/// Replay saved outputs offline, optionally refreshing same-book source evidence
/// and captions. The original run and review sidecar are never rewritten.
pub fn replay_to_run(request: ReplayRequest) -> Result<RunOutcome, WorkflowError> {
    let path = output_path(&request.out, false)?;
    let _lock = lock_output(&path)?;
    output_path(&path, false)?;
    let original = canonical(&request.run)?;
    let mut run = ReviewRun::read(&original)?;
    if let Some(source) = &request.source {
        let fresh = ReviewRun::inspect(&read(source)?, &run.source, &run.model)?;
        if fresh.epub_sha256 != run.epub_sha256 {
            return Err(WorkflowError::SourceMismatch(
                "source EPUB hash differs from run",
            ));
        }
        run.documents = fresh.documents;
    }
    if let Some(captions) = &request.image_text {
        run.image_text = Some(serde_json::from_slice(&read(captions)?)?);
    }
    run.parent = Some(original.to_string_lossy().into_owned());
    run.replay()?;
    run.charges.clear();
    run.metadata = Some(super::store::RunMetadata {
        parent_id: run
            .parent
            .as_deref()
            .and_then(|p| ReviewRun::read(Path::new(p)).ok())
            .and_then(|r| r.metadata.map(|m| m.id)),
        prompt_fingerprint: run
            .metadata
            .as_ref()
            .map(|m| m.prompt_fingerprint.clone())
            .unwrap_or_default(),
        id: super::store::new_id(),
        title: run
            .metadata
            .as_ref()
            .map(|m| m.title.clone())
            .unwrap_or_else(|| run.source.clone()),
        created_at: super::store::now(),
        updated_at: Some(super::store::now()),
        inherited_reserved_usd: run.reserved_usd,
        operation: "replay".into(),
    });
    run.save(&path)?;
    super::store::register(&run, &path)?;
    Ok(RunOutcome { path, run })
}

/// Export a run and its separate review sidecar without overwriting either destination.
pub fn export_run(source: &Path, destination: &Path) -> Result<(), WorkflowError> {
    let run = ReviewRun::read(source)?;
    let target = output_path(destination, false)?;
    let _lock = lock_output(&target)?;
    output_path(&target, false)?;
    let source_review = source.with_extension("review.json");
    let target_review = target.with_extension("review.json");
    let review = if source_review.exists() {
        output_path(&target_review, false)?;
        Some(super::ReviewDecisions::read(&source_review, &run)?)
    } else {
        None
    };
    run.save(&target)?;
    if let Some(review) = review {
        review.save(&target_review)?;
    }
    super::store::register(&run, &target)?;
    Ok(())
}
