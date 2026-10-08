import { Channel } from "@tauri-apps/api/core";
import type {
  BackendOptions,
  BackendStatus,
  BookImage,
  BundleExport,
  Catalog,
  CatalogOptions,
  CatalogProgress,
  CorpusResult,
  Estimate,
  GatewayStatus,
  IngredientRow,
  Inspection,
  LibraryBook,
  OpenedBook,
  OpenedRun,
  Progress,
  RecipeView,
  RichChunk,
  RunSummary,
  SourceLine,
} from "./generated";
import { core, isDesktop, isTauri, native } from "./transport";
import { wasm } from "./wasm";

export type * from "./generated";
export { engine, isDesktop, message, shell } from "./transport";

/** Fetches recipe pages for the web build (sites do not allow CORS). */
const CORS_PROXY: string = import.meta.env.VITE_CORS_PROXY ?? "https://cors.nicky.workers.dev/?target=";

async function fetchHtml(url: string): Promise<string> {
  if (isDesktop) return native<string>("fetch_html", { url });
  const response = await fetch(CORS_PROXY + encodeURIComponent(url.trim()));
  if (!response.ok) throw new Error(`Could not fetch the page (HTTP ${response.status}).`);
  return response.text();
}

/** A Tauri channel, or its fixture stand-in. */
function channel<T>(onMessage: (value: T) => void) {
  if (!isTauri()) return { onmessage: onMessage };
  const result = new Channel<T>();
  result.onmessage = onMessage;
  return result;
}

/** What this host can do. Everything else works on both. */
export const can = {
  /** Scan a Calibre folder, keep a run store, delete runs. */
  library: isDesktop,
  /** Call models: extraction and structural catalogs. */
  extract: isDesktop,
  exportBundle: isDesktop,
  reveal: isDesktop,
  /** Read arbitrary paths (corpus files) instead of opened files. */
  paths: isDesktop,
};

export const api = {
  // Portable: native Rust on the desktop, WASM in the browser.
  parseLines: (input: string) => core<IngredientRow[]>("parse_lines", { input }),
  inspect: (input: string) => core<Inspection>("inspect_ingredient", { input }),
  richText: (text: string, names: string[]) => core<RichChunk[]>("rich_text", { text, names }),
  recipe: async (url: string) => core<RecipeView>("recipe_from_html", { html: await fetchHtml(url), url: url.trim() }),
  scaleRecipe: (source: RecipeView["source"], factor: number) =>
    core<RecipeView>("scale_recipe", { source, factor }),
  /** `null` scores the corpus built into the parser. */
  corpus: (path: string | null) => core<CorpusResult>("score_corpus", { path }),
  book: (path: string) => core<OpenedBook>("open_book", { path }),
  image: (book: string, image: string) => core<BookImage>("book_image", { book, image }),
  cover: (path: string) => core<BookImage | null>("book_cover", { path }),
  /** Lines `start..end` of a book's cleaned line stream. */
  source: (path: string, start: number, end: number) =>
    core<SourceLine[]>("book_source", { path, start, end }),
  run: (path: string) => core<OpenedRun>("open_run", { path }),
  runs: () => (isDesktop ? native<RunSummary[]>("list_runs") : wasm.runs<RunSummary[]>()),

  // Desktop only.
  library: (directory: string) => native<LibraryBook[]>("scan_library", { directory }),
  estimate: (path: string, options: BackendOptions) => native<Estimate>("estimate_book", { path, options }),
  backends: () => native<BackendStatus[]>("backend_statuses"),
  gateway: () => native<GatewayStatus>("gateway_status"),
  catalogStatus: (path: string) => native<Catalog | null>("catalog_status", { path }),
  catalog: (paths: string[], options: CatalogOptions, progress: (value: CatalogProgress) => void) =>
    native<Catalog[]>("catalog_books", { paths, options, onProgress: channel(progress) }),
  exportBundle: (run: string, book: string) => native<BundleExport>("export_bundle", { run, book }),
  deleteRun: (path: string) => native<void>("delete_run", { path }),
  cancelExtraction: () => native<void>("cancel_extraction"),
  extract: (path: string, options: BackendOptions, progress: (value: Progress) => void) =>
    native<RunSummary>("extract_book", { path, options, onProgress: channel(progress) }),
};

/** Hand files the user opened in the browser to the worker; returns the names
 *  they are addressed by. */
export async function openFiles(files: Iterable<File>): Promise<string[]> {
  const paths: string[] = [];
  for (const file of files) {
    await wasm.put(file.name, await file.arrayBuffer());
    paths.push(file.name);
  }
  return paths;
}
