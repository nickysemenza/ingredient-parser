import { useCallback, useEffect, useRef, useState } from "react";
import { ArrowLeft } from "lucide-react";
import {
  api,
  discardChanges,
  pick,
  revealFile,
  type Extraction,
  type LibraryBook,
  type OpenedBook,
  type Progress,
  type RunSummary,
} from "../bridge";
import { Menu, Split, Tabs, useStored } from "../components";
import type { WorkspaceStatus } from "../shell";
import { BookPanel } from "./Extract";
import { BookTree, ItemView } from "./BookTree";
import { Diagnostics } from "./Diagnostics";
import { Library } from "./Library";
import { RunHistory } from "./RunHistory";
import { allItems, formatCost, formatDuration, formatEta } from "./format";

type View =
  | { kind: "library" }
  | { kind: "book"; book: OpenedBook }
  | { kind: "run"; path: string; extraction: Extraction };

/** The saved run, its navigation, and its diagnostics. */
function RunScreen({
  extraction,
  bookPath,
  onError,
}: {
  extraction: Extraction;
  bookPath: string | null;
  onError: (value: string) => void;
}) {
  const [tab, setTab] = useStored<"Recipes" | "Diagnostics">(
    "v1:run-tab",
    "Recipes",
    ["Recipes", "Diagnostics"],
  );
  const [selected, setSelected] = useState<string | null>(null);
  const chapters = extraction.cookbook.chapters;
  const items = allItems(chapters);
  const current = items.find((item) => item.id === selected) ?? items[0];
  const jump = useCallback(
    (id: string) => {
      setTab("Recipes");
      setSelected(id);
    },
    [setTab],
  );
  return (
    <>
      <Tabs
        value={tab}
        items={["Recipes", "Diagnostics"]}
        onChange={setTab}
        label="Run view"
      />
      {tab === "Recipes" ? (
        <Split
          id="cookbook-run"
          initial={32}
          min={20}
          max={55}
          className="run-split"
          left={
            <BookTree
              chapters={chapters}
              selected={current?.id ?? null}
              onSelect={setSelected}
            />
          }
          right={
            current ? (
              <ItemView
                key={current.id}
                item={current}
                chapters={chapters}
                bookPath={bookPath}
                onJump={jump}
                onError={onError}
              />
            ) : (
              <p className="empty-inline">This run produced no items.</p>
            )
          }
        />
      ) : (
        <Diagnostics report={extraction.report} />
      )}
    </>
  );
}

export function Cookbooks({
  onError,
  onBusy,
  onStatus,
  openSignal,
  active,
}: {
  onError: (value: string) => void;
  onBusy: (value: boolean) => void;
  onStatus: (value: WorkspaceStatus) => void;
  openSignal: number;
  active: boolean;
}) {
  const [directory, setDirectory] = useStored("v1:library-directory", "");
  const [books, setBooks] = useState<LibraryBook[]>([]);
  const [runs, setRuns] = useState<RunSummary[]>([]);
  const [scanning, setScanning] = useState(false);
  const [view, setView] = useState<View>({ kind: "library" });
  const [extracting, setExtracting] = useState(false);
  const [progress, setProgress] = useState<Progress | null>(null);
  const scanned = useRef(false);
  /** Runs do not carry their EPUB path; the library and opened books do. */
  const bookPaths = useRef(new Map<string, string>());
  const remember = useCallback((library: LibraryBook[]) => {
    for (const book of library)
      for (const run of book.runs) bookPaths.current.set(run.sha256, book.path);
  }, []);
  const scan = useCallback(
    (folder: string) => {
      setScanning(true);
      Promise.all([api.library(folder), api.runs()])
        .then(([library, saved]) => {
          remember(library);
          setBooks(library);
          setRuns(saved);
        })
        .catch((error: unknown) => onError(String(error)))
        .finally(() => setScanning(false));
    },
    [onError, remember],
  );
  useEffect(() => {
    if (!active || scanned.current) return;
    scanned.current = true;
    scan(directory);
  }, [active, directory, scan]);
  const openBook = useCallback(
    (path: string) => {
      void api
        .book(path)
        .then((book) => {
          bookPaths.current.set(book.outline.source.sha256, book.path);
          setView({ kind: "book", book });
        })
        .catch((error: unknown) => onError(String(error)));
    },
    [onError],
  );
  const openRun = useCallback(
    (path: string) => {
      void api
        .run(path)
        .then((extraction) => setView({ kind: "run", path, extraction }))
        .catch((error: unknown) => onError(String(error)));
    },
    [onError],
  );
  const openFile = useCallback(() => {
    void pick("epub")
      .then((path) => {
        if (path) openBook(path);
      })
      .catch((error: unknown) => onError(String(error)));
  }, [onError, openBook]);
  useEffect(() => {
    if (openSignal > 0) openFile();
  }, [openSignal, openFile]);
  const extract = useCallback(
    (path: string) => {
      setExtracting(true);
      setProgress(null);
      onBusy(true);
      api
        .extract(path, setProgress)
        .then((summary) => {
          openRun(summary.path);
          return api.runs().then(setRuns);
        })
        .catch((error: unknown) => onError(String(error)))
        .finally(() => {
          setExtracting(false);
          setProgress(null);
          onBusy(false);
        });
    },
    [onBusy, onError, openRun],
  );
  const cancel = useCallback(() => {
    void api
      .cancelExtraction()
      .catch((error: unknown) => onError(String(error)));
  }, [onError]);
  const removeRun = useCallback(
    async (path: string, label: string) => {
      if (
        !(await discardChanges(
          `Delete the saved run of ${label}? This cannot be undone.`,
          "Delete this run?",
          "Delete",
        ))
      )
        return;
      try {
        await api.deleteRun(path);
        setRuns(await api.runs());
        setView((previous) =>
          previous.kind === "run" && previous.path === path
            ? { kind: "library" }
            : previous,
        );
      } catch (error) {
        onError(String(error));
      }
    },
    [onError],
  );
  const title =
    view.kind === "library"
      ? "Library"
      : view.kind === "book"
        ? view.book.outline.source.title
        : view.extraction.cookbook.source.title;
  useEffect(() => {
    if (extracting) {
      onStatus({
        title,
        message: progress
          ? `Extracting ${progress.done}/${progress.total} · ${formatCost(progress.cost_so_far_usd)} · ${formatEta(progress.eta)}`
          : "Starting the extraction…",
        detail: progress ? progress.phase : "",
      });
      return;
    }
    if (view.kind === "library")
      onStatus({
        title: "Cookbooks",
        message: scanning
          ? "Scanning the library…"
          : `${books.length} book${books.length === 1 ? "" : "s"} in the library`,
        detail: `${runs.length} saved run${runs.length === 1 ? "" : "s"}`,
      });
    else if (view.kind === "book")
      onStatus({
        title,
        message: `${title} · ${view.book.classified.classification.replace("_", " ")}`,
        detail: `${view.book.outline.chunks} chunk${view.book.outline.chunks === 1 ? "" : "s"} · ${view.book.runs.length} saved run${view.book.runs.length === 1 ? "" : "s"}`,
      });
    else {
      const report = view.extraction.report;
      onStatus({
        title,
        message: `${allItems(view.extraction.cookbook.chapters).length} items · ${formatCost(report.total_cost_usd)}`,
        detail: `${formatDuration(report.wall_ms)}${report.incomplete ? " · incomplete" : ""}`,
      });
    }
  }, [view, title, extracting, progress, scanning, books, runs, onStatus]);
  const bookPath =
    view.kind === "run"
      ? (bookPaths.current.get(view.extraction.report.book.sha256) ?? null)
      : view.kind === "book"
        ? view.book.path
        : null;
  return (
    <div className="workspace cookbooks">
      <header className="workspace-heading">
        {view.kind !== "library" && (
          <button
            className="back"
            aria-label="Back to library"
            onClick={() => setView({ kind: "library" })}
          >
            <ArrowLeft size={15} />
          </button>
        )}
        <h1>{title}</h1>
        {view.kind === "run" && (
          <div className="actions">
            {view.extraction.report.incomplete && (
              <span className="badge warn">incomplete</span>
            )}
            <Menu label="Run actions">
              <button
                onClick={() =>
                  void revealFile(view.path).catch((error: unknown) =>
                    onError(String(error)),
                  )
                }
              >
                Reveal in Finder
              </button>
              <button
                disabled={!bookPath}
                onClick={() => bookPath && openBook(bookPath)}
              >
                Re-extract
              </button>
              <button
                onClick={() =>
                  void removeRun(
                    view.path,
                    `${title} (${view.extraction.report.run_id})`,
                  )
                }
              >
                Delete run
              </button>
            </Menu>
          </div>
        )}
      </header>
      {view.kind === "library" ? (
        <>
          <Library
            books={books}
            directory={directory}
            loading={scanning}
            onDirectory={(value) => {
              setDirectory(value);
              scan(value);
            }}
            onOpenBook={openBook}
            onOpenFile={openFile}
          />
          <RunHistory
            runs={runs}
            label="Run history"
            onOpen={openRun}
            onDelete={(run) =>
              void removeRun(run.path, `${run.book} (${run.run_id})`)
            }
            empty="No saved runs yet. Open a book and extract it."
          />
        </>
      ) : view.kind === "book" ? (
        <BookPanel
          book={view.book}
          extracting={extracting}
          progress={progress}
          onExtract={() => extract(view.book.path)}
          onCancel={cancel}
          onOpenRun={openRun}
          onError={onError}
        />
      ) : (
        <RunScreen
          extraction={view.extraction}
          bookPath={bookPath}
          onError={onError}
        />
      )}
    </div>
  );
}
