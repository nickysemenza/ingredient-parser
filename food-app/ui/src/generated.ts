// Generated from Rust application DTOs. Do not edit.
export type ModelChoice = { id: string, label: string, enabled: boolean, status: string, };
export type ExtractionPreview = { total: number, cached: number, pending: number, lowUsd: number | null, highUsd: number | null, reservationUsd: number | null, basis: string, };
export type SavedRun = { qualityFlags: number | null, status: string, epubSha256: string, path: string, title: string, model: string, promptVersion: string, configurations: Array<string>, createdAt: number | null, recipes: number, completed: number, total: number, incomplete: boolean, reservedUsd: number, newSpendUsd: number | null, inheritedReservedUsd: number | null, unresolvedUsd: number, };
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
export type CookbookResult = { qualityIssues: Array<QualityIssue>, status: string, path: string | null, source: string, model: string, sourceHash: string, incomplete: boolean, reservedUsd: number, documents: Array<SourceDocument>, recipes: Array<CookbookRecipe>, chunks: Array<CookbookChunk>, review: Array<ReviewNote>,
/**
 * Original library payload, preserved for source inspection and export.
 */
run: JsonValue, };
export type ExtractionRequest = { book: string, out: string, model: string, resume: boolean, from: string | null, allowNetwork: boolean, refresh: boolean, cacheDir: string | null, chunks: Array<string>, budgetUsd: number, };
export type ExtractionProgress = { active: number, failed: number, elapsedSeconds: number, estimatedUsd: number | null, stopping: boolean, path: string, completed: number, total: number, recipes: number, reservedUsd: number, };
export type BookImage = { path: string, dataUrl: string, };
