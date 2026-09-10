//! Native adapter for the portable recovery policy.
use super::{ExtractionControl, ReviewRun, RunOptions};
use crate::{
    EpubError,
    recovery::{AUTOMATIC, State},
};
use std::path::Path;

fn checkpoint(run: &mut ReviewRun, state: &State, path: &Path) -> Result<(), EpubError> {
    run.recovery = Some(state.clone());
    for (index, a) in state.attempts.iter().enumerate() {
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
        };
        if let Some(old) = run.charges.iter_mut().find(|c| c.chunk == id) {
            *old = charge;
        } else {
            run.charges.push(charge);
        }
    }
    run.reserved_usd = state.allocated(false)
        + state.allocated(true)
        + run
            .metadata
            .as_ref()
            .map_or(0.0, |m| m.inherited_reserved_usd);
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
    run.save(path)
}

pub(super) async fn execute(
    run: &mut ReviewRun,
    requested_model: &str,
    options: &RunOptions,
    path: &Path,
    control: &ExtractionControl,
    mut progress: impl FnMut(&ReviewRun),
) -> Result<(), EpubError> {
    let mut state = match &run.recovery {
        Some(s) => s.clone(),
        None => State::new(
            run.chunks.iter().map(|c| c.source.clone()).collect(),
            requested_model,
            options.budget_usd,
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
    // A larger explicitly supplied ceiling permits continuation, without
    // clearing any charges or reservations from earlier attempts.
    state.budget_usd = options.budget_usd;
    let directory = options
        .cache_dir
        .clone()
        .unwrap_or_else(crate::cache::default_dir)
        .join("verified-recovery-v1");
    std::fs::create_dir_all(&directory).map_err(|e| super::error(e.to_string()))?;
    let source = run.source.clone();
    let load_dir = directory.clone();
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
        load: move |a: &crate::recovery::Action| {
            std::fs::read(load_dir.join(format!("{}.json", a.key)))
                .ok()
                .and_then(|b| serde_json::from_slice(&b).ok())
        },
        store: move |a: &crate::recovery::Action, value: &serde_json::Value| {
            let target = directory.join(format!("{}.json", a.key));
            let temp = directory.join(format!("{}.tmp", super::store::new_id()));
            let result = (|| {
                std::fs::write(&temp, serde_json::to_vec(value).map_err(|e| e.to_string())?)
                    .map_err(|e| e.to_string())?;
                std::fs::rename(&temp, &target).map_err(|e| e.to_string())
            })();
            if result.is_err() {
                let _ = std::fs::remove_file(temp);
            }
            result
        },
        save: |s: &State| {
            checkpoint(run, s, path).map_err(|e| e.to_string())?;
            progress(run);
            Ok(())
        },
        call: move |action: crate::recovery::Action| {
            let source = source.clone();
            async move {
                use crate::recovery::Reply;
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
