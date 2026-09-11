// Generated from Rust application DTOs. Do not edit.
export type ModelChoice = { id: string, label: string, enabled: boolean, status: string, };
export type HybridStrategy = "indexed" | "hybrid";
export type SourceSpan = { chunk: number, start: number, end: number, };
export type FieldAssignment = {
/**
 * The source chunk whose extractor-local recipe and section coordinates
 * own this field.  Legacy saved assignments without this value are
 * deliberately unresolved: an empty field has no span from which a
 * chunk can safely be guessed.
 */
owner_chunk?: number | null, recipe: number, section: number | null, field: string, spans: Array<SourceSpan>, };
export type AssignmentIssue = { kind: string, message: string, spans: Array<SourceSpan>, };
export type TextOverride = { assignment: FieldAssignment, before: string, after: string, spans: Array<SourceSpan>, reason: string, };
export type AuditCorrection = { "kind": "move_assignment", from: FieldAssignment, to: FieldAssignment, reason: string, } | { "kind": "move_spans", from: FieldAssignment, to: FieldAssignment, spans: Array<SourceSpan>, reason: string, } | { "kind": "restore_span", assignment: FieldAssignment, span: SourceSpan, reason: string, } | { "kind": "replace_bounded_text", assignment: FieldAssignment, before: string, after: string, spans: Array<SourceSpan>, reason: string, source_supported: boolean, } | { "kind": "split_section", recipe: number, section: number, at: SourceSpan, reason: string, } | { "kind": "merge_sections",
/**
 * Required by current corrections because recipe/section coordinates
 * are scoped to a source chunk. Missing values are unresolved legacy
 * evidence and are never defaulted to chunk zero.
 */
owner_chunk?: number | null, recipe: number, first: number, second: number, reason: string, };
export type AppliedCorrection = { correction: AuditCorrection, before: Array<FieldAssignment>, after: Array<FieldAssignment>, reason: string, };
export type AuditResult = { source_sha256: string, candidate_revision: bigint, group: number, findings: Array<AssignmentIssue>, corrections: Array<AuditCorrection>, accepted: boolean, reaudited: boolean, };
export type AuditContextExpansion = { source_sha256: string, candidate_revision: bigint, group: number, reason: string, request_key: string, };
export type CostEstimate = { usd: [number, number] | null, basis: string, uncalibrated: boolean, };
export type ExtractionPreview = { policy?: Array<string>, extractionRemainingUsd?: number, verificationRemainingUsd?: number, total: number, cached: number, pending: number, lowUsd: number | null, highUsd: number | null, extraction: CostEstimate, verification: CostEstimate, warmMs?: number, reservationUsd: number | null, basis: string, };
export type SavedRun = { qualityFlags: number | null, status: string, epubSha256: string, path: string, title: string, model: string, promptVersion: string, configurations: Array<string>, createdAt: number | null, recipes: number, completed: number, total: number, incomplete: boolean, reservedUsd: number, newSpendUsd: number | null, inheritedReservedUsd: number | null, unresolvedUsd: number, };
export type ModelBookResult = { latest: SavedRun, runs: number, failedChunks: number, pendingChunks: number, contentReviewFlags: number, processingSuccessRate: number | null, attempts: number | null, failedAttempts: number | null, };
export type ModelBookResults = { rows: Array<ModelBookResult>, unreadable: Array<string>, };
export type JsonValue = number | string | boolean | Array<JsonValue> | { [key in string]: JsonValue } | null;
export type IngredientResult = { lineNumber: number, input: string, name: string, amounts: Array<string>, modifier: string | null, optional: boolean, usage: string, confidence: string, reviewReasons: Array<string>, json: JsonValue, };
export type TraceNode = { name: string, input: string, outcome: string, detail: string, children: Array<TraceNode>, };
export type IngredientInspection = { result: IngredientResult, stages: string, trace: TraceNode | null, traceText: string, jaegerJson: string, };
export type RecipeResult = { title: string, url: string, ingredients: Array<IngredientResult>, sections: Array<CookbookSection>, source: JsonValue, parsed: JsonValue, diagnostics: Array<string>, };
export type CorpusField = { field: string, matches: boolean, expected: string, actual: string, };
export type CorpusCase = { lineNumber: number, section: string, input: string, status: string, reason: string | null, fields: Array<CorpusField>, };
export type CorpusResult = { path: string | null, cases: Array<CorpusCase>, };
export type LibraryBook = { runs: Array<SavedRun>, path: string, title: string, authors: Array<string>, subjects: Array<string>, cookbook: boolean, error: string | null, };
export type ReviewNote = { document: string, status: string, note: string, };
export type SourceBlock = { id: string, tag: string, text: string, };
export type SourceDocument = { path: string, blocks: Array<SourceBlock>, images: Array<string>, };
export type CookbookSection = { name: string | null, ingredients: Array<string>, instructions: Array<string>, };
export type CookbookRecipe = { index: number, title: string, sourceDocument: string, recipeYield: string | null, sections: Array<CookbookSection>, description: string | null, image: string | null, references: Array<string>, };
export type CookbookChunk = { id: string, document: string, complete: boolean, cached: boolean, error: string | null, };
export type QualityIssue = { kind: string, source: string, chunk: string | null, recipe: number | null, message: string, detail: string | null, };
export type ExtractionFeedback = { phase: string, stopReason: string | null, policy: Array<string>, extractionUsd: number, verificationUsd: number, unresolvedUsd: number, findings: Array<ExtractionFinding>, checks: Array<string>, };
export type ExtractionFinding = { category: string, message: string, source: string, chunk?: string, lines: Array<number>, model: string, resolved: boolean, };
export type CookbookResult = { feedback?: ExtractionFeedback, qualityIssues: Array<QualityIssue>, status: string, path: string | null, source: string, model: string, sourceHash: string, incomplete: boolean, reservedUsd: number, documents: Array<SourceDocument>, recipes: Array<CookbookRecipe>, chunks: Array<CookbookChunk>, review: Array<ReviewNote>,
/**
 * Typed AI audit evidence; human decisions remain in the sidecar.
 */
hybridAudits: Array<AuditResult>, appliedCorrections: Array<AppliedCorrection>,
/**
 * Source-context expansion requests remain separate from AI findings and
 * applied corrections; they never establish acceptance by themselves.
 */
auditContextExpansions: Array<AuditContextExpansion>,
/**
 * Original library payload, preserved for source inspection and export.
 */
run: JsonValue, };
export type ExtractionRequest = { book: string, out: string, model: string, resume: boolean, from: string | null, allowNetwork: boolean, refresh: boolean, cacheDir: string | null, chunks: Array<string>, budgetUsd: number,
/**
 * Shared extraction contract name; indexed preserves the legacy default.
 */
strategy: HybridStrategy, concurrency?: number, };
export type ExtractionProgress = { activeModels?: Array<string>, unresolvedUsd?: number, phase?: string, active: number, failed: number, elapsedSeconds: number, estimatedUsd: number | null, stopping: boolean, path: string, completed: number, total: number, recipes: number, reservedUsd: number, };
export type BookImage = { path: string, dataUrl: string, };
