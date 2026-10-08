import { ArrowLeft, BookOpen, FolderOpen, MoreHorizontal, Package, RotateCcw, Trash2 } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import { toast } from "sonner";
import { api, can, isDesktop, message, openFiles, type BackendOptions, type Extraction, type LibraryBook, type OpenedBook, type Progress, type RunSummary } from "../../api";
import { confirmAction, pickPath, revealFile } from "../../api/platform";
import { OPEN_BOOK, useCommands } from "../../app/commands";
import { useReportStatus } from "../../app/status";
import { Split } from "../../components/layout";
import { ActionMenu, Badge, Button, EmptyState, IconButton, Tabs, type MenuEntry } from "../../components/ui";
import { useStored } from "../../lib/stored";
import { BookPanel } from "./BookPanel";
import { BookTree, ItemView } from "./BookTree";
import { Diagnostics } from "./Diagnostics";
import { allItems, formatDuration, formatEta } from "./format";
import { DropZone, Library } from "./Library";
import { costOf, Cover, plural, RunList } from "./parts";

type View = { kind: "home" } | { kind: "book"; book: OpenedBook } | { kind: "run"; path: string; extraction: Extraction };

function RunScreen({ extraction, bookPath }: { extraction: Extraction; bookPath: string | null }) {
  const [tab, setTab] = useStored<"Recipes" | "Diagnostics">("v2:run-tab", "Recipes", ["Recipes", "Diagnostics"]);
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
      <Tabs className="shrink-0 px-5" value={tab} items={["Recipes", "Diagnostics"] as const} onChange={setTab} label="Run view" />
      {tab === "Recipes" ? (
        <Split
          id="cookbook-run"
          initial={24}
          min={16}
          max={45}
          left={<BookTree chapters={chapters} selected={current?.id ?? null} onSelect={setSelected} />}
          right={current ? <ItemView key={current.id} item={current} chapters={chapters} bookPath={bookPath} onJump={jump} /> : <EmptyState title="This run produced no items" />}
        />
      ) : (
        <Diagnostics report={extraction.report} />
      )}
    </>
  );
}

/** Books opened in this browser tab. */
interface SessionBook {
  path: string;
  title: string;
  authors: string[];
}

export default function Cookbooks({ active }: { active: boolean }) {
  const [directory, setDirectory] = useStored("v2:library-directory", "");
  const [books, setBooks] = useState<LibraryBook[]>([]);
  const [session, setSession] = useState<SessionBook[]>([]);
  const [runs, setRuns] = useState<RunSummary[]>([]);
  const [scanning, setScanning] = useState(false);
  const [view, setView] = useState<View>({ kind: "home" });
  const [extracting, setExtracting] = useState(false);
  const [cataloging, setCataloging] = useState(false);
  const [exporting, setExporting] = useState(false);
  const [progress, setProgress] = useState<Progress | null>(null);
  const scanned = useRef(false);
  /** Runs do not carry their EPUB path; the library and opened books do. */
  const bookPaths = useRef(new Map<string, string>());

  const fail = (error: unknown) => toast.error(message(error));
  const scan = useCallback((folder: string) => {
    setScanning(true);
    Promise.all([api.library(folder), api.runs()])
      .then(([library, saved]) => {
        for (const book of library) for (const run of book.runs) bookPaths.current.set(run.sha256, book.path);
        setBooks(library);
        setRuns(saved);
      })
      .catch(fail)
      .finally(() => setScanning(false));
  }, []);
  useEffect(() => {
    if (!active || scanned.current || !can.library) return;
    scanned.current = true;
    scan(directory);
  }, [active, directory, scan]);

  const openBook = useCallback((path: string) => {
    api.book(path).then((book) => {
      bookPaths.current.set(book.outline.source.sha256, book.path);
      setSession((was) => (was.some((b) => b.path === path) ? was : [...was, { path, title: book.outline.source.title, authors: book.outline.source.authors }]));
      setView({ kind: "book", book });
    }, fail);
  }, []);
  const openRun = useCallback((path: string) => {
    api.run(path).then((opened) => setView({ kind: "run", path, extraction: opened.extraction }), fail);
  }, []);
  const openFile = useCallback(() => {
    pickPath("epub").then((path) => path && openBook(path), fail);
  }, [openBook]);
  useEffect(() => {
    const listener = () => openFile();
    window.addEventListener(OPEN_BOOK, listener);
    return () => window.removeEventListener(OPEN_BOOK, listener);
  }, [openFile]);

  /** Browser: the dropped files become readable by name in the worker. */
  const dropped = async (files: File[]) => {
    try {
      const names = await openFiles(files);
      const epubs = names.filter((n) => n.toLowerCase().endsWith(".epub"));
      const jsons = names.filter((n) => n.toLowerCase().endsWith(".json"));
      if (names.length > epubs.length + jsons.length) toast.warning("Only .epub and run .json files can be opened.");
      // Index every book first so a run dropped with its EPUB finds it.
      const opened = await Promise.all(epubs.map((path) => api.book(path)));
      for (const book of opened) bookPaths.current.set(book.outline.source.sha256, book.path);
      setSession((was) => [
        ...was.filter((b) => !epubs.includes(b.path)),
        ...opened.map((b) => ({ path: b.path, title: b.outline.source.title, authors: b.outline.source.authors })),
      ]);
      setRuns(await api.runs());
      if (jsons.length) openRun(jsons[jsons.length - 1]);
      else if (opened.length) setView({ kind: "book", book: opened[opened.length - 1] });
    } catch (error) {
      fail(error);
    }
  };

  const extract = (path: string, options: BackendOptions) => {
    setExtracting(true);
    setProgress(null);
    api
      .extract(path, options, setProgress)
      .then((summary) => {
        toast.success(`Extracted ${plural(summary.recipes, "recipe")}`);
        openRun(summary.path);
        return api.runs().then(setRuns);
      })
      .catch(fail)
      .finally(() => {
        setExtracting(false);
        setProgress(null);
      });
  };

  const removeRun = async (path: string, label: string) => {
    if (!(await confirmAction(`Delete the saved run of ${label}? This cannot be undone.`, { title: "Delete this run?", okLabel: "Delete" }))) return;
    try {
      await api.deleteRun(path);
      setRuns(await api.runs());
      setView((was) => (was.kind === "run" && was.path === path ? { kind: "home" } : was));
    } catch (error) {
      fail(error);
    }
  };

  const bookPath = view.kind === "run" ? (bookPaths.current.get(view.extraction.report.book.sha256) ?? null) : view.kind === "book" ? view.book.path : null;

  const exportBundle = async (runPath: string, choose = false) => {
    setExporting(true);
    try {
      const source = choose || !bookPath ? await pickPath("epub") : bookPath;
      if (!source) return;
      const result = await api.exportBundle(runPath, source);
      toast.success("Bundle exported", { description: result.path, action: { label: "Reveal", onClick: () => void revealFile(result.path).catch(fail) } });
    } catch (error) {
      toast.error(message(error), { action: { label: "Choose EPUB", onClick: () => void exportBundle(runPath, true) } });
    } finally {
      setExporting(false);
    }
  };

  const title = view.kind === "home" ? "Cookbooks" : view.kind === "book" ? view.book.outline.source.title : view.extraction.cookbook.source.title;
  const busy = extracting || cataloging || exporting;
  let status = { message: "", detail: "" };
  if (extracting)
    status = {
      message: progress ? `Extracting ${progress.done}/${progress.total} · ${costOf(progress.active_models, progress.cost_so_far_usd)} · ${formatEta(progress.eta)}` : "Starting the extraction…",
      detail: progress?.phase ?? "",
    };
  else if (view.kind === "home")
    status = can.library
      ? { message: scanning ? "Scanning the library…" : `${plural(books.length, "book")} in the library`, detail: plural(runs.length, "saved run") }
      : { message: session.length || runs.length ? `${plural(session.length, "book")} · ${plural(runs.length, "run")} open` : "No cookbook open", detail: "" };
  else if (view.kind === "book")
    status = { message: `${title} · ${view.book.classified.classification.replace("_", " ")}`, detail: `${plural(view.book.outline.chunks, "chunk")} · ${plural(view.book.runs.length, "saved run")}` };
  else {
    const report = view.extraction.report;
    status = { message: `${plural(allItems(view.extraction.cookbook.chapters).length, "item")} · ${costOf(report.options.ladder, report.total_cost_usd)}`, detail: `ran ${formatDuration(report.wall_ms)}${report.incomplete ? " · incomplete" : ""}` };
  }
  useReportStatus("cookbooks", { ...status, busy, title });

  useCommands(
    "cookbooks",
    [
      ...(isDesktop ? [{ id: "open-epub", title: "Open EPUB…", group: "Cookbooks", icon: <BookOpen />, shortcut: "⌘O", run: openFile }] : []),
      ...(view.kind !== "home" ? [{ id: "cookbooks-home", title: can.library ? "Back to library" : "Back to opened files", group: "Cookbooks", icon: <ArrowLeft />, run: () => setView({ kind: "home" }) }] : []),
    ],
    [view.kind],
  );

  const runMenu: MenuEntry[] =
    view.kind === "run"
      ? [
          ...(can.reveal ? [{ label: "Reveal in Finder", icon: <FolderOpen />, onSelect: () => void revealFile(view.path).catch(fail) }] : []),
          ...(can.exportBundle ? [{ label: "Export bundle", icon: <Package />, disabled: exporting, onSelect: () => void exportBundle(view.path) }] : []),
          ...(bookPath ? [{ label: can.extract ? "Re-extract" : "Open the book", icon: <RotateCcw />, disabled: exporting, onSelect: () => openBook(bookPath) }] : []),
          ...(can.library ? (["separator", { label: "Delete run", icon: <Trash2 />, danger: true, disabled: exporting, onSelect: () => void removeRun(view.path, `${title} (${view.extraction.report.run_id})`) }] as MenuEntry[]) : []),
        ]
      : [];

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <header className="flex h-12 shrink-0 items-center gap-2 border-b border-line px-4">
        {view.kind !== "home" && (
          <IconButton label={can.library ? "Back to library" : "Back to opened files"} disabled={exporting} onClick={() => setView({ kind: "home" })}>
            <ArrowLeft />
          </IconButton>
        )}
        <h1 className="min-w-0 truncate text-[15px] font-semibold tracking-tight">{title}</h1>
        {view.kind === "run" && (
          <div className="ml-auto flex items-center gap-2">
            {view.extraction.report.incomplete && <Badge tone="warn">incomplete</Badge>}
            {can.exportBundle && (
              <Button size="sm" busy={exporting} onClick={() => void exportBundle(view.path)}>
                <Package />
                Export bundle
              </Button>
            )}
            {runMenu.length > 0 && <ActionMenu label="Run actions" trigger={<MoreHorizontal />} items={runMenu} />}
          </div>
        )}
      </header>
      {view.kind === "home" ? (
        <div className="min-h-0 flex-1 overflow-auto">
          <div className="mx-auto max-w-6xl space-y-6 p-5">
            {can.library ? (
              <Library books={books} directory={directory} loading={scanning} onDirectory={(value) => { setDirectory(value); scan(value); }} onOpenBook={openBook} onOpenFile={openFile} onCataloging={setCataloging} />
            ) : (
              <>
                <DropZone onFiles={(files) => void dropped(files)} compact={session.length + runs.length > 0} />
                {session.length > 0 && (
                  <section aria-label="Opened books">
                    <h2 className="mb-2 text-[12.5px] font-semibold">Opened books</h2>
                    <ul className="grid grid-cols-[repeat(auto-fill,minmax(260px,1fr))] gap-2">
                      {session.map((book) => (
                        <li key={book.path}>
                          <button type="button" onClick={() => openBook(book.path)} className="flex w-full items-center gap-3 rounded-panel border border-line bg-evidence p-2.5 text-left hover:border-line-strong">
                            <Cover path={book.path} className="h-14 w-10 shrink-0" />
                            <span className="min-w-0">
                              <span className="block truncate text-[13px] font-medium">{book.title}</span>
                              <span className="block truncate text-[11.5px] text-muted">{book.authors.join(", ") || book.path}</span>
                            </span>
                          </button>
                        </li>
                      ))}
                    </ul>
                  </section>
                )}
              </>
            )}
            <RunList
              runs={runs}
              label={can.library ? "Run history" : "Opened runs"}
              onOpen={openRun}
              onReveal={can.reveal ? (run) => void revealFile(run.path).catch(fail) : undefined}
              onDelete={can.library ? (run) => void removeRun(run.path, `${run.book} (${run.run_id})`) : undefined}
              empty={can.library ? "No saved runs yet. Open a book and extract it." : "Drop a run's .json (from the desktop app or food-cli) to review it here."}
            />
          </div>
        </div>
      ) : view.kind === "book" ? (
        <BookPanel
          book={view.book}
          extracting={extracting}
          progress={progress}
          cataloging={cataloging}
          onCataloging={setCataloging}
          onExtract={(options) => extract(view.book.path, options)}
          onCancel={() => void api.cancelExtraction().catch(fail)}
          onOpenRun={openRun}
        />
      ) : (
        <RunScreen key={view.path} extraction={view.extraction} bookPath={bookPath} />
      )}
    </div>
  );
}
