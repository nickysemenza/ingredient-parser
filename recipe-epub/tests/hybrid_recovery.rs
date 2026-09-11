//! Behavioral tests at the portable scheduler seam: no providers or money.
#![allow(clippy::unwrap_used)]
use recipe_epub::{
    Chunk,
    hybrid::HybridStrategy,
    recovery::{Action, State},
};
use serde_json::{Value, json};

fn chunk(path: &str) -> Chunk {
    Chunk {
        doc_path: path.into(),
        text: "Soup\n1 cup water\nSimmer.\nServe hot.".into(),
        title_hint: None,
        links: vec![],
        images: vec![],
    }
}
fn state() -> State {
    State::new_with_strategy(
        vec![chunk("soup.xhtml")],
        "claude-haiku-4-5",
        10.0,
        HybridStrategy::Hybrid,
    )
    .unwrap()
}
fn payload(omit_last: bool) -> Value {
    json!({"recipes":[{
        "title":{"text":"Soup","spans":[{"start":0,"end":0}]},
        "description":[],"recipe_yield":[],"notes":[],"equipment":[],
        "sections":[{"name":{"text":"","spans":[]},"ingredients":[{"start":1,"end":1}],"instructions":[{"start":2,"end":if omit_last {2}else{3}}]}]
    }],"ignored":[]})
}

fn component_chunk() -> Chunk {
    Chunk {
        doc_path: "components.xhtml".into(),
        text: "Component dish\nSauce\n1 cup sauce base\nSalad\n1 cup salad base\n\nMake the sauce.\nPrepare the salad.\nServe the components together.".into(),
        title_hint: None,
        links: vec![],
        images: vec![],
    }
}

fn component_payload() -> Value {
    json!({"recipes":[{
        "title":{"text":"Component dish","spans":[{"start":0,"end":0}]},
        "description":[],"recipe_yield":[],"notes":[],"equipment":[],
        "sections":[
            {"name":{"text":"Salad","spans":[{"start":3,"end":3}]},"ingredients":[{"start":4,"end":4}],"instructions":[{"start":7,"end":7}]},
            {"name":{"text":"","spans":[]},"ingredients":[],"instructions":[{"start":8,"end":8}]},
            {"name":{"text":"Sauce","spans":[{"start":1,"end":1}]},"ingredients":[{"start":2,"end":2}],"instructions":[{"start":6,"end":6}]}
        ]
    }],"ignored":[{"start":5,"end":5}]})
}

fn component_payload_with_empty_tail() -> Value {
    let mut payload = component_payload();
    payload["recipes"][0]["sections"]
        .as_array_mut()
        .unwrap()
        .push(json!({"name":{"text":"","spans":[]},"ingredients":[],"instructions":[]}));
    payload
}
fn verdict(state: &State, action: &Action, corrections: Value) -> Value {
    let candidate = &state.groups[action.group].candidates[action.candidate];
    // Hybrid audit actions now carry the compact, assignment-ID wire contract.
    // Keep the older correction fixtures readable, but lower them here as a
    // provider would; production only accepts IDs and never infers one from a
    // returned assignment object.
    let request: Value = serde_json::from_str(&action.request.user).unwrap();
    let contract = request.get("correction_contract").and_then(Value::as_str);
    let compact = matches!(
        contract,
        Some(
            "hybrid-audit-corrections-v2"
                | "hybrid-audit-corrections-v3"
                | "hybrid-audit-corrections-v4"
        )
    );
    let corrections = if compact {
        corrections
            .as_array()
            .unwrap()
            .iter()
            .map(|correction| {
                if correction.get("assignment_id").is_some() {
                    return correction.clone();
                }
                let mut wire = correction.clone();
                let assignment_id = |assignment: &Value| {
                    candidate
                        .hybrid_assignments
                        .iter()
                        .position(|known| serde_json::to_value(known).unwrap() == *assignment)
                        .unwrap()
                };
                match correction["kind"].as_str().unwrap() {
                    "move_assignment" => {
                        let from = &correction["from"];
                        let to = &correction["to"];
                        wire = json!({
                            "kind":"move_assignment",
                            "assignment_id":assignment_id(from),
                            "target":{"recipe":to["recipe"],"section":to["section"],"field":to["field"]},
                            "reason":correction["reason"],
                        });
                        if to["spans"] != from["spans"] {
                            wire["spans"] = to["spans"].clone();
                        }
                    }
                    "restore_span" => {
                        wire = json!({"kind":"restore_span","assignment_id":assignment_id(&correction["assignment"]),"span":correction["span"],"reason":correction["reason"]});
                    }
                    "replace_bounded_text" => {
                        wire = json!({"kind":"replace_bounded_text","assignment_id":assignment_id(&correction["assignment"]),"after":correction["after"],"reason":correction["reason"]});
                    }
                    _ => {}
                }
                wire
            })
            .collect::<Vec<_>>()
            .into()
    } else {
        corrections
    };
    let corrections = if matches!(
        contract,
        Some("hybrid-audit-corrections-v3" | "hybrid-audit-corrections-v4")
    ) {
        let mut grouped = json!({"move_assignment":[],"restore_span":[],"replace_bounded_text":[],"split_section":[],"merge_sections":[]});
        for (order, entry) in corrections.as_array().unwrap().iter().enumerate() {
            let mut entry = entry.clone();
            let kind = entry.as_object_mut().unwrap().remove("kind").unwrap();
            entry["order"] = json!(order);
            grouped[kind.as_str().unwrap()]
                .as_array_mut()
                .unwrap()
                .push(entry);
        }
        grouped
    } else {
        corrections
    };
    let mut response = json!({
        "source_sha256":recipe_epub::recovery::canonical_source_sha256(&state.source).unwrap(),
        "candidate_revision":candidate.revision,
        "group":action.group,
        "coverage":state.groups[action.group].chunks.iter().map(|chunk| json!({"chunk":chunk,"start":0,"end":state.source[*chunk].text.lines().count()-1})).collect::<Vec<_>>(),
        "findings":[],"corrections":corrections,
    });
    let action_bound = serde_json::from_str::<Value>(&action.request.user)
        .ok()
        .and_then(|request| request.get("audit_response_contract").cloned())
        .is_some();
    if action_bound {
        response.as_object_mut().unwrap().remove("source_sha256");
        response
            .as_object_mut()
            .unwrap()
            .remove("candidate_revision");
        response.as_object_mut().unwrap().remove("group");
        let grouped = response
            .as_object_mut()
            .unwrap()
            .remove("corrections")
            .unwrap();
        for operation in [
            "move_assignment",
            "restore_span",
            "replace_bounded_text",
            "split_section",
            "merge_sections",
        ] {
            response[operation] = grouped[operation].clone();
        }
    }
    response
}

/// The current action-bound audit response does not ask the provider to copy
/// request identity. Corrections are flat root operation lists so their field
/// shape remains visible to weak tool-schema providers.
fn flat_audit_response(state: &State, action: &Action, corrections: Value) -> Value {
    verdict(state, action, corrections)
}

fn assert_audit_schema_binds_request_identity(action: &Action) {
    let request: Value = serde_json::from_str(&action.request.user).unwrap();
    let properties = &action.request.tool_schema["properties"];
    assert_eq!(
        request["audit_response_contract"],
        "hybrid-audit-response-v2"
    );
    assert!(request["source_sha256"].is_string());
    assert!(request["candidate_revision"].is_u64());
    assert!(request["group"].is_u64());
    for identity in [
        "source_sha256",
        "candidate_revision",
        "group",
        "corrections",
    ] {
        assert!(properties.get(identity).is_none(), "{identity}");
    }
}

#[test]
fn audit_correction_operations_require_their_own_arguments() {
    let mut state = state();
    let extraction = state.next_action().unwrap().unwrap();
    state.apply(&extraction, payload(true)).unwrap();
    let audit = state.next_action().unwrap().unwrap();
    let restore = &audit.request.tool_schema["properties"]["restore_span"]["items"];
    for field in ["assignment_id", "span", "reason", "order"] {
        assert!(
            restore["required"]
                .as_array()
                .unwrap()
                .contains(&json!(field))
        );
    }
    assert!(restore["properties"].get("target").is_none());
    let mut response = verdict(&state, &audit, json!([]));
    // Captured failure: a restore supplied a move target and omitted its ID.
    response["restore_span"] = json!([{
        "order":0,"span":{"chunk":0,"start":3,"end":3},
        "target":{"recipe":0,"section":0,"field":"instructions"},
        "reason":"Restore the serving step."
    }]);
    let before = serde_json::to_value(&state.groups[0].candidates[0]).unwrap();
    assert!(state.apply(&audit, response).is_err());
    assert_eq!(
        serde_json::to_value(&state.groups[0].candidates[0]).unwrap(),
        before
    );
}

#[test]
fn action_bound_flat_audit_response_applies_without_model_copied_identity() {
    let mut state = state();
    let extraction = state.next_action().unwrap().unwrap();
    state.apply(&extraction, payload(true)).unwrap();
    let audit = state.next_action().unwrap().unwrap();
    let instruction = state.groups[0].candidates[0]
        .hybrid_assignments
        .iter()
        .find(|assignment| assignment.field == "instructions")
        .unwrap()
        .clone();
    let response = flat_audit_response(
        &state,
        &audit,
        json!([{
            "kind":"restore_span","assignment":instruction,
            "span":{"chunk":0,"start":3,"end":3},
            "reason":"Restore the serving instruction."
        }]),
    );

    assert!(response.get("source_sha256").is_none());
    assert!(response.get("candidate_revision").is_none());
    assert!(response.get("group").is_none());
    state.apply(&audit, response).unwrap();
    assert_eq!(
        state.groups[0].candidates[0].outputs[0].as_ref().unwrap()[0].sections[0].instructions,
        ["Simmer.", "Serve hot."]
    );
    assert!(state.groups[0].accepted.is_none());
}

#[test]
fn action_bound_flat_audit_rejects_stale_action_and_mixed_correction_locations_atomically() {
    let mut state = state();
    let extraction = state.next_action().unwrap().unwrap();
    state.apply(&extraction, payload(false)).unwrap();
    let audit = state.next_action().unwrap().unwrap();
    let response = flat_audit_response(&state, &audit, json!([]));
    state.groups[0].candidates[0].revision += 1;
    let before = serde_json::to_value(&state.groups[0].candidates[0]).unwrap();
    assert!(state.apply(&audit, response).is_err());
    assert_eq!(
        serde_json::to_value(&state.groups[0].candidates[0]).unwrap(),
        before
    );

    let audit = state.next_action().unwrap().unwrap();
    let mut mixed = flat_audit_response(&state, &audit, json!([]));
    mixed["corrections"] = json!({
        "move_assignment":[],"restore_span":[],"replace_bounded_text":[],
        "split_section":[],"merge_sections":[]
    });
    let before = serde_json::to_value(&state.groups[0].candidates[0]).unwrap();
    assert!(state.apply(&audit, mixed).is_err());
    assert_eq!(
        serde_json::to_value(&state.groups[0].candidates[0]).unwrap(),
        before
    );
}

#[test]
fn action_bound_flat_audit_rejects_unrelated_source_change() {
    let mut state = State::new_with_strategy(
        vec![chunk("soup.xhtml"), chunk("copyright.xhtml")],
        "claude-haiku-4-5",
        10.0,
        HybridStrategy::Hybrid,
    )
    .unwrap();
    state.groups[1].enabled = false;
    let extraction = state.next_action().unwrap().unwrap();
    state.apply(&extraction, payload(false)).unwrap();
    let audit = state.next_action().unwrap().unwrap();
    let response = flat_audit_response(&state, &audit, json!([]));
    state.source[1]
        .text
        .push_str("\nChanged source outside the audit group.");
    let before = serde_json::to_value(&state.groups[0].candidates[0]).unwrap();
    assert!(state.apply(&audit, response).is_err());
    assert_eq!(
        serde_json::to_value(&state.groups[0].candidates[0]).unwrap(),
        before
    );
}

#[test]
fn markerless_audit_response_keeps_echo_identity_compatibility() {
    let mut state = state();
    let extraction = state.next_action().unwrap().unwrap();
    state.apply(&extraction, payload(false)).unwrap();
    let mut legacy = state.next_action().unwrap().unwrap();
    let mut request: Value = serde_json::from_str(&legacy.request.user).unwrap();
    request
        .as_object_mut()
        .unwrap()
        .remove("audit_response_contract");
    legacy.request.user = request.to_string();
    state
        .apply(&legacy, verdict(&state, &legacy, json!([])))
        .unwrap();
    assert_eq!(state.groups[0].accepted, Some(0));
}

#[test]
fn hybrid_state_apply_keeps_named_and_shared_component_methods_at_canonical_sections() {
    let mut state = State::new_with_strategy(
        vec![component_chunk()],
        "claude-haiku-4-5",
        10.0,
        HybridStrategy::Hybrid,
    )
    .unwrap();
    let extraction = state.next_action().unwrap().unwrap();
    state.apply(&extraction, component_payload()).unwrap();

    let candidate = &state.groups[0].candidates[0];
    assert!(candidate.hybrid_assignments.iter().any(|assignment| {
        assignment.section == Some(0)
            && assignment.field == "instructions"
            && assignment.spans
                == vec![recipe_epub::hybrid::SourceSpan {
                    chunk: 0,
                    start: 7,
                    end: 7,
                }]
    }));
    assert!(candidate.hybrid_assignments.iter().any(|assignment| {
        assignment.section == Some(2)
            && assignment.field == "instructions"
            && assignment.spans
                == vec![recipe_epub::hybrid::SourceSpan {
                    chunk: 0,
                    start: 6,
                    end: 6,
                }]
    }));
    let output = candidate.outputs[0].as_ref().unwrap();
    assert_eq!(output[0].sections.len(), 3);
    assert_eq!(output[0].sections[0].name.as_deref(), Some("Salad"));
    assert_eq!(output[0].sections[0].instructions, ["Prepare the salad."]);
    assert_eq!(output[0].sections[1].name, None);
    assert_eq!(
        output[0].sections[1].instructions,
        ["Serve the components together."]
    );
    assert_eq!(output[0].sections[2].name.as_deref(), Some("Sauce"));
    assert_eq!(output[0].sections[2].instructions, ["Make the sauce."]);
}

#[test]
fn historical_hybrid_child_rebuilds_section_layout_from_keyed_raw_output_once() {
    let mut state = State::new_with_strategy(
        vec![component_chunk()],
        "claude-haiku-4-5",
        10.0,
        HybridStrategy::Hybrid,
    )
    .unwrap();
    let extraction = state.next_action().unwrap().unwrap();
    state.reserve(&extraction).unwrap();
    state.apply(&extraction, component_payload()).unwrap();
    state.attempts[0].response = Some(component_payload());
    state.attempts[0].pending = false;

    // Emulate the historical positional normalizer: it drained the named
    // component methods into an unnamed section while canonical assignments
    // still pointed to the named component slots.
    let candidate = &mut state.groups[0].candidates[0];
    let output = candidate.outputs[0].as_mut().unwrap();
    let methods = output[0]
        .sections
        .iter()
        .flat_map(|section| section.instructions.clone())
        .collect::<Vec<_>>();
    for section in &mut output[0].sections {
        section.instructions.clear();
    }
    output[0].sections[1].instructions = methods;
    let outputs_before = candidate.outputs.clone();
    let revision_before = candidate.revision;
    candidate.verified = true;
    state.groups[0].accepted = Some(0);

    assert!(state.migrate_historical_hybrid_assembly().unwrap());
    let candidate = &state.groups[0].candidates[0];
    let output = candidate.outputs[0].as_ref().unwrap();
    assert_eq!(candidate.revision, revision_before + 1);
    assert!(!candidate.verified);
    assert_eq!(state.groups[0].accepted, None);
    assert_eq!(output[0].sections[0].instructions, ["Prepare the salad."]);
    assert_eq!(
        output[0].sections[1].instructions,
        ["Serve the components together."]
    );
    assert_eq!(output[0].sections[2].instructions, ["Make the sauce."]);
    assert_eq!(candidate.hybrid_assembly_migrations.len(), 1);
    assert_eq!(
        candidate.hybrid_assembly_migrations[0].outputs_before,
        outputs_before
    );
    let migration_count = candidate.hybrid_assembly_migrations.len();
    let _ = candidate;
    assert!(!state.migrate_historical_hybrid_assembly().unwrap());
    assert_eq!(
        state.groups[0].candidates[0]
            .hybrid_assembly_migrations
            .len(),
        migration_count
    );
}

#[test]
fn historical_hybrid_migration_preserves_explicit_empty_trailing_section_after_override() {
    let mut state = State::new_with_strategy(
        vec![component_chunk()],
        "claude-haiku-4-5",
        10.0,
        HybridStrategy::Hybrid,
    )
    .unwrap();
    let extraction = state.next_action().unwrap().unwrap();
    state.reserve(&extraction).unwrap();
    state
        .apply(&extraction, component_payload_with_empty_tail())
        .unwrap();
    state.attempts[0].response = Some(component_payload_with_empty_tail());
    state.attempts[0].pending = false;

    let candidate = &mut state.groups[0].candidates[0];
    let title = candidate
        .hybrid_assignments
        .iter()
        .find(|assignment| assignment.field == "title")
        .unwrap()
        .clone();
    candidate
        .hybrid_text_overrides
        .push(recipe_epub::hybrid::TextOverride {
            assignment: title,
            before: "Component dish".into(),
            after: "Component dish".into(),
            spans: vec![recipe_epub::hybrid::SourceSpan {
                chunk: 0,
                start: 0,
                end: 0,
            }],
            reason: "historical source-supported normalization".into(),
        });
    let methods = candidate.outputs[0].as_ref().unwrap()[0]
        .sections
        .iter()
        .flat_map(|section| section.instructions.clone())
        .collect::<Vec<_>>();
    for section in &mut candidate.outputs[0].as_mut().unwrap()[0].sections {
        section.instructions.clear();
    }
    candidate.outputs[0].as_mut().unwrap()[0].sections[1].instructions = methods;

    assert!(state.migrate_historical_hybrid_assembly().unwrap());
    let sections = &state.groups[0].candidates[0].outputs[0].as_ref().unwrap()[0].sections;
    assert_eq!(sections.len(), 4);
    assert_eq!(state.groups[0].candidates[0].hybrid_text_overrides.len(), 1);
    assert!(sections[3].name.is_none());
    assert!(sections[3].ingredients.is_empty());
    assert!(sections[3].instructions.is_empty());
}

#[test]
fn historical_hybrid_migration_keeps_consistent_cached_output_without_raw_attempt() {
    let mut state = State::new_with_strategy(
        vec![component_chunk()],
        "claude-haiku-4-5",
        10.0,
        HybridStrategy::Hybrid,
    )
    .unwrap();
    let extraction = state.next_action().unwrap().unwrap();
    state
        .apply(&extraction, component_payload_with_empty_tail())
        .unwrap();

    assert!(state.attempts.is_empty());
    assert!(!state.migrate_historical_hybrid_assembly().unwrap());
    assert!(
        state.groups[0].candidates[0]
            .hybrid_assembly_migrations
            .is_empty()
    );
}

#[test]
fn historical_hybrid_assembly_contract_invalidates_acceptance_without_losing_evidence() {
    let mut current = state();
    let action = current.next_action().unwrap().unwrap();
    let attempt = current.reserve(&action).unwrap();
    current.apply(&action, payload(false)).unwrap();
    current.settle(attempt, None, Some(payload(false)), None);
    current.groups[0].candidates[0].verified = true;
    current.groups[0].accepted = Some(0);
    assert!(current.complete());
    let mut historical = serde_json::to_value(&current).unwrap();
    historical
        .as_object_mut()
        .unwrap()
        .remove("hybrid_assembly_contract");
    let mut historical: State = serde_json::from_value(historical).unwrap();
    let mut indexed = historical.clone();
    indexed.strategy = HybridStrategy::Indexed;
    assert!(!indexed.needs_migration());
    assert!(indexed.complete());
    assert!(historical.needs_migration());
    assert!(!historical.complete());
    assert!(historical.migrate_legacy());
    assert!(!historical.needs_migration());
    assert_eq!(historical.groups[0].accepted, None);
    assert!(!historical.groups[0].candidates[0].verified);
    assert_eq!(
        historical.groups[0].candidates[0].outputs,
        current.groups[0].candidates[0].outputs
    );
    assert_eq!(
        historical.attempts[0].response,
        current.attempts[0].response
    );
    assert_eq!(
        historical.attempts[0].reservation_usd,
        current.attempts[0].reservation_usd
    );
    assert!(historical.attempts[0].inherited);
    assert!(historical.attempts[0].usage.is_none());
}

#[test]
fn historical_hybrid_migration_rejects_drift_without_raw_response_atomically() {
    let mut state = State::new_with_strategy(
        vec![component_chunk()],
        "claude-haiku-4-5",
        10.0,
        HybridStrategy::Hybrid,
    )
    .unwrap();
    let extraction = state.next_action().unwrap().unwrap();
    state.apply(&extraction, component_payload()).unwrap();
    state.groups[0].candidates[0].outputs[0].as_mut().unwrap()[0].sections[0]
        .instructions
        .clear();
    let before = serde_json::to_value(&state).unwrap();

    assert!(state.migrate_historical_hybrid_assembly().is_err());
    assert_eq!(serde_json::to_value(&state).unwrap(), before);
}

fn context_state() -> State {
    context_state_with_omission(false)
}

fn context_state_with_omission(omit_last: bool) -> State {
    context_state_with_link(omit_last, "soup.xhtml#soup")
}

fn context_state_with_link(omit_last: bool, href: &str) -> State {
    let link = recipe_epub::Link {
        text: "Soup".into(),
        href: href.into(),
    };
    let mut reference = chunk("reference.xhtml");
    reference.text = "Reference\nServes 2\n1 cup water\nCook gently.".into();
    reference.links.push(link.clone());
    let mut state = State::new_with_strategy(
        vec![chunk("soup.xhtml"), reference],
        "claude-haiku-4-5",
        10.0,
        HybridStrategy::Hybrid,
    )
    .unwrap();
    let provenance = state
        .source
        .iter()
        .enumerate()
        .map(|(chunk, source)| {
            source
                .text
                .lines()
                .enumerate()
                .map(|(line, _)| recipe_epub::SourceLine {
                    document_line: line,
                    contributors: vec![recipe_epub::SourceElement {
                        element_index: line,
                        tag: "p".into(),
                        classes: String::new(),
                        anchor: (chunk == 0 && line == 0).then(|| "soup".into()),
                        ancestors: vec![],
                    }],
                    anchors: vec![],
                    links: if chunk == 1 && line == 1 {
                        vec![link.clone()]
                    } else {
                        vec![]
                    },
                    images: vec![],
                    transformed: false,
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    state.bind_source_line_provenance(&provenance).unwrap();
    state.groups[1].enabled = false;
    let extraction = state.next_action().unwrap().unwrap();
    state.apply(&extraction, payload(omit_last)).unwrap();
    state
}

fn expansion(state: &State, action: &Action) -> Value {
    let mut response = verdict(state, action, json!([]));
    response["coverage"] = json!([]);
    response["context_expansion"] =
        json!({"reason":"The surrounding reference may contain a preparation dependency."});
    response
}

#[test]
fn ambiguous_reciprocal_context_is_deferred_until_the_single_full_expansion() {
    let mut state = context_state_with_link(false, "soup.xhtml#missing");
    let audit = state.next_action().unwrap().unwrap();
    let request: Value = serde_json::from_str(&audit.request.user).unwrap();
    assert_eq!(request["context_mode"], "selected");
    assert_eq!(request["source"][0]["chunk"], 0);
    assert_eq!(request["source"].as_array().unwrap().len(), 1);
    assert_eq!(
        request["deferred_reciprocal_context"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert!(
        request["omitted_context_spans"]
            .as_array()
            .unwrap()
            .iter()
            .any(|span| span["chunk"] == 1)
    );

    let response = expansion(&state, &audit);
    state.apply(&audit, response).unwrap();
    let expanded = state.next_action().unwrap().unwrap();
    let request: Value = serde_json::from_str(&expanded.request.user).unwrap();
    assert_eq!(request["context_mode"], "full");
    assert!(
        request["source"]
            .as_array()
            .unwrap()
            .iter()
            .any(|source| source["chunk"] == 1)
    );
    assert!(
        request["deferred_reciprocal_context"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn audit_owner_coordinates_survive_continuation_assembly() {
    let mut continuation = chunk("soup.xhtml");
    continuation.title_hint = Some("Soup".into());
    let mut state = State::new_with_strategy(
        vec![chunk("soup.xhtml"), continuation],
        "claude-haiku-4-5",
        10.0,
        HybridStrategy::Hybrid,
    )
    .unwrap();
    assert_eq!(state.groups.len(), 1);
    for _ in 0..2 {
        let extraction = state.next_action().unwrap().unwrap();
        assert!(extraction.chunk.is_some());
        state.apply(&extraction, payload(false)).unwrap();
    }
    let audit = state.next_action().unwrap().unwrap();
    let request: Value = serde_json::from_str(&audit.request.user).unwrap();
    assert_eq!(
        request["source_line_columns"],
        json!({"target":["line","text","assignment_ids"],"context":["line","text"]})
    );
    assert_eq!(request["assembled"].as_array().unwrap().len(), 1);
    assert_eq!(request["owners"].as_array().unwrap().len(), 2);
    assert!(
        request["assignments"]
            .as_array()
            .unwrap()
            .iter()
            .all(|assignment| assignment["owner_chunk"].is_u64())
    );
    for chunk in 0..2 {
        assert_eq!(request["owners"][chunk]["chunk"], chunk);
        assert_eq!(request["owners"][chunk]["recipes"][0]["recipe"], 0);
        assert_eq!(request["owners"][chunk]["recipes"][0]["title"], "Soup");
        assert_eq!(
            request["owners"][chunk]["recipes"][0]["sections"][0],
            json!({"section":0,"name":""})
        );
        assert_eq!(
            request["owners"][chunk]["recipes"][0]["assembly"],
            json!({"status":"retained","recipe":0,"section_offset":chunk})
        );
    }
    assert!(audit.request.system.contains("CHUNK-LOCAL"));
    assert!(
        audit
            .request
            .system
            .contains("never derive correction coordinates from assembled indexes")
    );
}

#[test]
fn audit_owners_trace_retained_and_filtered_occurrences_without_guessing_metadata() {
    let source = Chunk {
        doc_path: "owners.xhtml".into(),
        text: "Soup\n1 cup water\nNoise".into(),
        title_hint: None,
        links: vec![],
        images: vec![],
    };
    let mut state = State::new_with_strategy(
        vec![source],
        "claude-haiku-4-5",
        10.0,
        HybridStrategy::Hybrid,
    )
    .unwrap();
    let extraction = state.next_action().unwrap().unwrap();
    state
        .apply(
            &extraction,
            json!({"recipes":[
                {"title":{"text":"Soup","spans":[{"start":0,"end":0}]},"description":[],"recipe_yield":[],"notes":[],"equipment":[],"sections":[{"name":{"text":"","spans":[]},"ingredients":[{"start":1,"end":1}],"instructions":[]}]},
                {"title":{"text":"Noise","spans":[{"start":2,"end":2}]},"description":[],"recipe_yield":[],"notes":[],"equipment":[],"sections":[{"name":{"text":"","spans":[]},"ingredients":[],"instructions":[]}]}
            ],"ignored":[]}),
        )
        .unwrap();
    let audit = state.next_action().unwrap().unwrap();
    let request: Value = serde_json::from_str(&audit.request.user).unwrap();
    let recipes = request["owners"][0]["recipes"].as_array().unwrap();
    assert_eq!(
        recipes[0]["assembly"],
        json!({"status":"retained","recipe":0,"section_offset":0})
    );
    assert_eq!(
        recipes[1]["assembly"],
        json!({"status":"dropped","reason":"ingredient_less"})
    );
    assert!(
        audit
            .request
            .system
            .contains("never a correction coordinate")
    );
    assert!(
        audit
            .request
            .system
            .contains("metadata field was contributed")
    );
}

#[test]
fn hybrid_audit_assembly_keeps_title_matched_references() {
    let source = Chunk {
        doc_path: "references.xhtml".into(),
        text: "Cake\n1 cup flour\nTart\n1 recipe Cake (this page)".into(),
        title_hint: None,
        links: vec![],
        images: vec![],
    };
    let mut state = State::new_with_strategy(
        vec![source],
        "claude-haiku-4-5",
        10.0,
        HybridStrategy::Hybrid,
    )
    .unwrap();
    let extraction = state.next_action().unwrap().unwrap();
    state
        .apply(
            &extraction,
            json!({"recipes":[
                {"title":{"text":"Cake","spans":[{"start":0,"end":0}]},"description":[],"recipe_yield":[],"notes":[],"equipment":[],"sections":[{"name":{"text":"","spans":[]},"ingredients":[{"start":1,"end":1}],"instructions":[]}]},
                {"title":{"text":"Tart","spans":[{"start":2,"end":2}]},"description":[],"recipe_yield":[],"notes":[],"equipment":[],"sections":[{"name":{"text":"","spans":[]},"ingredients":[{"start":3,"end":3}],"instructions":[]}]}
            ],"ignored":[]}),
        )
        .unwrap();
    let audit = state.next_action().unwrap().unwrap();
    let request: Value = serde_json::from_str(&audit.request.user).unwrap();
    assert_eq!(request["assembled"][1]["references"][0]["title"], "Cake");
}

#[test]
fn legacy_owner_migration_is_atomic_idempotent_and_keeps_ambiguous_candidates_auditable() {
    let mut continuation = chunk("soup.xhtml");
    continuation.title_hint = Some("Soup".into());
    let mut parent = State::new_with_strategy(
        vec![chunk("soup.xhtml"), continuation],
        "claude-haiku-4-5",
        10.0,
        HybridStrategy::Hybrid,
    )
    .unwrap();
    for _ in 0..2 {
        let extraction = parent.next_action().unwrap().unwrap();
        parent.apply(&extraction, payload(false)).unwrap();
    }

    // The first candidate already has explicit owners. The second reproduces a
    // legacy continuation record with an empty field that cannot establish a
    // chunk-local owner, so it must remain readable and review-needed.
    let mut ambiguous = parent.groups[0].candidates[0].clone();
    let empty_name = ambiguous
        .hybrid_assignments
        .iter_mut()
        .find(|assignment| assignment.field == "name" && assignment.spans.is_empty())
        .unwrap();
    empty_name.owner_chunk = None;
    parent.groups[0].candidates.push(ambiguous);
    let parent_bytes = serde_json::to_vec(&parent).unwrap();

    let mut child = parent.clone();
    assert!(child.migrate_historical_hybrid_assembly().unwrap());
    assert_eq!(serde_json::to_vec(&parent).unwrap(), parent_bytes);
    let ambiguous = &child.groups[0].candidates[1];
    assert!(
        ambiguous
            .hybrid_assignment_issues
            .iter()
            .any(|issue| { issue.kind == "unresolved_owner_chunk" })
    );
    assert_eq!(ambiguous.hybrid_owner_migrations.len(), 1);
    assert!(
        ambiguous.hybrid_owner_migrations[0]
            .unresolved
            .iter()
            .any(|issue| { issue.kind == "unresolved_owner_chunk" })
    );
    assert!(
        child.groups[0].candidates[0]
            .hybrid_assignments
            .iter()
            .all(|assignment| assignment.owner_chunk.is_some())
    );

    // The ambiguous record is not auto-patched or accepted, but it does not
    // block the complete group from scheduling its normal AI audit.
    let audit = child.next_action().unwrap().unwrap();
    assert!(audit.chunk.is_none());
    assert!(!child.migrate_historical_hybrid_assembly().unwrap());
    assert_eq!(
        child.groups[0].candidates[1].hybrid_owner_migrations.len(),
        1
    );
    let response = verdict(&child, &audit, json!([]));
    child.apply(&audit, response).unwrap();
    assert!(!child.groups[0].candidates[audit.candidate].verified);
    assert!(child.groups[0].accepted.is_none());
}

#[test]
fn audit_source_target_rows_expose_frozen_assignment_ids_without_annotating_context() {
    let target = Chunk {
        doc_path: "target.xhtml".into(),
        text: (0..28)
            .map(|line| {
                if line == 25 {
                    "Target title".into()
                } else if line == 26 {
                    "Target subtitle".into()
                } else {
                    format!("Context line {line}")
                }
            })
            .collect::<Vec<_>>()
            .join("\n"),
        title_hint: None,
        links: vec![],
        images: vec![],
    };
    let mut state = State::new_with_strategy(
        vec![target],
        "claude-haiku-4-5",
        10.0,
        HybridStrategy::Hybrid,
    )
    .unwrap();
    let extraction = state.next_action().unwrap().unwrap();
    state
        .apply(
            &extraction,
            json!({"recipes":[{
                "title":{"text":"Target title\nTarget subtitle","spans":[{"start":25,"end":26}]},
                "description":[],"recipe_yield":[],"notes":[],"equipment":[],
                "sections":[{"name":{"text":"","spans":[]},"ingredients":[],"instructions":[]}]
            }],"ignored":[{"start":0,"end":26}]}),
        )
        .unwrap();
    let audit = state.next_action().unwrap().unwrap();
    let request: Value = serde_json::from_str(&audit.request.user).unwrap();
    let assignments = state.groups[0].candidates[0].hybrid_assignments.clone();
    let title_id = assignments
        .iter()
        .position(|assignment| assignment.field == "title")
        .unwrap();
    let ignored_id = assignments
        .iter()
        .position(|assignment| assignment.field == "ignored")
        .unwrap();
    let lines = request["source"][0]["lines"].as_array().unwrap();
    assert_eq!(
        lines[25],
        json!([25, "Target title", [title_id, ignored_id]])
    );
    assert_eq!(
        lines[26],
        json!([26, "Target subtitle", [title_id, ignored_id]])
    );
    assert_eq!(lines[27], json!([27, "Context line 27", []]));
    assert!(
        request["assignments"]
            .as_array()
            .unwrap()
            .iter()
            .all(|assignment| assignment.get("spans").is_none())
    );
    assert!(
        request["assignments"]
            .as_array()
            .unwrap()
            .iter()
            .any(|assignment| {
                assignment["field"] == "description"
                    && assignment["owner_chunk"] == 0
                    && assignment["recipe"] == 0
                    && assignment["section"].is_null()
            })
    );
    assert!(
        audit
            .request
            .system
            .contains("sole provider-facing source-membership representation")
    );

    let mut with_context = context_state();
    let context_audit = with_context.next_action().unwrap().unwrap();
    let context_request: Value = serde_json::from_str(&context_audit.request.user).unwrap();
    assert!(
        context_request["source"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|entry| entry["target"] == json!(false))
            .flat_map(|entry| entry["lines"].as_array().unwrap())
            .all(|line| line.as_array().unwrap().len() == 2)
    );
}

#[test]
fn context_expansion_is_durable_bounded_and_never_audit_acceptance() {
    let mut state = context_state();
    let selected = state.next_action().unwrap().unwrap();
    let selected_request: Value = serde_json::from_str(&selected.request.user).unwrap();
    assert_eq!(selected_request["context_mode"], "selected");
    assert_eq!(
        selected.request.tool_schema["properties"]["coverage"]["minItems"],
        0
    );
    assert_eq!(
        selected_request["source"][1]["lines"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let original_output = state.groups[0].candidates[0].outputs.clone();
    state
        .apply(&selected, expansion(&state, &selected))
        .unwrap();
    assert!(!state.complete());
    assert!(state.groups[0].accepted.is_none());
    assert!(state.groups[0].candidates[0].hybrid_audits.is_empty());
    assert_eq!(
        state.groups[0].candidates[0].audit_context_expansions.len(),
        1
    );
    assert_eq!(state.groups[0].candidates[0].outputs, original_output);
    // Checkpoint round-trip must preserve the used allowance and full scope.
    let mut state: State = serde_json::from_value(serde_json::to_value(&state).unwrap()).unwrap();
    let full = state.next_action().unwrap().unwrap();
    assert_ne!(selected.key, full.key);
    let full_request: Value = serde_json::from_str(&full.request.user).unwrap();
    assert_eq!(full_request["context_mode"], "full");
    assert_eq!(
        full.request.tool_schema["properties"]["coverage"]["minItems"],
        1
    );
    assert_eq!(
        full_request["source"][1]["lines"].as_array().unwrap().len(),
        4
    );
    let before = serde_json::to_value(&state.groups[0].candidates[0]).unwrap();
    assert!(
        state
            .apply(&selected, verdict(&state, &selected, json!([])))
            .is_err()
    );
    assert_eq!(
        serde_json::to_value(&state.groups[0].candidates[0]).unwrap(),
        before
    );
    assert!(state.apply(&full, expansion(&state, &full)).is_err());
    assert_eq!(
        serde_json::to_value(&state.groups[0].candidates[0]).unwrap(),
        before
    );
    state
        .apply(&full, verdict(&state, &full, json!([])))
        .unwrap();
    assert_eq!(state.groups[0].accepted, Some(0));
    // The disabled referring group is context only; it has not been audited.
    assert!(!state.complete());
    state.begin_audit(true).unwrap();
    assert_eq!(
        state.groups[0].candidates[0].audit_context_expansions.len(),
        1
    );
    let fresh = state.next_action().unwrap().unwrap();
    let fresh_request: Value = serde_json::from_str(&fresh.request.user).unwrap();
    assert_eq!(fresh_request["context_mode"], "selected");
}

#[test]
fn expansion_does_not_consume_the_one_correction_and_reaudit_pass() {
    let mut state = context_state_with_omission(true);
    let selected = state.next_action().unwrap().unwrap();
    state
        .apply(&selected, expansion(&state, &selected))
        .unwrap();
    let full = state.next_action().unwrap().unwrap();
    let assignment = state.groups[0].candidates[0]
        .hybrid_assignments
        .iter()
        .find(|assignment| assignment.field == "instructions")
        .unwrap()
        .clone();
    let correction = json!([{"kind":"restore_span","assignment":assignment,"span":{"chunk":0,"start":3,"end":3},"reason":"Restore the omitted serving instruction."}]);
    state
        .apply(&full, verdict(&state, &full, correction))
        .unwrap();
    assert!(state.groups[0].accepted.is_none());
    let reaudit = state.next_action().unwrap().unwrap();
    let request: Value = serde_json::from_str(&reaudit.request.user).unwrap();
    assert_eq!(request["context_mode"], "full");
    assert_ne!(reaudit.key, full.key);
    state
        .apply(&reaudit, verdict(&state, &reaudit, json!([])))
        .unwrap();
    assert_eq!(state.groups[0].accepted, Some(0));
    let candidate = &state.groups[0].candidates[0];
    assert_eq!(candidate.audit_context_expansions.len(), 1);
    assert_eq!(candidate.hybrid_audits.len(), 2);
    assert!(candidate.hybrid_audits[1].reaudited);
    assert!(state.next_action().unwrap().is_none());
}

#[test]
fn changed_source_evidence_rejects_an_inflight_context_audit() {
    for change in ["navigation", "provenance", "documents"] {
        let mut state = context_state();
        let audit = state.next_action().unwrap().unwrap();
        let response = verdict(&state, &audit, json!([]));
        match change {
            "navigation" => {
                state.bind_navigation_documents(std::collections::BTreeSet::from([
                    "reference.xhtml".into(),
                ]));
            }
            "provenance" => {
                let mut provenance = state.source_line_provenance.clone();
                provenance[1][1].contributors[0].classes = "new-source-evidence".into();
                state.bind_source_line_provenance(&provenance).unwrap();
            }
            "documents" => {
                state
                    .bind_documents(&[recipe_epub::source::SourceDocument {
                        path: "reference.xhtml".into(),
                        blocks: vec![],
                        images: vec![],
                        anchors: vec![],
                    }])
                    .unwrap();
            }
            _ => unreachable!(),
        }
        let before = serde_json::to_value(&state.groups[0].candidates[0]).unwrap();
        assert!(state.apply(&audit, response).is_err(), "{change}");
        assert_eq!(
            serde_json::to_value(&state.groups[0].candidates[0]).unwrap(),
            before
        );
        assert!(state.groups[0].accepted.is_none());
    }
}

#[test]
fn invalid_or_legacy_context_expansion_is_atomic() {
    for mutation in [
        "stale",
        "coverage",
        "finding",
        "correction",
        "legacy",
        "empty_reason",
    ] {
        let mut state = context_state();
        let mut action = state.next_action().unwrap().unwrap();
        let mut response = expansion(&state, &action);
        match mutation {
            "stale" => response["candidate_revision"] = json!(999),
            "coverage" => response["coverage"] = json!([{"chunk":0,"start":0,"end":3}]),
            "finding" => {
                response["findings"] =
                    json!([{"kind":"missing","message":"need source","spans":[]}])
            }
            "correction" => response["move_assignment"] = json!([{"kind":"move_assignment"}]),
            "legacy" => {
                let mut request: Value = serde_json::from_str(&action.request.user).unwrap();
                request
                    .as_object_mut()
                    .unwrap()
                    .remove("audit_context_contract");
                action.request.user = request.to_string();
            }
            "empty_reason" => response["context_expansion"]["reason"] = json!(" "),
            _ => unreachable!(),
        }
        let before = serde_json::to_value(&state.groups[0].candidates[0]).unwrap();
        assert!(state.apply(&action, response).is_err(), "{mutation}");
        assert_eq!(
            serde_json::to_value(&state.groups[0].candidates[0]).unwrap(),
            before,
            "{mutation}"
        );
    }
}

#[test]
fn omitted_method_survives_extraction_and_requires_correction_then_reaudit() {
    let mut state = state();
    let extraction = state.next_action().unwrap().unwrap();
    state.apply(&extraction, payload(true)).unwrap();
    assert!(state.groups[0].candidates[0].outputs[0].is_some());
    assert!(
        !state.groups[0].candidates[0]
            .hybrid_assignment_issues
            .is_empty()
    );
    let audit = state.next_action().unwrap().unwrap();
    assert_eq!(audit.request.tool_name, "audit_recipe_group");
    assert_audit_schema_binds_request_identity(&audit);
    let destination_assignment_id = state.groups[0].candidates[0]
        .hybrid_assignments
        .iter()
        .position(|assignment| assignment.field == "instructions")
        .unwrap();
    let request: Value = serde_json::from_str(&audit.request.user).unwrap();
    assert_eq!(
        request["assignments"][destination_assignment_id]["field"],
        "instructions"
    );
    assert!(
        audit
            .request
            .system
            .contains("A restore's assignment_id is its destination field")
    );
    assert!(
        audit
            .request
            .system
            .contains("Shared methods belong in an unnamed main section")
    );
    let restore_schema =
        &audit.request.tool_schema["properties"]["restore_span"]["items"]["properties"];
    assert!(
        restore_schema["assignment_id"]["description"]
            .as_str()
            .is_some_and(|description| description.contains("destination assignment"))
    );
    assert!(
        restore_schema["span"]["description"]
            .as_str()
            .is_some_and(|description| description.contains("previously unassigned"))
    );
    // `Serve hot.` has no canonical source owner after extraction. Restore
    // addresses the *destination* instruction assignment, then adds this
    // otherwise unassigned source span.
    let patch = json!([{"kind":"restore_span","assignment_id":destination_assignment_id,"span":{"chunk":0,"start":3,"end":3},"reason":"The final required serving step was omitted."}]);
    let response = verdict(&state, &audit, patch);
    state.apply(&audit, response).unwrap();
    assert!(!state.complete());
    let candidate = &state.groups[0].candidates[0];
    assert!(candidate.hybrid_assignment_issues.is_empty());
    assert!(
        candidate.outputs[0].as_ref().unwrap()[0].sections[0]
            .instructions
            .iter()
            .any(|text| text.contains("Serve hot."))
    );
    assert!(!candidate.hybrid_correction_history.is_empty());
    let reaudit = state.next_action().unwrap().unwrap();
    assert!(reaudit.chunk.is_none());
    assert_ne!(audit.key, reaudit.key);
    let request: Value = serde_json::from_str(&reaudit.request.user).unwrap();
    assert_eq!(
        request["audit_response_contract"],
        "hybrid-audit-response-v2"
    );
    assert_eq!(request["context_mode"], "selected");
    let response = verdict(&state, &reaudit, json!([]));
    state.apply(&reaudit, response).unwrap();
    assert!(state.complete());
    assert!(state.next_action().unwrap().is_none());
}

#[test]
fn audit_correction_rejects_context_source_outside_the_frozen_group_atomically() {
    let mut state = context_state();
    let audit = state.next_action().unwrap().unwrap();
    let destination = state.groups[0].candidates[0]
        .hybrid_assignments
        .iter()
        .find(|assignment| assignment.field == "instructions")
        .unwrap()
        .clone();
    let before = serde_json::to_value(&state.groups[0].candidates[0]).unwrap();
    let response = flat_audit_response(
        &state,
        &audit,
        json!([{
            "kind":"restore_span",
            "assignment":destination,
            "span":{"chunk":1,"start":0,"end":0},
            "reason":"This is valid global source but belongs to linked context."
        }]),
    );

    let error = state.apply(&audit, response).unwrap_err();
    assert!(error.contains("restore span belongs to a different source chunk"));
    assert_eq!(
        serde_json::to_value(&state.groups[0].candidates[0]).unwrap(),
        before
    );
}

#[test]
fn corrected_reaudit_keeps_selected_context_and_cannot_request_another_expansion() {
    let mut state = context_state_with_omission(true);
    let audit = state.next_action().unwrap().unwrap();
    let first_request: Value = serde_json::from_str(&audit.request.user).unwrap();
    assert_eq!(first_request["correction_passes_remaining"], 1);
    assert!(
        audit.request.tool_schema["properties"]["move_assignment"]
            .get("maxItems")
            .is_none()
    );
    let destination = state.groups[0].candidates[0]
        .hybrid_assignments
        .iter()
        .find(|assignment| assignment.field == "instructions")
        .unwrap()
        .clone();
    let correction = flat_audit_response(
        &state,
        &audit,
        json!([{
            "kind":"restore_span",
            "assignment":destination,
            "span":{"chunk":0,"start":3,"end":3},
            "reason":"Restore the omitted serving instruction."
        }]),
    );
    state.apply(&audit, correction).unwrap();

    let reaudit = state.next_action().unwrap().unwrap();
    let request: Value = serde_json::from_str(&reaudit.request.user).unwrap();
    assert_eq!(request["correction_passes_remaining"], 0);
    for operation in [
        "move_assignment",
        "restore_span",
        "replace_bounded_text",
        "split_section",
        "merge_sections",
    ] {
        assert_eq!(
            reaudit.request.tool_schema["properties"][operation]["maxItems"],
            0
        );
    }
    assert_eq!(request["context_mode"], "selected");
    assert!(
        !request["omitted_context_spans"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(
        reaudit.request.tool_schema["properties"]
            .get("context_expansion")
            .is_none()
    );
    let target = request["source"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["target"] == json!(true))
        .unwrap();
    assert_eq!(target["lines"].as_array().unwrap().len(), 4);
    let context = request["source"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["target"] == json!(false))
        .unwrap();
    assert_eq!(context["lines"].as_array().unwrap().len(), 1);

    let mut response = flat_audit_response(&state, &reaudit, json!([]));
    response["findings"] = json!([{"kind":"wrong_field","message":"A remaining source-supported defect requires review.","spans":[{"chunk":0,"start":2,"end":2}]}]);
    state.apply(&reaudit, response).unwrap();
    assert!(!state.complete());
    assert!(state.next_action().unwrap().is_none());
    let candidate = &state.groups[0].candidates[0];
    assert!(candidate.hybrid_audits.last().unwrap().reaudited);
    assert_eq!(candidate.hybrid_audits.last().unwrap().findings.len(), 1);
    assert!(!candidate.hybrid_audits.last().unwrap().accepted);
}

#[test]
fn stale_and_incomplete_audit_evidence_cannot_accept_a_candidate() {
    let mut state = state();
    let extraction = state.next_action().unwrap().unwrap();
    state.apply(&extraction, payload(false)).unwrap();
    let audit = state.next_action().unwrap().unwrap();
    let before = serde_json::to_value(&state.groups[0].candidates[0]).unwrap();
    let mut stale = verdict(&state, &audit, json!([]));
    stale["candidate_revision"] = json!(999);
    assert!(state.apply(&audit, stale).is_err());
    assert_eq!(
        serde_json::to_value(&state.groups[0].candidates[0]).unwrap(),
        before
    );
    let mut missing = verdict(&state, &audit, json!([]));
    missing["coverage"][0]["end"] = json!(2);
    assert!(state.apply(&audit, missing).is_err());
    assert!(!state.complete());
    let assignment = state.groups[0].candidates[0]
        .hybrid_assignments
        .iter()
        .find(|assignment| assignment.field == "ingredients")
        .unwrap()
        .clone();
    let invalid_patch = verdict(
        &state,
        &audit,
        json!([{
            "kind":"replace_bounded_text","assignment":assignment,
            "before":"1 cup water","after":"2 cups water",
            "spans":[{"chunk":0,"start":1,"end":1}],"reason":"Unsupported amount change","source_supported":true
        }]),
    );
    assert!(state.apply(&audit, invalid_patch).is_err());
    assert_eq!(
        serde_json::to_value(&state.groups[0].candidates[0]).unwrap(),
        before
    );
}

#[test]
fn audit_without_correction_does_not_accept_unapplied_patch() {
    let mut state = state();
    let extraction = state.next_action().unwrap().unwrap();
    state.apply(&extraction, payload(false)).unwrap();
    state.begin_audit(false).unwrap();
    let audit = state.next_action().unwrap().unwrap();
    let assignment = state.groups[0].candidates[0]
        .hybrid_assignments
        .iter()
        .find(|assignment| assignment.field == "instructions")
        .unwrap()
        .clone();
    let response = verdict(
        &state,
        &audit,
        json!([{"kind":"restore_span","assignment":assignment,"span":{"chunk":0,"start":3,"end":3},"reason":"Needs review"}]),
    );
    state.apply(&audit, response).unwrap();
    assert!(!state.complete());
    assert!(
        state.groups[0].candidates[0]
            .hybrid_correction_history
            .is_empty()
    );
    assert!(state.next_action().unwrap().is_none());
}

#[test]
fn independent_groups_do_not_report_each_others_source_as_missing() {
    let mut state = State::new_with_strategy(
        vec![chunk("a.xhtml"), chunk("b.xhtml")],
        "claude-haiku-4-5",
        10.0,
        HybridStrategy::Hybrid,
    )
    .unwrap();
    assert_eq!(state.groups.len(), 2);
    let mut audits = 0;
    for _ in 0..6 {
        let Some(action) = state.next_action().unwrap() else {
            break;
        };
        let response = if action.chunk.is_some() {
            payload(false)
        } else {
            audits += 1;
            assert!(
                state.groups[action.group].candidates[action.candidate]
                    .hybrid_assignment_issues
                    .is_empty()
            );
            verdict(&state, &action, json!([]))
        };
        state.apply(&action, response).unwrap();
    }
    assert_eq!(audits, 2);
    assert!(state.complete());
}

#[test]
fn multi_chunk_group_is_audited_once_with_complete_line_coverage() {
    let mut a = chunk("a.xhtml");
    let mut b = chunk("b.xhtml");
    a.title_hint = Some("Soup".into());
    b.title_hint = Some("Soup".into());
    let mut state =
        State::new_with_strategy(vec![a, b], "claude-haiku-4-5", 10.0, HybridStrategy::Hybrid)
            .unwrap();
    assert_eq!(state.groups.len(), 1);
    for _ in 0..2 {
        let extraction = state.next_action().unwrap().unwrap();
        assert!(extraction.chunk.is_some());
        state.apply(&extraction, payload(false)).unwrap();
    }
    let audit = state.next_action().unwrap().unwrap();
    assert!(audit.chunk.is_none());
    let response = verdict(&state, &audit, json!([]));
    assert_eq!(response["coverage"].as_array().unwrap().len(), 2);
    state.apply(&audit, response).unwrap();
    assert!(state.complete());
    assert!(state.next_action().unwrap().is_none());
}

#[test]
fn moving_an_assignment_removes_the_old_field_text() {
    let mut state = state();
    let extraction = state.next_action().unwrap().unwrap();
    state.apply(&extraction, payload(false)).unwrap();
    let audit = state.next_action().unwrap().unwrap();
    let from = state.groups[0].candidates[0]
        .hybrid_assignments
        .iter()
        .find(|assignment| assignment.field == "instructions")
        .unwrap()
        .clone();
    let mut to = from.clone();
    to.field = "notes".into();
    to.section = None;
    let response = verdict(
        &state,
        &audit,
        json!([{"kind":"move_assignment","from":from,"to":to,"reason":"Test reassignment mechanics; semantic acceptance requires the next audit."}]),
    );
    state.apply(&audit, response).unwrap();
    let output = &state.groups[0].candidates[0].outputs[0].as_ref().unwrap()[0];
    assert!(
        output
            .sections
            .iter()
            .all(|section| section.instructions.is_empty())
    );
    assert_eq!(output.meta.notes, ["Simmer.", "Serve hot."]);
    assert!(!state.complete());
}

#[rstest::rstest]
#[case(None)]
#[case(Some("hybrid-audit-corrections-v2"))]
fn legacy_saved_audit_actions_still_decode_corrections(#[case] contract: Option<&str>) {
    let mut state = state();
    let extraction = state.next_action().unwrap().unwrap();
    state.apply(&extraction, payload(false)).unwrap();
    let mut legacy = state.next_action().unwrap().unwrap();
    let mut request: Value = serde_json::from_str(&legacy.request.user).unwrap();
    request
        .as_object_mut()
        .unwrap()
        .remove("correction_contract");
    if let Some(contract) = contract {
        request["correction_contract"] = json!(contract);
    }
    request
        .as_object_mut()
        .unwrap()
        .remove("audit_context_contract");
    request
        .as_object_mut()
        .unwrap()
        .remove("audit_response_contract");
    legacy.request.user = request.to_string();
    let from = state.groups[0].candidates[0]
        .hybrid_assignments
        .iter()
        .find(|a| a.field == "instructions")
        .unwrap()
        .clone();
    let mut to = from.clone();
    to.section = None;
    to.field = "notes".into();
    let response = verdict(
        &state,
        &legacy,
        json!([{"kind":"move_assignment","from":from,"to":to,"reason":"Legacy replay mechanics"}]),
    );
    state.apply(&legacy, response).unwrap();
    let recipe = &state.groups[0].candidates[0].outputs[0].as_ref().unwrap()[0];
    assert_eq!(recipe.meta.notes, ["Simmer.", "Serve hot."]);
    assert!(!state.complete());
}

#[test]
fn compact_partial_title_move_preserves_the_remaining_title_and_source_content() {
    let mut source = chunk("subtitle.xhtml");
    source.text = "English Soup\nThai subtitle\n1 cup water\nSimmer.\nServe hot.".into();
    let mut state = State::new_with_strategy(
        vec![source],
        "claude-haiku-4-5",
        10.0,
        HybridStrategy::Hybrid,
    )
    .unwrap();
    let extraction = state.next_action().unwrap().unwrap();
    state
        .apply(
            &extraction,
            json!({"recipes":[{
                "title":{"text":"English Soup\nThai subtitle","spans":[{"start":0,"end":1}]},
                "description":[],"recipe_yield":[],"notes":[],"equipment":[],
                "sections":[{"name":{"text":"","spans":[]},"ingredients":[{"start":2,"end":2}],"instructions":[{"start":3,"end":4}]}]
            }],"ignored":[]}),
        )
        .unwrap();
    let audit = state.next_action().unwrap().unwrap();
    let title_id = state.groups[0].candidates[0]
        .hybrid_assignments
        .iter()
        .position(|assignment| assignment.field == "title")
        .unwrap();
    let response = verdict(
        &state,
        &audit,
        json!([{
            "kind":"move_assignment","assignment_id":title_id,
            "target":{"recipe":0,"section":null,"field":"description"},
            "spans":[{"chunk":0,"start":1,"end":1}],
            "reason":"The translated subtitle is descriptive context."
        }]),
    );
    state.apply(&audit, response).unwrap();
    let recipe = &state.groups[0].candidates[0].outputs[0].as_ref().unwrap()[0];
    assert_eq!(recipe.meta.title, "English Soup");
    assert_eq!(recipe.meta.description.as_deref(), Some("Thai subtitle"));
    assert_eq!(recipe.sections[0].ingredients, ["1 cup water"]);
    assert_eq!(recipe.sections[0].instructions, ["Simmer.", "Serve hot."]);
    assert_eq!(
        state.source[0].text,
        "English Soup\nThai subtitle\n1 cup water\nSimmer.\nServe hot."
    );
    assert!(
        !state.groups[0].candidates[0]
            .hybrid_correction_history
            .is_empty()
    );
}

#[test]
fn compact_assignment_id_and_stale_evidence_are_rejected_atomically() {
    let mut state = state();
    let extraction = state.next_action().unwrap().unwrap();
    state.apply(&extraction, payload(false)).unwrap();
    let audit = state.next_action().unwrap().unwrap();
    let before = serde_json::to_value(&state.groups[0].candidates[0]).unwrap();
    let invalid_id = verdict(
        &state,
        &audit,
        json!([{"kind":"restore_span","assignment_id":999,"span":{"chunk":0,"start":3,"end":3},"reason":"Bad frozen ID."}]),
    );
    assert!(state.apply(&audit, invalid_id).is_err());
    assert_eq!(
        serde_json::to_value(&state.groups[0].candidates[0]).unwrap(),
        before
    );

    let mut stale = verdict(&state, &audit, json!([]));
    stale["candidate_revision"] = json!(12345);
    assert!(state.apply(&audit, stale).is_err());
    assert_eq!(
        serde_json::to_value(&state.groups[0].candidates[0]).unwrap(),
        before
    );
}

#[test]
fn duplicate_claims_remain_auditable_and_block_clean_acceptance() {
    let mut state = state();
    let extraction = state.next_action().unwrap().unwrap();
    let mut duplicate = payload(false);
    duplicate["recipes"][0]["sections"][0]["name"] =
        json!({"text":"Simmer.","spans":[{"start":2,"end":2}]});
    state.apply(&extraction, duplicate).unwrap();
    assert!(state.groups[0].candidates[0].outputs[0].is_some());
    assert!(
        !state.groups[0].candidates[0]
            .hybrid_assignment_issues
            .is_empty()
    );
    let audit = state.next_action().unwrap().unwrap();
    let response = verdict(&state, &audit, json!([]));
    state.apply(&audit, response).unwrap();
    assert!(!state.complete());
}

#[test]
fn duplicate_move_into_existing_target_reaudits_before_acceptance() {
    let mut state = state();
    let extraction = state.next_action().unwrap().unwrap();
    let mut duplicate = payload(false);
    duplicate["recipes"][0]["sections"][0]["name"] =
        json!({"text":"Simmer.","spans":[{"start":2,"end":2}]});
    state.apply(&extraction, duplicate).unwrap();
    assert!(
        !state.groups[0].candidates[0]
            .hybrid_assignment_issues
            .is_empty()
    );
    let before_revision = state.groups[0].candidates[0].revision;
    let audit = state.next_action().unwrap().unwrap();
    let duplicate_name = state.groups[0].candidates[0]
        .hybrid_assignments
        .iter()
        .position(|assignment| assignment.field == "name" && assignment.spans[0].start == 2)
        .unwrap();
    state
        .apply(
            &audit,
            verdict(
                &state,
                &audit,
                json!([{
                    "kind":"move_assignment",
                    "assignment_id":duplicate_name,
                    "target":{"recipe":0,"section":0,"field":"instructions"},
                    "reason":"The procedure already owns this duplicate source line."
                }]),
            ),
        )
        .unwrap();
    let candidate = &state.groups[0].candidates[0];
    assert!(candidate.hybrid_assignment_issues.is_empty());
    assert!(candidate.revision > before_revision);
    assert!(state.groups[0].accepted.is_none());
    assert!(!state.complete());
    let recipe = &candidate.outputs[0].as_ref().unwrap()[0];
    assert_eq!(recipe.meta.title, "Soup");
    assert_eq!(recipe.sections[0].ingredients, ["1 cup water"]);
    assert_eq!(recipe.sections[0].instructions, ["Simmer.", "Serve hot."]);

    let reaudit = state.next_action().unwrap().unwrap();
    assert!(reaudit.chunk.is_none());
    assert_ne!(reaudit.key, audit.key);
    state
        .apply(&reaudit, verdict(&state, &reaudit, json!([])))
        .unwrap();
    assert_eq!(state.groups[0].accepted, Some(0));
    assert!(state.complete());
    assert_eq!(state.groups[0].candidates[0].hybrid_audits.len(), 2);
    assert!(state.groups[0].candidates[0].hybrid_audits[1].reaudited);
    assert!(state.next_action().unwrap().is_none());
}

#[rstest::rstest]
#[case("Add 1.5 cups.", "Add 15 cups.")]
#[case("Add 1/2 cup.", "Add 12 cup.")]
#[case("Cool to -5 C.", "Cool to 5 C.")]
#[case("Add 1 cup.", "Add 1 tbsp.")]
#[case("Do not boil.", "Do boil.")]
fn text_corrections_preserve_amounts_units_and_negation(
    #[case] original: &str,
    #[case] replacement: &str,
) {
    let mut source = chunk("soup.xhtml");
    source.text = format!("Soup\n1 cup water\n{original}\nServe hot.");
    let mut state = State::new_with_strategy(
        vec![source],
        "claude-haiku-4-5",
        10.0,
        HybridStrategy::Hybrid,
    )
    .unwrap();
    let extraction = state.next_action().unwrap().unwrap();
    state.apply(&extraction, payload(false)).unwrap();
    let audit = state.next_action().unwrap().unwrap();
    let assignment = state.groups[0].candidates[0]
        .hybrid_assignments
        .iter()
        .find(|assignment| assignment.field == "instructions")
        .unwrap()
        .clone();
    let response = verdict(
        &state,
        &audit,
        json!([{
            "kind":"replace_bounded_text","assignment":assignment,
            "before":format!("{original}\nServe hot."),"after":format!("{replacement}\nServe hot."),
            "spans":[{"chunk":0,"start":2,"end":3}],"reason":"Unsupported edit","source_supported":true
        }]),
    );
    assert!(state.apply(&audit, response).is_err());
    assert!(!state.complete());
}

#[test]
fn typographic_correction_is_persisted_and_still_requires_reaudit() {
    let mut source = chunk("soup.xhtml");
    source.text = "Soup\n1 cup water\nAdd cook’s salt.\nServe hot.".into();
    let mut state = State::new_with_strategy(
        vec![source],
        "claude-haiku-4-5",
        10.0,
        HybridStrategy::Hybrid,
    )
    .unwrap();
    let extraction = state.next_action().unwrap().unwrap();
    state.apply(&extraction, payload(false)).unwrap();
    let audit = state.next_action().unwrap().unwrap();
    let assignment = state.groups[0].candidates[0]
        .hybrid_assignments
        .iter()
        .find(|assignment| assignment.field == "instructions")
        .unwrap()
        .clone();
    let response = verdict(
        &state,
        &audit,
        json!([{
            "kind":"replace_bounded_text","assignment":assignment,
            "before":"Add cook’s salt.\nServe hot.","after":"Add cook's salt.\nServe hot.",
            "spans":[{"chunk":0,"start":2,"end":3}],"reason":"Normalize typographic apostrophe","source_supported":true
        }]),
    );
    state.apply(&audit, response).unwrap();
    let candidate = &state.groups[0].candidates[0];
    assert_eq!(
        candidate.outputs[0].as_ref().unwrap()[0].sections[0].instructions[0],
        "Add cook's salt."
    );
    assert!(!candidate.hybrid_text_overrides.is_empty());
    assert!(state.source[0].text.contains("cook’s"));
    assert!(!state.complete());
    assert!(state.next_action().unwrap().unwrap().chunk.is_none());
}

#[test]
fn section_split_rebuilds_both_sides_of_a_crossing_method_span() {
    let mut state = state();
    let extraction = state.next_action().unwrap().unwrap();
    state.apply(&extraction, payload(false)).unwrap();
    let audit = state.next_action().unwrap().unwrap();
    let response = verdict(
        &state,
        &audit,
        json!([{"kind":"split_section","recipe":0,"section":0,"at":{"chunk":0,"start":3,"end":3},"reason":"Separate the finishing section."}]),
    );
    state.apply(&audit, response).unwrap();
    let output = &state.groups[0].candidates[0].outputs[0].as_ref().unwrap()[0];
    assert_eq!(output.sections.len(), 2);
    assert_eq!(output.sections[0].instructions, ["Simmer."]);
    assert_eq!(output.sections[1].instructions, ["Serve hot."]);
    assert!(!state.complete());
}

#[test]
fn section_merge_removes_the_old_section_and_preserves_source_order() {
    let mut state = state();
    let extraction = state.next_action().unwrap().unwrap();
    let mut split = payload(true);
    split["recipes"][0]["sections"].as_array_mut().unwrap().push(json!({"name":{"text":"","spans":[]},"ingredients":[],"instructions":[{"start":3,"end":3}]}));
    state.apply(&extraction, split).unwrap();
    let audit = state.next_action().unwrap().unwrap();
    let response = verdict(
        &state,
        &audit,
        json!([{"kind":"merge_sections","chunk":0,"recipe":0,"first":0,"second":1,"reason":"These methods form one section."}]),
    );
    state.apply(&audit, response).unwrap();
    let output = &state.groups[0].candidates[0].outputs[0].as_ref().unwrap()[0];
    assert_eq!(output.sections.len(), 1);
    assert_eq!(output.sections[0].instructions, ["Simmer.", "Serve hot."]);
    assert_eq!(output.sections[0].ingredients, ["1 cup water"]);
    assert!(!state.complete());
}
