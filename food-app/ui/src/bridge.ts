import { Channel, invoke } from "@tauri-apps/api/core";
import { open, save, confirm } from "@tauri-apps/plugin-dialog";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import type {
  ModelBookResults,
  ModelChoice,
  ExtractionPreview,
  SavedRun,
  JsonValue as Json,
  IngredientResult,
  IngredientInspection,
  RecipeResult,
  CorpusResult,
  LibraryBook,
  CookbookRecipe,
  CookbookResult,
  ExtractionRequest,
  ExtractionProgress,
  BookImage,
} from "./generated";
export type {
  JsonValue as Json,
  IngredientResult,
  IngredientInspection,
  TraceNode,
  RecipeResult,
  CorpusResult,
  LibraryBook,
  CookbookRecipe,
  CookbookResult,
  ExtractionRequest,
  ExtractionProgress,
  BookImage,
  SourceDocument,
} from "./generated";
export interface Decision {
  status: string;
  note: string;
}
export const isNative = () => "__TAURI_INTERNALS__" in window;
declare global {
  interface Window {
    __FIXTURE_INVOKE__?: (
      command: string,
      args?: Record<string, unknown>,
    ) => Promise<unknown>;
  }
}
export function call<T>(
  command: string,
  args?: Record<string, unknown>,
): Promise<T> {
  if (window.__FIXTURE_INVOKE__)
    return window.__FIXTURE_INVOKE__(command, args) as Promise<T>;
  if (!isNative())
    return Promise.reject(
      new Error(
        "Open the desktop application to use local parser and cookbook commands.",
      ),
    );
  return invoke<T>(command, args);
}
export const api = {
  results: (book: string | null) => call<ModelBookResults>("cookbook_results", { book }),
  models: () => call<ModelChoice[]>("cookbook_models"),
  runs: (book: string | null) => call<SavedRun[]>("cookbook_runs", { book }),
  preview: (request: ExtractionRequest) =>
    call<ExtractionPreview>("extraction_preview", { request }),
  exportRun: (path: string, out: string) =>
    call<void>("export_run", { path, out }),
  parse: (input: string) => call<IngredientResult[]>("parse_batch", { input }),
  inspect: (input: string) =>
    call<IngredientInspection>("inspect_ingredient", { input }),
  recipe: (url: string) => call<RecipeResult>("load_recipe", { url }),
  corpus: (path: string | null) => call<CorpusResult>("load_corpus", { path }),
  library: (directory: string) =>
    call<LibraryBook[]>("scan_library", { directory }),
  book: (path: string) =>
    call<CookbookResult>("inspect_book", { path, model: null }),
  run: (path: string) => call<CookbookResult>("open_run", { path }),
  cancelExtraction: () => call<void>("cancel_extraction", {}),
  extract: (
    request: ExtractionRequest,
    progress: (value: ExtractionProgress) => void,
  ) => {
    if (window.__FIXTURE_INVOKE__)
      return call<CookbookResult>("extract_run", {
        request,
        onProgress: { onmessage: progress },
      });
    const onProgress = new Channel<ExtractionProgress>();
    onProgress.onmessage = progress;
    return call<CookbookResult>("extract_run", { request, onProgress });
  },
  scale: (path: string, index: number, factor: number) =>
    call<CookbookRecipe>("scale_recipe", { path, index, factor }),
  scaleWeb: (source: Json, factor: number) =>
    call<RecipeResult>("scale_web_recipe", { source, factor }),
  cover: (path: string) => call<BookImage | null>("load_cover", { path }),
  replay: (path: string, out: string) =>
    call<CookbookResult>("replay_run", { path, out }),
  review: (path: string, document: string, status: string, note: string) =>
    call<CookbookResult>("save_review", { path, document, status, note }),
  stats: (path: string) => call<Json>("run_stats", { path }),
  evaluate: (path: string, expectations: string) =>
    call<Json>("evaluate_run", { path, expectations }),
  audit: (path: string) => call<Json>("run_audit", { path }),
  diff: (before: string, after: string) =>
    call<Json>("run_diff", { before, after }),
  images: (runPath: string | null, bookPath: string) =>
    call<BookImage[]>("load_images", {
      runPath,
      bookPath,
    }),
};
export async function pick(kind: "epub" | "json" | "directory") {
  if (window.__FIXTURE_INVOKE__)
    return call<string | null>("dialog_open", { kind });
  return (await open({
    directory: kind === "directory",
    multiple: false,
    filters:
      kind === "directory"
        ? undefined
        : [
            {
              name: kind === "epub" ? "Cookbook EPUB" : "JSON",
              extensions: [kind],
            },
          ],
  })) as string | null;
}
export async function savePath(name: string) {
  if (window.__FIXTURE_INVOKE__)
    return call<string | null>("dialog_save", { name });
  return save({
    defaultPath: name,
    filters: [{ name: "JSON", extensions: ["json"] }],
  });
}
export async function discardChanges(
  message = "Discard the unsaved review changes?",
) {
  return isNative()
    ? confirm(message, {
        title: "Leave current work?",
        kind: "warning",
        okLabel: "Leave",
        cancelLabel: "Keep working",
      })
    : window.confirm(message);
}
export async function copy(text: string) {
  if (isNative()) await writeText(text);
  else await navigator.clipboard.writeText(text);
}
export function display(v: unknown): string {
  return v === null || v === undefined
    ? "—"
    : typeof v === "string"
      ? v
      : typeof v === "object"
        ? JSON.stringify(v)
        : String(v);
}

export const revealFile = (path: string, review = false) =>
  call<void>("reveal_file", { path, review });
export const openSourceUrl = (url: string) =>
  call<void>("open_source_url", { url });
