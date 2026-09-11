//! Saved-output AI review through the same checkpointed recovery executor.
use super::{ExtractionControl, ReviewRun, RunOptions, RunOutcome, WorkflowError};
use crate::recovery::{Action, Candidate, State};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

/// Native callers which need external durable accounting around the shared
/// recovery scheduler. `reserve` runs after the child checkpoint records the
/// pending attempt and before transport starts. `checkpoint` runs after each
/// subsequent child checkpoint, so a raw response is durable before settlement.
pub trait AuditAccounting: Send {
    fn reserve(&mut self, action: &Action) -> Result<(), String>;
    fn checkpoint(&mut self, state: &State) -> Result<(), String>;
}

/// Shared with the recovery adapter, whose futures must remain `Send` for
/// desktop callers. Locks are acquired only around synchronous ledger or
/// checkpoint work and are always dropped before transport awaits.
pub type AuditAccountingHandle = Arc<Mutex<dyn AuditAccounting + Send>>;

#[derive(Debug, Clone)]
pub struct AuditRequest {
    pub run: PathBuf,
    pub out: PathBuf,
    pub model: String,
    pub correct: bool,
    pub options: RunOptions,
}

/// Prepare a child without changing the parent, reserving money, or calling a
/// provider. Saved source is sufficient; original EPUB availability is not
/// required to audit a portable saved run.
pub fn prepare_audit(request: &AuditRequest) -> Result<ReviewRun, WorkflowError> {
    prepare_audit_scoped_for_experiment(request, None)
}

pub(crate) fn prepare_audit_scoped_for_experiment(
    request: &AuditRequest,
    selected_groups: Option<&[usize]>,
) -> Result<ReviewRun, WorkflowError> {
    if !request.options.allow_network {
        return Err(WorkflowError::InvalidRequest(
            "AI audit requires explicit network authorization",
        ));
    }
    if request.out.as_os_str().is_empty() {
        return Err(WorkflowError::InvalidRequest(
            "AI audit requires a child output path",
        ));
    }
    if !request.options.budget_usd.is_finite() || request.options.budget_usd < 0.0 {
        return Err(WorkflowError::InvalidRequest(
            "audit budget must be finite and non-negative",
        ));
    }
    if request.options.concurrency > 8 || !request.options.chunks.is_empty() {
        return Err(WorkflowError::InvalidRequest(
            "audit covers all saved groups with concurrency at most eight",
        ));
    }
    if request.model != crate::recovery::AUTOMATIC
        && !crate::models::catalog()
            .iter()
            .any(|model| model.id == request.model && model.enabled)
    {
        return Err(WorkflowError::InvalidRequest(
            "choose an enabled audit model",
        ));
    }
    let original = super::workflow::canonical(&request.run)?;
    super::workflow::output_path(&request.out, false)?;
    let mut run = ReviewRun::read(&original)?;
    let mut state = match run.recovery.take() {
        Some(mut state) => {
            state.migrate_legacy();
            state.validate().map_err(super::error)?;
            state
        }
        None => {
            let mut state = State::new_with_strategy(
                run.chunks
                    .iter()
                    .map(|chunk| chunk.source.clone())
                    .collect(),
                &request.model,
                request.options.budget_usd,
                crate::hybrid::HybridStrategy::Hybrid,
            )
            .map_err(super::error)?;
            for group in &mut state.groups {
                if let Some(outputs) = group
                    .chunks
                    .iter()
                    .map(|index| run.chunks[*index].output.clone())
                    .collect::<Option<Vec<_>>>()
                {
                    group
                        .candidates
                        .push(Candidate::from_outputs(run.model.clone(), outputs));
                }
            }
            state
        }
    };
    if serde_json::to_value(&state.source)?
        != serde_json::to_value(
            run.chunks
                .iter()
                .map(|chunk| &chunk.source)
                .collect::<Vec<_>>(),
        )?
    {
        return Err(WorkflowError::SourceMismatch(
            "saved recovery source differs from saved chunks",
        ));
    }
    state.bind_documents(&run.documents).map_err(super::error)?;
    state.bind_navigation_documents(run.navigation_documents.clone());
    if !run.source_line_provenance.is_empty() {
        state
            .bind_source_line_provenance(&run.source_line_provenance)
            .map_err(super::error)?;
    }
    state.models = if request.model == crate::recovery::AUTOMATIC {
        crate::recovery::ORDER
            .iter()
            .map(|model| (*model).into())
            .collect()
    } else {
        vec![request.model.clone()]
    };
    for attempt in &mut state.attempts {
        attempt.inherited = true;
    }
    if selected_groups.is_some_and(|selected| {
        selected.is_empty() || selected.iter().any(|index| *index >= state.groups.len())
    }) {
        return Err(WorkflowError::InvalidRequest(
            "audit scope contains no saved groups or an invalid group",
        ));
    }
    for (index, group) in state.groups.iter_mut().enumerate() {
        group.enabled = selected_groups.is_none_or(|selected| selected.contains(&index));
        group.paused = false;
    }
    if state.groups.iter().all(|group| !group.enabled) {
        return Err(WorkflowError::InvalidRequest(
            "audit requires at least one saved group",
        ));
    }
    // Older runs can carry spend without portable attempt records. Reserve
    // that lineage against the new ceiling as well; never reset its budget.
    let unrepresented =
        (run.reserved_usd - state.allocated(false) - state.allocated(true)).max(0.0);
    state.budget_usd = (request.options.budget_usd - unrepresented).max(0.0);
    state
        .migrate_historical_hybrid_assembly()
        .map_err(super::error)?;
    state.begin_audit(request.correct).map_err(super::error)?;
    state.validate().map_err(super::error)?;
    super::automatic::synchronize_outputs(&mut run, &state)?;
    run.parent = Some(original.to_string_lossy().into_owned());
    run.metadata = Some(super::store::RunMetadata {
        parent_id: run.metadata.as_ref().map(|metadata| metadata.id.clone()),
        prompt_fingerprint: crate::cache::prompt_fingerprint(),
        id: super::store::new_id(),
        title: run
            .metadata
            .as_ref()
            .map(|metadata| metadata.title.clone())
            .unwrap_or_else(|| run.source.clone()),
        created_at: super::store::now(),
        updated_at: Some(super::store::now()),
        inherited_reserved_usd: run.reserved_usd,
        operation: if request.correct {
            "audit_correct"
        } else {
            "audit"
        }
        .into(),
    });
    run.charges.clear();
    run.recovery = Some(state);
    run.execution_status = Some("prepared".into());
    Ok(run)
}

pub async fn audit_to_run(
    request: AuditRequest,
    progress: impl FnMut(&ReviewRun),
) -> Result<RunOutcome, WorkflowError> {
    audit_to_run_controlled(request, &ExtractionControl::default(), progress).await
}

pub async fn audit_to_run_controlled(
    request: AuditRequest,
    control: &ExtractionControl,
    progress: impl FnMut(&ReviewRun),
) -> Result<RunOutcome, WorkflowError> {
    audit_to_run_scoped_controlled(request, control, None, None, progress).await
}

/// Execute a frozen subset through the ordinary audit child lifecycle. The
/// public `AuditRequest` remains compatible; experiment-only scope/accounting
/// are supplied separately so ordinary CLI audits still cover every group.
pub async fn audit_to_run_scoped_controlled(
    request: AuditRequest,
    control: &ExtractionControl,
    selected_groups: Option<&[usize]>,
    accounting: Option<AuditAccountingHandle>,
    mut progress: impl FnMut(&ReviewRun),
) -> Result<RunOutcome, WorkflowError> {
    // Validate before creating even a lock file; reload after obtaining the
    // output lock so another process cannot race the no-overwrite check.
    prepare_audit_scoped_for_experiment(&request, selected_groups)?;
    let path = super::workflow::output_path(&request.out, false)?;
    let _lock = super::workflow::lock_output(&path)?;
    let mut run = prepare_audit_scoped_for_experiment(&request, selected_groups)?;
    let mut options = request.options.clone();
    if let Some(state) = &run.recovery {
        options.strategy = state.strategy;
    }
    run.execution_status = Some("running".into());
    run.save(&path)?;
    super::store::register(&run, &path)?;
    let execution = super::automatic::execute_with_audit_accounting(
        &mut run,
        &request.model,
        &options,
        &path,
        control,
        accounting,
        |run| progress(run),
    )
    .await;
    run.execution_status = Some(
        if control.is_cancelled() {
            "cancelled"
        } else if execution.is_err() {
            "failed"
        } else if run.incomplete() {
            "incomplete"
        } else {
            "complete"
        }
        .into(),
    );
    run.save(&path)?;
    super::store::register(&run, &path)?;
    progress(&run);
    execution.map_err(|source| WorkflowError::Execution {
        path: path.clone(),
        source,
    })?;
    Ok(RunOutcome { path, run })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn fixture() -> (tempfile::TempDir, AuditRequest) {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("parent.json");
        let mut run = ReviewRun::inspect(
            &recipe_epub_fixtures::cookbook_epub().unwrap(),
            "fixture.epub",
            "gemini-2.5-flash",
        )
        .unwrap();
        run.reserved_usd = 2.0;
        run.save(&path).unwrap();
        let request = AuditRequest {
            run: path,
            out: directory.path().join("child.json"),
            model: "gemini-2.5-flash".into(),
            correct: true,
            options: RunOptions {
                allow_network: true,
                budget_usd: 10.0,
                ..Default::default()
            },
        };
        (directory, request)
    }

    #[test]
    fn audit_preparation_preserves_parent_and_unrepresented_spend() {
        let (_directory, request) = fixture();
        let before = std::fs::read(&request.run).unwrap();
        let child = prepare_audit(&request).unwrap();
        assert_eq!(std::fs::read(&request.run).unwrap(), before);
        assert!(!request.out.exists());
        assert_eq!(child.metadata.as_ref().unwrap().inherited_reserved_usd, 2.0);
        assert!(child.charges.is_empty());
        let mut state = child.recovery.unwrap();
        assert_eq!(state.budget_usd, 8.0);
        assert!(state.audit_only);
        assert!(
            state.next_action().unwrap().is_none(),
            "audit must not extract missing output"
        );
        assert!(!state.complete());
    }

    #[test]
    fn audit_preparation_requires_explicit_network_and_new_child() {
        let (_directory, mut request) = fixture();
        request.options.allow_network = false;
        assert!(prepare_audit(&request).is_err());
        request.options.allow_network = true;
        request.out = request.run.clone();
        assert!(matches!(
            prepare_audit(&request),
            Err(WorkflowError::OutputExists(_))
        ));
    }

    #[test]
    fn audit_child_repairs_historical_hybrid_assembly_before_presenting_output() {
        let (_directory, request) = fixture();
        let mut parent = ReviewRun::read(&request.run).unwrap();
        let source = crate::Chunk {
            doc_path: "components.xhtml".into(),
            text: "Dish\nSauce\n1 cup water\nSalad\n1 carrot\nMake sauce.\nToss salad.".into(),
            title_hint: None,
            links: vec![],
            images: vec![],
        };
        let mut state = State::new_with_strategy(
            vec![source.clone()],
            "gemini-2.5-flash",
            10.0,
            crate::hybrid::HybridStrategy::Hybrid,
        )
        .unwrap();
        let response = serde_json::json!({"recipes":[{
            "title":{"text":"Dish","spans":[{"start":0,"end":0}]},
            "description":[],"recipe_yield":[],"notes":[],"equipment":[],
            "sections":[
                {"name":{"text":"Sauce","spans":[{"start":1,"end":1}]},"ingredients":[{"start":2,"end":2}],"instructions":[{"start":5,"end":5}]},
                {"name":{"text":"Salad","spans":[{"start":3,"end":3}]},"ingredients":[{"start":4,"end":4}],"instructions":[{"start":6,"end":6}]}
            ]
        }],"ignored":[]});
        let action = state.next_action().unwrap().unwrap();
        let attempt = state.reserve(&action).unwrap();
        state.apply(&action, response.clone()).unwrap();
        state.settle(attempt, Some(crate::Usage::default()), Some(response), None);
        let candidate = &mut state.groups[0].candidates[0];
        let recipes = candidate.outputs[0].as_mut().unwrap();
        for section in &mut recipes[0].sections {
            section.instructions.clear();
        }
        recipes[0].sections.push(recipe_types::RecipeSection {
            instructions: vec!["Make sauce.".into(), "Toss salad.".into()],
            ..Default::default()
        });
        let old_outputs = candidate.outputs.clone();
        let old_revision = candidate.revision;
        parent.chunks.truncate(1);
        parent.chunks[0].source = source;
        parent.chunks[0].output = old_outputs[0].clone();
        parent.documents = vec![crate::source::SourceDocument {
            path: "components.xhtml".into(),
            anchors: vec![],
            images: vec![],
            blocks: parent.chunks[0]
                .source
                .text
                .lines()
                .enumerate()
                .map(|(index, text)| crate::source::SourceBlock {
                    id: format!("block-{index}"),
                    element_index: index,
                    anchor: None,
                    tag: "p".into(),
                    classes: String::new(),
                    text: text.into(),
                    links: vec![],
                })
                .collect(),
        }];
        parent.navigation_documents.clear();
        parent.source_line_provenance.clear();
        parent.source_line_provenance_sha256 = None;
        parent.recovery = Some(state);
        parent.replay().unwrap();
        parent.save(&request.run).unwrap();
        let original_bytes = std::fs::read(&request.run).unwrap();

        let child = prepare_audit(&request).unwrap();
        assert_eq!(std::fs::read(&request.run).unwrap(), original_bytes);
        assert!(!request.out.exists());
        let state = child.recovery.as_ref().unwrap();
        let candidate = &state.groups[0].candidates[0];
        assert_eq!(candidate.revision, old_revision + 1);
        assert_ne!(candidate.outputs, old_outputs);
        assert_eq!(child.chunks[0].output, candidate.outputs[0]);
        let sections = &child.chunks[0].output.as_ref().unwrap()[0].sections;
        assert_eq!(sections.len(), 2);
        assert_eq!(sections[0].instructions, ["Make sauce."]);
        assert_eq!(sections[1].instructions, ["Toss salad."]);
        assert_eq!(child.recipes[0].sections, *sections);
        assert!(state.audit_only);
        assert!(!candidate.verified);
        assert!(child.incomplete());
        let mut planned = state.clone();
        let action = planned.next_action().unwrap().unwrap();
        let payload: serde_json::Value = serde_json::from_str(&action.request.user).unwrap();
        assert_eq!(
            payload["assembled"][0]["sections"],
            serde_json::to_value(sections).unwrap()
        );
    }

    #[test]
    fn legacy_saved_outputs_are_audited_without_source_assignment_guesses() {
        let (_directory, request) = fixture();
        let mut parent = ReviewRun::read(&request.run).unwrap();
        for chunk in &mut parent.chunks {
            chunk.output = Some(vec![]);
        }
        parent.save(&request.run).unwrap();
        let mut child = prepare_audit(&request).unwrap();
        let state = child.recovery.as_mut().unwrap();
        assert!(
            state
                .groups
                .iter()
                .all(|group| group.enabled && group.candidates.len() == 1)
        );
        assert!(
            state
                .groups
                .iter()
                .all(|group| group.candidates[0].source_roles.iter().all(Vec::is_empty))
        );
        let action = state.next_action().unwrap().unwrap();
        assert_eq!(action.model, request.model);
        assert!(
            action.chunk.is_none(),
            "audit should dispatch verification only"
        );
    }
}
