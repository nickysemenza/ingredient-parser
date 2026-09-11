//! Offline comparison of full hybrid audit requests and candidate source regions.
//! Does not dispatch, persist a run, alter acceptance, or claim quality/latency.
use recipe_epub::{hybrid::SourceSpan, recovery::State};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
};

fn hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn project(request: &Value, spans: &[SourceSpan], state: &State) -> Result<Value, String> {
    let selected: BTreeSet<_> = spans
        .iter()
        .flat_map(|span| (span.start..=span.end).map(move |line| (span.chunk, line)))
        .collect();
    let mut result = request.clone();
    let source = result["source"].as_array_mut().ok_or("missing source")?;
    let mut available = BTreeSet::new();
    for entry in source.iter() {
        let chunk = entry["chunk"].as_u64().ok_or("missing chunk")? as usize;
        for line in entry["lines"].as_array().ok_or("missing lines")? {
            available.insert((
                chunk,
                line[0].as_u64().ok_or("missing line coordinate")? as usize,
            ));
        }
    }
    if !selected.is_subset(&available) {
        return Err(
            "preview requires source absent from the full request; cannot compare by filtering"
                .into(),
        );
    }
    for entry in source.iter_mut() {
        let chunk = entry["chunk"].as_u64().ok_or("missing chunk")? as usize;
        let target = entry["target"].as_bool().ok_or("missing target flag")?;
        let lines = entry["lines"].as_array_mut().ok_or("missing lines")?;
        if target
            && lines.iter().any(|line| {
                line[0]
                    .as_u64()
                    .is_none_or(|line| !selected.contains(&(chunk, line as usize)))
            })
        {
            return Err("preview omitted a target line".into());
        }
        lines.retain(|line| {
            line[0]
                .as_u64()
                .is_some_and(|line| selected.contains(&(chunk, line as usize)))
        });
    }
    source.retain(|entry| {
        entry["target"] == true
            || entry["lines"]
                .as_array()
                .is_some_and(|lines| !lines.is_empty())
    });
    // Use the shared projection so selected element/anchor ancestry is encoded
    // exactly once, rather than maintaining a stale second DOM encoder here.
    let evidence = source
        .iter()
        .map(|entry| {
            let chunk = entry["chunk"].as_u64().ok_or("missing chunk")? as usize;
            let mut evidence = recipe_epub::source::chunk_source_evidence(
                state.source.get(chunk).ok_or("missing source chunk")?,
                state.source_line_provenance.get(chunk).map(Vec::as_slice),
                &state.documents,
            )?;
            let lines = selected
                .iter()
                .filter_map(|(index, line)| (*index == chunk).then_some(*line))
                .collect();
            evidence.retain_lines(&lines)?;
            Ok(json!({"chunk":chunk,"evidence":evidence.audit_projection()}))
        })
        .collect::<Result<Vec<_>, String>>()?;
    result["raw_dom_provenance"] = json!(evidence);
    Ok(result)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = env::args().skip(1);
    let path = args
        .next()
        .ok_or("usage: benchmark_audit_context STATE.json")?;
    if args.next().is_some() {
        return Err("unexpected argument".into());
    }
    let bytes = fs::read(path)?;
    let mut state: State = serde_json::from_slice(&bytes)?;
    state.migrate_historical_hybrid_assembly()?;
    state.begin_audit(true)?; // In-memory only; never write this diagnostic clone.
    let actions = state.planned_actions()?;
    let mut rows = vec![];
    let mut full_bytes = 0;
    let mut preview_bytes = 0;
    let mut context_transmitted_bytes = 0;
    let mut context_payloads = BTreeMap::<String, usize>::new();
    for action in actions {
        if action.chunk.is_some() || action.request.tool_name != "audit_recipe_group" {
            return Err("expected complete hybrid audit groups only".into());
        }
        let selected_request: Value = serde_json::from_str(&action.request.user)?;
        let mut assembly_placements_checked = 0;
        for owner in selected_request["owners"]
            .as_array()
            .ok_or("missing owners")?
        {
            let chunk = owner["chunk"].as_u64().ok_or("missing owner chunk")? as usize;
            let slot = state.groups[action.group]
                .chunks
                .iter()
                .position(|candidate| *candidate == chunk)
                .ok_or("owner chunk is outside the group")?;
            let input = state.groups[action.group].candidates[action.candidate].outputs[slot]
                .as_ref()
                .ok_or("owner has no candidate output")?;
            for owner_recipe in owner["recipes"].as_array().ok_or("missing owner recipes")? {
                let assembly = &owner_recipe["assembly"];
                if assembly["status"] != "retained" {
                    if assembly["status"] != "dropped" {
                        return Err("owner has no resolved assembly placement".into());
                    }
                    continue;
                }
                let recipe = owner_recipe["recipe"].as_u64().ok_or("missing recipe")? as usize;
                let output = assembly["recipe"].as_u64().ok_or("missing output recipe")? as usize;
                let offset = assembly["section_offset"]
                    .as_u64()
                    .ok_or("missing section offset")? as usize;
                let expected = &input.get(recipe).ok_or("owner recipe is absent")?.sections;
                let actual = selected_request["assembled"][output]["sections"]
                    .as_array()
                    .ok_or("assembled sections are absent")?;
                let actual = actual
                    .get(offset..offset + expected.len())
                    .ok_or("assembly section placement is outside output")?;
                if serde_json::to_value(expected)? != json!(actual) {
                    return Err("assembly placement changed the originating sections".into());
                }
                assembly_placements_checked += 1;
            }
        }
        let mut request = selected_request.clone();
        if selected_request["omitted_context_spans"]
            .as_array()
            .is_some_and(|spans| !spans.is_empty())
        {
            // Exercise the real expansion transition on a disposable clone.
            // This fabricated diagnostic request is never persisted as AI evidence.
            let mut full = state.clone();
            full.apply(&action, json!({
                "coverage":[],"findings":[],
                "move_assignment":[],"restore_span":[],"replace_bounded_text":[],"split_section":[],"merge_sections":[],
                "context_expansion":{"reason":"Offline context measurement; not provider evidence."},
            }))?;
            let full_action = full
                .planned_actions()?
                .into_iter()
                .find(|next| next.group == action.group && next.candidate == action.candidate)
                .ok_or("expansion did not schedule a full-context audit")?;
            request = serde_json::from_str(&full_action.request.user)?;
            if request["context_mode"] != "full" {
                return Err("expansion did not restore full context".into());
            }
        }
        for source in request["source"].as_array().ok_or("missing source")? {
            if source["target"] == true {
                continue;
            }
            let provenance = request["raw_dom_provenance"]
                .as_array()
                .ok_or("missing provenance")?
                .iter()
                .find(|entry| entry["chunk"] == source["chunk"])
                .ok_or("missing chunk evidence")?;
            let payload = serde_json::to_vec(&json!({"source":source,"provenance":provenance}))?;
            context_transmitted_bytes += payload.len();
            context_payloads
                .entry(hash(&payload))
                .or_insert(payload.len());
        }
        let spans = state.hybrid_audit_context_preview(action.group)?;
        let projected = project(&request, &spans, &state)?;
        if projected["source"] != selected_request["source"]
            || projected["raw_dom_provenance"] != selected_request["raw_dom_provenance"]
        {
            return Err("production selected source differs from the offline projection".into());
        }
        let before = serde_json::to_vec(&request)?.len();
        let after = serde_json::to_vec(&selected_request)?.len();
        let field_bytes = selected_request
            .as_object()
            .ok_or("audit request is not an object")?
            .iter()
            .map(|(field, value)| Ok((field.clone(), serde_json::to_vec(value)?.len())))
            .collect::<Result<BTreeMap<_, _>, serde_json::Error>>()?;
        full_bytes += before;
        preview_bytes += after;
        rows.push(json!({"group":action.group,"full_request_sha256":hash(request.to_string().as_bytes()),"selected_request_sha256":hash(action.request.user.as_bytes()),"full_user_bytes":before,"preview_user_bytes":after,"field_bytes":field_bytes,"assembly_placements_checked":assembly_placements_checked,"selected_spans":spans}));
    }
    if rows.is_empty() {
        return Err("no complete hybrid audits available".into());
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "state_sha256":hash(&bytes),
            "scope":"offline actual selected requests versus full expansion; no provider calls; no quality or runtime claim; selected DOM tables retain only referenced elements and lines",
            "production_context_changed":true,
            "requires_context_expansion_protocol":false,
            "full_user_bytes":full_bytes,"preview_user_bytes":preview_bytes,
            "reduction_fraction":1.0-preview_bytes as f64/full_bytes as f64,
            "exact_context_payloads":context_payloads.len(),
            "context_payload_bytes_transmitted":context_transmitted_bytes,
            "context_payload_bytes_unique":context_payloads.values().sum::<usize>(),
            "context_identity_scope":"exact serialized source plus provenance per context chunk, across this initial audit cohort; deduplication is hypothetical, not an implemented provider optimization",
            "groups":rows,
        }))?
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use recipe_epub::{Chunk, SourceLine};

    fn projection_state() -> State {
        let source = ["title\nmethod", "reference\nunrelated"]
            .into_iter()
            .enumerate()
            .map(|(chunk, text)| Chunk {
                title_hint: None,
                text: text.into(),
                doc_path: format!("chunk-{chunk}.xhtml"),
                links: vec![],
                images: vec![],
            })
            .collect();
        let mut state = State::new(source, "test-model", 1.0).unwrap();
        state.source_line_provenance = (0..2)
            .map(|chunk| {
                (0..2)
                    .map(|document_line| SourceLine {
                        document_line,
                        contributors: if chunk == 1 {
                            vec![recipe_epub::SourceElement {
                                element_index: 99 + document_line,
                                tag: "p".into(),
                                classes: String::new(),
                                anchor: None,
                                ancestors: vec![],
                            }]
                        } else {
                            vec![]
                        },
                        anchors: vec![],
                        links: vec![],
                        images: vec![],
                        transformed: false,
                    })
                    .collect()
            })
            .collect();
        state
    }

    #[test]
    fn projection_preserves_target_and_candidate_and_rejects_target_omissions() {
        let request = json!({"source":[{"chunk":0,"target":true,"lines":[[0,"title"],[1,"method"]]}, {"chunk":1,"target":false,"lines":[[0,"reference"],[1,"unrelated"]]}],"raw_dom_provenance":[{"chunk":0,"evidence":{"mode":"indexed_tables_v1","lines":[[0],[1]]}},{"chunk":1,"evidence":{"mode":"indexed_tables_v1","lines":[[0],[1]],"elements":[[99,"p"]]}}],"assembled":["unchanged"],"required_coverage":[{"chunk":0,"start":0,"end":1}]});
        let spans = vec![
            SourceSpan {
                chunk: 0,
                start: 0,
                end: 1,
            },
            SourceSpan {
                chunk: 1,
                start: 0,
                end: 0,
            },
        ];
        let state = projection_state();
        let result = project(&request, &spans, &state).unwrap();
        assert_eq!(result["source"][0], request["source"][0]);
        assert_eq!(result["assembled"], request["assembled"]);
        assert_eq!(result["required_coverage"], request["required_coverage"]);
        assert_eq!(result["source"][1]["lines"], json!([[0, "reference"]]));
        assert_eq!(
            result["raw_dom_provenance"][1]["evidence"]["elements"],
            json!([[99, "p", "", null, null]])
        );
        assert!(
            project(&request, &spans[1..], &state)
                .unwrap_err()
                .contains("target line")
        );
        let mut outside = spans;
        outside.push(SourceSpan {
            chunk: 9,
            start: 0,
            end: 0,
        });
        assert!(
            project(&request, &outside, &state)
                .unwrap_err()
                .contains("source absent")
        );
    }
}
