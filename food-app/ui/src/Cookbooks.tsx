import { RecipeScale } from "@ingredient-parser/recipe-ui";
import { useEffect, useMemo, useRef, useState } from "react";
import {
  ArrowLeft,
  BookOpen,
  Check,
  FileText,
  FolderOpen,
  Search,
  X,
} from "lucide-react";
import {
  api,
  isNative,
  revealFile,
  copy,
  discardChanges,
  pick,
  savePath,
  type CookbookRecipe,
  type CookbookResult,
  type ExtractionProgress,
  type Decision,
  type IngredientInspection,
  type Json,
  type LibraryBook,
  type SourceDocument,
} from "./bridge";
import {
  Fields,
  Menu,
  Modal,
  Split,
  Tabs,
  useStored,
  VirtualList,
} from "./components";
import { Inspector } from "./Inspector";
import {
  useRecentRuns,
  type ReviewAction,
  type WorkspaceStatus,
} from "./shell";
const titleOf = (doc: SourceDocument) =>
  doc.blocks.find((b) => /^h[123]$/.test(b.tag))?.text ??
  doc.blocks.find((b) => b.text)?.text ??
  doc.path;
export function Cookbooks({
  onError,
  onBusy,
  onDirty,
  openSignal,
  saveSignal,
  startupPath,
  active,
  reviewAction,
  onStatus,
}: {
  onError: (v: string) => void;
  onBusy: (v: boolean) => void;
  onDirty: (v: boolean) => void;
  openSignal: number;
  saveSignal: number;
  startupPath: string | null;
  active: boolean;
  reviewAction: ReviewAction;
  onStatus: (status: WorkspaceStatus) => void;
}) {
  const recent = useRecentRuns();
  const [book, setBook] = useState<CookbookResult | null>(null);
  const [library, setLibrary] = useState<LibraryBook[]>([]);
  const [librarySearch, setLibrarySearch] = useState("");
  const [onlyCookbooks, setOnlyCookbooks] = useState(true);
  const [showLibrary, setShowLibrary] = useState(false);
  const [libraryGrid, setLibraryGrid] = useStored("v1:library-grid", false);
  const [selected, setSelected] = useState(0);
  const [query, setQuery] = useState("");
  const [statusFilter, setStatusFilter] = useState("All");
  const [missingOnly, setMissingOnly] = useState(false);
  const [decision, setDecision] = useState<Decision>({
    status: "Unreviewed",
    note: "",
  });
  const [saved, setSaved] = useState(false);
  const [loading, setLoading] = useState("");
  const [inspection, setInspection] = useState<IngredientInspection | null>(
    null,
  );
  const [view, setView] = useState<
    | "Review"
    | "Statistics"
    | "References"
    | "Audit"
    | "Comparison"
    | "Evaluation"
  >("Review");
  const [toolData, setToolData] = useState<Json>(null);
  const [images, setImages] = useState<Record<string, string>>({});
  const [compactView, setCompactView] = useStored<
    "Source document" | "Extracted result"
  >("v1:comparison-view", "Extracted result", [
    "Source document",
    "Extracted result",
  ]);
  const [showExtraction, setShowExtraction] = useState(false);
  const [allowNetwork, setAllowNetwork] = useState(false);
  const [budget, setBudget] = useState(10);
  const [refresh, setRefresh] = useState(false);
  const [model, setModel] = useStored("v1:extraction-model", "");
  const [chunkSelection, setChunkSelection] = useState<string[]>([]);
  const [inspectPath, setInspectPath] = useStored("v1:book-path", "");
  const [showPath, setShowPath] = useState(false);
  const [size, setSize] = useState({ width: 1200, height: 800 });
  const work = useRef<HTMLDivElement>(null);
  const operation = useRef(0);
  const running = useRef(false);
  const didStartup = useRef(false);
  const [progress, setProgress] = useState<ExtractionProgress | null>(null);
  const doc = book?.documents[selected];
  const original = book?.review.find((d) => d.document === doc?.path) ?? {
    status: "Unreviewed",
    note: "",
  };
  const dirty =
    !!doc &&
    (decision.status !== original.status || decision.note !== original.note);
  useEffect(() => onDirty(dirty), [dirty, onDirty]);
  useEffect(() => {
    if (!work.current) return;
    const observer = new ResizeObserver((entries) => {
      const rect = entries[0].contentRect;
      setSize({ width: rect.width, height: rect.height });
    });
    observer.observe(work.current);
    return () => observer.disconnect();
  }, []);
  const bookName =
    book?.source
      .split(/[\\/]/)
      .pop()
      ?.replace(/\.epub$/i, "") ?? "Cookbooks";
  const title = doc ? `${titleOf(doc)} · ${bookName}` : bookName;
  const remaining =
    book?.documents.filter(
      (document) =>
        !book.review.some(
          (note) =>
            note.document === document.path && note.status !== "Unreviewed",
        ),
    ).length ?? 0;
  const canReview =
    !!book?.path &&
    !!doc &&
    !loading &&
    view === "Review" &&
    !showLibrary &&
    !showExtraction;
  const statusMessage =
    loading ||
    (dirty
      ? "Unsaved review"
      : saved
        ? "Review saved"
        : book?.path
          ? "Review up to date"
          : book
            ? "Source inspection"
            : "No cookbook open");
  const statusDetail =
    loading && progress
      ? `${progress.completed}/${progress.total} chunks · ${progress.recipes} recipes`
      : book?.path
        ? `${remaining} of ${book.documents.length} unreviewed`
        : "";
  useEffect(
    () =>
      onStatus({
        title,
        canReview,
        message: statusMessage,
        detail: statusDetail,
      }),
    [title, canReview, statusMessage, statusDetail, onStatus],
  );
  const accept = (next: CookbookResult) => {
    if (next.path)
      recent.remember(
        next.path,
        next.source
          .split(/[\\/]/)
          .pop()
          ?.replace(/\.epub$/i, "") || next.path,
      );
    setBook(next);
    setSelected(0);
    const nextDoc = next.documents[0];
    setDecision(
      next.review.find((d) => d.document === nextDoc?.path) ?? {
        status: "Unreviewed",
        note: "",
      },
    );
    setSaved(false);
    setInspection(null);
    setImages({});
    setView("Review");
    setChunkSelection([]);
    setQuery("");
    setModel(next.model);
  };
  const protect = async () => !dirty || (await discardChanges());
  const run = async <T,>(
    label: string,
    fn: () => Promise<T>,
    done: (value: T) => void,
  ) => {
    if (running.current) return;
    running.current = true;
    const id = ++operation.current;
    setLoading(label);
    setProgress(null);
    onBusy(true);
    onError("");
    try {
      const value = await fn();
      if (id === operation.current) done(value);
    } catch (e) {
      if (id === operation.current) onError(String(e));
    } finally {
      if (id === operation.current) {
        running.current = false;
        setLoading("");
        onBusy(false);
      }
    }
  };
  const openBook = async (kind: "epub" | "json" | "directory") => {
    if (running.current || !(await protect())) return;
    const path = await pick(kind);
    if (!path) return;
    if (kind === "directory")
      await run(
        "Scanning library…",
        () => api.library(path),
        (books) => {
          setLibrary(books);
          setShowLibrary(true);
        },
      );
    else
      await run(
        kind === "epub" ? "Inspecting source…" : "Opening saved run…",
        () => (kind === "epub" ? api.book(path) : api.run(path)),
        accept,
      );
  };
  const openRecent = async (path: string) => {
    if (running.current || !(await protect())) return;
    await run(
      "Opening saved run…",
      () => api.run(path),
      (next) => {
        setShowLibrary(false);
        accept(next);
      },
    );
  };
  const openPath = async (path: string) => {
    if (running.current || !(await protect())) return;
    await run("Inspecting source…", () => api.book(path), accept);
  };
  const selectDocument = async (index: number) => {
    if (running.current || index === selected || !(await protect()) || !book)
      return;
    setSelected(index);
    setDecision(
      book.review.find((d) => d.document === book.documents[index].path) ?? {
        status: "Unreviewed",
        note: "",
      },
    );
    setSaved(false);
    setInspection(null);
  };
  const saveReview = async (advance = false) => {
    if (!book?.path || !doc) return;
    const path = doc.path;
    const value = { ...decision };
    await run(
      "Saving review…",
      () => api.review(book.path!, path, value.status, value.note),
      () => {
        const review = [
          ...book.review.filter((d) => d.document !== path),
          { document: path, ...value },
        ];
        setBook({ ...book, review });
        setSaved(true);
        if (advance) {
          // Review queue follows source order, wraps once, and ignores presentation filters.
          const nextIndex = Array.from(
            { length: book.documents.length - 1 },
            (_, offset) => (selected + offset + 1) % book.documents.length,
          ).find(
            (index) =>
              !review.some(
                (note) =>
                  note.document === book.documents[index].path &&
                  note.status !== "Unreviewed",
              ),
          );
          if (nextIndex !== undefined) {
            setSelected(nextIndex);
            setDecision(
              review.find(
                (note) => note.document === book.documents[nextIndex].path,
              ) ?? { status: "Unreviewed", note: "" },
            );
            setInspection(null);
            setSaved(false);
            setQuery("");
            setStatusFilter("All");
            setMissingOnly(false);
          }
        }
      },
    );
  };
  const reviewCommand = (command: string) => {
    if (
      !active ||
      running.current ||
      !book?.path ||
      !doc ||
      view !== "Review" ||
      showLibrary ||
      showExtraction
    )
      return;
    if (command === "review-next") {
      void saveReview(true);
      return;
    }
    const status =
      command === "review-accept"
        ? "Accepted"
        : command === "review-incorrect"
          ? "Incorrect"
          : command === "review-uncertain"
            ? "Uncertain"
            : null;
    if (status) {
      setDecision({ ...decision, status });
      setSaved(false);
    }
  };
  useEffect(() => {
    if (reviewAction.id) reviewCommand(reviewAction.command);
  }, [reviewAction]);
  useEffect(() => {
    if (isNative() || !active) return;
    const keydown = (event: KeyboardEvent) => {
      if (!(event.metaKey || event.ctrlKey) || event.repeat) return;
      const command =
        event.shiftKey && event.key === "Enter"
          ? "review-next"
          : event.altKey
            ? {
                KeyA: "review-accept",
                KeyI: "review-incorrect",
                KeyU: "review-uncertain",
              }[event.code]
            : undefined;
      if (command) {
        event.preventDefault();
        reviewCommand(command);
      }
    };
    window.addEventListener("keydown", keydown);
    return () => window.removeEventListener("keydown", keydown);
  });
  useEffect(() => {
    if (openSignal) void openBook("epub");
  }, [openSignal]);
  useEffect(() => {
    if (saveSignal) void saveReview();
  }, [saveSignal]);
  useEffect(() => {
    if (startupPath && !didStartup.current) {
      didStartup.current = true;
      void run("Opening saved run…", () => api.run(startupPath), accept);
    }
  }, [startupPath]);
  const inspect = async (input: string) =>
    run("Inspecting ingredient…", () => api.inspect(input), setInspection);
  const visible = useMemo(
    () =>
      book?.documents
        .map((d, index) => ({ d, index }))
        .filter(({ d }) => {
          const status =
            book.review.find((r) => r.document === d.path)?.status ??
            "Unreviewed";
          return (
            (statusFilter === "All" || statusFilter === status) &&
            (!missingOnly ||
              book.chunks.some((c) => c.document === d.path && !c.complete)) &&
            (d.path + " " + titleOf(d))
              .toLowerCase()
              .includes(query.toLowerCase())
          );
        }) ?? [],
    [book, statusFilter, missingOnly, query],
  );
  const libraryRows = library.filter(
    (b) =>
      (!onlyCookbooks || b.cookbook) &&
      (b.title + " " + b.authors.join(" "))
        .toLowerCase()
        .includes(librarySearch.toLowerCase()),
  );
  const recipes =
    book?.recipes.filter((r) => r.sourceDocument === doc?.path) ?? [];
  const tool = async (which: typeof view) => {
    if (!book?.path) return;
    setView(which);
    setToolData(null);
    setInspection(null);
    if (which === "Statistics")
      await run(
        "Loading statistics…",
        () => api.stats(book.path!),
        setToolData,
      );
    if (which === "Audit")
      await run("Auditing run…", () => api.audit(book.path!), setToolData);
    if (which === "Evaluation") {
      const path = await pick("json");
      if (path)
        await run(
          "Comparing expectations…",
          () => api.evaluate(book.path!, path),
          setToolData,
        );
      else setView("Review");
    }
    if (which === "Comparison") {
      const before = await pick("json");
      if (before)
        await run(
          "Comparing runs…",
          () => api.diff(before, book.path!),
          setToolData,
        );
      else setView("Review");
    }
  };
  const extract = async () => {
    if (!book || !(await protect())) return;
    const out = book.path ?? (await savePath("cookbook-run.json"));
    if (!out) return;
    setShowExtraction(false);
    setProgress(null);
    await run(
      allowNetwork ? "Extracting cookbook…" : "Reading cached extraction…",
      () =>
        api.extract(
          {
            book: book.source,
            out,
            model: model || book.model,
            resume: !!book.path,
            from: null,
            allowNetwork,
            refresh,
            cacheDir: null,
            chunks: chunkSelection,
            budgetUsd: budget,
          },
          setProgress,
        ),
      accept,
    );
  };
  const navigation = (
    <section className="document-navigation">
      <div className="pane-header">
        <h2>Documents</h2>
        <Menu label="Filter">
          <label>
            <input
              type="checkbox"
              checked={missingOnly}
              onChange={(e) => setMissingOnly(e.target.checked)}
            />
            Missing extraction
          </label>
          <label>
            Review status
            <select
              value={statusFilter}
              onChange={(e) => setStatusFilter(e.target.value)}
            >
              {["All", "Unreviewed", "Accepted", "Incorrect", "Uncertain"].map(
                (s) => (
                  <option key={s}>{s}</option>
                ),
              )}
            </select>
          </label>
        </Menu>
      </div>
      <div className="search">
        <Search size={14} />
        <input
          aria-label="Find document"
          placeholder="Find document…"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
        />
      </div>
      <VirtualList
        rows={visible}
        selected={visible.findIndex((v) => v.index === selected)}
        onSelect={(i) => void selectDocument(visible[i].index)}
        height={54}
        label="Source documents"
        render={({ d }) => (
          <div className="document-row">
            <strong title={titleOf(d)}>{titleOf(d)}</strong>
            <span>
              {book?.chunks.some((c) => c.document === d.path && !c.complete)
                ? "Needs extraction"
                : (book?.review.find((r) => r.document === d.path)?.status ??
                  "Unreviewed")}
            </span>
          </div>
        )}
      />
    </section>
  );
  const source = doc && (
    <section className="source-evidence scroll">
      <h2 className="comparison-heading">Source document</h2>
      {doc.blocks.map((block) =>
        /^h[123]$/.test(block.tag) ? (
          <h3 key={block.id} title={block.id}>
            {block.text}
          </h3>
        ) : (
          <p key={block.id} title={block.id}>
            {block.text}
          </p>
        ),
      )}
      {doc.images.map((path) => (
        <figure key={path}>
          {images[path] ? (
            <img src={images[path]} alt={`Source image ${path}`} />
          ) : (
            <div className="image-placeholder">
              <FileText size={20} />
              Source image not loaded
            </div>
          )}
          <figcaption>{path}</figcaption>
        </figure>
      ))}
      {!doc.blocks.length && !doc.images.length && (
        <p>No source blocks or images in this document.</p>
      )}
      <details>
        <summary>Extraction chunks</summary>
        {book?.chunks
          .filter((c) => c.document === doc.path)
          .map((c) => (
            <div key={c.id} className="chunk">
              <label>
                <input
                  type="checkbox"
                  checked={chunkSelection.includes(c.id)}
                  onChange={(e) =>
                    setChunkSelection((prev) =>
                      e.target.checked
                        ? [...prev, c.id]
                        : prev.filter((id) => id !== c.id),
                    )
                  }
                />
                <span className="mono">{c.id}</span>
              </label>
              <span>
                {c.complete ? "Complete" : "Missing"}
                {c.cached ? " · Cached" : ""}
              </span>
              {c.error && <p className="error-text">{c.error}</p>}
            </div>
          ))}
      </details>
    </section>
  );
  const result = (
    <section className="extracted-result scroll">
      <h2 className="comparison-heading">Extracted result</h2>
      {recipes.map((recipe) => (
        <Recipe
          key={`${book?.sourceHash}:${recipe.index}`}
          recipe={recipe}
          onInspect={(input) => void inspect(input)}
          images={images}
          path={book?.path ?? null}
          onError={onError}
          hideTitle={size.width < 850 && recipe.title === titleOf(doc!)}
        />
      ))}
      {!recipes.length && (
        <div className="empty-inline">
          <h3>No extracted recipe for this document</h3>
          <p>
            Review the source before marking it accepted. Use Extraction to read
            cached results or explicitly allow network requests.
          </p>
        </div>
      )}
    </section>
  );
  const reviewContent = doc ? (
    <div className="review-content">
      <header className="document-heading">
        <h2>{titleOf(doc)}</h2>
        <p className="caption" title={doc.path}>
          {doc.path}
        </p>
      </header>
      <div className="review-controls">
        <label>
          Review status
          <select
            value={decision.status}
            disabled={!!loading}
            onChange={(e) => {
              setDecision({ ...decision, status: e.target.value });
              setSaved(false);
            }}
          >
            {["Unreviewed", "Accepted", "Incorrect", "Uncertain"].map((s) => (
              <option key={s}>{s}</option>
            ))}
          </select>
        </label>
        <button
          className="primary"
          disabled={!book?.path || !!loading || !dirty}
          onClick={() => void saveReview()}
        >
          Save review
        </button>
        <Menu label="Review actions">
          <button
            disabled={!book?.path || !!loading}
            onClick={() => reviewCommand("review-accept")}
          >
            Mark accepted <kbd>⌘⌥A</kbd>
          </button>
          <button
            disabled={!book?.path || !!loading}
            onClick={() => reviewCommand("review-incorrect")}
          >
            Mark incorrect <kbd>⌘⌥I</kbd>
          </button>
          <button
            disabled={!book?.path || !!loading}
            onClick={() => reviewCommand("review-uncertain")}
          >
            Mark uncertain <kbd>⌘⌥U</kbd>
          </button>
          <hr />
          <button
            disabled={!book?.path || !!loading}
            onClick={() => void saveReview(true)}
          >
            Save and next unreviewed <kbd>⌘⇧↵</kbd>
          </button>
        </Menu>
        <Menu label={decision.note ? "Review note •" : "Review note"}>
          <label>
            Review note
            <textarea
              aria-label="Review note"
              rows={4}
              value={decision.note}
              onChange={(e) => {
                setDecision({ ...decision, note: e.target.value });
                setSaved(false);
              }}
              placeholder="Evidence, uncertainty, or corrections…"
            />
          </label>
        </Menu>
        {saved && !dirty && (
          <span role="status" className="success">
            <Check size={14} />
            Saved
          </span>
        )}
        {dirty && <span className="caption">Unsaved</span>}
        {!book?.path && (
          <span className="caption">
            Extract to a saved run to save decisions
          </span>
        )}
      </div>
      <div className="comparison-tabs">
        <Tabs
          value={compactView}
          items={["Source document", "Extracted result"]}
          onChange={setCompactView}
          label="Document comparison"
        />
      </div>
      <div className="comparison-wide">
        <Split
          id="comparison"
          initial={45}
          min={30}
          max={65}
          left={source}
          right={result}
        />
      </div>
      <div className="comparison-compact">
        {compactView === "Source document" ? source : result}
      </div>
    </div>
  ) : (
    <div className="empty-inline">No source document selected.</div>
  );
  const focused = inspection && (size.width < 760 || size.height < 480);
  return (
    <div className="workspace cookbooks" ref={work}>
      <header className="workspace-heading">
        <h1>Cookbooks</h1>
        <Menu label="Open">
          <button disabled={!!loading} onClick={() => void openBook("epub")}>
            Cookbook EPUB…
          </button>
          <button disabled={!!loading} onClick={() => void openBook("json")}>
            Saved run…
          </button>
          <button
            disabled={!!loading}
            onClick={() => void openBook("directory")}
          >
            Library folder…
          </button>
          <hr />
          <button onClick={() => setShowPath(!showPath)}>Inspect path…</button>
          {recent.runs.length > 0 && (
            <>
              <hr />
              <p className="menu-section">Recent runs</p>
              {recent.runs.map((item) => (
                <button
                  key={item.path}
                  className="recent-run"
                  title={item.path}
                  disabled={!!loading}
                  onClick={() => void openRecent(item.path)}
                >
                  <strong>{item.name}</strong>
                  <span>{item.path.split(/[\\/]/).pop()}</span>
                </button>
              ))}
              <button onClick={recent.clear}>Clear recent runs</button>
            </>
          )}
        </Menu>
        {library.length > 0 && (
          <button
            aria-pressed={showLibrary}
            onClick={() => setShowLibrary(!showLibrary)}
          >
            Library
          </button>
        )}
      </header>
      {showPath && (
        <form
          className="source-bar"
          onSubmit={(e) => {
            e.preventDefault();
            void openPath(inspectPath);
          }}
        >
          <label htmlFor="epub-path">EPUB path</label>
          <input
            id="epub-path"
            value={inspectPath}
            onChange={(e) => setInspectPath(e.target.value)}
          />
          <button disabled={!!loading || !inspectPath.trim()}>
            Inspect source
          </button>
        </form>
      )}
      {loading && (
        <div role="status" className="progress">
          <span className="spinner" />
          {loading}
          {progress && (
            <span>
              {progress.completed}/{progress.total} chunks · {progress.recipes}{" "}
              recipes
            </span>
          )}
        </div>
      )}
      {book && (
        <header className="run-heading">
          <div>
            <h2
              title={`${book.recipes.length} recipes · ${book.documents.length} documents`}
            >
              {book.source
                .split(/[\\/]/)
                .pop()
                ?.replace(/\.epub$/i, "")}
            </h2>
            <p className="caption run-counts">
              {book.recipes.length} recipes · {book.documents.length} documents
              {book.incomplete && " · Incomplete extraction"}
            </p>
          </div>
          <div className="actions">
            <button
              disabled={!!loading}
              onClick={() => setShowExtraction(true)}
            >
              Extraction…
            </button>
            <Menu label="Run tools">
              <button
                onClick={() =>
                  void revealFile(book.source).catch((e) => onError(String(e)))
                }
              >
                Reveal EPUB in Finder
              </button>
              <button
                disabled={!book.path}
                onClick={() =>
                  void revealFile(book.path!).catch((e) => onError(String(e)))
                }
              >
                Reveal saved run in Finder
              </button>
              <button
                disabled={!book.path}
                onClick={() =>
                  void revealFile(book.path!, true).catch((e) =>
                    onError(String(e)),
                  )
                }
              >
                Reveal review file in Finder
              </button>
              <hr />
              <button
                onClick={() =>
                  void copy(JSON.stringify(book.run, null, 2)).catch((e) =>
                    onError(String(e)),
                  )
                }
              >
                Copy run JSON
              </button>
              <button
                onClick={() => void tool("Statistics")}
                disabled={!book.path}
              >
                Ingredient statistics
              </button>
              <button onClick={() => setView("References")}>
                Reference graph
              </button>
              <button disabled={!book.path} onClick={() => void tool("Audit")}>
                Audit completeness
              </button>
              <button
                disabled={!book.path}
                onClick={() => void tool("Evaluation")}
              >
                Load expectations…
              </button>
              <button
                disabled={!book.path}
                onClick={() => void tool("Comparison")}
              >
                Compare with run…
              </button>
              <hr />
              <button
                disabled={!book.path || !!loading}
                onClick={async () => {
                  if (running.current || !(await protect())) return;
                  const out = await savePath("replayed-run.json");
                  if (out)
                    await run(
                      "Replaying saved outputs…",
                      () => api.replay(book.path!, out),
                      accept,
                    );
                }}
              >
                Replay to new run…
              </button>
              <button
                onClick={async () => {
                  const path = await pick("epub");
                  if (path)
                    await run(
                      "Loading source images…",
                      () => api.images(book.path!, path),
                      (value) =>
                        setImages(
                          Object.fromEntries(
                            value.map((image) => [image.path, image.dataUrl]),
                          ),
                        ),
                    );
                }}
              >
                Load source images…
              </button>
              <p className="caption">
                ${book.reservedUsd.toFixed(4)} reserved/spent
              </p>
            </Menu>
          </div>
        </header>
      )}
      {showLibrary && (
        <section className="library">
          <div className="pane-header">
            <h2>Library</h2>
            <div className="actions">
              <button
                aria-pressed={libraryGrid}
                onClick={() => setLibraryGrid(!libraryGrid)}
              >
                {libraryGrid ? "List view" : "Grid view"}
              </button>
              <button
                className="icon-button"
                aria-label="Close library"
                onClick={() => setShowLibrary(false)}
              >
                <X size={16} />
              </button>
            </div>
          </div>
          <div className="filter-bar">
            <input
              aria-label="Find book"
              placeholder="Find book or author…"
              value={librarySearch}
              onChange={(e) => setLibrarySearch(e.target.value)}
            />
            <label>
              <input
                type="checkbox"
                checked={onlyCookbooks}
                onChange={(e) => setOnlyCookbooks(e.target.checked)}
              />
              Cookbooks only
            </label>
          </div>
          <div className={`library-books ${libraryGrid ? "grid" : ""}`}>
            {libraryRows.map((b) => (
              <button
                key={b.path}
                onClick={() => {
                  setShowLibrary(false);
                  void openPath(b.path);
                }}
              >
                <BookCover path={b.path} />
                <span>
                  <strong>{b.title}</strong>
                  <span className="caption">{b.authors.join(", ")}</span>
                  {b.error && <span className="error-text">{b.error}</span>}
                </span>
              </button>
            ))}
            {!libraryRows.length && <p>No matching books.</p>}
          </div>
        </section>
      )}
      {!book && !showLibrary && !loading && (
        <div className="empty">
          <BookOpen size={34} />
          <h2>Source and result, side by side</h2>
          <p>
            Open a cookbook to inspect its source, or continue reviewing a saved
            run. Extraction starts only when you request it.
          </p>
          <div className="actions">
            <button className="primary" onClick={() => void openBook("epub")}>
              <FolderOpen size={15} />
              Open cookbook
            </button>
            <button onClick={() => void openBook("json")}>
              Open saved run
            </button>
          </div>
        </div>
      )}
      {book &&
        !showLibrary &&
        (view !== "Review" ? (
          <section className="tool-view">
            <button className="back" onClick={() => setView("Review")}>
              <ArrowLeft size={15} />
              Back to source review
            </button>
            <h2>{view}</h2>
            <div className="scroll">
              {inspection && (
                <div className="tool-inspector">
                  <Inspector
                    data={inspection}
                    onClose={() => setInspection(null)}
                  />
                </div>
              )}
              {view === "References" ? (
                <References
                  recipes={book.recipes}
                  onOpen={(r) => {
                    const i = book.documents.findIndex(
                      (d) => d.path === r.sourceDocument,
                    );
                    if (i >= 0) {
                      void selectDocument(i);
                      setView("Review");
                    }
                  }}
                />
              ) : toolData !== null ? (
                view === "Statistics" ? (
                  <Statistics
                    value={toolData}
                    onInspect={(input) => void inspect(input)}
                  />
                ) : (
                  <Fields value={toolData} />
                )
              ) : (
                <p>{loading || "No report loaded."}</p>
              )}
            </div>
          </section>
        ) : focused ? (
          <section className="focused-inspector">
            <div className="focus-context">
              <button onClick={() => setInspection(null)}>
                <ArrowLeft size={15} />
                Back to review
              </button>
              <strong>{doc && titleOf(doc)}</strong>
            </div>
            <Inspector data={inspection} onClose={() => setInspection(null)} />
          </section>
        ) : (
          <div className="review-body">
            {size.width < 760 ? (
              <div className="compact-documents">
                <Menu label="Documents">{navigation}</Menu>
              </div>
            ) : null}
            <div className="review-main">
              {size.width >= 760 ? (
                <Split
                  id="document-navigation"
                  initial={22}
                  min={16}
                  max={30}
                  left={navigation}
                  right={reviewContent}
                />
              ) : (
                reviewContent
              )}
            </div>
            {inspection && (
              <aside className="cookbook-inspector">
                <Inspector
                  data={inspection}
                  onClose={() => setInspection(null)}
                />
              </aside>
            )}
          </div>
        ))}
      {showExtraction && (
        <Modal
          label="extraction-title"
          onClose={() => setShowExtraction(false)}
        >
          <header className="pane-header">
            <h2 id="extraction-title">Extract cookbook</h2>
            <button
              className="icon-button"
              aria-label="Close extraction settings"
              onClick={() => setShowExtraction(false)}
            >
              <X size={18} />
            </button>
          </header>
          <p>
            Cached outputs are used by default. Network access must be
            explicitly enabled for this extraction.
          </p>
          <label>
            Model
            <input
              value={model}
              onChange={(e) => setModel(e.target.value)}
              placeholder={book?.model}
            />
          </label>
          <label className="check-label">
            <input
              type="checkbox"
              checked={allowNetwork}
              onChange={(e) => setAllowNetwork(e.target.checked)}
            />
            Allow network requests
          </label>
          {allowNetwork && (
            <label>
              Total budget (USD)
              <input
                type="number"
                min="0"
                max="1000"
                step="0.25"
                value={budget}
                onChange={(e) => setBudget(Number(e.target.value))}
              />
            </label>
          )}
          <label className="check-label">
            <input
              type="checkbox"
              checked={refresh}
              onChange={(e) => setRefresh(e.target.checked)}
            />
            Refresh cached chunks
          </label>
          <p className="caption">
            {chunkSelection.length
              ? `${chunkSelection.length} selected chunks`
              : "All eligible chunks"}{" "}
            · {allowNetwork ? "Network enabled" : "Cache only"}
          </p>
          <footer>
            <button onClick={() => setShowExtraction(false)}>Cancel</button>
            <button
              className="primary"
              disabled={!!loading || !Number.isFinite(budget) || budget < 0}
              onClick={() => void extract()}
            >
              {book?.path ? "Resume extraction" : "Extract to saved run…"}
            </button>
          </footer>
        </Modal>
      )}
    </div>
  );
}
function Recipe({
  recipe,
  onInspect,
  images,
  hideTitle,
  path,
  onError,
}: {
  recipe: CookbookRecipe;
  onInspect: (input: string) => void;
  images: Record<string, string>;
  hideTitle: boolean;
  path: string | null;
  onError: (v: string) => void;
}) {
  const [scale, setScale] = useState(1);
  const [scaled, setScaled] = useState(recipe);
  const [scaling, setScaling] = useState(false);
  const generation = useRef(0);
  const updateScale = async (factor: number) => {
    if (!path) return;
    const id = ++generation.current;
    setScaling(true);
    try {
      const next = await api.scale(path, recipe.index, factor);
      if (generation.current === id) {
        setScaled(next);
        setScale(factor);
      }
    } catch (e) {
      onError(String(e));
    } finally {
      if (generation.current === id) setScaling(false);
    }
  };
  return (
    <article className="recipe">
      {!hideTitle && <h3>{recipe.title}</h3>}
      {recipe.description && <p>{recipe.description}</p>}
      {recipe.recipeYield && (
        <p className="recipe-yield">
          {scale !== 1 ? "Original yield: " : ""}
          {recipe.recipeYield}
        </p>
      )}
      {recipe.image && images[recipe.image] && (
        <img
          className="recipe-image"
          src={images[recipe.image]}
          alt={recipe.title}
        />
      )}
      <RecipeScale
        className="scale"
        disabled={!path || scaling}
        value={scale}
        onChange={(value) => void updateScale(value)}
      />
      {scaled.sections.map((section, i) => (
        <section key={i}>
          {section.name && <h4>{section.name}</h4>}
          {section.ingredients.length > 0 && <h4>Ingredients</h4>}
          <ul className="ingredient-lines">
            {section.ingredients.map((input, n) => (
              <li key={n}>
                <button onClick={() => onInspect(input)}>{input}</button>
              </li>
            ))}
          </ul>
          {section.instructions.length > 0 && (
            <>
              <h4>Instructions</h4>
              <ol className="instructions">
                {section.instructions.map((line, n) => (
                  <li key={n}>{line}</li>
                ))}
              </ol>
            </>
          )}
        </section>
      ))}
      {scaling && (
        <p role="status" className="caption">
          Scaling recipe…
        </p>
      )}
      {recipe.references.length > 0 && (
        <details>
          <summary>Recipe references</summary>
          {recipe.references.map((r, i) => (
            <p key={i}>{r}</p>
          ))}
        </details>
      )}
    </article>
  );
}
function References({
  recipes,
  onOpen,
}: {
  recipes: CookbookRecipe[];
  onOpen: (r: CookbookRecipe) => void;
}) {
  const [zoom, setZoom] = useState(1);
  const [selected, setSelected] = useState<number | null>(null);
  const linked = recipes.filter((r) => r.references.length);
  if (!recipes.length) return <p>No recipes in this run.</p>;
  const names = [
    ...new Set(recipes.flatMap((r) => [r.title, ...r.references])),
  ];
  const columns = Math.max(2, Math.ceil(Math.sqrt(names.length)));
  const graphWidth = columns * 240,
    graphHeight = Math.ceil(names.length / columns) * 110 + 40;
  const position = (name: string) => {
    const i = names.indexOf(name);
    return {
      x: (i % columns) * 240 + 20,
      y: Math.floor(i / columns) * 110 + 25,
    };
  };
  return (
    <>
      {!linked.length && (
        <p className="caption">
          No cross-recipe references in this run. Select a recipe to review its
          source.
        </p>
      )}
      <div className="filter-bar">
        <label>
          Zoom
          <select
            value={zoom}
            onChange={(e) => setZoom(Number(e.target.value))}
          >
            {[0.5, 0.75, 1, 1.25, 1.5].map((z) => (
              <option value={z} key={z}>
                {z * 100}%
              </option>
            ))}
          </select>
        </label>
        <span className="caption">
          {names.length} recipes ·{" "}
          {linked.reduce((n, r) => n + r.references.length, 0)} references
        </span>
      </div>
      <div className="scroll">
        <svg
          className="reference-graph"
          aria-label="Recipe reference graph"
          viewBox={`0 0 ${graphWidth} ${graphHeight}`}
          style={{ width: graphWidth * zoom, height: graphHeight * zoom }}
        >
          <defs>
            <marker
              id="reference-arrow"
              viewBox="0 0 10 10"
              refX="9"
              refY="5"
              markerWidth="6"
              markerHeight="6"
              orient="auto-start-reverse"
            >
              <path d="M 0 0 L 10 5 L 0 10 z" fill="currentColor" />
            </marker>
          </defs>
          {linked.flatMap((r) =>
            r.references.map((name, i) => {
              const a = position(r.title),
                b = position(name);
              return (
                <path
                  key={`${r.index}-${i}`}
                  d={`M${a.x + 100},${a.y + 34} C${a.x + 100},${a.y + 80} ${b.x + 100},${b.y - 40} ${b.x + 100},${b.y}`}
                  markerEnd="url(#reference-arrow)"
                />
              );
            }),
          )}
          {names.map((name) => {
            const p = position(name),
              recipe = recipes.find((r) => r.title === name);
            return (
              <g
                key={name}
                role="button"
                aria-label={
                  recipe ? `Open ${name}` : `Unresolved reference: ${name}`
                }
                tabIndex={0}
                onFocus={() => setSelected(recipe?.index ?? null)}
                onClick={() => {
                  if (recipe) {
                    setSelected(recipe.index);
                    onOpen(recipe);
                  }
                }}
                onKeyDown={(e) => {
                  if (recipe && (e.key === "Enter" || e.key === " ")) {
                    e.preventDefault();
                    onOpen(recipe);
                  }
                }}
              >
                <title>{name}</title>
                <rect
                  x={p.x}
                  y={p.y}
                  width="200"
                  height="34"
                  rx="4"
                  style={
                    recipe?.index === selected
                      ? { stroke: "var(--blue)" }
                      : undefined
                  }
                />
                <text x={p.x + 10} y={p.y + 22}>
                  {name.length > 25 ? name.slice(0, 24) + "…" : name}
                </text>
              </g>
            );
          })}
        </svg>
      </div>
    </>
  );
}

function BookCover({ path }: { path: string }) {
  const [cover, setCover] = useState<string | null>(null);
  useEffect(() => {
    let active = true;
    api
      .cover(path)
      .then((image) => {
        if (active) setCover(image?.dataUrl ?? null);
      })
      .catch(() => {});
    return () => {
      active = false;
    };
  }, [path]);
  return cover ? (
    <img className="library-cover" src={cover} alt="" />
  ) : (
    <BookOpen size={22} />
  );
}

function Statistics({
  value,
  onInspect,
}: {
  value: Json;
  onInspect: (input: string) => void;
}) {
  const stats = value as {
    total_occurrences?: number;
    total_recipes?: number;
    unique_names?: number;
    names?: {
      name: string;
      occurrences: number;
      recipes: number;
      distinct_inputs: number;
      examples: { input: string; recipe: string }[];
    }[];
  };
  const [query, setQuery] = useState("");
  const [selected, setSelected] = useState(0);
  const [sort, setSort] = useState("Occurrences");
  const names = (stats.names ?? [])
    .filter((n) => n.name.toLowerCase().includes(query.toLowerCase()))
    .sort((a, b) =>
      sort === "Name"
        ? a.name.localeCompare(b.name)
        : b.occurrences - a.occurrences,
    );
  return (
    <section className="statistics">
      <p className="caption">
        {stats.total_occurrences ?? 0} occurrences · {stats.total_recipes ?? 0}{" "}
        recipes · {stats.unique_names ?? 0} unique names
      </p>
      <div className="filter-bar">
        <input
          aria-label="Find ingredient name"
          placeholder="Find ingredient…"
          value={query}
          onChange={(e) => {
            setQuery(e.target.value);
            setSelected(0);
          }}
        />
        <label>
          Sort
          <select value={sort} onChange={(e) => setSort(e.target.value)}>
            <option>Occurrences</option>
            <option>Name</option>
          </select>
        </label>
      </div>
      <Split
        id="statistics"
        left={
          <VirtualList
            rows={names}
            selected={selected}
            onSelect={setSelected}
            label="Ingredient statistics"
            render={(n) => (
              <>
                <strong>{n.name}</strong>
                <span>
                  {n.occurrences} occurrences · {n.recipes} recipes
                </span>
              </>
            )}
          />
        }
        right={
          <section className="inspector">
            <h2>{names[selected]?.name ?? "Select an ingredient"}</h2>
            <div className="scroll">
              {names[selected]?.examples.map((example, i) => (
                <div key={i} className="stat-example">
                  <p className="caption">{example.recipe}</p>
                  <button onClick={() => onInspect(example.input)}>
                    {example.input}
                  </button>
                </div>
              ))}
            </div>
          </section>
        }
      />
    </section>
  );
}
