//! Evaluation of the frozen source-role cohort artifacts.
//!
//! These artifacts predate the generic review expectation formats. They bind
//! source coordinates, role allowances, and ownership separately, so they are
//! deliberately parsed as a narrow, hash-bound contract instead of being
//! treated as ordinary recipe expectations.

use super::{Result, hash, rejected};
use crate::recovery::{Candidate, State};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use unicode_casefold::UnicodeCaseFold;
use unicode_normalization::UnicodeNormalization;

pub(super) const KIND: &str = "frozen_source_roles_v1";

const V1_KIND: &str = "source-grounded-automatic-c4-cohort-expectations";
const V3_KIND: &str = "source-grounded-automatic-c4-headnote-settled-expectations";

#[derive(Debug, Clone)]
struct Mapping {
    source_index: usize,
    original_chunk_index: usize,
}

#[derive(Debug, Clone)]
struct Assignment {
    original_chunk_index: usize,
    line_index: usize,
    document_line: usize,
    text: String,
    text_sha256: String,
    role: String,
    recipe_title: Option<String>,
}

#[derive(Debug, Clone)]
struct Headnote {
    original_chunk_index: usize,
    line_index: usize,
    allowed_roles: BTreeSet<String>,
}

/// Build a durable bundle without parsing or rewriting either frozen artifact.
/// The strings are intentionally raw so the caller can preserve the original
/// evidence bytes alongside the native report.
pub(super) fn bundle(
    source_mapping: Value,
    expectations_v1: String,
    expectations_v3: String,
) -> Value {
    json!({
        "kind": KIND,
        "source_mapping": source_mapping,
        "expectations_v1": expectations_v1,
        "expectations_v1_sha256": hash(expectations_v1.as_bytes()),
        "expectations_v3": expectations_v3,
        "expectations_v3_sha256": hash(expectations_v3.as_bytes()),
    })
}

pub(super) fn evaluate(
    state: &State,
    epub_sha256: &str,
    bundle: &Value,
    audit_complete: bool,
) -> Result<Value> {
    let (mapping, v1, v3) = parse_bundle(bundle)?;
    validate_provenance(&v1, epub_sha256, &mapping, state)?;
    validate_provenance(&v3, epub_sha256, &mapping, state)?;
    let assignments = parse_assignments(&v1)?;
    let headnotes = parse_headnotes(&v3)?;
    validate_source_coordinates(state, &mapping, &assignments, &headnotes)?;
    let owners = parse_owners(&v1, &mapping)?;
    validate_assignment_owners(&assignments, &owners)?;
    let selected = selected_candidates(state, &mapping)?;

    let assignment_by_coordinate = assignments
        .iter()
        .map(|assignment| {
            (
                (assignment.original_chunk_index, assignment.line_index),
                assignment,
            )
        })
        .collect::<BTreeMap<_, _>>();
    let headnote_by_coordinate = headnotes
        .iter()
        .map(|headnote| {
            (
                (headnote.original_chunk_index, headnote.line_index),
                headnote,
            )
        })
        .collect::<BTreeMap<_, _>>();

    let mut output_proposals = BTreeMap::<String, Vec<Value>>::new();
    let mut source_role_findings = Vec::new();
    let mut canonical_source_role_findings = Vec::new();
    let mut ai_findings = Vec::new();
    let mut groups = Vec::new();
    let mut all_groups_accepted = true;
    let mut all_groups_terminal = true;

    for selected in &selected {
        let group = &state.groups[selected.group_index];
        let candidate = selected.candidate;
        let group_original = selected
            .source_indices
            .iter()
            .map(|source| selected.source_to_original[source])
            .collect::<BTreeSet<_>>();
        let owner_scope = owners
            .iter()
            .filter(|(_, owner_chunk)| group_original.contains(owner_chunk))
            .map(|(owner, _)| owner.clone())
            .collect::<BTreeSet<_>>();
        let candidate_accepted = group.accepted == Some(selected.candidate_index);
        all_groups_accepted &= candidate_accepted;
        all_groups_terminal &= candidate.verified && candidate.outputs.iter().all(Option::is_some);

        for recipes in candidate.outputs.iter().flatten() {
            for recipe in recipes {
                for owner in &owner_scope {
                    if title_contains(&recipe.meta.title, owner) {
                        output_proposals
                            .entry(owner.clone())
                            .or_default()
                            .push(json!({
                                "group": selected.group_index,
                                "candidate": selected.candidate_index,
                                "title": recipe.meta.title,
                                "recipe": recipe,
                            }));
                    }
                }
            }
        }

        for (position, source_index) in selected.source_indices.iter().enumerate() {
            let original = selected.source_to_original[source_index];
            let lines = state.source[*source_index].text.lines().collect::<Vec<_>>();
            let roles = candidate.source_roles.get(position).ok_or_else(|| {
                rejected("frozen source-role candidate has no role vector for a selected chunk")
            })?;
            if roles.len() != lines.len() {
                source_role_findings.push(json!({
                    "group": selected.group_index,
                    "candidate": selected.candidate_index,
                    "original_chunk_index": original,
                    "status": "role_length_mismatch",
                    "source_line_count": lines.len(),
                    "role_count": roles.len(),
                }));
                continue;
            }
            for (line_index, actual) in roles.iter().enumerate() {
                let Some(assignment) = assignment_by_coordinate.get(&(original, line_index)) else {
                    source_role_findings.push(json!({"group":selected.group_index,"candidate":selected.candidate_index,"original_chunk_index":original,"line_index":line_index,"status":"missing_frozen_coordinate","actual_candidate_role":actual}));
                    continue;
                };
                let allowed = allowed_candidate_roles(assignment, &headnote_by_coordinate)?;
                if !allowed.contains(actual) {
                    source_role_findings.push(json!({
                        "group": selected.group_index,
                        "candidate": selected.candidate_index,
                        "original_chunk_index": original,
                        "line_index": line_index,
                        "document_line": assignment.document_line,
                        "recipe_title": assignment.recipe_title,
                        "source_text_sha256": assignment.text_sha256,
                        "frozen_role": assignment.role,
                        "allowed_candidate_roles": allowed,
                        "actual_candidate_role": actual,
                        "status": "role_mismatch",
                    }));
                }
            }
        }

        canonical_source_role_findings.extend(canonical_role_findings(
            state,
            selected,
            &assignment_by_coordinate,
            &headnote_by_coordinate,
        )?);

        for finding in &candidate.feedback {
            let original = selected.source_to_original.get(&finding.chunk).copied();
            let comparisons = finding
                .lines
                .iter()
                .map(|line| {
                    let assignment =
                        original.and_then(|chunk| assignment_by_coordinate.get(&(chunk, *line)));
                    let actual = selected
                        .source_indices
                        .iter()
                        .position(|source| *source == finding.chunk)
                        .and_then(|position| candidate.source_roles.get(position))
                        .and_then(|roles| roles.get(*line));
                    let allowed = assignment
                        .map(|assignment| {
                            allowed_candidate_roles(assignment, &headnote_by_coordinate)
                        })
                        .transpose()?;
                    Ok(json!({
                        "line_index":line,
                        "frozen_allowed_candidate_roles":allowed,
                        "actual_candidate_role":actual,
                        "candidate_role_within_frozen_allowance":allowed.as_ref().zip(actual).map(|(roles, actual)| roles.contains(actual)),
                        "source_text_sha256":assignment.map(|item| item.text_sha256.clone()),
                    }))
                })
                .collect::<Result<Vec<_>>>()?;
            ai_findings.push(json!({
                "group":selected.group_index,
                "candidate":selected.candidate_index,
                "category":finding.category,
                "message":finding.message,
                "resolved":finding.resolved,
                "original_chunk_index":original,
                "lines":comparisons,
            }));
        }
        groups.push(json!({
            "group":selected.group_index,
            "candidate":selected.candidate_index,
            "candidate_accepted":candidate_accepted,
            "source_indices":selected.source_indices,
            "original_chunk_indices":group_original,
            "output_recipe_count":candidate.outputs.iter().flatten().map(Vec::len).sum::<usize>(),
        }));
    }

    let mut output_findings = Vec::new();
    for (owner, owner_chunk) in &owners {
        let expected = assignments
            .iter()
            .filter(|assignment| assignment.recipe_title.as_deref() == Some(owner.as_str()))
            .collect::<Vec<_>>();
        if expected.is_empty()
            || !selected.iter().any(|group| {
                group
                    .source_to_original
                    .values()
                    .any(|chunk| chunk == owner_chunk)
            })
        {
            continue;
        }
        let proposals = output_proposals
            .get(owner)
            .map(Vec::as_slice)
            .unwrap_or_default();
        if proposals.len() != 1 {
            output_findings.push(json!({"owner":owner,"status":"owner_title_missing_or_ambiguous","candidate_match_count":proposals.len()}));
            continue;
        }
        let recipe: crate::ExtractedRecipe =
            serde_json::from_value(proposals[0]["recipe"].clone())?;
        let values = field_values(&recipe);
        for assignment in expected {
            let allowed = allowed_output_fields(assignment, &headnote_by_coordinate)?;
            if allowed.is_empty() {
                continue;
            }
            let found = allowed.iter().any(|field| {
                values
                    .get(*field)
                    .into_iter()
                    .flatten()
                    .any(|actual| text_in_value(&assignment.text, actual, field))
            });
            if !found {
                let present = values.iter().any(|(field, values)| {
                    values
                        .iter()
                        .any(|actual| text_in_value(&assignment.text, actual, field))
                });
                output_findings.push(json!({
                    "owner":owner,
                    "status":if present {"frozen_source_wrong_field_placement"} else {"frozen_source_text_missing_from_candidate"},
                    "original_chunk_index":assignment.original_chunk_index,
                    "line_index":assignment.line_index,
                    "document_line":assignment.document_line,
                    "source_text_sha256":assignment.text_sha256,
                    "frozen_role":assignment.role,
                    "allowed_output_fields":allowed,
                }));
            }
        }
    }
    let owner_count = output_findings
        .iter()
        .filter(|entry| entry["status"] == "owner_title_missing_or_ambiguous")
        .count();
    let placement_count = output_findings
        .iter()
        .filter(|entry| entry["status"] == "frozen_source_wrong_field_placement")
        .count();
    let missing_count = output_findings
        .iter()
        .filter(|entry| entry["status"] == "frozen_source_text_missing_from_candidate")
        .count();
    let historical_role_count = source_role_findings.len();
    let canonical_role_count = canonical_source_role_findings.len();
    let required_role_count = if state.strategy == crate::hybrid::HybridStrategy::Hybrid {
        canonical_role_count
    } else {
        historical_role_count
    };
    let required_findings =
        owner_count + placement_count + missing_count + required_role_count + ai_findings.len();
    let no_pending_attempts = state.attempts.iter().all(|attempt| !attempt.pending);
    let accepted = audit_complete
        && no_pending_attempts
        && all_groups_accepted
        && all_groups_terminal
        && required_findings == 0;
    Ok(json!({
        "status":"scored",
        "evaluation":{
            "kind":KIND,
            "scope":"Selected source-mapped recovery groups only; candidate outputs are evaluated per chunk and do not prove final merged component membership or a full-book quality gate. Historical source-role comparison intentionally uses the candidate's original extraction role evidence; post-extraction hybrid reassembly does not rewrite that evidence.",
            "groups":groups,
            "output_owner_and_field_findings":output_findings,
            "source_role_mismatches":source_role_findings,
            "canonical_source_role_findings":canonical_source_role_findings,
            "ai_findings":ai_findings,
            "summary":{
                "owner_missing_or_ambiguous_count":owner_count,
                "wrong_field_placement_count":placement_count,
                "text_missing_from_candidate_count":missing_count,
                "source_role_mismatch_count":historical_role_count,
                "canonical_source_role_finding_count":canonical_role_count,
                "ai_finding_count":ai_findings.len(),
                "selected_group_count":selected.len(),
                "all_selected_groups_accepted":all_groups_accepted,
                "all_selected_groups_terminal":all_groups_terminal,
                "no_pending_attempts":no_pending_attempts,
            },
            "role_acceptance_criterion":if state.strategy == crate::hybrid::HybridStrategy::Hybrid {
                "current_canonical_hybrid_assignments"
            } else {
                "historical_stored_source_roles"
            },
        },
        "accepted":accepted,
    }))
}

struct Selected<'a> {
    group_index: usize,
    candidate_index: usize,
    candidate: &'a Candidate,
    source_indices: Vec<usize>,
    source_to_original: BTreeMap<usize, usize>,
}

/// Derive current hybrid roles from canonical source ownership.  Stored
/// `source_roles` record the extraction response and deliberately remain
/// immutable after a correction; the canonical assignments are the current
/// ownership evidence for a hybrid candidate.
fn canonical_role_findings(
    state: &State,
    selected: &Selected<'_>,
    frozen: &BTreeMap<(usize, usize), &Assignment>,
    headnotes: &BTreeMap<(usize, usize), &Headnote>,
) -> Result<Vec<Value>> {
    let candidate = selected.candidate;
    let mut findings = crate::hybrid::assignment_issues_for_chunks(
        &state.source,
        &candidate.hybrid_assignments,
        &selected.source_indices,
    )
    .into_iter()
    .map(|issue| {
        json!({
            "group":selected.group_index,
            "candidate":selected.candidate_index,
            "status":format!("canonical_assignment_{}", issue.kind),
            "message":issue.message,
            "spans":issue.spans,
        })
    })
    .collect::<Vec<_>>();
    let mut claims = BTreeMap::<(usize, usize), Vec<String>>::new();
    for assignment in &candidate.hybrid_assignments {
        let Ok(owner) = assignment.validate_owner(&state.source) else {
            continue;
        };
        if !selected.source_indices.contains(&owner) {
            continue;
        }
        let Some(role) = canonical_role_for_field(&assignment.field) else {
            findings.push(json!({
                "group":selected.group_index,
                "candidate":selected.candidate_index,
                "status":"canonical_unknown_assignment_field",
                "field":assignment.field,
                "spans":assignment.spans,
            }));
            continue;
        };
        if let Some(status) = canonical_target_error(selected, assignment, owner) {
            findings.push(json!({
                "group":selected.group_index,
                "candidate":selected.candidate_index,
                "status":status,
                "field":assignment.field,
                "owner_chunk":owner,
                "recipe":assignment.recipe,
                "section":assignment.section,
                "spans":assignment.spans,
            }));
            continue;
        }
        for span in &assignment.spans {
            for line in span.start..=span.end {
                claims
                    .entry((span.chunk, line))
                    .or_default()
                    .push(role.clone());
            }
        }
    }
    for source_index in &selected.source_indices {
        let original = selected.source_to_original[source_index];
        for line_index in 0..state.source[*source_index].text.lines().count() {
            let Some(frozen) = frozen.get(&(original, line_index)) else {
                // Frozen coordinate completeness is validated before evaluation.
                continue;
            };
            let claim = claims.get(&(*source_index, line_index));
            let Some(claim) = claim else {
                findings.push(json!({
                    "group":selected.group_index,
                    "candidate":selected.candidate_index,
                    "original_chunk_index":original,
                    "line_index":line_index,
                    "document_line":frozen.document_line,
                    "status":"canonical_missing_assignment",
                }));
                continue;
            };
            if claim.len() != 1 {
                findings.push(json!({
                    "group":selected.group_index,
                    "candidate":selected.candidate_index,
                    "original_chunk_index":original,
                    "line_index":line_index,
                    "document_line":frozen.document_line,
                    "status":"canonical_conflicting_assignment",
                    "claimed_roles":claim,
                }));
                continue;
            }
            let allowed = allowed_candidate_roles(frozen, headnotes)?;
            if !allowed.contains(&claim[0]) {
                findings.push(json!({
                    "group":selected.group_index,
                    "candidate":selected.candidate_index,
                    "original_chunk_index":original,
                    "line_index":line_index,
                    "document_line":frozen.document_line,
                    "frozen_role":frozen.role,
                    "allowed_candidate_roles":allowed,
                    "actual_canonical_role":claim[0],
                    "status":"canonical_role_mismatch",
                }));
            }
        }
    }
    Ok(findings)
}

fn canonical_role_for_field(field: &str) -> Option<String> {
    Some(
        match field {
            "title" => "title",
            "ingredients" => "ingredient",
            "instructions" => "method",
            "ignored" => "non_recipe",
            "description" | "recipe_yield" | "notes" | "equipment" | "name" => "metadata",
            _ => return None,
        }
        .to_owned(),
    )
}

/// Canonical recipe/section indexes are local to the owning source chunk.
/// Ignored source has the documented placeholder recipe coordinate and does
/// not establish output ownership, so it has no recipe/section target check.
fn canonical_target_error(
    selected: &Selected<'_>,
    assignment: &crate::hybrid::FieldAssignment,
    owner: usize,
) -> Option<&'static str> {
    if assignment.field == "ignored" {
        if assignment.recipe == 0 && assignment.section.is_none() {
            return None;
        }
        return Some("canonical_invalid_ignored_target");
    }
    let Some(position) = selected
        .source_indices
        .iter()
        .position(|source| *source == owner)
    else {
        return Some("canonical_invalid_owner_target");
    };
    let Some(recipes) = selected
        .candidate
        .outputs
        .get(position)
        .and_then(Option::as_ref)
    else {
        return Some("canonical_invalid_recipe_target");
    };
    let Some(recipe) = recipes.get(assignment.recipe) else {
        return Some("canonical_invalid_recipe_target");
    };
    match assignment.field.as_str() {
        "title" | "description" | "recipe_yield" | "notes" | "equipment" => {
            if assignment.section.is_none() {
                None
            } else {
                Some("canonical_invalid_recipe_section_target")
            }
        }
        "name" | "ingredients" | "instructions" => assignment
            .section
            .and_then(|section| recipe.sections.get(section))
            .map(|_| ())
            .map_or(Some("canonical_invalid_section_target"), |_| None),
        _ => Some("canonical_unknown_assignment_field"),
    }
}

fn parse_bundle(bundle: &Value) -> Result<(Vec<Mapping>, Value, Value)> {
    if bundle["kind"] != KIND {
        return Err(rejected("unsupported frozen source-role bundle kind"));
    }
    let raw = |name: &str| {
        bundle[name]
            .as_str()
            .ok_or_else(|| rejected("frozen source-role bundle has no raw expectation text"))
    };
    let v1_raw = raw("expectations_v1")?;
    let v3_raw = raw("expectations_v3")?;
    if bundle["expectations_v1_sha256"] != hash(v1_raw.as_bytes())
        || bundle["expectations_v3_sha256"] != hash(v3_raw.as_bytes())
    {
        return Err(rejected("frozen source-role expectation hash differs"));
    }
    let v1: Value = serde_json::from_str(v1_raw)
        .map_err(|_| rejected("frozen v1 expectations are not valid JSON"))?;
    let v3: Value = serde_json::from_str(v3_raw)
        .map_err(|_| rejected("frozen v3 expectations are not valid JSON"))?;
    if v1["kind"] != V1_KIND
        || v1["schema_version"] != 1
        || v3["kind"] != V3_KIND
        || v3["schema_version"] != 3
    {
        return Err(rejected(
            "unsupported frozen source-role expectation schema",
        ));
    }
    let mapping = bundle["source_mapping"]
        .as_array()
        .ok_or_else(|| rejected("frozen source-role bundle has no source mapping"))?
        .iter()
        .map(|entry| {
            Ok(Mapping {
                source_index: index(entry, "source_index")?,
                original_chunk_index: index(entry, "original_chunk_index")?,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    if mapping.is_empty()
        || mapping
            .windows(2)
            .any(|pair| pair[0].original_chunk_index >= pair[1].original_chunk_index)
        || mapping
            .iter()
            .map(|entry| entry.source_index)
            .collect::<BTreeSet<_>>()
            .len()
            != mapping.len()
        || mapping
            .iter()
            .map(|entry| entry.original_chunk_index)
            .collect::<BTreeSet<_>>()
            .len()
            != mapping.len()
    {
        return Err(rejected(
            "frozen source-role mapping is not unique exact selected mapping",
        ));
    }
    Ok((mapping, v1, v3))
}

fn validate_provenance(
    v: &Value,
    epub_sha256: &str,
    mapping: &[Mapping],
    state: &State,
) -> Result<()> {
    let provenance = v["provenance"]
        .as_object()
        .ok_or_else(|| rejected("frozen source-role expectations have no provenance"))?;
    if provenance.get("epub_sha256").and_then(Value::as_str) != Some(epub_sha256) {
        return Err(rejected("frozen source-role EPUB identity differs"));
    }
    let selected = mapping
        .iter()
        .map(|entry| {
            state
                .source
                .get(entry.source_index)
                .ok_or_else(|| rejected("frozen source-role mapping source index is unavailable"))
        })
        .collect::<Result<Vec<_>>>()?;
    let source_bytes = serde_json::to_vec(&selected)?;
    if provenance
        .get("selected_source_sha256")
        .and_then(Value::as_str)
        != Some(hash(&source_bytes).as_str())
    {
        return Err(rejected(
            "frozen source-role selected source identity differs",
        ));
    }
    Ok(())
}

fn parse_assignments(v1: &Value) -> Result<Vec<Assignment>> {
    let assignments = v1["line_assignments"]
        .as_array()
        .ok_or_else(|| rejected("frozen v1 has no line assignments"))?
        .iter()
        .map(|entry| {
            Ok(Assignment {
                original_chunk_index: index(entry, "original_chunk_index")?,
                line_index: index(entry, "line_index")?,
                document_line: index(entry, "document_line")?,
                text: string(entry, "text")?.to_owned(),
                text_sha256: string(entry, "text_sha256")?.to_owned(),
                role: string(entry, "role")?.to_owned(),
                recipe_title: entry
                    .get("recipe_title")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    if assignments.is_empty()
        || assignments
            .iter()
            .map(|entry| (entry.original_chunk_index, entry.line_index))
            .collect::<BTreeSet<_>>()
            .len()
            != assignments.len()
    {
        return Err(rejected("frozen v1 assignments are empty or duplicate"));
    }
    Ok(assignments)
}

fn parse_headnotes(v3: &Value) -> Result<Vec<Headnote>> {
    let entries = v3["entries"]
        .as_array()
        .ok_or_else(|| rejected("frozen v3 has no headnote entries"))?
        .iter()
        .map(|entry| {
            let coordinate = entry["coordinate"]
                .as_object()
                .ok_or_else(|| rejected("frozen v3 headnote coordinate is invalid"))?;
            let allowed_roles = entry["allowed_roles"]
                .as_array()
                .ok_or_else(|| rejected("frozen v3 headnote has no allowed roles"))?
                .iter()
                .map(|role| {
                    role.as_str()
                        .map(str::to_owned)
                        .ok_or_else(|| rejected("frozen v3 headnote role is invalid"))
                })
                .collect::<Result<BTreeSet<_>>>()?;
            if allowed_roles.is_empty()
                || !allowed_roles
                    .iter()
                    .all(|role| matches!(role.as_str(), "method" | "description" | "notes"))
            {
                return Err(rejected("frozen v3 headnote has unsupported allowed role"));
            }
            Ok(Headnote {
                original_chunk_index: index(
                    &Value::Object(coordinate.clone()),
                    "original_chunk_index",
                )?,
                line_index: index(&Value::Object(coordinate.clone()), "line_index")?,
                allowed_roles,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    if entries
        .iter()
        .map(|entry| (entry.original_chunk_index, entry.line_index))
        .collect::<BTreeSet<_>>()
        .len()
        != entries.len()
    {
        return Err(rejected("frozen v3 headnote coordinates are duplicate"));
    }
    Ok(entries)
}

fn parse_owners(v1: &Value, mapping: &[Mapping]) -> Result<BTreeMap<String, usize>> {
    let mapped = mapping
        .iter()
        .map(|entry| entry.original_chunk_index)
        .collect::<BTreeSet<_>>();
    let mut owners = BTreeMap::new();
    for recipe in v1["recipes"]
        .as_array()
        .ok_or_else(|| rejected("frozen v1 has no owners"))?
    {
        let title = string(recipe, "title")?.to_owned();
        let chunk = index(recipe, "original_chunk_index")?;
        if !mapped.contains(&chunk) {
            return Err(rejected("frozen v1 owner lies outside selected mapping"));
        }
        if owners.insert(title, chunk).is_some() {
            return Err(rejected("frozen v1 owner title is duplicate"));
        }
    }
    if owners.is_empty() {
        return Err(rejected("frozen v1 has no owners in mapped source"));
    }
    Ok(owners)
}

fn validate_assignment_owners(
    assignments: &[Assignment],
    owners: &BTreeMap<String, usize>,
) -> Result<()> {
    let ignored = |role: &str| role.starts_with("ignored_");
    let assigned_owners = assignments
        .iter()
        .filter(|assignment| !ignored(&assignment.role))
        .filter_map(|assignment| assignment.recipe_title.as_deref())
        .collect::<BTreeSet<_>>();
    for (owner, chunk) in owners {
        if !assigned_owners.contains(owner.as_str()) {
            return Err(rejected("frozen v1 owner has no source assignments"));
        }
        if assignments.iter().any(|assignment| {
            !ignored(&assignment.role)
                && assignment.recipe_title.as_deref() == Some(owner.as_str())
                && assignment.original_chunk_index != *chunk
        }) {
            return Err(rejected("frozen v1 assignment owner chunk differs"));
        }
    }
    for assignment in assignments {
        // Publisher captions can name a linked recipe in another chunk or
        // outside this cohort; they do not assign recipe-bearing text to it.
        if ignored(&assignment.role) {
            continue;
        }
        match assignment.recipe_title.as_deref() {
            Some(owner) if !owners.contains_key(owner) => {
                return Err(rejected("frozen v1 assignment owner is undefined"));
            }
            None if !ignored(&assignment.role) => {
                return Err(rejected("frozen v1 recipe-bearing assignment has no owner"));
            }
            _ => {}
        }
    }
    Ok(())
}

fn validate_source_coordinates(
    state: &State,
    mapping: &[Mapping],
    assignments: &[Assignment],
    headnotes: &[Headnote],
) -> Result<()> {
    let lookup = mapping
        .iter()
        .map(|entry| (entry.original_chunk_index, entry.source_index))
        .collect::<BTreeMap<_, _>>();
    for assignment in assignments {
        let source = lookup
            .get(&assignment.original_chunk_index)
            .ok_or_else(|| rejected("frozen v1 assignment lies outside selected mapping"))?;
        let line = state.source[*source]
            .text
            .lines()
            .nth(assignment.line_index)
            .ok_or_else(|| rejected("frozen v1 line coordinate is unavailable"))?;
        if hash(line.as_bytes()) != assignment.text_sha256 || line != assignment.text {
            return Err(rejected("frozen v1 source text hash differs"));
        }
    }
    for entry in mapping {
        for line_index in 0..state.source[entry.source_index].text.lines().count() {
            if !assignments.iter().any(|assignment| {
                (assignment.original_chunk_index, assignment.line_index)
                    == (entry.original_chunk_index, line_index)
            }) {
                return Err(rejected(
                    "frozen v1 does not cover every selected source line",
                ));
            }
        }
    }
    let v1_headnotes = assignments
        .iter()
        .filter(|assignment| assignment.role == "headnote")
        .map(|assignment| (assignment.original_chunk_index, assignment.line_index))
        .collect::<BTreeSet<_>>();
    let v3_headnotes = headnotes
        .iter()
        .map(|headnote| (headnote.original_chunk_index, headnote.line_index))
        .collect::<BTreeSet<_>>();
    if v1_headnotes != v3_headnotes {
        return Err(rejected("frozen v3 headnote coordinates differ from v1"));
    }
    for headnote in headnotes {
        if !lookup.contains_key(&headnote.original_chunk_index)
            || !assignments.iter().any(|assignment| {
                (assignment.original_chunk_index, assignment.line_index)
                    == (headnote.original_chunk_index, headnote.line_index)
                    && assignment.role == "headnote"
            })
        {
            return Err(rejected(
                "frozen v3 headnote does not bind a mapped v1 headnote",
            ));
        }
    }
    Ok(())
}

fn selected_candidates<'a>(state: &'a State, mapping: &[Mapping]) -> Result<Vec<Selected<'a>>> {
    let map = mapping
        .iter()
        .map(|entry| (entry.source_index, entry.original_chunk_index))
        .collect::<BTreeMap<_, _>>();
    let selected_sources = map.keys().copied().collect::<BTreeSet<_>>();
    let mut covered = BTreeSet::new();
    let mut selected = Vec::new();
    for (group_index, group) in state.groups.iter().enumerate() {
        let intersects = group
            .chunks
            .iter()
            .any(|chunk| selected_sources.contains(chunk));
        if !intersects {
            continue;
        }
        if !group
            .chunks
            .iter()
            .all(|chunk| selected_sources.contains(chunk))
        {
            return Err(rejected(
                "frozen source-role mapping selects only part of a recovery group",
            ));
        }
        if group.chunks.iter().any(|chunk| !covered.insert(*chunk)) {
            return Err(rejected(
                "frozen source-role groups overlap selected source",
            ));
        }
        let candidate_index = group
            .accepted
            .filter(|index| *index < group.candidates.len())
            .unwrap_or_else(|| group.candidates.len().saturating_sub(1));
        let candidate = group
            .candidates
            .get(candidate_index)
            .ok_or_else(|| rejected("selected frozen source-role group has no candidate"))?;
        if candidate.outputs.len() != group.chunks.len()
            || candidate.source_roles.len() != group.chunks.len()
        {
            return Err(rejected(
                "selected frozen source-role candidate shape is invalid",
            ));
        }
        selected.push(Selected {
            group_index,
            candidate_index,
            candidate,
            source_indices: group.chunks.clone(),
            source_to_original: map.clone(),
        });
    }
    if selected.is_empty() || covered != selected_sources {
        return Err(rejected(
            "frozen source-role mapping does not select exact recovery groups",
        ));
    }
    Ok(selected)
}

fn allowed_candidate_roles(
    assignment: &Assignment,
    headnotes: &BTreeMap<(usize, usize), &Headnote>,
) -> Result<BTreeSet<String>> {
    let roles = match assignment.role.as_str() {
        "ignored_nonrecipe_context"
        | "ignored_title_alias"
        | "ignored_publisher_caption"
        | "ignored_chapter_context" => ["non_recipe"].as_slice(),
        "title" | "subtitle" => ["title"].as_slice(),
        "ingredient" => ["ingredient"].as_slice(),
        "method" => ["method"].as_slice(),
        "yield" | "ingredient_section_name" => ["metadata"].as_slice(),
        "headnote" => {
            return headnotes
                .get(&(assignment.original_chunk_index, assignment.line_index))
                .ok_or_else(|| rejected("frozen headnote role is unavailable"))
                .map(|headnote| {
                    headnote
                        .allowed_roles
                        .iter()
                        .filter_map(|role| match role.as_str() {
                            "method" => Some("method".to_owned()),
                            "description" | "notes" => Some("metadata".to_owned()),
                            _ => None,
                        })
                        .collect()
                });
        }
        _ => return Err(rejected("unknown frozen v1 source role")),
    };
    Ok(roles.iter().map(|role| (*role).to_owned()).collect())
}

fn allowed_output_fields(
    assignment: &Assignment,
    headnotes: &BTreeMap<(usize, usize), &Headnote>,
) -> Result<Vec<&'static str>> {
    match assignment.role.as_str() {
        "title" | "subtitle" => Ok(vec!["title"]),
        "ingredient" => Ok(vec!["ingredients"]),
        "method" => Ok(vec!["instructions"]),
        "yield" => Ok(vec!["recipe_yield"]),
        "ingredient_section_name" => Ok(vec!["section_name"]),
        "headnote" => headnotes
            .get(&(assignment.original_chunk_index, assignment.line_index))
            .ok_or_else(|| rejected("frozen headnote output role is unavailable"))
            .map(|headnote| {
                headnote
                    .allowed_roles
                    .iter()
                    .filter_map(|role| match role.as_str() {
                        "method" => Some("instructions"),
                        "description" => Some("description"),
                        "notes" => Some("notes"),
                        _ => None,
                    })
                    .collect()
            }),
        "ignored_nonrecipe_context"
        | "ignored_title_alias"
        | "ignored_publisher_caption"
        | "ignored_chapter_context" => Ok(vec![]),
        _ => Err(rejected("unknown frozen v1 source role")),
    }
}

fn field_values(recipe: &crate::ExtractedRecipe) -> BTreeMap<&'static str, Vec<String>> {
    let mut values = BTreeMap::from([
        ("title", vec![recipe.meta.title.clone()]),
        (
            "description",
            recipe.meta.description.clone().into_iter().collect(),
        ),
        (
            "recipe_yield",
            recipe.meta.recipe_yield.clone().into_iter().collect(),
        ),
        ("notes", recipe.meta.notes.clone()),
        ("section_name", vec![]),
        ("ingredients", vec![]),
        ("instructions", vec![]),
    ]);
    for section in &recipe.sections {
        if let Some(name) = &section.name
            && let Some(names) = values.get_mut("section_name")
        {
            names.push(name.clone());
        }
        if let Some(ingredients) = values.get_mut("ingredients") {
            ingredients.extend(section.ingredients.clone());
        }
        if let Some(instructions) = values.get_mut("instructions") {
            instructions.extend(section.instructions.clone());
        }
    }
    values
}

fn normalize(value: &str) -> String {
    value
        .nfkc()
        .case_fold()
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}
fn title_contains(title: &str, constituent: &str) -> bool {
    let (title, constituent) = (normalize(title), normalize(constituent));
    title == constituent
        || title
            .strip_prefix(&constituent)
            .is_some_and(|tail| tail.starts_with(' '))
}
fn text_in_value(expected: &str, actual: &str, field: &str) -> bool {
    let (expected, actual) = (normalize(expected), normalize(actual));
    if matches!(field, "title" | "description" | "notes") {
        actual.contains(&expected)
    } else {
        actual == expected
    }
}
fn index(value: &Value, key: &str) -> Result<usize> {
    value[key]
        .as_u64()
        .and_then(|value| usize::try_from(value).ok())
        .ok_or_else(|| rejected("frozen source-role coordinate is invalid"))
}
fn string<'a>(value: &'a Value, key: &str) -> Result<&'a str> {
    value[key]
        .as_str()
        .ok_or_else(|| rejected("frozen source-role field is invalid"))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::{
        Chunk, RecipeMeta, RecipeSection,
        hybrid::{FieldAssignment, SourceSpan},
        recovery::Candidate,
    };

    fn fixture() -> (State, Value) {
        let source = Chunk {
            title_hint: None,
            text: "Soup\nintro\n1 cup water\nSimmer.".into(),
            doc_path: "soup.xhtml".into(),
            links: vec![],
            images: vec![],
        };
        let mut state = State::new_with_strategy(
            vec![source],
            "gemini-2.5-flash",
            1.0,
            crate::hybrid::HybridStrategy::Hybrid,
        )
        .unwrap();
        let recipe = crate::ExtractedRecipe {
            meta: RecipeMeta {
                title: "Soup".into(),
                description: Some("intro".into()),
                ..Default::default()
            },
            sections: vec![RecipeSection {
                name: None,
                ingredients: vec!["1 cup water".into()],
                instructions: vec!["Simmer.".into()],
            }],
        };
        let mut candidate = Candidate::from_outputs("model".into(), vec![vec![recipe]]);
        candidate.source_roles = vec![vec![
            "title".into(),
            "metadata".into(),
            "ingredient".into(),
            "method".into(),
        ]];
        candidate.hybrid_assignments = [
            ("title", None, 0),
            ("description", None, 1),
            ("ingredients", Some(0), 2),
            ("instructions", Some(0), 3),
        ]
        .into_iter()
        .map(|(field, section, line)| FieldAssignment {
            owner_chunk: Some(0),
            recipe: 0,
            section,
            field: field.into(),
            spans: vec![SourceSpan {
                chunk: 0,
                start: line,
                end: line,
            }],
        })
        .collect();
        candidate.verified = true;
        state.groups[0].candidates.push(candidate);
        state.groups[0].accepted = Some(0);
        let lines = state.source[0].text.lines().collect::<Vec<_>>();
        let assignment = |line: usize, role: &str| json!({"original_chunk_index":9,"line_index":line,"document_line":line,"text":lines[line],"text_sha256":hash(lines[line].as_bytes()),"role":role,"recipe_title":"Soup"});
        let v1 = json!({"kind":V1_KIND,"schema_version":1,"provenance":{"epub_sha256":"e".repeat(64),"selected_source_sha256":hash(&serde_json::to_vec(&vec![&state.source[0]]).unwrap())},"recipes":[{"title":"Soup","original_chunk_index":9}],"line_assignments":[assignment(0,"title"),assignment(1,"headnote"),assignment(2,"ingredient"),assignment(3,"method")]});
        let v3 = json!({"kind":V3_KIND,"schema_version":3,"provenance":{"epub_sha256":"e".repeat(64),"selected_source_sha256":hash(&serde_json::to_vec(&vec![&state.source[0]]).unwrap())},"entries":[{"coordinate":{"original_chunk_index":9,"line_index":1},"allowed_roles":["description"]}]});
        (
            state,
            bundle(
                json!([{"source_index":0,"original_chunk_index":9}]),
                serde_json::to_string(&v1).unwrap(),
                serde_json::to_string(&v3).unwrap(),
            ),
        )
    }
    #[test]
    fn passes_complete_matching_fixture() {
        let (state, bundle) = fixture();
        let value = evaluate(&state, &"e".repeat(64), &bundle, true).unwrap();
        assert_eq!(value["accepted"], true);
    }
    #[test]
    fn distinguishes_missing_and_wrong_field() {
        let (mut state, bundle) = fixture();
        state.groups[0].candidates[0].outputs[0].as_mut().unwrap()[0]
            .meta
            .description = None;
        let value = evaluate(&state, &"e".repeat(64), &bundle, true).unwrap();
        assert_eq!(
            value["evaluation"]["summary"]["text_missing_from_candidate_count"],
            1
        );
        state.groups[0].candidates[0].outputs[0].as_mut().unwrap()[0]
            .meta
            .notes = vec!["intro".into()];
        let value = evaluate(&state, &"e".repeat(64), &bundle, true).unwrap();
        assert_eq!(
            value["evaluation"]["summary"]["wrong_field_placement_count"],
            1
        );
    }
    #[test]
    fn allows_headnote_role_alternatives() {
        let (mut state, artifacts) = fixture();
        state.groups[0].candidates[0].source_roles[0][1] = "method".into();
        state.groups[0].candidates[0].outputs[0].as_mut().unwrap()[0]
            .meta
            .description = None;
        state.groups[0].candidates[0].outputs[0].as_mut().unwrap()[0].sections[0]
            .instructions
            .push("intro".into());
        let mut v: Value =
            serde_json::from_str(artifacts["expectations_v3"].as_str().unwrap()).unwrap();
        v["entries"][0]["allowed_roles"] = json!(["description", "method"]);
        let artifacts = bundle(
            artifacts["source_mapping"].clone(),
            artifacts["expectations_v1"].as_str().unwrap().into(),
            serde_json::to_string(&v).unwrap(),
        );
        let value = evaluate(&state, &"e".repeat(64), &artifacts, true).unwrap();
        assert_eq!(value["accepted"], true);
    }
    #[test]
    fn hybrid_acceptance_uses_current_canonical_roles_not_initial_history() {
        let (mut state, artifacts) = fixture();
        state.groups[0].candidates[0].source_roles[0][2] = "metadata".into();

        let value = evaluate(&state, &"e".repeat(64), &artifacts, true).unwrap();
        assert_eq!(
            value["evaluation"]["summary"]["source_role_mismatch_count"],
            1
        );
        assert_eq!(
            value["evaluation"]["summary"]["canonical_source_role_finding_count"],
            0
        );
        assert_eq!(
            value["evaluation"]["role_acceptance_criterion"],
            "current_canonical_hybrid_assignments"
        );
        assert_eq!(value["accepted"], true);
    }
    #[test]
    fn canonical_missing_or_conflicting_claims_cannot_pass() {
        let (mut state, artifacts) = fixture();
        state.groups[0].candidates[0].hybrid_assignments.clear();
        let value = evaluate(&state, &"e".repeat(64), &artifacts, true).unwrap();
        assert!(
            value["evaluation"]["canonical_source_role_findings"]
                .as_array()
                .unwrap()
                .iter()
                .any(|entry| entry["status"] == "canonical_missing_assignment")
        );
        assert_eq!(value["accepted"], false);

        let (mut state, artifacts) = fixture();
        let duplicate = state.groups[0].candidates[0].hybrid_assignments[0].clone();
        state.groups[0].candidates[0]
            .hybrid_assignments
            .push(duplicate);
        let value = evaluate(&state, &"e".repeat(64), &artifacts, true).unwrap();
        assert!(
            value["evaluation"]["canonical_source_role_findings"]
                .as_array()
                .unwrap()
                .iter()
                .any(|entry| entry["status"] == "canonical_conflicting_assignment")
        );
        assert_eq!(value["accepted"], false);
    }
    #[test]
    fn canonical_unknown_field_or_invalid_section_cannot_pass() {
        let (mut state, artifacts) = fixture();
        state.groups[0].candidates[0].hybrid_assignments[0].field = "invented".into();
        let value = evaluate(&state, &"e".repeat(64), &artifacts, true).unwrap();
        assert!(
            value["evaluation"]["canonical_source_role_findings"]
                .as_array()
                .unwrap()
                .iter()
                .any(|entry| entry["status"] == "canonical_unknown_assignment_field")
        );
        assert_eq!(value["accepted"], false);

        let (mut state, artifacts) = fixture();
        state.groups[0].candidates[0].hybrid_assignments[2].section = None;
        let value = evaluate(&state, &"e".repeat(64), &artifacts, true).unwrap();
        assert!(
            value["evaluation"]["canonical_source_role_findings"]
                .as_array()
                .unwrap()
                .iter()
                .any(|entry| entry["status"] == "canonical_invalid_section_target")
        );
        assert_eq!(value["accepted"], false);
    }
    #[test]
    fn rejects_duplicate_owner_malformed_mapping_hash_and_unicode() {
        let (state, mut artifacts) = fixture();
        artifacts["expectations_v1_sha256"] = "wrong".into();
        assert!(evaluate(&state, &"e".repeat(64), &artifacts, true).is_err());
        let (_state, artifacts) = fixture();
        let mut v: Value =
            serde_json::from_str(artifacts["expectations_v1"].as_str().unwrap()).unwrap();
        v["recipes"]
            .as_array_mut()
            .unwrap()
            .push(json!({"title":"Soup","original_chunk_index":9}));
        let malformed = bundle(
            artifacts["source_mapping"].clone(),
            serde_json::to_string(&v).unwrap(),
            artifacts["expectations_v3"].as_str().unwrap().into(),
        );
        assert!(evaluate(&state, &"e".repeat(64), &malformed, true).is_err());
        let (mut state, artifacts) = fixture();
        state.groups[0].candidates[0].outputs[0].as_mut().unwrap()[0]
            .meta
            .title = "ＳＯＵＰ".into();
        let value = evaluate(&state, &"e".repeat(64), &artifacts, true).unwrap();
        assert_eq!(value["accepted"], true);
    }
    #[test]
    fn incomplete_audit_cannot_accept() {
        let (state, bundle) = fixture();
        assert_eq!(
            evaluate(&state, &"e".repeat(64), &bundle, false).unwrap()["accepted"],
            false
        );
    }
    #[test]
    fn caller_cannot_accept_unverified_or_pending_candidate() {
        let (mut state, bundle) = fixture();
        state.groups[0].candidates[0].verified = false;
        assert_eq!(
            evaluate(&state, &"e".repeat(64), &bundle, true).unwrap()["accepted"],
            false
        );
        state.groups[0].candidates[0].verified = true;
        state.attempts.push(crate::recovery::Attempt {
            key: "pending".into(),
            model: "model".into(),
            verification: true,
            reservation_usd: 0.0,
            estimated_usd: None,
            usage: None,
            error: None,
            response: None,
            pending: true,
            rates_usd_per_million: [0.0; 4],
            pricing_checked: String::new(),
            pricing_source: String::new(),
            failure_details: None,
            raw_usage: None,
            started_at: None,
            inherited: false,
            telemetry: Default::default(),
        });
        assert_eq!(
            evaluate(&state, &"e".repeat(64), &bundle, true).unwrap()["accepted"],
            false
        );
    }
    #[test]
    fn rejects_incomplete_coordinates_and_overlapping_mapping_groups() {
        let (mut state, artifacts) = fixture();
        let mut v: Value =
            serde_json::from_str(artifacts["expectations_v1"].as_str().unwrap()).unwrap();
        v["line_assignments"].as_array_mut().unwrap().pop();
        let invalid = bundle(
            artifacts["source_mapping"].clone(),
            serde_json::to_string(&v).unwrap(),
            artifacts["expectations_v3"].as_str().unwrap().into(),
        );
        assert!(evaluate(&state, &"e".repeat(64), &invalid, true).is_err());
        state.groups.push(state.groups[0].clone());
        assert!(evaluate(&state, &"e".repeat(64), &artifacts, true).is_err());
    }
    #[test]
    fn ignored_caption_can_reference_owner_outside_cohort() {
        let (mut state, artifacts) = fixture();
        let mut v1: Value =
            serde_json::from_str(artifacts["expectations_v1"].as_str().unwrap()).unwrap();
        v1["line_assignments"][0]["role"] = json!("ignored_publisher_caption");
        v1["line_assignments"][0]["recipe_title"] = json!("Linked recipe outside cohort");
        state.groups[0].candidates[0].source_roles[0][0] = "non_recipe".into();
        state.groups[0].candidates[0].hybrid_assignments[0].field = "ignored".into();
        let artifacts = bundle(
            artifacts["source_mapping"].clone(),
            serde_json::to_string(&v1).unwrap(),
            artifacts["expectations_v3"].as_str().unwrap().into(),
        );
        assert_eq!(
            evaluate(&state, &"e".repeat(64), &artifacts, true).unwrap()["accepted"],
            true
        );
    }

    #[test]
    fn rejects_assignment_with_undefined_owner() {
        let (state, artifacts) = fixture();
        let mut v: Value =
            serde_json::from_str(artifacts["expectations_v1"].as_str().unwrap()).unwrap();
        v["line_assignments"][0]["recipe_title"] = "Unknown".into();
        let invalid = bundle(
            artifacts["source_mapping"].clone(),
            serde_json::to_string(&v).unwrap(),
            artifacts["expectations_v3"].as_str().unwrap().into(),
        );
        assert!(evaluate(&state, &"e".repeat(64), &invalid, true).is_err());
    }
}
