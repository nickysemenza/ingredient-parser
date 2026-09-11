//! File-backed workflows used by both maintainer tools. No terminal or UI policy.
use super::{ReviewRun, RunOptions};
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

#[derive(Debug, Clone)]
pub struct RunProgress {
    pub active_models: Vec<String>,
    pub unresolved_usd: f64,
    pub phase: &'static str,
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

fn revalidate_source(path: &Path, expected_hash: &str) -> Result<(), WorkflowError> {
    let actual_hash = super::store::source_hash(path)?;
    if actual_hash != expected_hash {
        return Err(WorkflowError::SourceMismatch(
            "source EPUB changed after preparation",
        ));
    }
    Ok(())
}

/// Recovery evidence can only cross a child-run boundary when it was produced
/// under the same extraction contract.  The strategy is deliberately part of
/// that contract: hybrid candidates carry source-span ownership and audit
/// evidence that indexed candidates do not have.
fn compatible_parent_recovery(
    previous: &crate::recovery::State,
    state: &crate::recovery::State,
) -> bool {
    previous.policy == state.policy
        && previous.models == state.models
        && previous.strategy == state.strategy
        && previous.source.len() == state.source.len()
        && previous
            .source
            .iter()
            .zip(&state.source)
            .all(|(a, b)| a.text == b.text && a.doc_path == b.doc_path)
}

/// Accounting crosses a strategy boundary when the frozen source does. The
/// action keys themselves include strategy, so inherited attempts cannot be
/// mistaken for reusable hybrid work.
fn compatible_parent_accounting(
    previous: &crate::recovery::State,
    state: &crate::recovery::State,
) -> bool {
    previous.source.len() == state.source.len()
        && previous
            .source
            .iter()
            .zip(&state.source)
            .all(|(a, b)| a.text == b.text && a.doc_path == b.doc_path)
}

pub(super) fn effective_budget(
    requested_budget: f64,
    inherited_reserved: f64,
    represented_inherited: f64,
) -> f64 {
    (requested_budget - (inherited_reserved - represented_inherited).max(0.0)).max(0.0)
}

/// Replace inspected documents and immediately reassess any recovery state
/// against their full bound identity. Both resume preparation and offline
/// replay use this so a refreshed source inspection cannot leave an accepted
/// checkpoint/status attached to stale source-backed validation evidence.
fn bind_current_documents(
    run: &mut ReviewRun,
    documents: Vec<crate::source::SourceDocument>,
) -> Result<(), WorkflowError> {
    run.documents = documents;
    if let Some(state) = &mut run.recovery {
        state.bind_documents(&run.documents).map_err(super::error)?;
    }
    Ok(())
}

/// Replace the exact indexed lineage from a fresh source inspection. The
/// accompanying digest is checked before either the run or recovery checkpoint
/// is changed, so a source-line vector cannot be retargeted to matching text.
fn bind_current_source_line_provenance(
    run: &mut ReviewRun,
    provenance: Vec<Vec<crate::SourceLine>>,
    identity: Option<String>,
) -> Result<(), WorkflowError> {
    let source: Vec<_> = run
        .chunks
        .iter()
        .map(|chunk| chunk.source.clone())
        .collect();
    let actual = crate::recovery::source_line_provenance_sha256(&source, &provenance)
        .map_err(super::error)?;
    if identity.as_deref() != Some(actual.as_str()) {
        return Err(WorkflowError::Run(super::error(
            "fresh source-line provenance identity is invalid",
        )));
    }
    if let Some(state) = &mut run.recovery {
        state
            .bind_source_line_provenance(&provenance)
            .map_err(super::error)?;
    }
    run.source_line_provenance = provenance;
    run.source_line_provenance_sha256 = Some(actual);
    Ok(())
}

pub(super) fn canonical(path: &Path) -> Result<PathBuf, WorkflowError> {
    path.canonicalize().map_err(|source| WorkflowError::Io {
        path: path.into(),
        source,
    })
}

pub(super) fn output_path(path: &Path, resume: bool) -> Result<PathBuf, WorkflowError> {
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

pub(super) fn lock_output(path: &Path) -> Result<std::fs::File, WorkflowError> {
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
    let initial_model = if request.model == crate::recovery::AUTOMATIC {
        crate::recovery::ORDER[0]
    } else {
        &request.model
    };
    let fresh = super::store::inspect(&request.book, initial_model)?;
    let run = prepare_from_inspection(request, fresh)?;
    super::preflight::plan(&run, &request.options)?;
    Ok(run)
}

/// Prepare using a source inspection already loaded by a caller. The EPUB
/// identity is still checked by the shared execution workflow before dispatch;
/// this seam only avoids reparsing the same source during desktop previews.
pub fn prepare_from_inspection(
    request: &ExtractionRequest,
    fresh: ReviewRun,
) -> Result<ReviewRun, WorkflowError> {
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
    let initial_model = if request.model == crate::recovery::AUTOMATIC {
        crate::recovery::ORDER[0]
    } else {
        &request.model
    };
    let fresh_epub_sha256 = fresh.epub_sha256.clone();
    // A regular new run can consume its inspected result directly. Previewing
    // options is frequent, and cloning a document-heavy inspection here adds
    // no isolation: the fresh state below owns the same source evidence. Keep
    // fresh available for resume/parent paths, which must rebind it to the
    // historical run before planning.
    let mut fresh = Some(fresh);
    let mut run = if request.resume {
        let path = output_path(&request.out, true)?;
        let run = ReviewRun::read(&path)?;
        // v1/v2 acceptance evidence cannot be silently upgraded in place. A child
        // created with `--from` keeps this artifact (and its historic charges)
        // as the durable source of truth while v3 reassesses saved outputs.
        if run
            .recovery
            .as_ref()
            .is_some_and(crate::recovery::State::needs_migration)
        {
            return Err(WorkflowError::InvalidRequest(
                "legacy verification evidence requires a new run with --from; the original run is preserved",
            ));
        }
        run
    } else if let Some(parent) = &request.from {
        let parent = canonical(parent)?;
        fresh
            .as_ref()
            .ok_or(WorkflowError::InvalidRequest(
                "fresh inspection is unavailable for parent inheritance",
            ))?
            .inherit_outputs(&ReviewRun::read(&parent)?, &parent)?
    } else {
        fresh.take().ok_or(WorkflowError::InvalidRequest(
            "fresh inspection is unavailable for a new run",
        ))?
    };
    if run.epub_sha256 != fresh_epub_sha256 || run.model != initial_model {
        return Err(WorkflowError::SourceMismatch(
            "resume requires the same EPUB and model",
        ));
    }
    if request.resume
        && run
            .recovery
            .as_ref()
            .is_some_and(|state| state.strategy != request.options.strategy)
    {
        return Err(WorkflowError::InvalidRequest(
            "resume requires the same extraction strategy",
        ));
    }
    run.source = request.book.to_string_lossy().into_owned();
    let navigation_documents = run.navigation_documents.clone();
    if let Some(state) = &mut run.recovery {
        state.bind_navigation_documents(navigation_documents.clone());
    }
    if let Some(fresh) = fresh {
        bind_current_documents(&mut run, fresh.documents)?;
        run.navigation_documents = fresh.navigation_documents;
        if let Some(state) = &mut run.recovery {
            state.bind_navigation_documents(run.navigation_documents.clone());
        }
        bind_current_source_line_provenance(
            &mut run,
            fresh.source_line_provenance,
            fresh.source_line_provenance_sha256,
        )?;
    }
    if run.recovery.is_none() {
        let mut state = crate::recovery::State::new_with_strategy(
            run.chunks.iter().map(|c| c.source.clone()).collect(),
            &request.model,
            request.options.budget_usd,
            request.options.strategy,
        )
        .map_err(super::error)?;
        state.bind_documents(&run.documents).map_err(super::error)?;
        state.bind_navigation_documents(navigation_documents);
        if !run.source_line_provenance.is_empty() {
            state
                .bind_source_line_provenance(&run.source_line_provenance)
                .map_err(super::error)?;
        }
        if let Some(parent) = &request.from {
            let old = ReviewRun::read(parent)?;
            if let Some(mut previous) = old.recovery {
                let migrated = previous.migrate_legacy();
                let accounting_compatible = compatible_parent_accounting(&previous, &state);
                let recovery_compatible = compatible_parent_recovery(&previous, &state);
                if accounting_compatible {
                    state.attempts = previous.attempts;
                    for attempt in &mut state.attempts {
                        attempt.inherited = true;
                    }
                }
                if recovery_compatible {
                    state.groups = previous.groups;
                    // A child inherits every compatible parent reservation for
                    // admission, regardless of whether its verifier contract
                    // was legacy. They are historical spend, never child-run
                    // charges; without this boundary a refresh could spend a
                    // second full budget after copying a parent artifact.
                    if !migrated {
                        for group in &mut state.groups {
                            let selected = request.options.chunks.is_empty()
                                || group
                                    .chunks
                                    .iter()
                                    .any(|i| request.options.chunks.contains(&run.chunks[*i].id));
                            if selected {
                                group.candidates.clear();
                                group.accepted = None;
                            }
                        }
                    }
                }
            }
        }
        for group in &mut state.groups {
            group.enabled = request.options.chunks.is_empty()
                || group
                    .chunks
                    .iter()
                    .any(|i| request.options.chunks.contains(&run.chunks[*i].id));
        }
        run.recovery = Some(state);
    }
    if let Some(state) = &run.recovery {
        let expected: Vec<String> = if request.model == crate::recovery::AUTOMATIC {
            crate::recovery::ORDER.iter().map(|s| (*s).into()).collect()
        } else {
            vec![request.model.clone()]
        };
        if state.models != expected {
            return Err(WorkflowError::InvalidRequest(
                "resume requires the same extraction policy",
            ));
        }
    }
    if let Some(state) = &mut run.recovery {
        let inherited_reserved = run.metadata.as_ref().map_or_else(
            || {
                if request.from.is_some() {
                    run.reserved_usd
                } else {
                    0.0
                }
            },
            |metadata| metadata.inherited_reserved_usd,
        );
        let represented = state
            .attempts
            .iter()
            .filter(|attempt| attempt.inherited)
            .map(|attempt| attempt.estimated_usd.unwrap_or(attempt.reservation_usd))
            .sum();
        state.budget_usd =
            effective_budget(request.options.budget_usd, inherited_reserved, represented);
    }
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
        automatic = super::store::destination(&request.book, &run.model)?;
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
    // Preparation intentionally happens before the output lock so validation
    // errors do not create lock files. Re-read the source after locking to
    // close the replacement window before metadata or network dispatch.
    revalidate_source(&request.book, &run.epub_sha256)?;
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
    // Explicit paths are outside the automatic store scan. Publish the initial
    // checkpoint so other callers can discover this run before it finishes.
    run.save(&path)?;
    super::store::register(&run, &path)?;
    let extraction = super::automatic::execute(
        &mut run,
        &request.model,
        &request.options,
        &path,
        control,
        |run| {
            let charges = &run.charges[first_charge..];
            progress(RunProgress {
                active_models: run
                    .recovery
                    .as_ref()
                    .map(|s| {
                        let mut labels: Vec<_> = s
                            .attempts
                            .iter()
                            .filter(|a| a.pending)
                            .map(|a| {
                                format!(
                                    "{} ({})",
                                    crate::models::catalog()
                                        .iter()
                                        .find(|m| m.id == a.model)
                                        .map_or(a.model.as_str(), |m| m.label),
                                    if a.verification {
                                        "verifying"
                                    } else {
                                        "extracting"
                                    }
                                )
                            })
                            .collect();
                        labels.sort();
                        labels.dedup();
                        labels
                    })
                    .unwrap_or_default(),
                unresolved_usd: run.recovery.as_ref().map_or(0.0, |s| {
                    s.attempts
                        .iter()
                        .filter(|a| a.estimated_usd.is_none())
                        .map(|a| a.reservation_usd)
                        .sum()
                }),
                phase: match run.recovery.as_ref().map(|s| s.phase.as_str()) {
                    Some("Verifying") => "Verifying",
                    Some("Recovering") => "Recovering",
                    Some("Complete") => "Complete",
                    Some("Incomplete") => "Incomplete",
                    _ => "Extracting",
                },
                active: charges.iter().filter(|c| c.status == "pending").count(),
                failed: charges
                    .iter()
                    .filter(|c| c.status == "failed" || c.status == "truncated")
                    .count(),
                elapsed_seconds: started.elapsed().as_secs(),
                estimated_usd: charges
                    .iter()
                    .filter(|c| c.status != "pending")
                    .filter_map(|c| c.estimated_usd)
                    .sum::<f64>()
                    .max(0.0)
                    .into(),
                stopping: control.is_cancelled(),
                completed: run.chunks.iter().filter(|c| c.output.is_some()).count(),
                total: run.chunks.len(),
                recipes: run.recipes.len(),
                reserved_usd: run.reserved_usd,
            });
        },
    )
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
        bind_current_documents(&mut run, fresh.documents)?;
        bind_current_source_line_provenance(
            &mut run,
            fresh.source_line_provenance,
            fresh.source_line_provenance_sha256,
        )?;
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

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;
    use serde_json::json;

    #[test]
    fn parent_recovery_requires_the_same_extraction_strategy() {
        let source = crate::Chunk {
            text: "One cup water".into(),
            doc_path: "recipe.xhtml".into(),
            title_hint: None,
            links: vec![],
            images: vec![],
        };
        let indexed = crate::recovery::State::new_with_strategy(
            vec![source.clone()],
            "gemini-2.5-flash",
            1.0,
            crate::hybrid::HybridStrategy::Indexed,
        )
        .expect("indexed state");
        let hybrid = crate::recovery::State::new_with_strategy(
            vec![source],
            "gemini-2.5-flash",
            1.0,
            crate::hybrid::HybridStrategy::Hybrid,
        )
        .expect("hybrid state");

        assert!(!compatible_parent_recovery(&indexed, &hybrid));
    }

    #[test]
    fn incompatible_parent_strategy_still_carries_spending_holds() {
        let source = crate::Chunk {
            text: "One cup water".into(),
            doc_path: "recipe.xhtml".into(),
            title_hint: None,
            links: vec![],
            images: vec![],
        };
        let mut parent = crate::recovery::State::new_with_strategy(
            vec![source.clone()],
            "gemini-2.5-flash",
            10.0,
            crate::hybrid::HybridStrategy::Indexed,
        )
        .expect("parent state");
        let action = parent.next_action().expect("next").expect("action");
        parent.reserve(&action).expect("reserve");
        let mut child = crate::recovery::State::new_with_strategy(
            vec![source],
            "gemini-2.5-flash",
            10.0,
            crate::hybrid::HybridStrategy::Hybrid,
        )
        .expect("child state");

        assert!(compatible_parent_accounting(&parent, &child));
        assert!(!compatible_parent_recovery(&parent, &child));
        child.attempts = parent.attempts;
        for attempt in &mut child.attempts {
            attempt.inherited = true;
        }
        let represented = child.allocated(false) + child.allocated(true);
        child.budget_usd = effective_budget(10.0, 2.0, represented);

        assert!(child.attempts.iter().all(|attempt| attempt.inherited));
        assert_eq!(child.budget_usd + (2.0 - represented).max(0.0), 10.0);
    }

    #[test]
    fn revalidate_source_rejects_same_size_replacement() {
        let path = std::env::temp_dir().join(format!(
            "recipe-epub-source-check-{}",
            super::super::store::new_id()
        ));
        let original = b"original bytes";
        std::fs::write(&path, original).expect("test source should be writable");
        let expected = super::super::hash(original);
        std::fs::write(&path, b"changed bytes!").expect("replacement should be writable");
        assert_eq!(original.len(), b"changed bytes!".len());
        assert!(matches!(
            revalidate_source(&path, &expected),
            Err(WorkflowError::SourceMismatch(_))
        ));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn refreshed_documents_immediately_reassess_a_completed_recovery_run() {
        let source = crate::Chunk {
            text: "Copyright".into(),
            doc_path: "source.xhtml".into(),
            title_hint: None,
            links: vec![],
            images: vec![],
        };
        let document = |text: &str| crate::source::SourceDocument {
            path: "source.xhtml".into(),
            anchors: vec![],
            images: vec![],
            blocks: vec![crate::source::SourceBlock {
                id: "source".into(),
                element_index: 0,
                anchor: None,
                tag: "p".into(),
                classes: String::new(),
                text: text.into(),
                links: vec![],
            }],
        };
        let mut state = crate::recovery::State::new(vec![source.clone()], "gemini-2.5-flash", 10.0)
            .expect("state");
        state
            .bind_documents(&[document("Copyright")])
            .expect("bind");
        let extraction = state.next_action().expect("next").expect("extraction");
        state.reserve(&extraction).expect("reserve extraction");
        state
            .apply(&extraction, json!({"recipes":[],"ignored":[0]}))
            .expect("apply extraction");
        let verification = state.next_action().expect("next").expect("verification");
        state.reserve(&verification).expect("reserve verification");
        state
            .apply(
                &verification,
                json!({"classifications":[{"chunk":0,"line":0,"kind":"non_recipe"}],"findings":[]}),
            )
            .expect("apply verification");
        assert!(state.complete());
        let attempts = state.attempts.len();
        let allocated = state.allocated(false) + state.allocated(true);
        let mut run = ReviewRun {
            recovery: Some(state),
            execution_status: Some("complete".into()),
            metadata: None,
            charges: vec![],
            version: super::super::RUN_VERSION,
            epub_sha256: "epub".into(),
            source: "book".into(),
            model: "gemini-2.5-flash".into(),
            prompt_version: crate::cache::PROMPT_VERSION.into(),
            parent: None,
            chunks: vec![super::super::RunChunk {
                id: "chunk".into(),
                source,
                output: Some(vec![]),
                error: None,
                cached: false,
                usage: crate::Usage::default(),
                model: Some("gemini-2.5-flash".into()),
                prompt_version: Some(crate::cache::PROMPT_VERSION.into()),
                request_identity: None,
            }],
            documents: vec![document("Copyright")],
            navigation_documents: std::collections::BTreeSet::new(),
            hybrid_audits: vec![],
            source_line_provenance: vec![],
            source_line_provenance_sha256: None,
            recipes: vec![],
            parsed: json!([]),
            reserved_usd: allocated,
            image_text: None,
        };
        bind_current_documents(&mut run, vec![document("Changed source evidence")])
            .expect("refresh bind");
        let state = run.recovery.as_ref().expect("recovery");
        assert!(!state.complete());
        assert_eq!(run.status(), "incomplete");
        assert_eq!(state.attempts.len(), attempts);
        assert_eq!(state.allocated(false) + state.allocated(true), allocated);
        assert!(state.groups[0].accepted.is_none());
        assert_eq!(
            state.groups[0].candidates[0]
                .obsolete_verification_stages
                .len(),
            1
        );
    }
}
