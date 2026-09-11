import { Channel, invoke } from "@tauri-apps/api/core";
import { open, confirm } from "@tauri-apps/plugin-dialog";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import type {
  JsonValue as Json,
  IngredientResult,
  IngredientInspection,
  RecipeResult,
  CorpusResult,
  LibraryBook,
  OpenedBook,
  BookImage,
  GatewayStatus,
  Estimate,
  Extraction,
  Progress,
  RunSummary,
} from "./generated";
/** The generated DTOs the workspaces name; the file itself stays the contract. */
export type {
  JsonValue as Json,
  IngredientResult,
  IngredientInspection,
  TraceNode,
  RecipeResult,
  CorpusResult,
  LibraryBook,
  OpenedBook,
  BookImage,
  GatewayStatus,
  Estimate,
  Extraction,
  Progress,
  Eta,
  RunSummary,
  RunReport,
  CallRecord,
  CallOutcome,
  ChunkReport,
  Flag,
  Chapter,
  Item,
  Recipe,
  Technique,
  Essay,
  Section,
  Step,
  RecipeRef,
  ImageRef,
  Span,
  Measure,
} from "./generated";
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
  parse: (input: string) => call<IngredientResult[]>("parse_batch", { input }),
  inspect: (input: string) =>
    call<IngredientInspection>("inspect_ingredient", { input }),
  recipe: (url: string) => call<RecipeResult>("load_recipe", { url }),
  scaleWeb: (source: Json, factor: number) =>
    call<RecipeResult>("scale_web_recipe", { source, factor }),
  corpus: (path: string | null) => call<CorpusResult>("load_corpus", { path }),
  /** An empty directory means the default Calibre folder. */
  library: (directory: string) =>
    call<LibraryBook[]>("scan_library", { directory }),
  /** Offline: the outline, the structural verdict, and this book's runs. */
  book: (path: string) => call<OpenedBook>("open_book", { path }),
  estimate: (path: string) => call<Estimate>("estimate_book", { path }),
  gateway: () => call<GatewayStatus>("gateway_status"),
  runs: () => call<RunSummary[]>("list_runs"),
  run: (path: string) => call<Extraction>("open_run", { path }),
  deleteRun: (path: string) => call<void>("delete_run", { path }),
  /** One archive image of `book` (an EPUB path), as a data URL. */
  image: (book: string, image: string) =>
    call<BookImage>("book_image", { book, image }),
  cover: (path: string) => call<BookImage | null>("load_cover", { path }),
  cancelExtraction: () => call<void>("cancel_extraction"),
  extract: (path: string, progress: (value: Progress) => void) => {
    if (window.__FIXTURE_INVOKE__)
      return call<RunSummary>("extract_book", {
        path,
        onProgress: { onmessage: progress },
      });
    const onProgress = new Channel<Progress>();
    onProgress.onmessage = progress;
    return call<RunSummary>("extract_book", { path, onProgress });
  },
};
export async function pick(kind: "epub" | "directory") {
  if (window.__FIXTURE_INVOKE__)
    return call<string | null>("dialog_open", { kind });
  return (await open({
    directory: kind === "directory",
    multiple: false,
    filters:
      kind === "directory"
        ? undefined
        : [{ name: "Cookbook EPUB", extensions: ["epub"] }],
  })) as string | null;
}
export async function discardChanges(
  message = "Discard the current work?",
  title = "Leave current work?",
  okLabel = "Leave",
) {
  return isNative()
    ? confirm(message, {
        title,
        kind: "warning",
        okLabel,
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

export const revealFile = (path: string) => call<void>("reveal_file", { path });
export const openSourceUrl = (url: string) =>
  call<void>("open_source_url", { url });
