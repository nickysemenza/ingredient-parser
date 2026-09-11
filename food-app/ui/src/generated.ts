// Generated from Rust DTOs (food-app + cookbook). Do not edit.
export type JsonValue = number | string | boolean | Array<JsonValue> | { [key in string]: JsonValue } | null;
export type IngredientResult = { lineNumber: number, input: string, name: string, amounts: Array<string>, modifier: string | null, optional: boolean, usage: string, confidence: string, reviewReasons: Array<string>, json: JsonValue, };
export type TraceNode = { name: string, input: string, outcome: string, detail: string, children: Array<TraceNode>, };
export type IngredientInspection = { result: IngredientResult, stages: string, trace: TraceNode | null, traceText: string, jaegerJson: string, };
export type WebSection = { name: string | null, ingredients: Array<string>, instructions: Array<string>, };
export type RecipeResult = { title: string, url: string, ingredients: Array<IngredientResult>, sections: Array<WebSection>, source: JsonValue, parsed: JsonValue, diagnostics: Array<string>, };
export type CorpusField = { field: string, matches: boolean, expected: string, actual: string, };
export type CorpusCase = { lineNumber: number, section: string, input: string, status: string, reason: string | null, fields: Array<CorpusField>, };
export type CorpusResult = { path: string | null, cases: Array<CorpusCase>, };
export type LibraryBook = { path: string, title: string, authors: Array<string>, subjects: Array<string>,
/**
 * The catalog subjects mention cooking. The structural verdict comes
 * with `open_book`.
 */
cookbookHint: boolean,
/**
 * Saved runs of this exact file, newest first (matched by file name until
 * the book is opened and hashed).
 */
runs: Array<RunSummary>, error: string | null, };
export type OpenedBook = { path: string, outline: BookOutline, classified: Classified,
/**
 * Runs of this exact file (by content hash), newest first.
 */
runs: Array<RunSummary>, openMs: number, };
export type BookImage = { path: string, dataUrl: string, };
export type GatewayStatus = { configured: boolean, baseUrl: string | null,
/**
 * The file the app reads when launched outside a shell.
 */
configPath: string | null, cacheDir: string | null, runsDir: string | null, ladder: Array<string>, error: string | null, };
export type RunSummary = { path: string, run_id: string, book: string, sha256: string, started_at: string, ladder: Array<string>, recipes: number, items: number, recall: number | null, cost_usd: number, wall_ms: number, incomplete: boolean, };
export type Classified = { classification: Classification,
/**
 * 0 = surely not, 1 = surely a cookbook.
 */
score: number,
/**
 * `structure` or the model id that decided.
 */
method: string, reasons: Array<string>, quantity_lines: number, ingredient_runs: number, nav_recipe_titles: number, };
export type Classification = "cookbook" | "not_cookbook" | "ambiguous";
export type BookOutline = { source: BookSource, cover: ImageRef | null,
/**
 * Depth-1 table-of-contents labels.
 */
chapters: Array<string>,
/**
 * Table-of-contents entries that name recipes.
 */
nav_recipe_titles: number, chunks: number, lines: number, };
export type Estimate = { chunks: number, lines: number, chars: number,
/**
 * Chunks the cache already answers.
 */
cache_hits: number, input_tokens: number, output_tokens: number, calls_low: number, calls_high: number, wall_ms_low: number, wall_ms_high: number, cost_usd_low: number, cost_usd_high: number, ladder: Array<string>, concurrency: number,
/**
 * Human-readable assumptions the numbers rest on.
 */
assumptions: Array<string>, };
export type Progress = { phase: Phase,
/**
 * Chunks settled (extracted, cached, or failed).
 */
done: number, total: number, in_flight: number, failed: number, cached: number, recipes_so_far: number, cost_so_far_usd: number, elapsed_ms: number, eta: Eta,
/**
 * Models currently answering calls.
 */
active_models: Array<string>, };
export type Phase = "open" | "clean" | "nav" | "chunk" | "extract" | "crosscheck" | "second_opinion" | "escalation" | "assemble" | "refs" | "names" | "parse" | "done";
export type Eta = { remaining_low_ms: number, remaining_high_ms: number, projected_cost_usd: number, };
export type Extraction = { cookbook: Cookbook, report: RunReport, };
export type Cookbook = {
/**
 * The model contract version that produced this tree (`CONTRACT_VERSION`).
 */
contract: string, source: BookSource, cover: ImageRef | null,
/**
 * In reading order. A leading chapter with `title: None` holds anything
 * that precedes the first table-of-contents entry.
 */
chapters: Array<Chapter>,
/**
 * Every cross-reference between items, deduplicated. Dependencies are the
 * edges whose `kind` is `ingredient` or `variation`.
 */
edges: Array<Edge>, };
export type BookSource = {
/**
 * The caller's label (a file name, a display title).
 */
label: string,
/**
 * SHA-256 of the EPUB bytes, hex.
 */
sha256: string,
/**
 * `<dc:title>`.
 */
title: string, authors: Array<string>,
/**
 * `<dc:identifier>` values (ISBNs, URNs), verbatim.
 */
identifiers: Array<string>, subjects: Array<string>, spine_docs: number,
/**
 * Cleaned text lines in the whole book.
 */
lines: number, };
export type Chapter = {
/**
 * `"ch00"`, `"ch01"`, … in reading order.
 */
id: string,
/**
 * The table-of-contents label; `None` for the leading pre-contents chapter.
 */
title: string | null, span: Span,
/**
 * Prose lines between the chapter heading and its first item.
 */
intro: Array<string>, items: Array<Item>, };
export type Item = { "kind": "recipe" } & Recipe | { "kind": "technique" } & Technique | { "kind": "essay" } & Essay;
export type Recipe = {
/**
 * Deterministic for a given EPUB: `"{spine_index:03}.{doc_line:04}"` of
 * the title line, so re-extraction yields the same ids.
 */
id: string,
/**
 * Verbatim from the source.
 */
title: string,
/**
 * Unique within the book: the title, disambiguated when the book repeats
 * it (variation qualifier, then chapter, then a counter).
 */
name: string, meta: RecipeMeta, sections: Array<Section>,
/**
 * Hero photo first.
 */
photos: Array<ImageRef>, notes: Array<Note>,
/**
 * The parent recipe's id when this is a titled variation with its own
 * ingredient list.
 */
variant_of: string | null, span: Span, };
export type Technique = { id: string, title: string, name: string, description: Array<string>, steps: Array<Step>, photos: Array<ImageRef>, span: Span, };
export type Essay = { id: string, title: string, name: string, text: Array<string>, photos: Array<ImageRef>, span: Span, };
export type RecipeMeta = {
/**
 * Headnote paragraphs, verbatim.
 */
description: Array<string>, recipe_yield: string | null, times: RecipeTimes | null, equipment: Array<string>,
/**
 * A category printed with the recipe (not the chapter, which is
 * structural).
 */
category: string | null,
/**
 * The printed page, when the source carries one.
 */
page: string | null, };
export type RecipeTimes = { active?: string | null, total?: string | null, prep?: string | null, cook?: string | null, active_minutes?: number | null, total_minutes?: number | null, prep_minutes?: number | null, cook_minutes?: number | null, };
export type Section = {
/**
 * `None` for the main or only section.
 */
name: string | null, ingredients: Array<IngredientLine>, steps: Array<Step>, };
export type IngredientLine = {
/**
 * The source line, verbatim.
 */
raw: string,
/**
 * Global line index in the book's cleaned line stream.
 */
line: number,
/**
 * The `ingredient` parser's structured reading of `raw`.
 */
parsed: Ingredient, confidence: Confidence,
/**
 * Set when the line names another item in this book.
 */
ref: RecipeRef | null, };
export type Ingredient = {
/**
 * The main ingredient name
 */
name: string,
/**
 * Vector of measurements with units
 */
amounts: Array<Measure>,
/**
 * Optional preparation instructions or modifiers
 */
modifier: string | null,
/**
 * Whether this ingredient is optional (e.g., wrapped in parentheses)
 */
optional?: boolean,
/**
 * The role this line plays in the recipe (frying medium, garnish, …).
 * Required on purpose — no serde default — so stale serialized data fails
 * loudly instead of silently reading as `Normal`.
 */
usage: IngredientUsage, };
export type Measure = { unit: string, value: number, upper_value: number | null, };
export type IngredientUsage = "normal" | "frying_medium" | "pan_grease" | "seasoning" | "dredging" | "garnish" | "marinade";
export type Confidence = "high" | "medium" | "low";
export type Step = { text: string, line: number, refs: Array<RecipeRef>, };
export type Note = {
/**
 * A printed label such as `DO AHEAD` or a variation's title.
 */
label: string | null, text: string, line: number, refs: Array<RecipeRef>, };
export type RecipeRef = { target_id: string,
/**
 * The text that carried the reference (link text or matched title).
 */
text: string, kind: RefKind, method: RefMethod, };
export type RefKind = "ingredient" | "step" | "note" | "variation" | "caption";
export type RefMethod = "anchor" | "page" | "title";
export type Edge = { from: string, to: string, kind: RefKind, method: RefMethod, };
export type ImageRef = {
/**
 * Archive-relative path (resolved against the referencing document).
 */
path: string, mime: string, alt: string | null,
/**
 * Caption text printed with the image, when any.
 */
caption: string | null,
/**
 * Global line index the image sits at; `None` for the cover.
 */
line: number | null, };
export type Span = { start: number,
/**
 * Exclusive.
 */
end: number, doc_path: string, page: string | null, };
export type RunReport = { run_id: string,
/**
 * RFC 3339.
 */
started_at: string, finished_at: string, book: BookSource, options: ExtractOptions,
/**
 * The estimate taken before the first call, for comparison.
 */
estimate: Estimate, stages: Array<StageTiming>, calls: Array<CallRecord>, chunks: Array<ChunkReport>, crosscheck: CrossCheck, escalation: Escalation | null, unresolved_refs: Array<UnresolvedRef>, usage_by_model: Array<ModelUsage>, total_cost_usd: number,
/**
 * `false` when some call used an unpriced model.
 */
cost_complete: boolean, eta_trace: Array<EtaSample>, wall_ms: number,
/**
 * Some chunk failed every model, or escalation ran out of ladder.
 */
incomplete: boolean, cancelled: boolean, };
export type ExtractOptions = {
/**
 * The caller's label for the book (shown in reports and gateway metadata).
 */
label: string,
/**
 * Chunks in flight at once.
 */
concurrency: number,
/**
 * Model ids, cheapest first. Empty = `models::DEFAULT_LADDER`.
 */
ladder: Array<string>,
/**
 * Ask a second model to re-read chunks that were flagged.
 */
second_opinion: boolean,
/**
 * Re-run the whole book with a stronger model when too much is flagged.
 */
whole_book_escalation: boolean, max_output_tokens: number, };
export type StageTiming = { stage: Phase, ms: number, };
export type CallRecord = { seq: number, chunk_id: string, model: string,
/**
 * Position of `model` in the ladder.
 */
tier: number, purpose: CallPurpose,
/**
 * 1-based attempt on this chunk with this model.
 */
attempt: number,
/**
 * Milliseconds since the run started.
 */
started_ms: number, latency_ms: number, cached: boolean, status: number | null, request_id: string | null, usage: Usage,
/**
 * `None` when the model is unpriced.
 */
cost_usd: number | null, truncated: boolean, outcome: CallOutcome, };
export type CallPurpose = "extract" | "retry" | "second_opinion" | "escalation" | "classify";
export type CallOutcome = { "outcome": "ok" } | { "outcome": "invalid", faults: Array<string>, } | { "outcome": "transport", kind: string, message: string, };
export type Usage = { input_tokens: number, output_tokens: number, cache_read_input_tokens: number, cache_creation_input_tokens: number, };
export type ChunkReport = { id: string,
/**
 * Global line range, end exclusive.
 */
start: number, end: number, chars: number, status: ChunkStatus, final_model: string | null, attempts: number, flags: Array<Flag>, second_opinion: SecondOpinion | null, recipes: number, cached: boolean, };
export type ChunkStatus = "ok" | "failed";
export type Flag = { "flag": "low_amount_parse_rate", rate: number, lines: number, } | { "flag": "ingredient_like_ignored", count: number, } | { "flag": "recipe_without_steps", title: string, } | { "flag": "missing_nav_title", title: string, line: number, } | { "flag": "phantom_title", title: string, } | { "flag": "truncated" } | { "flag": "caption_as_title", line: number, } | { "flag": "unassigned_lines", count: number, } | { "flag": "prose_ingredients", title: string, };
export type SecondOpinion = { model: string, chosen: Chosen,
/**
 * The rule that decided.
 */
reason: string, };
export type Chosen = "first" | "second";
export type CrossCheck = { nav_titles: number, matched: number, missing: Array<string>, phantom: Array<string>,
/**
 * `None` when the book has too few contents entries to judge.
 */
recall: number | null, };
export type Escalation = { reason: string, flagged_fraction: number, from_model: string, to_model: string, };
export type UnresolvedRef = { item_id: string, line: number, text: string, attempted: Array<RefMethod>, };
export type ModelUsage = { model: string, calls: number, usage: Usage, cost_usd: number | null, };
export type EtaSample = { elapsed_ms: number, remaining_low_ms: number, remaining_high_ms: number, };
