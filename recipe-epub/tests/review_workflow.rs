#![cfg(feature = "native")]

use recipe_epub::review::{
    ExtractionRequest, ReplayRequest, ReviewRun, RunOptions, WorkflowError, extract_to_run,
    replay_to_run,
};
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
            "review-workflow-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path)?;
        let fixture = Self(path.canonicalize()?);
        std::fs::write(
            fixture.path("book.epub"),
            recipe_epub_fixtures::cookbook_epub()?,
        )?;
        Ok(fixture)
    }
    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
    fn request(&self) -> ExtractionRequest {
        ExtractionRequest {
            book: self.path("book.epub"),
            out: self.path("run.json"),
            model: "gemini-2.5-flash".into(),
            resume: false,
            from: None,
            options: RunOptions {
                cache_dir: Some(self.path("empty-cache")),
                ..Default::default()
            },
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[tokio::test]
async fn extraction_owns_validation_resume_parent_and_offline_progress() -> Result {
    let fixture = Fixture::new()?;
    let request = fixture.request();
    let mut missing = request.clone();
    missing.resume = true;
    assert!(matches!(
        extract_to_run(missing, |_| {}).await,
        Err(WorkflowError::MissingRun(_))
    ));
    let mut updates = vec![];
    let mut discovered_while_running = false;
    let outcome = extract_to_run(request.clone(), |p| {
        discovered_while_running |= recipe_epub::review::store::list(Some(&request.book))
            .is_ok_and(|rows| {
                rows.iter()
                    .any(|row| row.path == request.out && row.status == "running")
            });
        updates.push(p);
    })
    .await?;
    assert!(
        discovered_while_running,
        "explicit paths must be discoverable before extraction returns"
    );
    assert!(outcome.run.incomplete());
    assert_eq!(updates.len(), 1);
    assert_eq!(updates[0].total, outcome.run.chunks.len());
    assert_eq!(updates[0].completed, 0);
    let before = std::fs::read(&outcome.path)?;
    assert!(matches!(
        extract_to_run(request.clone(), |_| {}).await,
        Err(WorkflowError::OutputExists(_))
    ));
    assert_eq!(std::fs::read(&outcome.path)?, before);
    let mut resume = request.clone();
    resume.resume = true;
    assert_eq!(
        extract_to_run(resume.clone(), |_| {}).await?.path,
        outcome.path
    );
    let checkpoint = std::fs::read(&outcome.path)?;
    let mut invalid = vec![];
    for budget in [-1.0, f64::NAN, f64::INFINITY] {
        let mut r = resume.clone();
        r.options.budget_usd = budget;
        invalid.push(r);
    }
    let mut r = resume.clone();
    r.from = Some(outcome.path.clone());
    invalid.push(r);
    let mut r = resume.clone();
    r.options.refresh = true;
    invalid.push(r);
    for r in invalid {
        let mut called = false;
        assert!(matches!(
            extract_to_run(r, |_| called = true).await,
            Err(WorkflowError::InvalidRequest(_))
        ));
        assert!(!called);
        assert_eq!(std::fs::read(&outcome.path)?, checkpoint);
    }
    resume.model = "different-model".into();
    assert!(matches!(
        extract_to_run(resume, |_| {}).await,
        Err(WorkflowError::SourceMismatch(_))
    ));
    let mut child = request;
    child.out = fixture.path("child.json");
    child.from = Some(outcome.path.clone());
    let child = extract_to_run(child, |_| {}).await?;
    assert_eq!(child.run.parent.as_deref(), outcome.path.to_str());
    assert_eq!(child.run.epub_sha256, outcome.run.epub_sha256);
    assert_eq!(std::fs::read(&outcome.path)?, checkpoint);
    Ok(())
}

#[tokio::test]
async fn replay_refreshes_same_source_and_preserves_original_evidence() -> Result {
    let fixture = Fixture::new()?;
    let original = extract_to_run(fixture.request(), |_| {}).await?;
    let before = std::fs::read(&original.path)?;
    let sidecar = original.path.with_extension("review.json");
    std::fs::write(&sidecar, b"original review evidence")?;
    let request = ReplayRequest {
        run: original.path.clone(),
        out: fixture.path("replay.json"),
        source: Some(fixture.path("book.epub")),
        image_text: None,
    };
    let replay = replay_to_run(request.clone())?;
    assert_eq!(replay.run.parent.as_deref(), original.path.to_str());
    assert_eq!(replay.run.parsed, original.run.parsed);
    assert_eq!(replay.run.epub_sha256, original.run.epub_sha256);
    assert!(matches!(
        replay_to_run(request.clone()),
        Err(WorkflowError::OutputExists(_))
    ));
    let mut bad = request.clone();
    bad.out = fixture.path("bad.json");
    let mut bytes = std::fs::read(fixture.path("book.epub"))?;
    bytes.extend_from_slice(b"different EPUB identity");
    std::fs::write(fixture.path("other.epub"), bytes)?;
    bad.source = Some(fixture.path("other.epub"));
    assert!(matches!(
        replay_to_run(bad.clone()),
        Err(WorkflowError::SourceMismatch(_))
    ));
    assert!(!bad.out.exists());
    bad.source = None;
    bad.image_text = Some(fixture.path("captions.json"));
    std::fs::write(fixture.path("captions.json"), b"invalid json")?;
    assert!(matches!(
        replay_to_run(bad),
        Err(WorkflowError::ImageText(_))
    ));
    assert_eq!(std::fs::read(&original.path)?, before);
    assert_eq!(std::fs::read(sidecar)?, b"original review evidence");
    assert!(ReviewRun::read(&replay.path)?.incomplete());
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn aliases_share_canonical_identity_and_dangling_outputs_are_protected() -> Result {
    let fixture = Fixture::new()?;
    let original = extract_to_run(fixture.request(), |_| {}).await?;
    let alias = fixture.path("alias.json");
    std::os::unix::fs::symlink(&original.path, &alias)?;
    let mut resume = fixture.request();
    resume.out = alias.clone();
    resume.resume = true;
    assert_eq!(extract_to_run(resume, |_| {}).await?.path, original.path);
    assert!(std::fs::symlink_metadata(&alias)?.file_type().is_symlink());
    let replay = replay_to_run(ReplayRequest {
        run: alias,
        out: fixture.path("replay.json"),
        source: None,
        image_text: None,
    })?;
    assert_eq!(replay.run.parent.as_deref(), original.path.to_str());
    let dangling = fixture.path("dangling.json");
    std::os::unix::fs::symlink(fixture.path("missing.json"), &dangling)?;
    let mut request = fixture.request();
    request.out = dangling.clone();
    assert!(matches!(
        extract_to_run(request, |_| {}).await,
        Err(WorkflowError::OutputExists(_))
    ));
    assert!(
        std::fs::symlink_metadata(dangling)?
            .file_type()
            .is_symlink()
    );
    Ok(())
}

#[tokio::test]
async fn named_runs_and_preflight_share_source_and_resume_validation() -> Result {
    let fixture = Fixture::new()?;
    let request = fixture.request();
    let fresh = recipe_epub::review::prepare(&request)?;
    let before = serde_json::to_value(&fresh)?;
    let plan = recipe_epub::review::preflight::plan(&fresh, &request.options)?;
    assert_eq!(plan.pending, fresh.chunks.len());
    assert_eq!(plan.estimated_high_usd, Some(0.0));
    assert_eq!(serde_json::to_value(&fresh)?, before);
    assert!(!request.out.exists(), "preflight must not checkpoint");
    let outcome = extract_to_run(request.clone(), |_| {}).await?;
    let discovered = recipe_epub::review::store::list(Some(&fixture.path("book.epub")))?;
    assert_eq!(
        discovered
            .iter()
            .filter(|r| r.path.canonicalize().ok().as_ref() == Some(&outcome.path))
            .count(),
        1
    );
    let meta = outcome.run.metadata.as_ref().ok_or("metadata missing")?;
    assert!(!meta.title.is_empty());
    let summary = recipe_epub::review::store::summary(&outcome.run, &outcome.path);
    assert_eq!(summary.new_spend_usd, Some(0.0));
    let mut resume = request;
    resume.resume = true;
    resume.model = "claude-haiku-4-5".into();
    assert!(recipe_epub::review::prepare(&resume).is_err());
    let mut legacy = serde_json::to_value(&outcome.run)?;
    legacy.as_object_mut().ok_or("object")?.remove("metadata");
    legacy.as_object_mut().ok_or("object")?.remove("charges");
    let legacy: ReviewRun = serde_json::from_value(legacy)?;
    assert_eq!(
        recipe_epub::review::store::summary(&legacy, &outcome.path).new_spend_usd,
        None
    );
    Ok(())
}

#[tokio::test]
async fn automatic_paths_are_unique_and_moved_sources_keep_identity() -> Result {
    let fixture = Fixture::new()?;
    let a =
        recipe_epub::review::store::destination(&fixture.path("book.epub"), "gemini-2.5-flash")?;
    let b =
        recipe_epub::review::store::destination(&fixture.path("book.epub"), "gemini-2.5-flash")?;
    assert_ne!(a, b);
    assert!(
        a.file_name()
            .ok_or("name")?
            .to_string_lossy()
            .contains("gemini-2-5-flash")
    );
    assert!(!a.exists());
    let original = recipe_epub::review::store::source_hash(&fixture.path("book.epub"))?;
    std::fs::rename(fixture.path("book.epub"), fixture.path("moved.epub"))?;
    assert_eq!(
        original,
        recipe_epub::review::store::source_hash(&fixture.path("moved.epub"))?
    );
    Ok(())
}

#[tokio::test]
async fn concurrent_new_runs_do_not_overwrite_and_exports_keep_reviews() -> Result {
    let fixture = Fixture::new()?;
    let request = fixture.request();
    let (a, b) = tokio::join!(
        extract_to_run(request.clone(), |_| {}),
        extract_to_run(request, |_| {})
    );
    assert_ne!(a.is_ok(), b.is_ok());
    let run = a.or(b)?;
    let decisions =
        recipe_epub::review::ReviewDecisions::read(&fixture.path("none.review.json"), &run.run)?;
    decisions.save(&run.path.with_extension("review.json"))?;
    let exported = fixture.path("export.json");
    recipe_epub::review::export_run(&run.path, &exported)?;
    assert_eq!(
        std::fs::read(run.path.with_extension("review.json"))?,
        std::fs::read(exported.with_extension("review.json"))?
    );
    assert!(recipe_epub::review::export_run(&run.path, &exported).is_err());
    let replay = replay_to_run(ReplayRequest {
        run: run.path.clone(),
        out: fixture.path("child.json"),
        source: None,
        image_text: None,
    })?;
    assert_eq!(
        replay
            .run
            .metadata
            .as_ref()
            .and_then(|m| m.parent_id.as_ref()),
        run.run.metadata.as_ref().map(|m| &m.id)
    );
    assert_ne!(
        replay.run.metadata.as_ref().map(|m| &m.id),
        run.run.metadata.as_ref().map(|m| &m.id)
    );
    Ok(())
}

#[tokio::test]
async fn cancellation_is_durable_and_history_distinguishes_interruption_and_failure() -> Result {
    let fixture = Fixture::new()?;
    let mut request = fixture.request();
    request.options.allow_network = true;
    request.options.budget_usd = 10.0;
    let control = recipe_epub::review::ExtractionControl::default();
    control.cancel();
    let outcome = recipe_epub::review::extract_to_run_controlled(request, &control, |_| {}).await?;
    assert_eq!(outcome.run.status(), "cancelled");
    assert_eq!(ReviewRun::read(&outcome.path)?.status(), "cancelled");
    assert!(outcome.run.charges.is_empty());
    let mut interrupted = outcome.run.clone();
    interrupted.execution_status = Some("running".into());
    assert_eq!(
        recipe_epub::review::store::summary(&interrupted, &outcome.path).status,
        "interrupted"
    );
    let lock = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(outcome.path.with_extension("run-lock"))?;
    fs2::FileExt::lock_exclusive(&lock)?;
    assert_eq!(
        recipe_epub::review::store::summary(&interrupted, &outcome.path).status,
        "running"
    );
    fs2::FileExt::unlock(&lock)?;
    assert_eq!(
        recipe_epub::review::store::summary(&interrupted, &outcome.path).status,
        "interrupted"
    );
    interrupted.execution_status = Some("failed".into());
    assert_eq!(interrupted.status(), "failed");
    let comparison = recipe_epub::review::diff(&outcome.run, &interrupted)?;
    assert_eq!(comparison["before_status"], "cancelled");
    assert_eq!(comparison["after_status"], "failed");
    let mut other = interrupted.clone();
    other.epub_sha256 = "another book".into();
    assert!(recipe_epub::review::diff(&interrupted, &other).is_err());
    Ok(())
}

#[tokio::test]
async fn native_workflow_reuses_portable_verified_cache_without_network() -> Result {
    let fixture = Fixture::new()?;
    let mut request = fixture.request();
    request.options.budget_usd = 10.0;
    let prepared = recipe_epub::review::prepare(&request)?;
    let mut state = prepared.recovery.clone().ok_or("missing state")?;
    let cache = request
        .options
        .cache_dir
        .as_ref()
        .ok_or("missing cache")?
        .join("verified-recovery-v2");
    std::fs::create_dir_all(&cache)?;
    while let Some(action) = state.next_action()? {
        let value = if let Some(index) = action.chunk {
            let chunk = &state.source[index];
            let doc = prepared
                .documents
                .iter()
                .find(|d| d.path == chunk.doc_path)
                .ok_or("missing source")?;
            let mut title = vec![];
            let mut ingredients = vec![];
            let mut instructions = vec![];
            let mut notes = vec![];
            let mut ignored = vec![];
            for (line, text) in chunk.text.lines().enumerate() {
                let block = doc.blocks.iter().find(|b| b.text.trim() == text.trim());
                if text.trim().is_empty() {
                    ignored.push(line);
                } else if block.is_some_and(|b| b.tag == "h1") {
                    title.push(line);
                } else if block
                    .is_some_and(|b| b.classes.split_whitespace().any(|c| c == "ingredient"))
                {
                    ingredients.push(line);
                } else if block
                    .is_some_and(|b| b.classes.split_whitespace().any(|c| c == "instruction"))
                {
                    instructions.push(line);
                } else {
                    notes.push(line);
                }
            }
            serde_json::json!({"recipes":[{"title":title,"description":[],"notes":notes,"equipment":[],"sections":[{"name":[],"ingredients":ingredients,"instructions":instructions}]}],"ignored":ignored})
        } else {
            let index = action.verification_chunk.ok_or("missing target")?;
            let source = &state.source[index];
            let doc = prepared
                .documents
                .iter()
                .find(|d| d.path == source.doc_path)
                .ok_or("missing source")?;
            serde_json::json!({"classifications":source.text.lines().enumerate().map(|(line,text)| {
                let block = doc.blocks.iter().find(|b| b.text.trim() == text.trim());
                let kind = if block.is_some_and(|b| b.classes.split_whitespace().any(|c| c == "ingredient")) { "ingredient" }
                    else if block.is_some_and(|b| b.classes.split_whitespace().any(|c| c == "instruction")) { "method" }
                    else if text.trim().is_empty() { "non_recipe" } else { "metadata" };
                serde_json::json!({"chunk":index,"line":line,"kind":kind})
            }).collect::<Vec<_>>(),"findings":[]})
        };
        state.apply(&action, value.clone())?;
        if !state.groups[action.group].candidates[action.candidate]
            .feedback
            .is_empty()
        {
            return Err("authored fixture unexpectedly failed validation".into());
        }
        std::fs::write(
            cache.join(format!("{}.json", action.key)),
            serde_json::to_vec(&serde_json::json!({
                "version": 1,
                "key": action.key,
                "contract": recipe_epub::recovery::VERIFICATION_CONTRACT,
                "model": action.model,
                "output_limit": action.output_limit,
                "payload": value,
            }))?,
        )?;
    }
    assert!(state.complete());
    let outcome = extract_to_run(request, |_| {}).await?;
    assert!(!outcome.run.incomplete());
    assert_eq!(outcome.run.status(), "complete");
    assert!(
        outcome
            .run
            .recovery
            .as_ref()
            .is_some_and(|s| s.attempts.is_empty())
    );
    assert_eq!(outcome.run.reserved_usd, 0.0);
    let restored = ReviewRun::read(&outcome.path)?;
    assert!(restored.recovery.as_ref().is_some_and(|s| s.complete()));
    Ok(())
}

#[tokio::test]
async fn legacy_recovery_migrates_to_a_child_without_recharging_parent_attempts() -> Result {
    let fixture = Fixture::new()?;
    let mut request = fixture.request();
    request.options.budget_usd = 10.0;
    let mut parent = recipe_epub::review::prepare(&request)?;
    let mut state = parent.recovery.clone().ok_or("missing recovery")?;
    let action = state.next_action()?.ok_or("missing action")?;
    state.reserve(&action)?;
    state.policy = recipe_epub::recovery::LEGACY_POLICY.into();
    state.groups[0].candidates[0]
        .verification_evidence
        .push(serde_json::json!({"legacy":true}));
    parent.reserved_usd = state.allocated(false) + state.allocated(true);
    parent.recovery = Some(state);
    let parent_path = fixture.path("legacy-parent.json");
    parent.save(&parent_path)?;
    let parent_bytes = std::fs::read(&parent_path)?;

    let mut child_request = request;
    child_request.out = fixture.path("legacy-child.json");
    child_request.from = Some(parent_path.clone());
    let child = extract_to_run(child_request, |_| {}).await?;

    assert_eq!(std::fs::read(&parent_path)?, parent_bytes);
    let state = child
        .run
        .recovery
        .as_ref()
        .ok_or("missing child recovery")?;
    assert!(state.attempts.iter().all(|attempt| attempt.inherited));
    assert_eq!(state.allocated(false), parent.reserved_usd);
    assert_eq!(state.allocated_new(false), 0.0);
    assert!(
        state.groups[0].candidates[0]
            .verification_evidence
            .is_empty()
    );
    assert_eq!(
        state.groups[0].candidates[0]
            .obsolete_verification_evidence
            .len(),
        1
    );
    assert_eq!(child.run.reserved_usd, parent.reserved_usd);
    assert!(child.run.charges.is_empty());
    assert_eq!(
        recipe_epub::review::store::summary(&child.run, &child.path).new_spend_usd,
        Some(0.0)
    );
    Ok(())
}

#[tokio::test]
async fn v2_accepted_recovery_becomes_a_v3_child_without_mutating_the_parent() -> Result {
    let fixture = Fixture::new()?;
    let mut request = fixture.request();
    request.options.budget_usd = 10.0;
    let mut parent = recipe_epub::review::prepare(&request)?;
    let mut state = parent.recovery.clone().ok_or("missing recovery")?;
    let action = state.next_action()?.ok_or("missing action")?;
    state.reserve(&action)?;
    let group_chunks = state.groups[0].chunks.clone();
    let source_roles = group_chunks
        .iter()
        .map(|chunk| vec!["non_recipe".to_owned(); state.source[*chunk].text.lines().count()])
        .collect();
    let candidate = &mut state.groups[0].candidates[0];
    for output in &mut candidate.outputs {
        *output = Some(vec![]);
    }
    candidate.source_roles = source_roles;
    candidate.verified = true;
    candidate.verified_chunks = group_chunks.clone();
    candidate
        .verification_evidence
        .push(serde_json::json!({"v2": "passed"}));
    candidate
        .verification_stages
        .push(recipe_epub::recovery::VerificationStage {
            target: group_chunks[0],
            stage: 0,
            model: "gemini-2.5-flash".into(),
            status: "passed".into(),
            attempts: vec![action.key],
            evidence: vec![serde_json::json!({"classifications": []})],
            findings: vec![recipe_epub::recovery::Finding {
                category: "coverage".into(),
                message: "preserve staged legacy finding".into(),
                chunk: group_chunks[0],
                lines: vec![0],
                resolved: false,
                model: "gemini-2.5-flash".into(),
            }],
        });
    state.groups[0].accepted = Some(0);
    state.policy = recipe_epub::recovery::PREVIOUS_POLICY.into();
    assert!(!state.complete());
    parent.reserved_usd = state.allocated(false) + state.allocated(true);
    parent.recovery = Some(state);
    let parent_path = fixture.path("v2-parent.json");
    parent.save(&parent_path)?;
    let parent_bytes = std::fs::read(&parent_path)?;
    let restored_parent = ReviewRun::read(&parent_path)?;
    assert!(
        restored_parent
            .recovery
            .as_ref()
            .is_some_and(|state| !state.complete())
    );
    assert_eq!(restored_parent.status(), "not_assessed");
    assert_eq!(std::fs::read(&parent_path)?, parent_bytes);

    let mut child_request = request;
    child_request.out = fixture.path("v3-child.json");
    child_request.from = Some(parent_path.clone());
    let child = extract_to_run(child_request, |_| {}).await?;

    assert_eq!(std::fs::read(&parent_path)?, parent_bytes);
    let state = child
        .run
        .recovery
        .as_ref()
        .ok_or("missing child recovery")?;
    let candidate = &state.groups[0].candidates[0];
    assert_eq!(state.policy, recipe_epub::recovery::POLICY);
    assert_eq!(state.groups[0].accepted, None);
    assert!(!candidate.verified);
    assert!(candidate.verified_chunks.is_empty());
    assert!(candidate.verification_evidence.is_empty());
    assert_eq!(candidate.obsolete_verification_evidence.len(), 1);
    // Offline execution may prepare fresh v3 stages, but none may inherit the
    // old stage's acceptance, response, findings, or dispatched attempts.
    assert!(!candidate.verification_stages.is_empty());
    assert!(candidate.verification_stages.iter().all(|stage| {
        stage.status == "pending"
            && stage.attempts.is_empty()
            && stage.evidence.is_empty()
            && stage.findings.is_empty()
    }));
    assert_eq!(candidate.obsolete_verification_stages.len(), 1);
    let stage = &candidate.obsolete_verification_stages[0];
    assert_eq!(stage.target, state.groups[0].chunks[0]);
    assert_eq!(stage.status, "passed");
    assert_eq!(
        stage.evidence,
        vec![serde_json::json!({"classifications": []})]
    );
    assert_eq!(stage.findings[0].message, "preserve staged legacy finding");
    assert!(state.attempts.iter().all(|attempt| attempt.inherited));
    assert_eq!(state.allocated_new(false), 0.0);
    assert_eq!(child.run.reserved_usd, parent.reserved_usd);
    assert!(child.run.charges.is_empty());
    Ok(())
}
