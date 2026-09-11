//! Measure read-only planning of saved candidates, including verification context.
//! No provider calls, checkpoint writes, or source prose in the report.
use recipe_epub::{IndexedChunk, recovery::State};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::{env, fs, time::Instant};

/// Rebuild only an in-memory measurement state. Historical attempts, findings,
/// checkpoints, and acceptance are never rewritten or exported as a new run.
fn with_full_context(saved: &State, indexed: &[IndexedChunk]) -> Result<State, String> {
    saved.validate()?;
    let mut state = State::new(
        indexed.iter().map(|entry| entry.chunk.clone()).collect(),
        recipe_epub::recovery::AUTOMATIC,
        saved.budget_usd,
    )?;
    state.bind_documents(&saved.documents)?;
    state.bind_source_line_provenance(
        &indexed
            .iter()
            .map(|entry| entry.lines.clone())
            .collect::<Vec<_>>(),
    )?;
    if state
        .source
        .iter()
        .any(|chunk| !saved.documents.iter().any(|doc| doc.path == chunk.doc_path))
    {
        return Err("full context is missing inspected documents".into());
    }
    let mapping: Vec<_> = saved
        .source
        .iter()
        .map(|chunk| {
            let expected = serde_json::to_value(chunk).map_err(|error| error.to_string())?;
            let matches: Vec<_> = state
                .source
                .iter()
                .enumerate()
                .filter_map(|(index, candidate)| {
                    (serde_json::to_value(candidate).ok().as_ref() == Some(&expected))
                        .then_some(index)
                })
                .collect();
            match matches.as_slice() {
                [index] => Ok(*index),
                _ => {
                    Err("saved chunk must have exactly one unchanged full-source match".to_string())
                }
            }
        })
        .collect::<Result<_, _>>()?;
    if mapping.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err("saved source mapping is not unique and ordered".into());
    }
    for group in &mut state.groups {
        group.enabled = false;
    }
    for saved_group in &saved.groups {
        if !saved_group.enabled {
            continue;
        }
        let mapped: Vec<_> = saved_group
            .chunks
            .iter()
            .map(|index| mapping[*index])
            .collect();
        let group = state
            .groups
            .iter_mut()
            .find(|group| group.chunks == mapped)
            .ok_or("full-source group differs from frozen execution group")?;
        if saved_group.candidates.len() != 1 {
            return Err("measurement requires one frozen candidate per group".into());
        }
        let mut candidate = saved_group.candidates[0].clone();
        if candidate.outputs.iter().any(Option::is_none) || candidate.seed_provenance.is_some() {
            return Err("measurement requires complete unseeded frozen candidates".into());
        }
        // Outputs and source roles are group-local and copied byte-for-byte.
        // Only audit state is reset, in this disposable measurement clone.
        candidate.extraction_keys.fill(None);
        candidate.cached_chunks.fill(false);
        candidate.feedback.clear();
        candidate.obsolete_feedback.clear();
        candidate.verified = false;
        candidate.verification_evidence.clear();
        candidate.obsolete_verification_evidence.clear();
        candidate.obsolete_verification_stages.clear();
        candidate.verified_chunks.clear();
        candidate.verification_stages.clear();
        group.enabled = true;
        group.candidates.push(candidate);
    }
    state.validate()?;
    Ok(state)
}

fn prepare_extraction_requests(
    state: &State,
    directory: &std::path::Path,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let mut entries = Vec::new();
    let mut files = Vec::new();
    for group in state.groups.iter().filter(|group| group.enabled) {
        for &index in &group.chunks {
            let chunk = &state.source[index];
            let evidence = recipe_epub::source::chunk_source_evidence(
                chunk,
                state.source_line_provenance.get(index).map(Vec::as_slice),
                &state.documents,
            )?;
            let legacy = recipe_epub::build_chunk_request(chunk);
            let indexed = recipe_epub::indexed::build_indexed_chunk_request_with_source_evidence(
                chunk, &evidence,
            )?;
            for (contract, request) in [("legacy", legacy), ("indexed", indexed)] {
                let bytes = serde_json::to_vec(&request)?;
                let filename = format!("{contract}-{index}.json");
                let proposed_reservations: Vec<_> = ["claude-haiku-4-5", "claude-sonnet-4-6"].into_iter().map(|model| {
                    let identity = json!({"kind":"extraction-contract-comparison-action-v1","model":model,"contract":contract,"chunk":index,"source_line_provenance_sha256":state.source_line_provenance_sha256,"output_limit":16000,"request":request}).to_string().into_bytes();
                    let usage = recipe_epub::Usage { input_tokens: identity.len().saturating_mul(2) as u64, output_tokens:16000, ..Default::default() };
                    let reservation = recipe_epub::ExtractionStats { model:model.into(),usage, ..Default::default() }.cost_usd();
                    json!({"model":model,"enabled":recipe_epub::models::catalog().iter().any(|entry| entry.id==model && entry.enabled),"reservation_usd":reservation,"pricing_checked":recipe_epub::models::pricing_checked(model),"pricing_source":recipe_epub::models::pricing_source(model),"basis":"two input tokens per serialized action-identity byte plus 16000 output tokens; an envelope only, not ledger admission"})
                }).collect();
                entries.push(json!({"proposed_reservations":proposed_reservations,"contract":contract,"chunk":index,"file":filename,"sha256":hash(&bytes),"user_bytes":request.user.len(),"system_bytes":request.system.len(),"schema_bytes":serde_json::to_vec(&request.tool_schema)?.len(),"target_lines":chunk.text.lines().count()}));
                files.push((filename, bytes));
            }
        }
    }
    let manifest = json!({
        "kind":"extraction-contract-comparison-prepare-v2", "dispatch_requested":false,
        "source_sha256":hash(&serde_json::to_vec(&state.source)?),
        "source_line_provenance_sha256":state.source_line_provenance_sha256,
        "context_chunks":state.source.len(), "requests":entries,
        "proposed_controls":{"primary_model":"claude-haiku-4-5","fallback_model":"claude-sonnet-4-6","concurrency":4,"output_limit":16000},
        "scope":"Offline requests from current shared builders. No execution, pricing admission, candidate generation, or quality comparison. Legacy reproduces Cubby's request format, not its deployed build or eight-request concurrency. Indexed and legacy outputs require their respective decoders before common assembly and independent evaluation."
    });
    fs::create_dir(directory)?;
    for (filename, bytes) in files {
        fs::write(directory.join(filename), bytes)?;
    }
    fs::write(
        directory.join("manifest.json"),
        serde_json::to_vec_pretty(&manifest)?,
    )?;
    Ok(manifest)
}

fn hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = env::args().skip(1);
    let path = args.next().ok_or("usage: benchmark_recovery_plan STATE.json [--source-index INDEX.json] [--requests-out NEW_PRIVATE_DIRECTORY | --extraction-requests-out NEW_PRIVATE_DIRECTORY]")?;
    let mut index_path = None;
    let mut requests_out = None;
    let mut extraction_out = None;
    while let Some(flag) = args.next() {
        match flag.as_str() {
            "--extraction-requests-out" => {
                extraction_out = Some(PathBuf::from(
                    args.next().ok_or("missing extraction output directory")?,
                ))
            }
            "--source-index" => index_path = Some(args.next().ok_or("missing source index")?),
            "--requests-out" => {
                requests_out = Some(PathBuf::from(
                    args.next().ok_or("missing output directory")?,
                ))
            }
            _ => return Err("unknown argument".into()),
        }
    }
    let input = fs::read(&path)?;
    let saved: State = serde_json::from_slice(&input)?;
    let state = if let Some(path) = index_path {
        let indexed: Vec<IndexedChunk> = serde_json::from_slice(&fs::read(path)?)?;
        with_full_context(&saved, &indexed)?
    } else {
        saved.clone()
    };
    if let Some(directory) = extraction_out {
        if requests_out.is_some() {
            return Err("choose extraction preparation or verifier measurement, not both".into());
        }
        let manifest = prepare_extraction_requests(&state, &directory)?;
        println!("{}", serde_json::to_string(&manifest)?);
        return Ok(());
    }
    let actions = state.planned_actions()?;
    if let Some(directory) = &requests_out {
        fs::create_dir(directory)?;
        for action in &actions {
            if action.chunk.is_some() {
                return Err("request export expected verification only".into());
            }
            let target = action
                .verification_chunk
                .ok_or("verification target missing")?;
            fs::write(
                directory.join(format!("target-{target}.json")),
                serde_json::to_vec(&action.request)?,
            )?;
        }
    }
    let mut elapsed = Vec::with_capacity(100);
    for _ in 0..100 {
        let started = Instant::now();
        let planned = state.planned_actions()?;
        elapsed.push(started.elapsed().as_secs_f64() * 1000.0);
        if planned.iter().map(|action| &action.key).collect::<Vec<_>>()
            != actions.iter().map(|action| &action.key).collect::<Vec<_>>()
        {
            return Err("read-only plan changed between samples".into());
        }
    }
    elapsed.sort_by(f64::total_cmp);
    let verification: Vec<_> = actions
        .iter()
        .filter(|action| action.chunk.is_none())
        .map(|action| {
            let request: serde_json::Value = serde_json::from_str(&action.request.user)?;
            Ok(json!({
                "target": action.verification_chunk,
                "request_bytes": action.request.user.len(),
                "context_source_indices": request["source"].as_array().map(|source| source.iter().map(|entry| entry["chunk"].clone()).collect::<Vec<_>>()),
                "context_chunks": request["source"].as_array().map(Vec::len),
                "source_bytes": action.telemetry.source_bytes,
                "context_bytes": action.telemetry.context_bytes,
                "candidate_bytes": action.telemetry.candidate_bytes,
            }))
        })
        .collect::<Result<_, serde_json::Error>>()?;
    println!(
        "{}",
        serde_json::to_string(&json!({
            "policy": recipe_epub::recovery::POLICY,
            "input_policy": saved.policy,
            "input_sha256": Sha256::digest(&input).iter().map(|byte| format!("{byte:02x}")).collect::<String>(),
            "full_context_reconstruction": state.source.len() != saved.source.len(),
            "samples": elapsed.len(), "p50_ms": elapsed[49],
            "p95_ms": elapsed[94], "max_ms": elapsed[99],
            "source_chunks": state.source.len(), "actions": actions.len(),
            "verification": verification,
            "scope": "saved-state read-only planning including clone and in-memory migration; excludes native inspection, cache lookup and UI debounce",
        }))?
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use recipe_epub::{Chunk, SourceLine, recovery::Candidate, source::SourceDocument};

    fn indexed(title: &str, path: &str) -> IndexedChunk {
        let text = format!("{title}\nServes 2\n1 cup water\nCook until tender.");
        IndexedChunk {
            lines: text
                .lines()
                .enumerate()
                .map(|(document_line, _)| SourceLine {
                    document_line,
                    contributors: vec![],
                    anchors: vec![],
                    links: vec![],
                    images: vec![],
                    transformed: false,
                })
                .collect(),
            chunk: Chunk {
                text,
                doc_path: path.into(),
                title_hint: None,
                links: vec![],
                images: vec![],
            },
        }
    }

    fn fixture() -> (State, Vec<IndexedChunk>) {
        let full = vec![
            indexed("Before", "a.xhtml"),
            indexed("Target", "b.xhtml"),
            indexed("After", "c.xhtml"),
        ];
        let mut saved = State::new(
            vec![full[1].chunk.clone()],
            recipe_epub::recovery::AUTOMATIC,
            10.0,
        )
        .unwrap();
        saved
            .bind_documents(
                &full
                    .iter()
                    .map(|entry| SourceDocument {
                        path: entry.chunk.doc_path.clone(),
                        blocks: vec![],
                        images: vec![],
                        anchors: vec![],
                    })
                    .collect::<Vec<_>>(),
            )
            .unwrap();
        let candidate: Candidate = serde_json::from_value(json!({
            "model":"gemini-2.5-flash", "outputs":[[]], "extraction_keys":["historical"],
            "cached_chunks":[false], "source_roles":[["title","yield","ingredient","method"]],
            "feedback":[], "verified":true, "verification_evidence":[{"historical":true}],
            "verified_chunks":[0], "revision":3,
        }))
        .unwrap();
        saved.groups[0].candidates.push(candidate);
        saved.groups[0].accepted = Some(0);
        (saved, full)
    }

    #[test]
    fn expands_context_without_changing_frozen_candidates_or_scheduling_outside_groups() {
        let (saved, full) = fixture();
        let original = serde_json::to_value(&saved).unwrap();
        let state = with_full_context(&saved, &full).unwrap();
        assert_eq!(serde_json::to_value(&saved).unwrap(), original);
        assert_eq!(state.source.len(), 3);
        assert_eq!(state.groups.iter().filter(|group| group.enabled).count(), 1);
        let candidate = &state.groups[1].candidates[0];
        assert_eq!(candidate.outputs, saved.groups[0].candidates[0].outputs);
        assert_eq!(
            candidate.source_roles,
            saved.groups[0].candidates[0].source_roles
        );
        assert!(!candidate.verified);
        assert!(state.groups[1].accepted.is_none());
        let actions = state.planned_actions().unwrap();
        assert_eq!(actions.len(), 1);
        assert!(actions[0].chunk.is_none());
        assert_eq!(actions[0].verification_chunk, Some(1));
    }

    #[test]
    fn rejects_changed_source_and_incomplete_candidates() {
        let (mut saved, mut full) = fixture();
        full[1].chunk.text.push_str(" changed");
        assert!(with_full_context(&saved, &full).is_err());
        let (_, full) = fixture();
        saved.groups[0].accepted = None;
        saved.groups[0].candidates[0].outputs[0] = None;
        assert!(with_full_context(&saved, &full).is_err());
    }

    #[test]
    fn rejects_group_expansion_instead_of_silently_changing_the_cohort() {
        let (saved, mut full) = fixture();
        full[2].chunk.doc_path = "b.xhtml".into();
        full[2].chunk.title_hint = Some("Target".into());
        assert!(
            with_full_context(&saved, &full)
                .unwrap_err()
                .contains("group differs")
        );
    }
}
