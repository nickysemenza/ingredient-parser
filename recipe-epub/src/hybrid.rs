//! Hybrid source-indexed extraction.
//!
//! The model names structure; all recipe text is reconstructed from immutable
//! source spans.  This is intentionally a portable module: native review and
//! browser consumers use the same schema, validation and correction rules.
use crate::{Chunk, ChunkRequest, EpubError, ExtractedRecipe};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// Opt-in extraction protocol. `Indexed` is the established default.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub enum HybridStrategy {
    #[default]
    Indexed,
    Hybrid,
}

/// An ordered, inclusive line range in the immutable indexed source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct SourceSpan {
    pub chunk: usize,
    pub start: usize,
    pub end: usize,
}
impl SourceSpan {
    pub fn validate(&self, source: &[Chunk]) -> Result<(), String> {
        let lines = source
            .get(self.chunk)
            .ok_or("span chunk is out of bounds")?
            .text
            .lines()
            .count();
        if self.start > self.end || self.end >= lines {
            return Err("span line range is out of bounds".into());
        }
        Ok(())
    }
    pub fn text(&self, source: &[Chunk]) -> Result<String, String> {
        self.validate(source)?;
        Ok(source[self.chunk]
            .text
            .lines()
            .skip(self.start)
            .take(self.end - self.start + 1)
            .collect::<Vec<_>>()
            .join("\n"))
    }
}

/// A field's source ownership.  The explicit location is retained for audit
/// even when a field has no assigned spans.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct FieldAssignment {
    /// The source chunk whose extractor-local recipe and section coordinates
    /// own this field.  Legacy saved assignments without this value are
    /// deliberately unresolved: an empty field has no span from which a
    /// chunk can safely be guessed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_chunk: Option<usize>,
    pub recipe: usize,
    pub section: Option<usize>,
    pub field: String,
    #[serde(default)]
    pub spans: Vec<SourceSpan>,
}

impl FieldAssignment {
    /// Return the explicit source owner.  Callers that need to mutate or
    /// assemble a field must reject legacy ownership that has not been
    /// deterministically migrated yet.
    pub fn owner_chunk(&self) -> Result<usize, String> {
        self.owner_chunk
            .ok_or_else(|| "field assignment owner chunk is unresolved".into())
    }

    /// Verify that this field's immutable source claims agree with its
    /// explicit owner. Empty fields are valid when their owner is known.
    pub fn validate_owner(&self, source: &[Chunk]) -> Result<usize, String> {
        let owner = self.owner_chunk()?;
        source
            .get(owner)
            .ok_or_else(|| "field assignment owner chunk is out of bounds".to_owned())?;
        if self.spans.iter().any(|span| span.chunk != owner) {
            return Err("field assignment spans cross its owner chunk".into());
        }
        for span in &self.spans {
            span.validate(source)?;
        }
        Ok(owner)
    }

    /// Fill legacy ownership only when the stored evidence identifies exactly
    /// one source chunk. Ambiguous empty fields in multi-chunk groups remain
    /// `None` for the caller to surface as incomplete rather than guessing.
    pub fn resolve_legacy_owner_chunk(&mut self, group_chunks: &[usize]) -> bool {
        if self.owner_chunk.is_some() {
            return false;
        }
        let chunks = self
            .spans
            .iter()
            .map(|span| span.chunk)
            .collect::<std::collections::BTreeSet<_>>();
        let inferred = match chunks.len() {
            0 if group_chunks.len() == 1 => group_chunks.first().copied(),
            1 => chunks
                .iter()
                .next()
                .copied()
                .filter(|chunk| group_chunks.contains(chunk)),
            _ => None,
        };
        if let Some(owner) = inferred {
            self.owner_chunk = Some(owner);
            true
        } else {
            false
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct AssignmentIssue {
    pub kind: String,
    pub message: String,
    #[serde(default)]
    pub spans: Vec<SourceSpan>,
}

/// A bounded candidate carries immutable identity and a revision. Corrections
/// always produce the next revision so stale verifier patches cannot apply.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct HybridCandidate {
    pub source_sha256: String,
    pub revision: u64,
    #[serde(default)]
    pub assignments: Vec<FieldAssignment>,
    #[serde(default)]
    pub issues: Vec<AssignmentIssue>,
    #[serde(default)]
    pub text_overrides: Vec<TextOverride>,
    #[serde(default)]
    pub correction_history: Vec<AppliedCorrection>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct TextOverride {
    pub assignment: FieldAssignment,
    pub before: String,
    pub after: String,
    pub spans: Vec<SourceSpan>,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub enum AuditCorrection {
    MoveAssignment {
        from: FieldAssignment,
        to: FieldAssignment,
        reason: String,
    },
    /// A source-line subset moved from one canonical field to another. This is
    /// additive for durable replay; legacy `move_assignment` remains intact.
    MoveSpans {
        from: FieldAssignment,
        to: FieldAssignment,
        spans: Vec<SourceSpan>,
        reason: String,
    },
    RestoreSpan {
        assignment: FieldAssignment,
        span: SourceSpan,
        reason: String,
    },
    ReplaceBoundedText {
        assignment: FieldAssignment,
        before: String,
        after: String,
        spans: Vec<SourceSpan>,
        reason: String,
        source_supported: bool,
    },
    SplitSection {
        recipe: usize,
        section: usize,
        at: SourceSpan,
        reason: String,
    },
    MergeSections {
        /// Required by current corrections because recipe/section coordinates
        /// are scoped to a source chunk. Missing values are unresolved legacy
        /// evidence and are never defaulted to chunk zero.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        owner_chunk: Option<usize>,
        recipe: usize,
        first: usize,
        second: usize,
        reason: String,
    },
}

pub(crate) const COMPACT_AUDIT_CORRECTION_CONTRACT: &str = "hybrid-audit-corrections-v2";
/// Grouped wire format avoids provider-discriminated unions while retaining
/// an explicit order for corrections that must be applied interleaved.
pub(crate) const GROUPED_AUDIT_CORRECTION_CONTRACT: &str = "hybrid-audit-corrections-v3";
/// Owner-aware successor to the grouped contract. It retains v3 decoding for
/// saved actions, but requires the chunk-local owner for structural merges.
pub(crate) const OWNER_AWARE_GROUPED_AUDIT_CORRECTION_CONTRACT: &str =
    "hybrid-audit-corrections-v4";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CompactTarget {
    recipe: usize,
    section: Option<usize>,
    field: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CompactCorrection {
    kind: String,
    reason: String,
    #[serde(default)]
    assignment_id: Option<usize>,
    #[serde(default)]
    target: Option<CompactTarget>,
    #[serde(default)]
    spans: Option<Vec<SourceSpan>>,
    #[serde(default)]
    span: Option<SourceSpan>,
    #[serde(default)]
    after: Option<String>,
    #[serde(default)]
    recipe: Option<usize>,
    #[serde(default)]
    section: Option<usize>,
    #[serde(default)]
    at: Option<SourceSpan>,
    #[serde(default)]
    first: Option<usize>,
    #[serde(default)]
    second: Option<usize>,
    #[serde(default)]
    chunk: Option<usize>,
}

/// Flat provider schema for compact correction wire records. Branch fields are
/// deliberately optional here; [`parse_compact_audit_corrections`] resolves the
/// stable assignment id and enforces the selected branch before any mutation.
pub(crate) fn compact_audit_correction_schema() -> Value {
    let span = json!({"type":"object","additionalProperties":false,"properties":{"chunk":{"type":"integer","minimum":0},"start":{"type":"integer","minimum":0},"end":{"type":"integer","minimum":0}},"required":["chunk","start","end"]});
    let mut restore_span = span.clone();
    restore_span["description"] = json!(
        "One source span to add with restore_span; every line must be previously unassigned. Do not restore any line already covered by an assignment, including inside a larger inclusive span. Use a move to correct existing ownership."
    );
    let target = json!({"type":"object","description":"Move destination. Recipe-level title, description, recipe_yield, notes, and equipment use section:null; name, ingredients, and instructions use an existing section index for that chunk-local recipe owner. Ignored source is unowned and uses section:null with the canonical recipe:0 placeholder.","additionalProperties":false,"properties":{"recipe":{"type":"integer","minimum":0},"section":{"type":["integer","null"],"minimum":0},"field":{"type":"string","enum":["title","description","recipe_yield","notes","equipment","name","ingredients","instructions","ignored"]}},"required":["recipe","section","field"]});
    json!({"type":"object","additionalProperties":false,"properties":{"kind":{"type":"string","enum":["move_assignment","restore_span","replace_bounded_text","split_section","merge_sections"]},"reason":{"type":"string","minLength":1},"assignment_id":{"type":"integer","minimum":0,"description":"Frozen canonical assignment ID. For restore_span this is the destination assignment, not an owner of the omitted source span."},"target":target,"spans":{"type":"array","items":span.clone(),"description":"Source spans selected from the move assignment_id; each must be a subset of that assignment's canonical spans."},"span":restore_span,"after":{"type":"string"},"recipe":{"type":"integer","minimum":0},"section":{"type":"integer","minimum":0},"at":span.clone(),"first":{"type":"integer","minimum":0},"second":{"type":"integer","minimum":0},"chunk":{"type":"integer","minimum":0,"description":"Owner source chunk for a structural section correction."}},"required":["kind","reason"]})
}

/// Schema for the grouped v3 correction wire contract. Each named array has
/// one exact record shape, so providers never need to select a JSON-schema
/// union branch. `order` restores the original interleaving before the v2
/// lowerer validates and produces durable corrections.
#[cfg(test)]
pub(crate) fn grouped_audit_correction_schema() -> Value {
    grouped_audit_correction_schema_with_owner(false)
}

/// Schema for current owner-aware corrections. Only structural operations
/// need an additional source coordinate: assignment-ID operations inherit it
/// from their frozen source or destination field.
pub(crate) fn owner_aware_grouped_audit_correction_schema() -> Value {
    grouped_audit_correction_schema_with_owner(true)
}

fn grouped_audit_correction_schema_with_owner(require_owner_chunk: bool) -> Value {
    // Keep nested source-span, target, and common primitive definitions in the
    // v2 schema. This makes both protocols describe the same wire values.
    let compact = compact_audit_correction_schema();
    let compact_properties = compact["properties"].clone();
    let property = |name: &str| compact_properties.get(name).cloned().unwrap_or_default();
    let order = json!({"type":"integer","minimum":0});
    let record = |properties: Value, required: Value| json!({"type":"object","additionalProperties":false,"properties":properties,"required":required});
    json!({
        "type":"object",
        "additionalProperties":false,
        "properties":{
            "move_assignment":{"type":"array","items":record(json!({"order":order,"assignment_id":property("assignment_id"),"target":property("target"),"spans":property("spans"),"reason":property("reason")}),json!(["order","assignment_id","target","reason"]))},
            "restore_span":{"type":"array","items":record(json!({"order":order,"assignment_id":property("assignment_id"),"span":property("span"),"reason":property("reason")}),json!(["order","assignment_id","span","reason"]))},
            "replace_bounded_text":{"type":"array","items":record(json!({"order":order,"assignment_id":property("assignment_id"),"after":property("after"),"reason":property("reason")}),json!(["order","assignment_id","after","reason"]))},
            "split_section":{"type":"array","items":record(json!({"order":order,"recipe":property("recipe"),"section":property("section"),"at":property("at"),"reason":property("reason")}),json!(["order","recipe","section","at","reason"]))},
            "merge_sections":{"type":"array","items":record(if require_owner_chunk { json!({"order":order,"chunk":property("chunk"),"recipe":property("recipe"),"first":property("first"),"second":property("second"),"reason":property("reason")}) } else { json!({"order":order,"recipe":property("recipe"),"first":property("first"),"second":property("second"),"reason":property("reason")}) },if require_owner_chunk { json!(["order","chunk","recipe","first","second","reason"]) } else { json!(["order","recipe","first","second","reason"]) })}
        },
        "required":["move_assignment","restore_span","replace_bounded_text","split_section","merge_sections"]
    })
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct GroupedAuditCorrections {
    move_assignment: Vec<Value>,
    restore_span: Vec<Value>,
    replace_bounded_text: Vec<Value>,
    split_section: Vec<Value>,
    merge_sections: Vec<Value>,
}

fn lower_grouped_audit_record(
    kind: &str,
    record: &mut Value,
    required: &[&str],
    allowed: &[&str],
) -> Result<usize, String> {
    let object = record
        .as_object_mut()
        .ok_or("grouped audit correction must be an object")?;
    if let Some(field) = object
        .keys()
        .find(|field| !allowed.contains(&field.as_str()))
        .cloned()
    {
        return Err(format!(
            "grouped {kind} correction has incompatible field `{field}`"
        ));
    }
    if let Some(field) = required
        .iter()
        .find(|field| object.get(**field).is_none_or(Value::is_null))
    {
        return Err(format!("grouped {kind} correction `{field}` is missing"));
    }
    let order = object
        .remove("order")
        .and_then(|value| value.as_u64())
        .ok_or("grouped audit correction order is missing or invalid")?;
    let order =
        usize::try_from(order).map_err(|_| "grouped audit correction order is out of range")?;
    object.insert("kind".into(), Value::String(kind.into()));
    Ok(order)
}

/// Lower grouped v3 records into the established v2 compact parser. Every
/// record must participate in one dense, globally unique ordering sequence;
/// named array order alone is deliberately not allowed to reorder edits.
pub(crate) fn parse_grouped_audit_corrections(
    value: Value,
    assignments: &[FieldAssignment],
    source: &[Chunk],
) -> Result<Vec<AuditCorrection>, String> {
    parse_grouped_audit_corrections_with_owner(value, assignments, source, false)
}

/// Decode the current owner-aware grouped contract. Saved v3 corrections use
/// [`parse_grouped_audit_corrections`] and may retain unresolved merge owners
/// until recovery migrates them from durable source evidence.
pub(crate) fn parse_owner_aware_grouped_audit_corrections(
    value: Value,
    assignments: &[FieldAssignment],
    source: &[Chunk],
) -> Result<Vec<AuditCorrection>, String> {
    parse_grouped_audit_corrections_with_owner(value, assignments, source, true)
}

fn parse_grouped_audit_corrections_with_owner(
    value: Value,
    assignments: &[FieldAssignment],
    source: &[Chunk],
    require_owner_chunk: bool,
) -> Result<Vec<AuditCorrection>, String> {
    let grouped: GroupedAuditCorrections = serde_json::from_value(value)
        .map_err(|error| format!("invalid grouped audit corrections: {error}"))?;
    let mut ordered = Vec::new();
    let move_required = ["order", "assignment_id", "target", "reason"];
    let move_allowed = ["order", "assignment_id", "target", "spans", "reason"];
    let restore_required = ["order", "assignment_id", "span", "reason"];
    let restore_allowed = ["order", "assignment_id", "span", "reason"];
    let text_required = ["order", "assignment_id", "after", "reason"];
    let text_allowed = ["order", "assignment_id", "after", "reason"];
    let split_required = vec!["order", "recipe", "section", "at", "reason"];
    let split_allowed = split_required.clone();
    let merge_required = if require_owner_chunk {
        vec!["order", "chunk", "recipe", "first", "second", "reason"]
    } else {
        vec!["order", "recipe", "first", "second", "reason"]
    };
    let merge_allowed = merge_required.clone();
    for (kind, records, required, allowed) in [
        (
            "move_assignment",
            grouped.move_assignment,
            &move_required[..],
            &move_allowed[..],
        ),
        (
            "restore_span",
            grouped.restore_span,
            &restore_required[..],
            &restore_allowed[..],
        ),
        (
            "replace_bounded_text",
            grouped.replace_bounded_text,
            &text_required[..],
            &text_allowed[..],
        ),
        (
            "split_section",
            grouped.split_section,
            &split_required,
            &split_allowed,
        ),
        (
            "merge_sections",
            grouped.merge_sections,
            &merge_required,
            &merge_allowed,
        ),
    ] {
        for mut record in records {
            let order = lower_grouped_audit_record(kind, &mut record, required, allowed)?;
            ordered.push((order, record));
        }
    }
    ordered.sort_by_key(|(order, _)| *order);
    if ordered
        .iter()
        .enumerate()
        .any(|(expected, (actual, _))| expected != *actual)
    {
        return Err("grouped audit correction orders must be dense and unique from zero".into());
    }
    parse_compact_audit_corrections(
        Value::Array(ordered.into_iter().map(|(_, record)| record).collect()),
        assignments,
        source,
    )
}

pub(crate) fn parse_compact_audit_corrections(
    value: Value,
    assignments: &[FieldAssignment],
    source: &[Chunk],
) -> Result<Vec<AuditCorrection>, String> {
    let values: Vec<Value> = serde_json::from_value(value)
        .map_err(|error| format!("invalid compact audit corrections: {error}"))?;
    values
        .into_iter()
        .map(|value| {
            if value.get("spans").is_some_and(Value::is_null) {
                return Err(
                    "move correction spans cannot be null; omit for a whole assignment".into(),
                );
            }
            let wire: CompactCorrection = serde_json::from_value(value)
                .map_err(|error| format!("invalid compact audit correction: {error}"))?;
            if wire.reason.trim().is_empty() {
                return Err("compact correction reason is empty".into());
            }
            let assignment = |id: Option<usize>| -> Result<FieldAssignment, String> {
                assignments
                    .get(id.ok_or_else(|| "compact correction assignment_id missing".to_owned())?)
                    .cloned()
                    .ok_or_else(|| {
                        "compact correction assignment_id is outside frozen assignments".to_owned()
                    })
            };
            match wire.kind.as_str() {
                "move_assignment" => {
                    if wire.span.is_some()
                        || wire.after.is_some()
                        || wire.recipe.is_some()
                        || wire.section.is_some()
                        || wire.at.is_some()
                        || wire.first.is_some()
                        || wire.second.is_some()
                        || wire.chunk.is_some()
                    {
                        return Err("move correction has conflicting fields".into());
                    }
                    let from = assignment(wire.assignment_id)?;
                    let target = wire.target.ok_or("move correction target missing")?;
                    if !valid_assignment_target(&target) {
                        return Err("move correction target placement is invalid".into());
                    }
                    let explicit = wire.spans;
                    let spans = explicit.clone().unwrap_or_else(|| from.spans.clone());
                    validate_moved_spans(&from, &spans, source)?;
                    let to = FieldAssignment {
                        owner_chunk: from.owner_chunk,
                        recipe: target.recipe,
                        section: target.section,
                        field: target.field,
                        spans: spans.clone(),
                    };
                    if from.recipe == to.recipe
                        && from.section == to.section
                        && from.field == to.field
                    {
                        return Err("move correction target is unchanged".into());
                    }
                    Ok(AuditCorrection::MoveSpans {
                        from,
                        to,
                        spans,
                        reason: wire.reason,
                    })
                }
                "restore_span" => {
                    if wire.target.is_some()
                        || wire.spans.is_some()
                        || wire.after.is_some()
                        || wire.recipe.is_some()
                        || wire.section.is_some()
                        || wire.at.is_some()
                        || wire.first.is_some()
                        || wire.second.is_some()
                        || wire.chunk.is_some()
                    {
                        return Err("restore correction has conflicting fields".into());
                    }
                    let assignment = assignment(wire.assignment_id)?;
                    let span = wire.span.ok_or("restore correction span missing")?;
                    span.validate(source)?;
                    Ok(AuditCorrection::RestoreSpan {
                        assignment,
                        span,
                        reason: wire.reason,
                    })
                }
                "replace_bounded_text" => {
                    if wire.target.is_some()
                        || wire.spans.is_some()
                        || wire.span.is_some()
                        || wire.recipe.is_some()
                        || wire.section.is_some()
                        || wire.at.is_some()
                        || wire.first.is_some()
                        || wire.second.is_some()
                        || wire.chunk.is_some()
                    {
                        return Err("text correction has conflicting fields".into());
                    }
                    let assignment = assignment(wire.assignment_id)?;
                    let after = wire.after.ok_or("text correction after missing")?;
                    let before = assignment_text(source, &assignment)?;
                    if typographic_normalization(&before) != typographic_normalization(&after) {
                        return Err(
                            "text correction changes ordered source content and needs review"
                                .into(),
                        );
                    }
                    Ok(AuditCorrection::ReplaceBoundedText {
                        spans: assignment.spans.clone(),
                        assignment,
                        before,
                        after,
                        reason: wire.reason,
                        source_supported: true,
                    })
                }
                "split_section" => {
                    if wire.assignment_id.is_some()
                        || wire.target.is_some()
                        || wire.spans.is_some()
                        || wire.span.is_some()
                        || wire.after.is_some()
                        || wire.first.is_some()
                        || wire.second.is_some()
                        || wire.chunk.is_some()
                    {
                        return Err("split correction has conflicting fields".into());
                    }
                    Ok(AuditCorrection::SplitSection {
                        recipe: wire.recipe.ok_or("split correction recipe missing")?,
                        section: wire.section.ok_or("split correction section missing")?,
                        at: wire.at.ok_or("split correction span missing")?,
                        reason: wire.reason,
                    })
                }
                "merge_sections" => {
                    if wire.assignment_id.is_some()
                        || wire.target.is_some()
                        || wire.spans.is_some()
                        || wire.span.is_some()
                        || wire.after.is_some()
                        || wire.section.is_some()
                        || wire.at.is_some()
                    {
                        return Err("merge correction has conflicting fields".into());
                    }
                    Ok(AuditCorrection::MergeSections {
                        owner_chunk: wire.chunk,
                        recipe: wire.recipe.ok_or("merge correction recipe missing")?,
                        first: wire.first.ok_or("merge correction first missing")?,
                        second: wire.second.ok_or("merge correction second missing")?,
                        reason: wire.reason,
                    })
                }
                _ => Err("compact correction kind is invalid".into()),
            }
        })
        .collect()
}

fn valid_assignment_target(target: &CompactTarget) -> bool {
    match target.field.as_str() {
        "title" | "description" | "recipe_yield" | "notes" | "equipment" | "ignored" => {
            target.section.is_none()
        }
        "name" | "ingredients" | "instructions" => target.section.is_some(),
        _ => false,
    }
}

fn validate_moved_spans(
    from: &FieldAssignment,
    spans: &[SourceSpan],
    source: &[Chunk],
) -> Result<(), String> {
    if spans.is_empty() {
        return Err("move correction spans must be nonempty".into());
    }
    let mut moved = std::collections::BTreeSet::new();
    let mut owned = std::collections::BTreeSet::new();
    for span in &from.spans {
        span.validate(source)?;
        for line in span.start..=span.end {
            owned.insert((span.chunk, line));
        }
    }
    for span in spans {
        span.validate(source)?;
        for line in span.start..=span.end {
            if !moved.insert((span.chunk, line)) {
                return Err("move correction spans overlap".into());
            }
            if !owned.contains(&(span.chunk, line)) {
                return Err("move correction span is outside canonical assignment".into());
            }
        }
    }
    Ok(())
}

fn spans_contained(owned: &[SourceSpan], requested: &[SourceSpan]) -> bool {
    requested.iter().all(|span| {
        (span.start..=span.end).all(|line| {
            owned
                .iter()
                .any(|owner| owner.chunk == span.chunk && owner.start <= line && line <= owner.end)
        })
    })
}

fn spans_overlap(first: &SourceSpan, second: &SourceSpan) -> bool {
    first.chunk == second.chunk && first.start <= second.end && second.start <= first.end
}

fn subtract_spans(owned: &[SourceSpan], removed: &[SourceSpan]) -> Vec<SourceSpan> {
    let removed: std::collections::BTreeSet<_> = removed
        .iter()
        .flat_map(|span| (span.start..=span.end).map(move |line| (span.chunk, line)))
        .collect();
    let mut lines: Vec<_> = owned
        .iter()
        .flat_map(|span| {
            (span.start..=span.end).filter_map(|line| {
                (!removed.contains(&(span.chunk, line))).then_some((span.chunk, line))
            })
        })
        .collect();
    lines.sort_unstable();
    let mut out: Vec<SourceSpan> = Vec::new();
    for (chunk, line) in lines {
        if let Some(last) = out.last_mut()
            && last.chunk == chunk
            && last.end + 1 == line
        {
            last.end = line;
        } else {
            out.push(SourceSpan {
                chunk,
                start: line,
                end: line,
            });
        }
    }
    out
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct AppliedCorrection {
    pub correction: AuditCorrection,
    pub before: Vec<FieldAssignment>,
    pub after: Vec<FieldAssignment>,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
pub struct AuditResult {
    pub source_sha256: String,
    pub candidate_revision: u64,
    pub group: usize,
    #[serde(default)]
    pub findings: Vec<AssignmentIssue>,
    #[serde(default)]
    pub corrections: Vec<AuditCorrection>,
    pub accepted: bool,
    pub reaudited: bool,
}

/// Reject stale identity/revision and unsupported meaning changes. The caller
/// persists the returned clone atomically with its run checkpoint.
pub fn apply_source_supported_corrections(
    source: &[Chunk],
    candidate: &HybridCandidate,
    audit: &AuditResult,
) -> Result<HybridCandidate, String> {
    if audit.source_sha256 != candidate.source_sha256 {
        return Err("audit source identity is stale".into());
    }
    if audit.candidate_revision != candidate.revision {
        return Err("audit candidate revision is stale".into());
    }
    // A correction must never use recipe/section coordinates from one source
    // chunk to mutate another. Legacy ownership is deliberately a hard
    // blocker until State-level migration can establish it deterministically.
    for assignment in &candidate.assignments {
        assignment.validate_owner(source)?;
    }
    let mut next = candidate.clone();
    for correction in &audit.corrections {
        let before = next.assignments.clone();
        match correction {
            AuditCorrection::MoveAssignment { from, to, reason } => {
                if from.spans != to.spans {
                    return Err("move correction changes source spans".into());
                }
                let owner = from.validate_owner(source)?;
                if to.validate_owner(source)? != owner {
                    return Err("move correction target belongs to a different source chunk".into());
                }
                let found = next
                    .assignments
                    .iter()
                    .position(|item| item == from)
                    .ok_or("move correction no longer matches assignment")?;
                next.assignments[found] = to.clone();
                next.correction_history.push(AppliedCorrection {
                    correction: correction.clone(),
                    before,
                    after: next.assignments.clone(),
                    reason: reason.clone(),
                });
            }
            AuditCorrection::MoveSpans {
                from,
                to,
                spans,
                reason,
            } => {
                if !candidate.assignments.contains(from) {
                    return Err("move source does not match the frozen candidate".into());
                }
                let owner = from.validate_owner(source)?;
                if to.spans != *spans
                    || to.validate_owner(source)? != owner
                    || !valid_assignment_target(&CompactTarget {
                        recipe: to.recipe,
                        section: to.section,
                        field: to.field.clone(),
                    })
                {
                    return Err(
                        "move target does not match its source spans or field placement".into(),
                    );
                }
                validate_moved_spans(from, spans, source)?;
                if from.recipe == to.recipe && from.section == to.section && from.field == to.field
                {
                    return Err("move correction target is unchanged".into());
                }
                let found = next
                    .assignments
                    .iter()
                    .position(|assignment| {
                        assignment.recipe == from.recipe
                            && assignment.owner_chunk == Some(owner)
                            && assignment.section == from.section
                            && assignment.field == from.field
                            && spans_contained(&assignment.spans, spans)
                    })
                    .ok_or("move correction no longer matches source assignment")?;
                let remaining = subtract_spans(&next.assignments[found].spans, spans);
                next.assignments[found].spans = remaining;
                if next.assignments[found].spans.is_empty() {
                    next.assignments.remove(found);
                }
                let target = next.assignments.iter_mut().find(|assignment| {
                    assignment.owner_chunk == Some(owner)
                        && assignment.recipe == to.recipe
                        && assignment.section == to.section
                        && assignment.field == to.field
                });
                if let Some(target) = target {
                    // A target can already own a moved coordinate when this
                    // patch resolves a duplicate claim. Retain its existing
                    // segmentation and add only source lines it does not own.
                    let newly_owned = subtract_spans(spans, &target.spans);
                    target.spans.extend(newly_owned);
                    target
                        .spans
                        .sort_by_key(|span| (span.chunk, span.start, span.end));
                } else {
                    next.assignments.push(FieldAssignment {
                        owner_chunk: Some(owner),
                        recipe: to.recipe,
                        section: to.section,
                        field: to.field.clone(),
                        spans: spans.clone(),
                    });
                }
                next.correction_history.push(AppliedCorrection {
                    correction: correction.clone(),
                    before,
                    after: next.assignments.clone(),
                    reason: reason.clone(),
                });
            }
            AuditCorrection::RestoreSpan {
                assignment,
                span,
                reason,
            } => {
                let owner = assignment.validate_owner(source)?;
                span.validate(source)?;
                if span.chunk != owner {
                    return Err("restore span belongs to a different source chunk".into());
                }
                // Restore is exclusively for omitted source. If a line is
                // already claimed—by its destination, another field, or an
                // earlier correction in this batch—it must remain a finding
                // or be expressed as a move. Never create duplicate source
                // ownership by treating overlap as a harmless no-op.
                if next.assignments.iter().any(|owned| {
                    owned
                        .spans
                        .iter()
                        .any(|existing| spans_overlap(existing, span))
                }) {
                    return Err("restore span overlaps existing source ownership".into());
                }
                let found = next
                    .assignments
                    .iter_mut()
                    .find(|item| {
                        item.recipe == assignment.recipe
                            && item.owner_chunk == Some(owner)
                            && item.section == assignment.section
                            && item.field == assignment.field
                    })
                    .ok_or("restore correction has no target assignment")?;
                found.spans.push(span.clone());
                found
                    .spans
                    .sort_by_key(|item| (item.chunk, item.start, item.end));
                next.correction_history.push(AppliedCorrection {
                    correction: correction.clone(),
                    before,
                    after: next.assignments.clone(),
                    reason: reason.clone(),
                });
            }
            AuditCorrection::ReplaceBoundedText {
                assignment,
                before: expected,
                after,
                spans,
                reason,
                source_supported,
            } => {
                if !source_supported {
                    return Err("text correction is not source-supported and needs review".into());
                }
                let owner = assignment.validate_owner(source)?;
                for span in spans {
                    span.validate(source)?;
                    if span.chunk != owner {
                        return Err(
                            "text correction span belongs to a different source chunk".into()
                        );
                    }
                }
                let actual = assignment_text(source, assignment)?;
                if &actual != expected {
                    return Err("text correction before value is stale".into());
                }
                let replacement = spans
                    .iter()
                    .map(|span| span.text(source))
                    .collect::<Result<Vec<_>, _>>()?
                    .join("\n");
                if typographic_normalization(&replacement) != typographic_normalization(after) {
                    return Err(
                        "text correction changes ordered source content and needs review".into(),
                    );
                }
                let updated = {
                    let found = next
                        .assignments
                        .iter_mut()
                        .find(|item| **item == *assignment)
                        .ok_or("text correction has no matching assignment")?;
                    found.spans = spans.clone();
                    found.clone()
                };
                next.text_overrides.push(TextOverride {
                    assignment: updated,
                    before: expected.clone(),
                    after: after.clone(),
                    spans: spans.clone(),
                    reason: reason.clone(),
                });
                next.correction_history.push(AppliedCorrection {
                    correction: correction.clone(),
                    before,
                    after: next.assignments.clone(),
                    reason: reason.clone(),
                });
            }
            AuditCorrection::SplitSection {
                recipe,
                section,
                at,
                reason,
            } => {
                at.validate(source)?;
                let chunk = at.chunk;
                require_recipe_sections_in_owner(&next.assignments, chunk, *recipe, &[*section])?;
                let mut split = Vec::new();
                for mut assignment in std::mem::take(&mut next.assignments) {
                    if assignment.owner_chunk == Some(chunk)
                        && assignment.recipe == *recipe
                        && assignment.section.is_some_and(|current| current > *section)
                    {
                        assignment.section = assignment.section.map(|current| current + 1);
                    }
                    if assignment.owner_chunk == Some(chunk)
                        && assignment.recipe == *recipe
                        && assignment.section == Some(*section)
                    {
                        let mut before_spans = Vec::new();
                        let mut after_spans = Vec::new();
                        for span in assignment.spans.drain(..) {
                            if span.chunk != chunk || span.end < at.start {
                                before_spans.push(span);
                            } else if span.start >= at.start {
                                after_spans.push(span);
                            } else {
                                before_spans.push(SourceSpan {
                                    end: at.start - 1,
                                    ..span.clone()
                                });
                                after_spans.push(SourceSpan {
                                    start: at.start,
                                    ..span
                                });
                            }
                        }
                        if !before_spans.is_empty() || after_spans.is_empty() {
                            assignment.spans = before_spans;
                            split.push(assignment.clone());
                        }
                        if !after_spans.is_empty() {
                            assignment.section = Some(section + 1);
                            assignment.spans = after_spans;
                            split.push(assignment);
                        }
                    } else {
                        split.push(assignment);
                    }
                }
                next.assignments = split;
                next.correction_history.push(AppliedCorrection {
                    correction: correction.clone(),
                    before,
                    after: next.assignments.clone(),
                    reason: reason.clone(),
                });
            }
            AuditCorrection::MergeSections {
                owner_chunk,
                recipe,
                first,
                second,
                reason,
            } => {
                if first >= second {
                    return Err("merge sections must be in source order".into());
                }
                let chunk = owner_chunk.ok_or("merge section owner chunk is unresolved")?;
                if source.get(chunk).is_none() {
                    return Err("merge section owner chunk is out of bounds".into());
                }
                require_recipe_sections_in_owner(
                    &next.assignments,
                    chunk,
                    *recipe,
                    &[*first, *second],
                )?;
                for assignment in &mut next.assignments {
                    if assignment.owner_chunk == Some(chunk) && assignment.recipe == *recipe {
                        if assignment.section == Some(*second) {
                            assignment.section = Some(*first);
                        } else if assignment.section.is_some_and(|current| current > *second) {
                            assignment.section = assignment.section.map(|current| current - 1);
                        }
                    }
                }
                coalesce_section_assignments(&mut next.assignments, *recipe, chunk);
                next.correction_history.push(AppliedCorrection {
                    correction: correction.clone(),
                    before,
                    after: next.assignments.clone(),
                    reason: reason.clone(),
                });
            }
        }
    }
    if !audit.corrections.is_empty() {
        next.revision = next
            .revision
            .checked_add(1)
            .ok_or("candidate revision overflow")?;
    }
    next.issues = audit.findings.clone();
    Ok(next)
}

fn typographic_normalization(value: &str) -> String {
    value
        .replace(['‘', '’'], "'")
        .replace(['“', '”'], "\"")
        .replace(['–', '—'], "-")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn require_recipe_sections_in_owner(
    assignments: &[FieldAssignment],
    owner_chunk: usize,
    recipe: usize,
    sections: &[usize],
) -> Result<(), String> {
    for assignment in assignments {
        if assignment.owner_chunk.is_none() {
            return Err("section correction has unresolved assignment ownership".into());
        }
    }
    if !sections.iter().all(|section| {
        assignments.iter().any(|assignment| {
            assignment.owner_chunk == Some(owner_chunk)
                && assignment.recipe == recipe
                && assignment.section == Some(*section)
        })
    }) {
        return Err("section correction targets a nonexistent section".into());
    }
    Ok(())
}

fn coalesce_section_assignments(
    assignments: &mut Vec<FieldAssignment>,
    recipe: usize,
    chunk: usize,
) {
    let mut result: Vec<FieldAssignment> = Vec::new();
    for assignment in std::mem::take(assignments) {
        let mergeable = assignment.recipe == recipe
            && assignment.section.is_some()
            && assignment.field != "ignored"
            && assignment.owner_chunk == Some(chunk);
        if mergeable
            && let Some(existing) = result.iter_mut().find(|existing| {
                existing.recipe == assignment.recipe
                    && existing.section == assignment.section
                    && existing.field == assignment.field
                    && existing.owner_chunk == Some(chunk)
            })
        {
            existing.spans.extend(assignment.spans);
            existing.spans.sort_by_key(|span| (span.start, span.end));
            continue;
        }
        result.push(assignment);
    }
    *assignments = result;
}

pub fn assignment_text(source: &[Chunk], assignment: &FieldAssignment) -> Result<String, String> {
    assignment
        .spans
        .iter()
        .map(|span| span.text(source))
        .collect::<Result<Vec<_>, _>>()
        .map(|parts| parts.join("\n"))
}

/// Preserve malformed ownership as explicit evidence for an audit instead of
/// silently dropping duplicated or missing source lines.  Callers may still
/// reject the candidate for assembly, but the issue list is durable and can
/// drive a targeted correction request.
pub fn assignment_issues(
    source: &[Chunk],
    assignments: &[FieldAssignment],
) -> Vec<AssignmentIssue> {
    assignment_issues_for_chunks(source, assignments, &(0..source.len()).collect::<Vec<_>>())
}

pub fn assignment_issues_for_chunks(
    source: &[Chunk],
    assignments: &[FieldAssignment],
    chunks: &[usize],
) -> Vec<AssignmentIssue> {
    let mut owners = std::collections::BTreeMap::<(usize, usize), Vec<SourceSpan>>::new();
    let mut issues = Vec::new();
    for assignment in assignments {
        match assignment.owner_chunk {
            None => issues.push(AssignmentIssue {
                kind: "unresolved_owner_chunk".into(),
                message: "field assignment has no explicit source owner chunk".into(),
                spans: assignment.spans.clone(),
            }),
            Some(owner) if source.get(owner).is_none() => issues.push(AssignmentIssue {
                kind: "invalid_owner_chunk".into(),
                message: "field assignment owner chunk is unavailable".into(),
                spans: assignment.spans.clone(),
            }),
            Some(owner) if !chunks.contains(&owner) => issues.push(AssignmentIssue {
                kind: "owner_outside_group".into(),
                message: "field assignment owner chunk is outside this candidate group".into(),
                spans: assignment.spans.clone(),
            }),
            Some(owner) if assignment.spans.iter().any(|span| span.chunk != owner) => {
                issues.push(AssignmentIssue {
                    kind: "owner_chunk_mismatch".into(),
                    message: "field assignment spans cross its explicit source owner chunk".into(),
                    spans: assignment.spans.clone(),
                });
            }
            _ => {}
        }
        for span in &assignment.spans {
            if let Err(message) = span.validate(source) {
                issues.push(AssignmentIssue {
                    kind: "invalid_span".into(),
                    message,
                    spans: vec![span.clone()],
                });
                continue;
            }
            for line in span.start..=span.end {
                owners
                    .entry((span.chunk, line))
                    .or_default()
                    .push(span.clone());
            }
        }
    }
    for chunk in chunks {
        let Some(source_chunk) = source.get(*chunk) else {
            issues.push(AssignmentIssue {
                kind: "invalid_chunk".into(),
                message: "group refers to an unavailable source chunk".into(),
                spans: vec![],
            });
            continue;
        };
        for line in 0..source_chunk.text.lines().count() {
            match owners.get(&(*chunk, line)) {
                None => issues.push(AssignmentIssue {
                    kind: "missing_assignment".into(),
                    message: format!("source line {line} has no assignment"),
                    spans: vec![SourceSpan {
                        chunk: *chunk,
                        start: line,
                        end: line,
                    }],
                }),
                Some(spans) if spans.len() > 1 => issues.push(AssignmentIssue {
                    kind: "conflicting_assignment".into(),
                    message: format!("source line {line} has conflicting assignments"),
                    spans: spans.clone(),
                }),
                _ => {}
            }
        }
    }
    issues
}

/// Decode the model's ownership claims without lowering them. Unlike assembly,
/// this intentionally tolerates duplicate and missing spans so an audit can
/// see and correct the claim rather than losing the candidate at parse time.
pub fn assignments_from_payload(
    chunk: usize,
    payload: &Value,
) -> Result<Vec<FieldAssignment>, String> {
    fn label(value: &Value) -> Result<&Value, String> {
        value
            .get("spans")
            .ok_or_else(|| "hybrid label spans missing".into())
    }
    fn spans(chunk: usize, value: &Value) -> Result<Vec<SourceSpan>, String> {
        value
            .as_array()
            .ok_or("hybrid spans must be an array")?
            .iter()
            .map(|span| {
                Ok(SourceSpan {
                    chunk,
                    start: span
                        .get("start")
                        .and_then(Value::as_u64)
                        .ok_or("hybrid span start missing")? as usize,
                    end: span
                        .get("end")
                        .and_then(Value::as_u64)
                        .ok_or("hybrid span end missing")? as usize,
                })
            })
            .collect()
    }
    fn optional_spans(chunk: usize, value: Option<&Value>) -> Result<Vec<SourceSpan>, String> {
        // A provider may omit an optional empty field despite the requested
        // shape. Absence means an explicit empty ownership assignment below;
        // a present non-array remains malformed and is never discarded.
        value.map_or_else(|| Ok(vec![]), |value| spans(chunk, value))
    }
    let mut assignments = vec![];
    for (recipe, value) in payload
        .get("recipes")
        .and_then(Value::as_array)
        .ok_or("hybrid recipes missing")?
        .iter()
        .enumerate()
    {
        assignments.push(FieldAssignment {
            owner_chunk: Some(chunk),
            recipe,
            section: None,
            field: "title".into(),
            spans: spans(chunk, label(&value["title"])?)?,
        });
        for field in ["description", "recipe_yield", "notes", "equipment"] {
            assignments.push(FieldAssignment {
                owner_chunk: Some(chunk),
                recipe,
                section: None,
                field: field.into(),
                spans: optional_spans(chunk, value.get(field))?,
            });
        }
        for (section, item) in value["sections"]
            .as_array()
            .ok_or("hybrid sections missing")?
            .iter()
            .enumerate()
        {
            assignments.push(FieldAssignment {
                owner_chunk: Some(chunk),
                recipe,
                section: Some(section),
                field: "name".into(),
                spans: spans(chunk, label(&item["name"])?)?,
            });
            for field in ["ingredients", "instructions"] {
                assignments.push(FieldAssignment {
                    owner_chunk: Some(chunk),
                    recipe,
                    section: Some(section),
                    field: field.into(),
                    spans: spans(chunk, &item[field])?,
                });
            }
        }
    }
    assignments.push(FieldAssignment {
        owner_chunk: Some(chunk),
        // Ignored source has no recipe owner. Keep this ordinary zero value
        // unused by ignored handling rather than serializing a JS-unsafe
        // sentinel coordinate.
        recipe: 0,
        section: None,
        field: "ignored".into(),
        spans: spans(
            chunk,
            payload.get("ignored").ok_or("hybrid ignored missing")?,
        )?,
    });
    Ok(assignments)
}

/// The hybrid request differs only in its response contract. Titles and labels
/// use short source spans; ingredients and instructions use ordered source
/// spans, avoiding copied model text while keeping complete source ownership.
pub fn build_hybrid_chunk_request(chunk: &Chunk) -> ChunkRequest {
    let mut request = crate::indexed::build_indexed_chunk_request(chunk);
    let max = chunk.text.lines().count().saturating_sub(1);
    let span = json!({"type":"object","additionalProperties":false,"properties":{"start":{"type":"integer","minimum":0,"maximum":max},"end":{"type":"integer","minimum":0,"maximum":max}},"required":["start","end"]});
    let spans = json!({"type":"array","items":span});
    let label = json!({"type":"object","additionalProperties":false,"properties":{"text":{"type":"string"},"spans":spans},"required":["text","spans"]});
    request.tool_schema = json!({"type":"object","additionalProperties":false,"properties":{"recipes":{"type":"array","items":{"type":"object","additionalProperties":false,"properties":{"title":label,"description":spans,"recipe_yield":spans,"notes":spans,"equipment":spans,"sections":{"type":"array","items":{"type":"object","additionalProperties":false,"properties":{"name":label,"ingredients":spans,"instructions":spans},"required":["name","ingredients","instructions"]}}},"required":["title","sections"]}},"ignored":spans},"required":["recipes","ignored"]});
    request.system = format!(
        "{}\n\nReturn ordered inclusive source spans {{start,end}}. Copy each complete authored title exactly, including any subtitle, translation, or transliteration, with its source spans. Do not ignore a naming line merely because another-language title exists; join its exact title source lines with spaces. Copy each section-label text exactly with its spans; the engine validates copied labels against source. If a component has no source heading, use the unlabeled label {{\"text\":\"\",\"spans\":[]}}; never invent a generic label such as Ingredients or assign a label span to an ingredient. For long ingredient/instruction fields return spans only; the engine assembles their text. Ingredients and instructions must be complete ordered source spans. Every source line is assigned exactly once to a field or ignored. Preserve recipe ownership, continuations, component boundaries, all required methods, yields, notes and equipment; do not invent source text.",
        crate::indexed::SOURCE_ROLE_RULES
    );
    request
}

/// Preserve the same source-evidence envelope used by indexed native
/// extraction. Only the response schema changes, so DOM provenance remains
/// identical across strategies and cannot become an untracked hybrid prompt.
pub fn build_hybrid_chunk_request_with_source_evidence(
    chunk: &Chunk,
    evidence: &crate::source::SourceChunkEvidence,
) -> Result<ChunkRequest, EpubError> {
    let mut evidence_request =
        crate::indexed::build_indexed_chunk_request_with_source_evidence(chunk, evidence)?;
    let hybrid = build_hybrid_chunk_request(chunk);
    evidence_request.tool_schema = hybrid.tool_schema;
    evidence_request.tool_name = hybrid.tool_name;
    // Hybrid requests carry the compact structural projection once. Indexed
    // requests keep their legacy lossless projection unchanged for protocol
    // compatibility.
    let prefix = evidence_request
        .user
        .split("\n\nRaw DOM provenance")
        .next()
        .unwrap_or_default();
    evidence_request.user = format!(
        "{prefix}\n\nRaw DOM provenance (compact structural evidence): {}",
        evidence.audit_projection()
    );
    evidence_request.system = hybrid.system;
    Ok(evidence_request)
}

/// Parse a one-chunk hybrid response through the existing strict indexed
/// lowerer, so ownership validation and deterministic text assembly stay one
/// implementation.
pub fn parse_hybrid_recipes(
    chunk: &Chunk,
    payload: Value,
) -> Result<Vec<ExtractedRecipe>, EpubError> {
    let indexed = hybrid_to_indexed(chunk, payload)?;
    crate::indexed::parse_hybrid_indexed_recipes(chunk, indexed)
}

pub fn hybrid_to_indexed(chunk: &Chunk, payload: Value) -> Result<Value, EpubError> {
    fn lines(value: &Value, line_count: usize) -> Result<Vec<usize>, EpubError> {
        let mut result = Vec::new();
        for span in value
            .as_array()
            .ok_or_else(|| EpubError::Proxy("hybrid spans must be arrays".into()))?
        {
            let start = span
                .get("start")
                .and_then(Value::as_u64)
                .ok_or_else(|| EpubError::Proxy("hybrid span start missing".into()))?
                as usize;
            let end = span
                .get("end")
                .and_then(Value::as_u64)
                .ok_or_else(|| EpubError::Proxy("hybrid span end missing".into()))?
                as usize;
            if start > end || end >= line_count {
                return Err(EpubError::Proxy("hybrid span is out of bounds".into()));
            }
            result.extend(start..=end);
        }
        Ok(result)
    }
    let line_count = chunk.text.lines().count();
    let optional_lines = |object: &Value, field: &str| -> Result<Vec<usize>, EpubError> {
        object
            .get(field)
            .map_or_else(|| Ok(vec![]), |value| lines(value, line_count))
    };
    let label_lines = |value: &Value| -> Result<Vec<usize>, EpubError> {
        let spans = value
            .get("spans")
            .ok_or_else(|| EpubError::Proxy("hybrid label spans missing".into()))?;
        let selected = lines(spans, line_count)?;
        let source_text = selected
            .iter()
            .filter_map(|line| chunk.text.lines().nth(*line))
            .collect::<Vec<_>>()
            .join(" ");
        let copied = value
            .get("text")
            .and_then(Value::as_str)
            .ok_or_else(|| EpubError::Proxy("hybrid label text missing".into()))?;
        if crate::extractor::normalize_source_whitespace(&source_text)
            != crate::extractor::normalize_source_whitespace(copied)
        {
            return Err(EpubError::Proxy(
                "hybrid copied label does not match source spans".into(),
            ));
        }
        Ok(selected)
    };
    let recipes = payload
        .get("recipes")
        .and_then(Value::as_array)
        .ok_or_else(|| EpubError::Proxy("hybrid recipes missing".into()))?;
    let mut out = Vec::new();
    for recipe in recipes {
        let sections = recipe.get("sections").and_then(Value::as_array).ok_or_else(|| EpubError::Proxy("hybrid sections missing".into()))?.iter().map(|s| Ok(json!({"name":label_lines(&s["name"] )?,"ingredients":lines(&s["ingredients"], line_count )?,"instructions":lines(&s["instructions"], line_count )?}))).collect::<Result<Vec<_>, EpubError>>()?;
        out.push(json!({"title":label_lines(&recipe["title"] )?,"description":optional_lines(recipe, "description")?,"recipe_yield":optional_lines(recipe, "recipe_yield")?,"notes":optional_lines(recipe, "notes")?,"equipment":optional_lines(recipe, "equipment")?,"sections":sections}));
    }
    Ok(
        json!({"recipes":out,"ignored":lines(payload.get("ignored").ok_or_else(|| EpubError::Proxy("hybrid ignored missing".into()))?, line_count)?}),
    )
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use rstest::rstest;
    fn source() -> Vec<Chunk> {
        vec![Chunk {
            title_hint: None,
            text: "Soup\n1 cup water\nSimmer.".into(),
            doc_path: "x.xhtml".into(),
            links: vec![],
            images: vec![],
        }]
    }

    fn component_source() -> Chunk {
        Chunk {
            title_hint: None,
            text: "Component dish\nSauce\n1 cup sauce base\nSalad\n1 cup salad base\n\nMake the sauce.\nPrepare the salad.\nServe the components together.".into(),
            doc_path: "components.xhtml".into(),
            links: vec![],
            images: vec![],
        }
    }

    fn component_payload() -> Value {
        // Deliberately retain the model's explicit section coordinate order:
        // Salad, an explicitly unnamed shared section, then Sauce. Source
        // order must not silently renumber these canonical owners.
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
    #[test]
    fn hybrid_assembles_only_source_spans() {
        let output = parse_hybrid_recipes(&source()[0], json!({"recipes":[{"title":{"text":"Soup","spans":[{"start":0,"end":0}]},"description":[],"recipe_yield":[],"notes":[],"equipment":[],"sections":[{"name":{"text":"","spans":[]},"ingredients":[{"start":1,"end":1}],"instructions":[{"start":2,"end":2}]}]}],"ignored":[]})).unwrap();
        assert_eq!(output[0].meta.title, "Soup");
        assert_eq!(output[0].sections[0].ingredients, ["1 cup water"]);
    }

    #[test]
    fn hybrid_title_preserves_multiline_authored_subtitle_and_ignores_adjacent_caption() {
        let chunk = Chunk {
            title_hint: None,
            text: "A photo caption\nCRISPY DUMPLINGS\nthung thong\n1 cup filling\nFry until crisp."
                .into(),
            doc_path: "x.xhtml".into(),
            links: vec![],
            images: vec![],
        };
        let payload = json!({"recipes":[{
            "title":{"text":"CRISPY DUMPLINGS thung thong","spans":[{"start":1,"end":2}]},
            "sections":[{"name":{"text":"","spans":[]},"ingredients":[{"start":3,"end":3}],"instructions":[{"start":4,"end":4}]}]
        }],"ignored":[{"start":0,"end":0}]});
        let assignments = assignments_from_payload(0, &payload).unwrap();
        let title = assignments
            .iter()
            .find(|assignment| assignment.field == "title")
            .unwrap();
        assert_eq!(
            title.spans,
            vec![SourceSpan {
                chunk: 0,
                start: 1,
                end: 2,
            }]
        );
        assert!(assignment_issues(std::slice::from_ref(&chunk), &assignments).is_empty());
        let output = parse_hybrid_recipes(&chunk, payload).unwrap();
        assert_eq!(output[0].meta.title, "CRISPY DUMPLINGS thung thong");
    }

    #[test]
    fn empty_field_owner_survives_serialization_and_legacy_absence_stays_unresolved() {
        let assignments = assignments_from_payload(7, &component_payload()).unwrap();
        let empty = assignments
            .iter()
            .find(|assignment| assignment.field == "description" && assignment.spans.is_empty())
            .unwrap();
        let mut encoded = serde_json::to_value(empty).unwrap();
        let restored: FieldAssignment = serde_json::from_value(encoded.clone()).unwrap();
        assert_eq!(restored.owner_chunk().unwrap(), 7);
        encoded.as_object_mut().unwrap().remove("owner_chunk");
        let legacy: FieldAssignment = serde_json::from_value(encoded).unwrap();
        assert!(legacy.owner_chunk().is_err());
        assert!(
            serde_json::to_value(legacy)
                .unwrap()
                .get("owner_chunk")
                .is_none()
        );
        assert!(
            assignments
                .iter()
                .all(|assignment| assignment.owner_chunk == Some(7))
        );
    }

    #[test]
    fn hybrid_strict_parse_preserves_explicit_component_section_coordinates() {
        let source = component_source();
        let payload = component_payload();
        let assignments = assignments_from_payload(0, &payload).unwrap();
        assert!(assignments.iter().any(|assignment| {
            assignment.section == Some(0)
                && assignment.field == "instructions"
                && assignment.spans
                    == vec![SourceSpan {
                        chunk: 0,
                        start: 7,
                        end: 7,
                    }]
        }));
        assert!(assignments.iter().any(|assignment| {
            assignment.section == Some(2)
                && assignment.field == "instructions"
                && assignment.spans
                    == vec![SourceSpan {
                        chunk: 0,
                        start: 6,
                        end: 6,
                    }]
        }));

        let output = parse_hybrid_recipes(&source, payload).unwrap();
        assert_eq!(output[0].sections.len(), 3);
        assert_eq!(output[0].sections[0].name.as_deref(), Some("Salad"));
        assert_eq!(output[0].sections[0].ingredients, ["1 cup salad base"]);
        assert_eq!(output[0].sections[0].instructions, ["Prepare the salad."]);
        assert_eq!(output[0].sections[1].name, None);
        assert!(output[0].sections[1].ingredients.is_empty());
        assert_eq!(
            output[0].sections[1].instructions,
            ["Serve the components together."]
        );
        assert_eq!(output[0].sections[2].name.as_deref(), Some("Sauce"));
        assert_eq!(output[0].sections[2].ingredients, ["1 cup sauce base"]);
        assert_eq!(output[0].sections[2].instructions, ["Make the sauce."]);
    }

    #[test]
    fn real_provider_pattern_omits_optional_empty_span_arrays() {
        // Captured hybrid responses omitted empty description, notes, and
        // equipment arrays. Their absence must lower to an empty field while
        // retaining explicit empty assignments for later ownership audit.
        let chunk = Chunk {
            title_hint: None,
            text: "HERB-BAKED CASHEWS\nSERVES 4\n1 cup cashews\nBake.".into(),
            doc_path: "x.xhtml".into(),
            links: vec![],
            images: vec![],
        };
        let payload = json!({"recipes":[{
            "title":{"text":"HERB-BAKED CASHEWS","spans":[{"start":0,"end":0}]},
            "recipe_yield":[{"start":1,"end":1}],
            "sections":[{"name":{"text":"","spans":[]},"ingredients":[{"start":2,"end":2}],"instructions":[{"start":3,"end":3}]}]
        }],"ignored":[]});
        let assignments = assignments_from_payload(0, &payload).unwrap();
        for field in ["description", "notes", "equipment"] {
            assert!(assignments.iter().any(|assignment| {
                assignment.owner_chunk == Some(0)
                    && assignment.recipe == 0
                    && assignment.section.is_none()
                    && assignment.field == field
                    && assignment.spans.is_empty()
            }));
        }
        let output = parse_hybrid_recipes(&chunk, payload).unwrap();
        assert_eq!(output[0].meta.title, "HERB-BAKED CASHEWS");
        assert_eq!(output[0].meta.recipe_yield.as_deref(), Some("SERVES 4"));
    }

    #[test]
    fn present_optional_nonarray_span_claim_is_rejected() {
        let payload = json!({"recipes":[{
            "title":{"text":"Soup","spans":[{"start":0,"end":0}]},
            "notes":{},
            "sections":[{"name":{"text":"","spans":[]},"ingredients":[{"start":1,"end":1}],"instructions":[{"start":2,"end":2}]}]
        }],"ignored":[]});
        assert!(assignments_from_payload(0, &payload).is_err());
        assert!(parse_hybrid_recipes(&source()[0], payload).is_err());
    }

    #[test]
    fn request_schema_makes_only_empty_span_fields_optional() {
        let schema = build_hybrid_chunk_request(&source()[0]).tool_schema;
        let required = schema["properties"]["recipes"]["items"]["required"]
            .as_array()
            .unwrap();
        assert_eq!(required, &[json!("title"), json!("sections")]);
        assert!(
            schema["properties"]["recipes"]["items"]["properties"]
                .get("notes")
                .is_some()
        );
    }

    #[test]
    fn request_explicitly_requires_empty_unlabeled_section_labels() {
        let request = build_hybrid_chunk_request(&source()[0]);
        assert!(request.system.contains("\"text\":\"\",\"spans\":[]"));
        assert!(request.system.contains("never invent a generic label"));
    }
    #[test]
    fn corrections_reject_stale_or_invented_text() {
        let sources = source();
        let assignment = FieldAssignment {
            owner_chunk: Some(0),
            recipe: 0,
            section: Some(0),
            field: "ingredients".into(),
            spans: vec![SourceSpan {
                chunk: 0,
                start: 1,
                end: 1,
            }],
        };
        let candidate = HybridCandidate {
            source_sha256: "source".into(),
            revision: 2,
            assignments: vec![assignment.clone()],
            issues: vec![],
            text_overrides: vec![],
            correction_history: vec![],
        };
        let audit = AuditResult {
            source_sha256: "source".into(),
            candidate_revision: 2,
            group: 0,
            findings: vec![],
            corrections: vec![AuditCorrection::ReplaceBoundedText {
                assignment,
                before: "1 cup water".into(),
                after: "2 cups water".into(),
                spans: vec![SourceSpan {
                    chunk: 0,
                    start: 1,
                    end: 1,
                }],
                reason: "bad".into(),
                source_supported: true,
            }],
            accepted: false,
            reaudited: false,
        };
        assert!(apply_source_supported_corrections(&sources, &candidate, &audit).is_err());
    }

    #[test]
    fn text_overrides_preserve_numeric_punctuation_and_negation() {
        for (source, replacement) in [
            ("1.5 cups", "15 cups"),
            ("1/2 cup", "12 cup"),
            ("-5 C", "5 C"),
            ("do not boil", "boil"),
        ] {
            assert_ne!(
                typographic_normalization(source),
                typographic_normalization(replacement)
            );
        }
        assert_eq!(
            typographic_normalization("“1½ cups” – warm"),
            typographic_normalization("\"1½ cups\" - warm")
        );
    }

    #[test]
    fn compact_correction_schema_is_flat_and_typed() {
        let schema = compact_audit_correction_schema();
        assert!(schema.get("oneOf").is_none());
        assert_eq!(schema["additionalProperties"], json!(false));
        assert_eq!(
            schema["properties"]["kind"]["enum"],
            json!([
                "move_assignment",
                "restore_span",
                "replace_bounded_text",
                "split_section",
                "merge_sections"
            ])
        );
        assert_eq!(schema["required"], json!(["kind", "reason"]));
        for field in [
            "assignment_id",
            "target",
            "span",
            "after",
            "spans",
            "recipe",
            "section",
            "at",
            "first",
            "second",
        ] {
            assert!(schema["properties"].get(field).is_some());
        }
        let nested = &schema["properties"]["target"];
        assert_eq!(nested["additionalProperties"], json!(false));
        assert_eq!(nested["required"], json!(["recipe", "section", "field"]));
    }

    #[test]
    fn grouped_correction_schema_has_exact_branch_records() {
        let schema = grouped_audit_correction_schema();
        assert!(schema.get("oneOf").is_none());
        assert!(schema.get("anyOf").is_none());
        assert_eq!(schema["additionalProperties"], json!(false));
        assert_eq!(
            schema["required"],
            json!([
                "move_assignment",
                "restore_span",
                "replace_bounded_text",
                "split_section",
                "merge_sections"
            ])
        );
        let cases = [
            (
                "move_assignment",
                json!(["order", "assignment_id", "target", "reason"]),
            ),
            (
                "restore_span",
                json!(["order", "assignment_id", "span", "reason"]),
            ),
            (
                "replace_bounded_text",
                json!(["order", "assignment_id", "after", "reason"]),
            ),
            (
                "split_section",
                json!(["order", "recipe", "section", "at", "reason"]),
            ),
            (
                "merge_sections",
                json!(["order", "recipe", "first", "second", "reason"]),
            ),
        ];
        for (kind, required) in cases {
            let item = &schema["properties"][kind]["items"];
            assert_eq!(item["additionalProperties"], json!(false));
            assert_eq!(item["required"], required, "{kind}");
            assert_eq!(item["properties"]["order"]["minimum"], json!(0));
        }
    }

    #[test]
    fn grouped_corrections_preserve_explicit_interleaving() {
        let sources = source();
        let assignment = FieldAssignment {
            owner_chunk: Some(0),
            recipe: 0,
            section: Some(0),
            field: "instructions".into(),
            spans: vec![SourceSpan {
                chunk: 0,
                start: 2,
                end: 2,
            }],
        };
        let corrections = parse_grouped_audit_corrections(
            json!({
                "move_assignment":[{
                    "order":2,"assignment_id":0,
                    "target":{"recipe":0,"section":null,"field":"notes"},
                    "reason":"Move the preparation note."
                }],
                "restore_span":[{
                    "order":1,"assignment_id":0,
                    "span":{"chunk":0,"start":1,"end":1},
                    "reason":"Restore source coverage."
                }],
                "replace_bounded_text":[],
                "split_section":[{
                    "order":0,"recipe":0,"section":0,
                    "at":{"chunk":0,"start":2,"end":2},
                    "reason":"Split at the procedural boundary."
                }],
                "merge_sections":[]
            }),
            std::slice::from_ref(&assignment),
            &sources,
        )
        .unwrap();
        assert!(matches!(
            corrections[0],
            AuditCorrection::SplitSection { .. }
        ));
        assert!(matches!(
            corrections[1],
            AuditCorrection::RestoreSpan { .. }
        ));
        assert!(matches!(corrections[2], AuditCorrection::MoveSpans { .. }));
    }

    #[test]
    fn grouped_corrections_reject_missing_branch_fields_and_invalid_ordering() {
        let sources = source();
        let assignment = FieldAssignment {
            owner_chunk: Some(0),
            recipe: 0,
            section: Some(0),
            field: "instructions".into(),
            spans: vec![SourceSpan {
                chunk: 0,
                start: 2,
                end: 2,
            }],
        };
        let empty = || {
            json!({
                "move_assignment":[],"restore_span":[],"replace_bounded_text":[],
                "split_section":[],"merge_sections":[]
            })
        };

        assert!(
            parse_grouped_audit_corrections(empty(), &[], &sources)
                .unwrap()
                .is_empty()
        );

        let mut missing_branch = empty();
        missing_branch
            .as_object_mut()
            .unwrap()
            .remove("merge_sections");
        assert!(parse_grouped_audit_corrections(missing_branch, &[], &sources).is_err());

        let mut missing_order = empty();
        missing_order["restore_span"] = json!([{
            "assignment_id":0,"span":{"chunk":0,"start":1,"end":1},"reason":"Restore source coverage."
        }]);
        assert_eq!(
            parse_grouped_audit_corrections(
                missing_order,
                std::slice::from_ref(&assignment),
                &sources,
            )
            .unwrap_err(),
            "grouped restore_span correction `order` is missing"
        );

        let mut duplicate_order = empty();
        duplicate_order["restore_span"] = json!([{
            "order":0,"assignment_id":0,"span":{"chunk":0,"start":1,"end":1},"reason":"Restore source coverage."
        }]);
        duplicate_order["replace_bounded_text"] = json!([{
            "order":0,"assignment_id":0,"after":"Simmer.","reason":"Normalize typography."
        }]);
        assert_eq!(
            parse_grouped_audit_corrections(
                duplicate_order,
                std::slice::from_ref(&assignment),
                &sources,
            )
            .unwrap_err(),
            "grouped audit correction orders must be dense and unique from zero"
        );

        let mut conflicting = empty();
        conflicting["merge_sections"] = json!([{
            "order":0,"recipe":0,"first":0,"second":1,"assignment_id":0,"reason":"Merge sections."
        }]);
        assert_eq!(
            parse_grouped_audit_corrections(conflicting, &[], &sources).unwrap_err(),
            "grouped merge_sections correction has incompatible field `assignment_id`"
        );

        let mut supplied_kind = empty();
        supplied_kind["restore_span"] = json!([{
            "order":0,"kind":"move_assignment","assignment_id":0,
            "span":{"chunk":0,"start":1,"end":1},"reason":"Restore source coverage."
        }]);
        assert_eq!(
            parse_grouped_audit_corrections(
                supplied_kind,
                std::slice::from_ref(&assignment),
                &sources,
            )
            .unwrap_err(),
            "grouped restore_span correction has incompatible field `kind`"
        );

        let mut inapplicable_null = empty();
        inapplicable_null["restore_span"] = json!([{
            "order":0,"assignment_id":0,"span":{"chunk":0,"start":1,"end":1},
            "target":null,"reason":"Restore source coverage."
        }]);
        assert_eq!(
            parse_grouped_audit_corrections(
                inapplicable_null,
                std::slice::from_ref(&assignment),
                &sources,
            )
            .unwrap_err(),
            "grouped restore_span correction has incompatible field `target`"
        );
    }

    #[rstest]
    #[case(
        json!({"kind":"move_assignment","reason":"missing assignments"}),
        "compact correction assignment_id missing"
    )]
    #[case(
        json!({"kind":"restore_span","reason":"missing span"}),
        "compact correction assignment_id missing"
    )]
    #[case(
        json!({"kind":"replace_bounded_text","reason":"missing text"}),
        "compact correction assignment_id missing"
    )]
    #[case(
        json!({"kind":"split_section","reason":"missing boundary"}),
        "split correction recipe missing"
    )]
    #[case(
        json!({"kind":"merge_sections","reason":"missing sections"}),
        "merge correction recipe missing"
    )]
    #[case(
        json!({"kind":"AuditRecipeGroupCorrections0","reason":"generated name"}),
        "compact correction kind is invalid"
    )]
    fn compact_correction_parser_rejects_missing_fields_for_selected_kind(
        #[case] input: Value,
        #[case] expected: &str,
    ) {
        let sources = source();
        let assignment = FieldAssignment {
            owner_chunk: Some(0),
            recipe: 0,
            section: Some(0),
            field: "instructions".into(),
            spans: vec![SourceSpan {
                chunk: 0,
                start: 1,
                end: 2,
            }],
        };
        let error = parse_compact_audit_corrections(
            json!([input]),
            std::slice::from_ref(&assignment),
            &sources,
        )
        .unwrap_err();
        assert_eq!(error, expected);
    }

    #[rstest]
    #[case("assignment_id", json!(0))]
    #[case("target", json!({"recipe":0,"section":0,"field":"instructions"}))]
    #[case("spans", json!([{ "chunk":0,"start":1,"end":1 }]))]
    #[case("span", json!({"chunk":0,"start":1,"end":1}))]
    #[case("after", json!("rewritten"))]
    #[case("first", json!(0))]
    #[case("second", json!(1))]
    fn compact_split_rejects_each_conflicting_branch_field(
        #[case] field: &str,
        #[case] value: Value,
    ) {
        let sources = source();
        let mut split = json!({
            "kind":"split_section",
            "recipe":0,
            "section":0,
            "at":{"chunk":0,"start":2,"end":2},
            "reason":"split",
        });
        split[field] = value;
        assert!(parse_compact_audit_corrections(json!([split]), &[], &sources).is_err());
    }

    #[rstest]
    #[case("assignment_id", json!(0))]
    #[case("target", json!({"recipe":0,"section":0,"field":"instructions"}))]
    #[case("spans", json!([{ "chunk":0,"start":1,"end":1 }]))]
    #[case("span", json!({"chunk":0,"start":1,"end":1}))]
    #[case("after", json!("rewritten"))]
    #[case("section", json!(0))]
    #[case("at", json!({"chunk":0,"start":1,"end":1}))]
    fn compact_merge_rejects_each_conflicting_branch_field(
        #[case] field: &str,
        #[case] value: Value,
    ) {
        let sources = source();
        let mut merge = json!({
            "kind":"merge_sections",
            "recipe":0,
            "first":0,
            "second":1,
            "reason":"merge",
        });
        merge[field] = value;
        assert!(parse_compact_audit_corrections(json!([merge]), &[], &sources).is_err());
    }

    #[test]
    fn compact_section_corrections_accept_clean_selected_fields() {
        let sources = source();
        let corrections = parse_compact_audit_corrections(
            json!([
                {"kind":"split_section","recipe":0,"section":0,"at":{"chunk":0,"start":2,"end":2},"reason":"split"},
                {"kind":"merge_sections","recipe":0,"first":0,"second":1,"reason":"merge"},
            ]),
            &[],
            &sources,
        )
        .unwrap();
        assert!(matches!(
            corrections[0],
            AuditCorrection::SplitSection { .. }
        ));
        assert!(matches!(
            corrections[1],
            AuditCorrection::MergeSections { .. }
        ));
    }

    #[test]
    fn compact_partial_move_preserves_remaining_title_lines() {
        let mut sources = source();
        sources[0].text = "Thai title\nEnglish subtitle\n1 cup water\nCook.".into();
        let title = FieldAssignment {
            owner_chunk: Some(0),
            recipe: 0,
            section: None,
            field: "title".into(),
            spans: vec![SourceSpan {
                chunk: 0,
                start: 0,
                end: 1,
            }],
        };
        let corrections = parse_compact_audit_corrections(
            json!([{"kind":"move_assignment","assignment_id":0,"target":{"recipe":0,"section":null,"field":"description"},"spans":[{"chunk":0,"start":1,"end":1}],"reason":"subtitle"}]),
            std::slice::from_ref(&title),
            &sources,
        ).unwrap();
        assert!(matches!(corrections[0], AuditCorrection::MoveSpans { .. }));
        let corrected = apply_source_supported_corrections(
            &sources,
            &candidate(vec![title]),
            &audit(corrections[0].clone()),
        )
        .unwrap();
        assert_eq!(
            corrected.assignments[0].spans,
            vec![SourceSpan {
                chunk: 0,
                start: 0,
                end: 0
            }]
        );
        assert!(
            corrected
                .assignments
                .iter()
                .any(|item| item.field == "description"
                    && item.spans
                        == vec![SourceSpan {
                            chunk: 0,
                            start: 1,
                            end: 1
                        }])
        );
    }

    #[test]
    fn compact_corrections_reject_bad_ids_outside_spans_and_quantity_edits() {
        let sources = source();
        let assignment = FieldAssignment {
            owner_chunk: Some(0),
            recipe: 0,
            section: Some(0),
            field: "ingredients".into(),
            spans: vec![SourceSpan {
                chunk: 0,
                start: 1,
                end: 1,
            }],
        };
        for wire in [
            json!([{"kind":"restore_span","assignment_id":9,"span":{"chunk":0,"start":1,"end":1},"reason":"bad id"}]),
            json!([{"kind":"move_assignment","assignment_id":0,"target":{"recipe":0,"section":null,"field":"notes"},"spans":[{"chunk":0,"start":2,"end":2}],"reason":"outside"}]),
            json!([{"kind":"move_assignment","assignment_id":0,"target":{"recipe":0,"section":null,"field":"notes"},"spans":null,"reason":"null is not whole move"}]),
            json!([{"kind":"replace_bounded_text","assignment_id":0,"after":"2 cups water","reason":"quantity"}]),
            json!([{"kind":"restore_span","assignment_id":0,"span":{"chunk":0,"start":1,"end":1},"reason":""}]),
            json!([{"kind":"restore_span","assignment_id":0,"span":{"chunk":0,"start":1,"end":1},"from":{},"reason":"stale field"}]),
        ] {
            assert!(
                parse_compact_audit_corrections(wire, std::slice::from_ref(&assignment), &sources)
                    .is_err()
            );
        }
    }

    fn candidate(assignments: Vec<FieldAssignment>) -> HybridCandidate {
        HybridCandidate {
            source_sha256: "source".into(),
            revision: 1,
            assignments,
            issues: vec![],
            text_overrides: vec![],
            correction_history: vec![],
        }
    }

    fn restore(assignment: FieldAssignment, start: usize, end: usize) -> AuditCorrection {
        AuditCorrection::RestoreSpan {
            assignment,
            span: SourceSpan {
                chunk: 0,
                start,
                end,
            },
            reason: "Restore omitted source text.".into(),
        }
    }

    #[test]
    fn restore_rejects_exact_and_subset_overlap_with_its_destination_atomically() {
        let sources = source();
        let destination = assignment(0, 0, "instructions", 1, 2);
        for correction in [
            restore(destination.clone(), 1, 2),
            restore(destination.clone(), 1, 1),
        ] {
            let original = candidate(vec![destination.clone()]);
            let before = serde_json::to_value(&original).unwrap();
            assert!(
                apply_source_supported_corrections(&sources, &original, &audit(correction))
                    .is_err()
            );
            assert_eq!(serde_json::to_value(&original).unwrap(), before);
        }
    }

    #[test]
    fn restore_rejects_span_owned_by_another_assignment_atomically() {
        let sources = source();
        let destination = assignment(0, 0, "instructions", 1, 1);
        let other = FieldAssignment {
            owner_chunk: Some(0),
            recipe: 0,
            section: None,
            field: "notes".into(),
            spans: vec![SourceSpan {
                chunk: 0,
                start: 2,
                end: 2,
            }],
        };
        let original = candidate(vec![destination.clone(), other]);
        let before = serde_json::to_value(&original).unwrap();
        assert!(
            apply_source_supported_corrections(
                &sources,
                &original,
                &audit(restore(destination, 2, 2)),
            )
            .is_err()
        );
        assert_eq!(serde_json::to_value(&original).unwrap(), before);
    }

    #[test]
    fn repeated_restore_of_one_missing_line_rejects_the_whole_batch_atomically() {
        let sources = source();
        let destination = assignment(0, 0, "instructions", 1, 1);
        let original = candidate(vec![destination.clone()]);
        let before = serde_json::to_value(&original).unwrap();
        let mut review = audit(restore(destination.clone(), 2, 2));
        review.corrections.push(restore(destination, 2, 2));

        assert!(apply_source_supported_corrections(&sources, &original, &review).is_err());
        assert_eq!(serde_json::to_value(&original).unwrap(), before);
    }

    #[test]
    fn restore_of_truly_unassigned_source_remains_supported() {
        let sources = source();
        let destination = assignment(0, 0, "instructions", 1, 1);
        let original = candidate(vec![destination.clone()]);
        let corrected = apply_source_supported_corrections(
            &sources,
            &original,
            &audit(restore(destination, 2, 2)),
        )
        .unwrap();
        assert_eq!(corrected.correction_history.len(), 1);
        assert_eq!(
            corrected.assignments[0].spans,
            vec![
                SourceSpan {
                    chunk: 0,
                    start: 1,
                    end: 1,
                },
                SourceSpan {
                    chunk: 0,
                    start: 2,
                    end: 2,
                }
            ]
        );
    }

    #[test]
    fn frozen_ids_support_disjoint_moves_and_reject_overlapping_batches_atomically() {
        let mut sources = source();
        sources[0].text = "Soup\nFirst note\nSimmer.\nLast note".into();
        let original = candidate(vec![assignment(0, 0, "instructions", 1, 3)]);
        let wire = json!([
            {"kind":"move_assignment","assignment_id":0,"target":{"recipe":0,"section":null,"field":"notes"},"spans":[{"chunk":0,"start":1,"end":1}],"reason":"First note"},
            {"kind":"move_assignment","assignment_id":0,"target":{"recipe":0,"section":null,"field":"notes"},"spans":[{"chunk":0,"start":3,"end":3}],"reason":"Last note"}
        ]);
        let corrections =
            parse_compact_audit_corrections(wire.clone(), &original.assignments, &sources).unwrap();
        let mut review = audit(corrections[0].clone());
        review.corrections = corrections;
        let result = apply_source_supported_corrections(&sources, &original, &review).unwrap();
        assert_eq!(
            result
                .assignments
                .iter()
                .find(|a| a.field == "instructions")
                .unwrap()
                .spans,
            vec![SourceSpan {
                chunk: 0,
                start: 2,
                end: 2
            }]
        );
        assert_eq!(
            result
                .assignments
                .iter()
                .find(|a| a.field == "notes")
                .unwrap()
                .spans,
            vec![
                SourceSpan {
                    chunk: 0,
                    start: 1,
                    end: 1
                },
                SourceSpan {
                    chunk: 0,
                    start: 3,
                    end: 3
                }
            ]
        );
        assert_eq!(result.correction_history.len(), 2);
        let mut overlapping = wire;
        overlapping[1]["spans"] = overlapping[0]["spans"].clone();
        review.corrections =
            parse_compact_audit_corrections(overlapping, &original.assignments, &sources).unwrap();
        let before = serde_json::to_value(&original).unwrap();
        assert!(apply_source_supported_corrections(&sources, &original, &review).is_err());
        assert_eq!(serde_json::to_value(&original).unwrap(), before);
        if let AuditCorrection::MoveSpans { from, .. } = &mut review.corrections[0] {
            from.spans[0].end = 1;
        }
        review.corrections.truncate(1);
        assert!(
            apply_source_supported_corrections(&sources, &original, &review)
                .unwrap_err()
                .contains("frozen candidate")
        );
    }
    #[test]
    fn duplicate_move_removes_redundant_source_claim_without_losing_coverage() {
        let sources = source();
        let from = assignment(0, 0, "ingredients", 1, 1);
        let target = FieldAssignment {
            owner_chunk: Some(0),
            recipe: 0,
            section: None,
            field: "notes".into(),
            spans: from.spans.clone(),
        };
        let correction = AuditCorrection::MoveSpans {
            from: from.clone(),
            to: FieldAssignment {
                owner_chunk: Some(0),
                recipe: target.recipe,
                section: target.section,
                field: target.field.clone(),
                spans: from.spans.clone(),
            },
            spans: from.spans.clone(),
            reason: "The note already owns this source line.".into(),
        };
        let result = apply_source_supported_corrections(
            &sources,
            &candidate(vec![from, target]),
            &audit(correction),
        )
        .unwrap();
        assert_eq!(result.assignments.len(), 1);
        assert_eq!(result.assignments[0].field, "notes");
        assert_eq!(
            result.assignments[0].spans,
            vec![SourceSpan {
                chunk: 0,
                start: 1,
                end: 1,
            }]
        );
        assert_eq!(result.correction_history.len(), 1);
    }

    #[test]
    fn partial_duplicate_move_adds_only_the_unowned_tail() {
        let mut sources = source();
        sources[0].text = "Soup\nOne\nTwo\nThree".into();
        let from = assignment(0, 0, "ingredients", 1, 3);
        let target = FieldAssignment {
            owner_chunk: Some(0),
            recipe: 0,
            section: None,
            field: "notes".into(),
            spans: vec![SourceSpan {
                chunk: 0,
                start: 1,
                end: 1,
            }],
        };
        let result = apply_source_supported_corrections(
            &sources,
            &candidate(vec![from.clone(), target]),
            &audit(AuditCorrection::MoveSpans {
                from: from.clone(),
                to: FieldAssignment {
                    owner_chunk: Some(0),
                    recipe: 0,
                    section: None,
                    field: "notes".into(),
                    spans: from.spans.clone(),
                },
                spans: from.spans,
                reason: "Move the procedural tail.".into(),
            }),
        )
        .unwrap();
        assert_eq!(result.assignments.len(), 1);
        assert_eq!(
            result.assignments[0].spans,
            vec![
                SourceSpan {
                    chunk: 0,
                    start: 1,
                    end: 1,
                },
                SourceSpan {
                    chunk: 0,
                    start: 2,
                    end: 3,
                },
            ]
        );
    }

    #[test]
    fn duplicate_coordinates_in_different_chunks_are_not_collapsed() {
        let mut sources = source();
        let mut second = sources[0].clone();
        second.doc_path = "second.xhtml".into();
        sources.push(second);
        let from = FieldAssignment {
            owner_chunk: Some(1),
            recipe: 0,
            section: Some(0),
            field: "ingredients".into(),
            spans: vec![SourceSpan {
                chunk: 1,
                start: 1,
                end: 1,
            }],
        };
        let target = FieldAssignment {
            owner_chunk: Some(0),
            recipe: 0,
            section: None,
            field: "notes".into(),
            spans: vec![SourceSpan {
                chunk: 0,
                start: 1,
                end: 1,
            }],
        };
        let result = apply_source_supported_corrections(
            &sources,
            &candidate(vec![
                target,
                FieldAssignment {
                    owner_chunk: Some(1),
                    recipe: 0,
                    section: None,
                    field: "notes".into(),
                    spans: vec![],
                },
                from.clone(),
            ]),
            &audit(AuditCorrection::MoveSpans {
                from: from.clone(),
                to: FieldAssignment {
                    owner_chunk: Some(1),
                    recipe: 0,
                    section: None,
                    field: "notes".into(),
                    spans: from.spans.clone(),
                },
                spans: from.spans,
                reason: "Move the other document line.".into(),
            }),
        )
        .unwrap();
        assert!(result.assignments.iter().any(|assignment| {
            assignment.owner_chunk == Some(0)
                && assignment.field == "notes"
                && assignment.spans
                    == vec![SourceSpan {
                        chunk: 0,
                        start: 1,
                        end: 1,
                    }]
        }));
        assert!(result.assignments.iter().any(|assignment| {
            assignment.owner_chunk == Some(1)
                && assignment.field == "notes"
                && assignment.spans
                    == vec![SourceSpan {
                        chunk: 1,
                        start: 1,
                        end: 1,
                    }]
        }));
    }

    #[test]
    fn unresolved_empty_owner_and_same_local_coordinates_are_not_patchable() {
        let mut sources = source();
        let mut second = sources[0].clone();
        second.doc_path = "second.xhtml".into();
        sources.push(second);
        let source_assignment = FieldAssignment {
            owner_chunk: Some(1),
            recipe: 0,
            section: Some(0),
            field: "ingredients".into(),
            spans: vec![SourceSpan {
                chunk: 1,
                start: 1,
                end: 1,
            }],
        };
        // Both fields have the same chunk-local recipe/section coordinates.
        // The empty destination carries no source span under the legacy shape,
        // so applying a correction would silently select the first chunk.
        let empty_destination = FieldAssignment {
            owner_chunk: None,
            recipe: 0,
            section: Some(0),
            field: "instructions".into(),
            spans: vec![],
        };
        let correction = AuditCorrection::MoveSpans {
            from: source_assignment.clone(),
            to: FieldAssignment {
                owner_chunk: Some(1),
                recipe: 0,
                section: Some(0),
                field: "instructions".into(),
                spans: source_assignment.spans.clone(),
            },
            spans: source_assignment.spans.clone(),
            reason: "The line belongs in the second chunk's method.".into(),
        };
        assert!(
            apply_source_supported_corrections(
                &sources,
                &candidate(vec![empty_destination, source_assignment]),
                &audit(correction),
            )
            .is_err()
        );
    }

    #[test]
    fn owner_chunk_mismatch_is_an_explicit_issue_and_rejects_corrections() {
        let mut sources = source();
        sources.push(sources[0].clone());
        let mismatch = FieldAssignment {
            owner_chunk: Some(0),
            recipe: 0,
            section: Some(0),
            field: "instructions".into(),
            spans: vec![SourceSpan {
                chunk: 1,
                start: 2,
                end: 2,
            }],
        };
        assert!(
            assignment_issues_for_chunks(&sources, std::slice::from_ref(&mismatch), &[0, 1])
                .iter()
                .any(|issue| issue.kind == "owner_chunk_mismatch")
        );
        assert!(
            apply_source_supported_corrections(
                &sources,
                &candidate(vec![mismatch.clone()]),
                &audit(AuditCorrection::RestoreSpan {
                    assignment: mismatch,
                    span: SourceSpan {
                        chunk: 1,
                        start: 1,
                        end: 1
                    },
                    reason: "This must not cross the explicit owner.".into(),
                }),
            )
            .is_err()
        );
    }

    #[test]
    fn legacy_owner_resolution_never_guesses_an_empty_multichunk_field() {
        let mut from_span = FieldAssignment {
            owner_chunk: None,
            recipe: 0,
            section: None,
            field: "title".into(),
            spans: vec![SourceSpan {
                chunk: 1,
                start: 0,
                end: 0,
            }],
        };
        assert!(from_span.resolve_legacy_owner_chunk(&[0, 1]));
        assert_eq!(from_span.owner_chunk, Some(1));
        let mut singleton_empty = FieldAssignment {
            owner_chunk: None,
            recipe: 0,
            section: None,
            field: "description".into(),
            spans: vec![],
        };
        assert!(singleton_empty.resolve_legacy_owner_chunk(&[1]));
        assert_eq!(singleton_empty.owner_chunk, Some(1));
        let mut ambiguous_empty = singleton_empty.clone();
        ambiguous_empty.owner_chunk = None;
        assert!(!ambiguous_empty.resolve_legacy_owner_chunk(&[0, 1]));
        assert_eq!(ambiguous_empty.owner_chunk, None);
    }

    #[test]
    fn owner_aware_merge_requires_chunk_while_v3_preserves_legacy_none() {
        let sources = source();
        let v3 = json!({
            "move_assignment":[],"restore_span":[],"replace_bounded_text":[],"split_section":[],
            "merge_sections":[{"order":0,"recipe":0,"first":0,"second":1,"reason":"legacy"}]
        });
        let legacy = parse_grouped_audit_corrections(v3, &[], &sources).unwrap();
        assert!(matches!(
            legacy.as_slice(),
            [AuditCorrection::MergeSections {
                owner_chunk: None,
                ..
            }]
        ));
        let v4_missing_chunk = json!({
            "move_assignment":[],"restore_span":[],"replace_bounded_text":[],"split_section":[],
            "merge_sections":[{"order":0,"recipe":0,"first":0,"second":1,"reason":"current"}]
        });
        assert!(
            parse_owner_aware_grouped_audit_corrections(v4_missing_chunk, &[], &sources).is_err()
        );
        let v4 = json!({
            "move_assignment":[],"restore_span":[],"replace_bounded_text":[],"split_section":[],
            "merge_sections":[{"order":0,"chunk":0,"recipe":0,"first":0,"second":1,"reason":"current"}]
        });
        assert!(matches!(
            parse_owner_aware_grouped_audit_corrections(v4, &[], &sources)
                .unwrap()
                .as_slice(),
            [AuditCorrection::MergeSections {
                owner_chunk: Some(0),
                ..
            }]
        ));
    }

    #[test]
    fn invalid_patch_after_duplicate_repair_is_atomic() {
        let sources = source();
        let from = assignment(0, 0, "ingredients", 1, 1);
        let target = FieldAssignment {
            owner_chunk: Some(0),
            recipe: 0,
            section: None,
            field: "notes".into(),
            spans: from.spans.clone(),
        };
        let correction = || AuditCorrection::MoveSpans {
            from: from.clone(),
            to: FieldAssignment {
                owner_chunk: Some(0),
                recipe: 0,
                section: None,
                field: "notes".into(),
                spans: from.spans.clone(),
            },
            spans: from.spans.clone(),
            reason: "Resolve a duplicate claim.".into(),
        };
        let original = candidate(vec![from.clone(), target]);
        let mut review = audit(correction());
        review.corrections.push(correction());
        let before = serde_json::to_value(&original).unwrap();
        assert!(apply_source_supported_corrections(&sources, &original, &review).is_err());
        assert_eq!(serde_json::to_value(&original).unwrap(), before);
    }

    fn audit(correction: AuditCorrection) -> AuditResult {
        AuditResult {
            source_sha256: "source".into(),
            candidate_revision: 1,
            group: 0,
            findings: vec![],
            corrections: vec![correction],
            accepted: false,
            reaudited: false,
        }
    }
    fn assignment(
        recipe: usize,
        section: usize,
        field: &str,
        start: usize,
        end: usize,
    ) -> FieldAssignment {
        FieldAssignment {
            owner_chunk: Some(0),
            recipe,
            section: Some(section),
            field: field.into(),
            spans: vec![SourceSpan {
                chunk: 0,
                start,
                end,
            }],
        }
    }

    #[test]
    fn split_crossing_span_preserves_exact_ownership_in_new_section() {
        let mut sources = source();
        sources[0].text = "Soup\nA\nB\nC".into();
        let original = assignment(0, 0, "instructions", 1, 3);
        let corrected = apply_source_supported_corrections(
            &sources,
            &candidate(vec![assignment(0, 0, "title", 0, 0), original]),
            &audit(AuditCorrection::SplitSection {
                recipe: 0,
                section: 0,
                at: SourceSpan {
                    chunk: 0,
                    start: 2,
                    end: 2,
                },
                reason: "boundary".into(),
            }),
        )
        .unwrap();
        assert_eq!(
            corrected
                .assignments
                .iter()
                .find(|item| item.field == "instructions" && item.section == Some(0))
                .unwrap()
                .spans,
            vec![SourceSpan {
                chunk: 0,
                start: 1,
                end: 1
            }]
        );
        assert_eq!(
            corrected
                .assignments
                .iter()
                .find(|item| item.field == "instructions" && item.section == Some(1))
                .unwrap()
                .spans,
            vec![SourceSpan {
                chunk: 0,
                start: 2,
                end: 3
            }]
        );
    }

    #[test]
    fn merge_coalesces_component_spans_in_source_order() {
        let mut sources = source();
        sources[0].text = "Soup\nA\nB\nC\nD".into();
        let corrected = apply_source_supported_corrections(
            &sources,
            &candidate(vec![
                assignment(0, 0, "ingredients", 1, 1),
                assignment(0, 1, "ingredients", 2, 2),
                assignment(0, 0, "instructions", 3, 3),
                assignment(0, 1, "instructions", 4, 4),
            ]),
            &audit(AuditCorrection::MergeSections {
                owner_chunk: Some(0),
                recipe: 0,
                first: 0,
                second: 1,
                reason: "one component".into(),
            }),
        )
        .unwrap();
        assert_eq!(
            corrected
                .assignments
                .iter()
                .filter(|item| item.section == Some(0) && item.field == "ingredients")
                .count(),
            1
        );
        let ingredients = corrected
            .assignments
            .iter()
            .find(|item| item.section == Some(0) && item.field == "ingredients")
            .unwrap();
        assert_eq!(
            ingredients.spans,
            vec![
                SourceSpan {
                    chunk: 0,
                    start: 1,
                    end: 1
                },
                SourceSpan {
                    chunk: 0,
                    start: 2,
                    end: 2
                }
            ]
        );
    }

    #[test]
    fn invalid_or_cross_chunk_section_target_is_atomic() {
        let mut sources = source();
        sources.push(Chunk {
            title_hint: None,
            text: "Other\n1 cup".into(),
            doc_path: "other.xhtml".into(),
            links: vec![],
            images: vec![],
        });
        let original = candidate(vec![
            assignment(0, 0, "ingredients", 1, 1),
            FieldAssignment {
                owner_chunk: Some(0),
                recipe: 0,
                section: Some(0),
                field: "instructions".into(),
                spans: vec![SourceSpan {
                    chunk: 1,
                    start: 0,
                    end: 0,
                }],
            },
        ]);
        let result = apply_source_supported_corrections(
            &sources,
            &original,
            &audit(AuditCorrection::SplitSection {
                recipe: 0,
                section: 0,
                at: SourceSpan {
                    chunk: 0,
                    start: 1,
                    end: 1,
                },
                reason: "ambiguous".into(),
            }),
        );
        assert!(result.is_err());
        assert_eq!(original.assignments.len(), 2);
    }
}
