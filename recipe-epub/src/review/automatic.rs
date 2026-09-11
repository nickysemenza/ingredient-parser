//! Native adapter for the portable recovery policy.
use super::{ExtractionControl, ReviewRun, RunOptions};
use crate::{
    EpubError,
    recovery::{AUTOMATIC, State},
};
use std::path::Path;

fn checkpoint(run: &mut ReviewRun, state: &State, path: &Path) -> Result<(), EpubError> {
    for (index, a) in state.attempts.iter().enumerate() {
        if a.inherited {
            // Parent attempt evidence remains in `run.recovery` for budget and
            // audit continuity. Do not recreate it as a child-run charge.
            continue;
        }
        let id = format!("recovery-attempt-{index}");

        let charge = super::store::Charge {
            attempts: vec![super::store::CallRecord {
                failure: a.failure_details.clone(),
                raw_usage: a.raw_usage.clone(),
                source_key: a.key.clone(),
                request_fingerprint: a.key.clone(),
                usage: a.usage.clone(),
                estimated_usd: a.estimated_usd,
                error: a.error.clone(),
                telemetry: Some(a.telemetry.clone()),
            }],
            chunk: id.clone(),
            model: a.model.clone(),
            started_at: a.started_at.unwrap_or_default(),
            reservation_usd: a.reservation_usd,
            usage: a.usage.clone(),
            estimated_usd: a.estimated_usd,
            input_rate: Some(a.rates_usd_per_million[0]),
            output_rate: Some(a.rates_usd_per_million[1]),
            cache_read_rate: Some(a.rates_usd_per_million[2]),
            cache_creation_rate: Some(a.rates_usd_per_million[3]),
            rate_date: a.pricing_checked.clone(),
            rate_source: Some(a.pricing_source.clone()),
            status: if a.pending {
                "pending"
            } else if a.error.is_some() {
                "failed"
            } else {
                "complete"
            }
            .into(),
            telemetry: Some(a.telemetry.clone()),
        };
        if let Some(old) = run.charges.iter_mut().find(|c| c.chunk == id) {
            *old = charge;
        } else {
            run.charges.push(charge);
        }
    }
    run.reserved_usd = state.allocated_new(false)
        + state.allocated_new(true)
        + run
            .metadata
            .as_ref()
            .map_or(0.0, |m| m.inherited_reserved_usd);
    synchronize_outputs(run, state)?;
    run.save(path)
}

/// Keep the saved presentation and portable candidate in agreement before a
/// child is shown or checkpointed. This performs no I/O or accounting changes.
pub(super) fn synchronize_outputs(run: &mut ReviewRun, state: &State) -> Result<(), EpubError> {
    run.recovery = Some(state.clone());
    run.hybrid_audits = state
        .groups
        .iter()
        .flat_map(|group| group.candidates.iter())
        .flat_map(|candidate| candidate.hybrid_audits.iter().cloned())
        .collect();
    let mut outputs_changed = false;
    for g in &state.groups {
        let candidate = g
            .accepted
            .and_then(|i| g.candidates.get(i))
            .or_else(|| g.candidates.last());
        if let Some(c) = candidate {
            for (pos, (index, output)) in g.chunks.iter().zip(&c.outputs).enumerate() {
                if let Some(output) = output {
                    if run.chunks[*index].output.as_ref() != Some(output) {
                        run.chunks[*index].output = Some(output.clone());
                        outputs_changed = true;
                    }
                    run.chunks[*index].cached = c.cached_chunks.get(pos).copied().unwrap_or(false);
                    run.chunks[*index].model = Some(c.model.clone());
                    run.chunks[*index].prompt_version = Some(run.prompt_version.clone());
                    run.chunks[*index].error = None;
                }
            }
        }
    }
    if outputs_changed {
        run.replay()?;
    }
    Ok(())
}

pub(super) async fn execute(
    run: &mut ReviewRun,
    requested_model: &str,
    options: &RunOptions,
    path: &Path,
    control: &ExtractionControl,
    progress: impl FnMut(&ReviewRun),
) -> Result<(), EpubError> {
    execute_with_audit_accounting(run, requested_model, options, path, control, None, progress)
        .await
}

pub(super) async fn execute_with_audit_accounting(
    run: &mut ReviewRun,
    requested_model: &str,
    options: &RunOptions,
    path: &Path,
    control: &ExtractionControl,
    accounting: Option<super::audit::AuditAccountingHandle>,
    mut progress: impl FnMut(&ReviewRun),
) -> Result<(), EpubError> {
    let mut state = match &run.recovery {
        Some(s) => s.clone(),
        None => State::new_with_strategy(
            run.chunks.iter().map(|c| c.source.clone()).collect(),
            requested_model,
            options.budget_usd,
            options.strategy,
        )
        .map_err(super::error)?,
    };
    if state
        .source
        .iter()
        .zip(&run.chunks)
        .any(|(s, c)| s.text != c.source.text || s.doc_path != c.source.doc_path)
        || state.source.len() != run.chunks.len()
    {
        return Err(super::error("recovery source changed"));
    }
    let expected: Vec<String> = if requested_model == AUTOMATIC {
        crate::recovery::ORDER.iter().map(|s| (*s).into()).collect()
    } else {
        vec![requested_model.into()]
    };
    if expected != state.models {
        return Err(super::error("resume requires the same extraction policy"));
    }
    if state.strategy != options.strategy {
        return Err(super::error("resume requires the same extraction strategy"));
    }
    // `State::new` has no inspected documents, and legacy checkpoints may
    // carry documents without their action-bound identity. Bind the current
    // run before cache lookup or provider dispatch; changed evidence clears
    // only acceptance, never historic attempts/findings/spend.
    if state.bind_documents(&run.documents).map_err(super::error)? {
        checkpoint(run, &state, path)?;
    }
    if !run.source_line_provenance.is_empty()
        && state
            .bind_source_line_provenance(&run.source_line_provenance)
            .map_err(super::error)?
    {
        checkpoint(run, &state, path)?;
    }
    // A parent can contain an unknown reservation with no portable attempt
    // record. Keep that hold by reducing the effective child ceiling, while
    // represented inherited attempts remain visible to scheduler admission.
    let inherited_reserved = run
        .metadata
        .as_ref()
        .map_or(0.0, |metadata| metadata.inherited_reserved_usd);
    let represented = state
        .attempts
        .iter()
        .filter(|attempt| attempt.inherited)
        .map(|attempt| attempt.estimated_usd.unwrap_or(attempt.reservation_usd))
        .sum::<f64>();
    state.budget_usd =
        super::workflow::effective_budget(options.budget_usd, inherited_reserved, represented);
    let directory = options
        .cache_dir
        .clone()
        .unwrap_or_else(crate::cache::default_dir)
        .join("verified-recovery-v2");
    std::fs::create_dir_all(&directory).map_err(|e| super::error(e.to_string()))?;
    let source = run.source.clone();
    let load_dir = directory.clone();
    let call_accounting = accounting.clone();
    let save_accounting = accounting;
    let mut adapter = crate::recovery::Adapter {
        concurrency: if options.concurrency == 0 {
            4
        } else {
            options.concurrency
        },
        allow_network: options.allow_network,
        refresh: options.refresh,
        now: || Some(super::store::now()),
        cancelled: || control.is_cancelled(),
        wait: |seconds| async move { tokio::time::sleep(std::time::Duration::from_secs(seconds)).await },
        load: move |a: &crate::recovery::Action| super::recovery_cache::read(&load_dir, a),
        store: move |a: &crate::recovery::Action, value: &serde_json::Value| {
            super::recovery_cache::write(&directory, a, value)
        },
        save: |s: &State| {
            checkpoint(run, s, path).map_err(|e| e.to_string())?;
            if let Some(accounting) = &save_accounting {
                accounting
                    .lock()
                    .map_err(|_| "audit accounting lock poisoned".to_owned())?
                    .checkpoint(s)
                    .map_err(|e| e.to_string())?;
            }
            progress(run);
            Ok(())
        },
        call: move |action: crate::recovery::Action| {
            let admitted = call_accounting
                .as_ref()
                .map(|accounting| {
                    accounting
                        .lock()
                        .map_err(|_| "audit accounting lock poisoned".to_owned())?
                        .reserve(&action)
                })
                .transpose();
            let source = source.clone();
            async move {
                use crate::recovery::Reply;
                if let Err(error) = admitted {
                    return Reply {
                        error: Some(format!(
                            "audit dispatch was not started; reservation remains held: {error}"
                        )),
                        ..Default::default()
                    };
                }
                let backend = match crate::backend::Backend::from_env(
                    &crate::Options {
                        model: Some(action.model.clone()),
                        ..Default::default()
                    },
                    &source,
                ) {
                    Ok(b) => b,
                    Err(e) => {
                        return Reply {
                            error: Some(e.to_string()),
                            usage: Some(crate::Usage::default()),
                            ..Default::default()
                        };
                    }
                };
                let response = backend.recovery_call(&action).await;
                let raw_usage = backend.recovery_usage(&action.key);
                match response {
                    Ok((payload, usage, reason)) => Reply {
                        payload,
                        usage: (usage != crate::Usage::default()).then_some(usage),
                        raw_usage,
                        error: matches!(reason.as_deref(), Some("length" | "max_tokens"))
                            .then(|| "truncated response".into()),
                        failure: None,
                    },
                    Err(f) => {
                        let error = Some(f.error.to_string());
                        Reply {
                            payload: None,
                            usage: (f.usage != crate::Usage::default()).then_some(f.usage),
                            raw_usage,
                            error,
                            failure: match f.error {
                                EpubError::Request(details) => Some(*details),
                                _ => None,
                            },
                        }
                    }
                }
            }
        },
    };
    crate::recovery::run(&mut state, &mut adapter)
        .await
        .map_err(super::error)
}
