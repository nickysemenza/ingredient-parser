import { ModelResults } from "./ModelResults";
import type { CostEstimate, ModelChoice, ExtractionPreview, SavedRun } from "./generated";
import { RecipeScale } from "@ingredient-parser/recipe-ui";
import { useEffect, useMemo, useRef, useState } from "react";
import {
  ArrowLeft,
  BookOpen,
  FileText,
  FolderOpen,
  Search,
  X,
} from "lucide-react";
import {
  api,
  revealFile,
  copy,
  discardChanges,
  pick,
  savePath,
  type CookbookRecipe,
  type CookbookResult,
  type AuditCorrection,
  type ExtractionProgress,
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
import { schedulePreview } from "./preview";
const titleOf = (doc: SourceDocument) =>
  doc.blocks.find((b) => /^h[123]$/.test(b.tag))?.text ??
  doc.blocks.find((b) => b.text)?.text ??
  doc.path;
const correctionText = (correction: AuditCorrection) => {
  const spans = "spans" in correction ? correction.spans :
    "span" in correction ? [correction.span] :
    "at" in correction ? [correction.at] :
    "from" in correction ? correction.from.spans : [];
  const fallbackChunk =
    correction.kind === "move_assignment" || correction.kind === "move_spans"
      ? correction.from.owner_chunk
      : correction.kind === "restore_span" || correction.kind === "replace_bounded_text"
        ? correction.assignment.owner_chunk
        : correction.kind === "merge_sections"
          ? correction.owner_chunk
          : undefined;
  const source = spans.length
    ? `source chunk ${spans.map((span) => `${span.chunk + 1}:${span.start + 1}-${span.end + 1}`).join(", ")}`
    : typeof fallbackChunk === "number"
      ? `source chunk ${fallbackChunk + 1}`
      : "source chunk unresolved (legacy correction)";
  if (correction.kind === "replace_bounded_text")
    return `${correction.reason} · ${correction.before} → ${correction.after} · ${source}`;
  return `${correction.reason} · ${correction.kind.replaceAll("_", " ")} · ${source}`;
};
export const auditStatus = (
  audits: CookbookResult["hybridAudits"],
  status: string,
  incomplete: boolean,
) =>
  audits.length === 0
    ? "not assessed"
    : status === "complete" && !incomplete
      ? "accepted"
      : "findings";
export function Cookbooks({
  onError,
  onBusy,
  onDirty,
  openSignal,
  saveSignal: _saveSignal,
  startupPath,
  active,
  reviewAction: _reviewAction,
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
  const money = (value: number) =>
    value < 0.005 ? "<$0.01" : `$${value.toFixed(2)}`;
  const range = (usd: CostEstimate["usd"]) =>
    usd ? `${money(usd[0])}–${money(usd[1])}` : "unknown";
  const recent = useRecentRuns();
  const [book, setBook] = useState<CookbookResult | null>(null);
  const [library, setLibrary] = useState<LibraryBook[]>([]);
  const [libraryDirectory, setLibraryDirectory] = useState("");
  const [librarySearch, setLibrarySearch] = useState("");
  const [onlyCookbooks, setOnlyCookbooks] = useState(true);
  const [showLibrary, setShowLibrary] = useState(false);
  const [libraryGrid, setLibraryGrid] = useStored("v1:library-grid", false);
  const [selected, setSelected] = useState(0);
  const [query, setQuery] = useState("");

  const [missingOnly, setMissingOnly] = useState(false);
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
  const [modelChoices, setModelChoices] = useState<ModelChoice[]>([]);
  const [preview, setPreview] = useState<ExtractionPreview | null>(null);
  const [previewError, setPreviewError] = useState("");
  const [resumeRun, setResumeRun] = useState(false);
  const [stopping, setStopping] = useState(false);
  const [comparePaths, setComparePaths] = useState<string[]>([]);
  const [savedRuns, setSavedRuns] = useState<SavedRun[]>([]);
  const [showResults, setShowResults] = useState(false);
  const [showHistory, setShowHistory] = useState(false);
  const [showExtraction, setShowExtraction] = useState(false);
  const [allowNetwork, setAllowNetwork] = useState(true);
  const [budget, setBudget] = useState(10);
  const [concurrency, setConcurrency] = useState(4);
  const [refresh, setRefresh] = useState(false);
  const [model, setModel] = useStored("v2:extraction-model", "automatic");
  const [strategy, setStrategy] = useStored<"indexed" | "hybrid">(
    "v1:extraction-strategy",
    "indexed",
    ["indexed", "hybrid"],
  );
  const [chunkSelection, setChunkSelection] = useState<string[]>([]);
  const [inspectPath, setInspectPath] = useStored("v1:book-path", "");
  const [showPath, setShowPath] = useState(false);
  const [size, setSize] = useState({ width: 1200, height: 800 });
  const work = useRef<HTMLDivElement>(null);
  const operation = useRef(0);
  const previewGeneration = useRef(0);
  const running = useRef(false);
  const didStartup = useRef(false);
  const [progress, setProgress] = useState<ExtractionProgress | null>(null);
  const doc = book?.documents[selected];
  const audits = book?.hybridAudits ?? [];
  const appliedCorrections = book?.appliedCorrections ?? [];
  const contextExpansions = book?.auditContextExpansions ?? [];
  const dirty = false;
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
  const canReview = false;
  const statusMessage = (loading && progress?.phase) || loading || (book?.path ? book.status : book ? "Source inspection" : "No cookbook open");
  const statusDetail = loading && progress ? `${progress.completed}/${progress.total} chunks · ${progress.recipes} recipes` : "";
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
    setShowLibrary(false);
    setShowHistory(false);
    setShowResults(false);
    setResumeRun(false);
    setStopping(false);
    setRefresh(false);
    setSelected(0);

    setInspection(null);
    setImages({});
    setView("Review");
    setChunkSelection([]);
    setQuery("");
    setModel("automatic");
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
  const openBook = async (kind: "epub" | "directory") => {
    if (running.current || !(await protect())) return;
    const path = await pick(kind);
    if (!path) return;
    if (kind === "directory")
      await run(
        "Scanning library…",
        () => api.library(path),
        (books) => {
          setLibraryDirectory(path);
          setLibrary(books);
          setShowLibrary(true);
        },
      );
    else await run("Inspecting source…", () => api.book(path), accept);
  };
  const browseLibrary = async () => {
    if (running.current || !(await protect())) return;
    setShowHistory(false);
    setShowResults(false);
    await run(
      "Loading cookbook library…",
      () => api.library(libraryDirectory),
      (books) => {
        setLibrary(books);
        setShowLibrary(true);
      },
    );
  };
  const openRecent = async (path: string) => {
    if (running.current || !(await protect())) return;
    await run(
      "Opening extraction…",
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

    setInspection(null);
  };
  useEffect(() => {
    if (openSignal) void openBook("epub");
  }, [openSignal]);
  useEffect(() => {
    if (!active || didStartup.current) return;
    didStartup.current = true;
    if (startupPath)
      void run("Opening extraction…", () => api.run(startupPath), accept);
    else void browseLibrary();
  }, [startupPath, active]);
  const inspect = async (input: string) =>
    run("Inspecting ingredient…", () => api.inspect(input), setInspection);
  const visible = useMemo(
    () =>
      book?.documents
        .map((d, index) => ({ d, index }))
        .filter(({ d }) => {
          return (
            (!missingOnly ||
              book.chunks.some((c) => c.document === d.path && !c.complete)) &&
            (d.path + " " + titleOf(d))
              .toLowerCase()
              .includes(query.toLowerCase())
          );
        }) ?? [],
    [book, missingOnly, query],
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
      setComparePaths([book.path]);
      setShowHistory(true);
      setSavedRuns(await api.runs(null));
    }
  };
  useEffect(() => {
    if (!showExtraction) return;
    let active = true;
    void api
      .models()
      .then((rows) => {
        if (active) {
          setModelChoices(rows);
          if (
            !rows.some(
              (row) => row.enabled && row.id === (model || book?.model),
            )
          ) {
            const first = rows.find((row) => row.enabled);
            if (first) setModel(first.id);
          }
        }
      })
      .catch((e) => {
        if (active) setPreviewError(String(e));
      });
    return () => {
      active = false;
    };
  }, [showExtraction]);
  useEffect(() => {
    if (!showExtraction || !book) return;
    let active = true;
    const generation = ++previewGeneration.current;
    setPreview(null);
    setPreviewError("");
    const dispose = schedulePreview(
      () =>
        api.preview({
          book: book.source,
          out: resumeRun ? (book.path ?? "") : "",
          model: model || book.model,
          resume: resumeRun,
          from: refresh ? book.path : null,
          allowNetwork,
          refresh,
          cacheDir: null,
          chunks: chunkSelection,
          budgetUsd: budget,
          strategy,
          concurrency,
        }),
      (value) => {
        if (active && generation === previewGeneration.current) setPreview(value);
      },
      (e) => {
        if (active && generation === previewGeneration.current)
          setPreviewError(String(e));
      },
    );
    return () => {
      active = false;
      dispose();
    };
  }, [
    showExtraction,
    book,
    model,
    resumeRun,
    allowNetwork,
    refresh,
    chunkSelection,
    budget,
    strategy,
    concurrency,
  ]);
  useEffect(() => {
    if (!showHistory) return;
    let active = true;
    void api
      .runs(null)
      .then((rows) => {
        if (active) setSavedRuns(rows);
      })
      .catch((e) => {
        if (active) setPreviewError(String(e));
      });
    return () => {
      active = false;
    };
  }, [showHistory, book]);
  const extract = async () => {
    if (!book || !(await protect())) return;
    const out = resumeRun ? (book.path ?? "") : "";
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
            resume: resumeRun,
            from: refresh ? book.path : null,
            allowNetwork,
            refresh,
            cacheDir: null,
            chunks: chunkSelection,
            budgetUsd: budget,
            strategy,
            concurrency,
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
                : "Processed"}
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
            Use Extraction to read
            cached results or start a new extraction.
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
              <p className="menu-section">Recent extractions</p>
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
              <button onClick={recent.clear}>Clear recent extractions</button>
            </>
          )}
        </Menu>
        <button
          disabled={!!loading}
          aria-pressed={showLibrary}
          onClick={() => void browseLibrary()}
        >
          Library
        </button>
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
          {stopping || progress?.stopping ? "Stopping; saving active requests…" : (progress?.phase || loading)}
          {progress && (
            <span>
              {progress.completed}/{progress.total} chunks · {progress.recipes}{" "}
              recipes · {progress.activeModels?.join(", ")} · {progress.active} active · {progress.failed} failed attempts · {Math.floor(progress.elapsedSeconds)}s
              {" · "}{progress.estimatedUsd == null ? "Cost unknown" : `$${progress.estimatedUsd.toFixed(4)} estimated`}
              {" · "}${(progress.unresolvedUsd ?? progress.reservedUsd).toFixed(4)} reserved (not confirmed spend)
            </span>
          )}
          {progress && <button disabled={stopping || progress.stopping} onClick={() => {
            setStopping(true);
            void api.cancelExtraction().catch((e) => { setStopping(false); setPreviewError(String(e)); });
          }}>Stop extraction</button>}
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
              {book.path && ` · ${book.status || (book.incomplete ? "Incomplete" : "Complete")} extraction`}
              {book.run && typeof book.run === "object" && !Array.isArray(book.run) && "strategy" in book.run && ` · ${String(book.run.strategy)} strategy`}
              {audits.length > 0 && ` · AI audit: ${auditStatus(audits, book.status, book.incomplete)}`}
              {!!audits.reduce((count, audit) => count + audit.corrections.length, 0) && ` · ${audits.reduce((count, audit) => count + audit.corrections.length, 0)} proposed corrections`}
              {!!appliedCorrections.length && ` · ${appliedCorrections.length} applied corrections`}
            </p>
          </div>
          <div className="actions">
            <button
              disabled={!!loading}
              onClick={() => setShowExtraction(true)}
            >
              Extraction…
            </button>
            <Menu label="Extraction tools">
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
                Reveal extraction in Finder
              </button>

              <hr />
              <button
                onClick={() =>
                  void copy(JSON.stringify(book.run, null, 2)).catch((e) =>
                    onError(String(e)),
                  )
                }
              >
                Copy extraction JSON
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
      <div className="actions">
        <button
          disabled={!!loading}
          onClick={() => {
            setShowLibrary(false);
            setShowResults(false);
            setShowHistory(!showHistory);
          }}
        >
          Extraction history
        </button>
        <button disabled={!!loading} aria-pressed={showResults} onClick={() => {
          setShowLibrary(false); setShowHistory(false); setShowResults(!showResults);
        }}>Model results</button>
      </div>
      {showResults && <ModelResults onOpen={(path) => { setShowResults(false); void openRecent(path); }} />}
      {showHistory && (
        <section className="library run-history">
          <h2>Extraction history</h2>
          <p>Select two extractions of the same cookbook to compare.</p>
          <button disabled={comparePaths.length !== 2 || !!loading} onClick={() => {
            void run("Comparing extractions…", async () => ({
              book: await api.run(comparePaths[1]),
              comparison: await api.diff(comparePaths[0], comparePaths[1]),
            }), (result) => { accept(result.book); setView("Comparison"); setToolData(result.comparison); });
          }}>Compare selected extractions</button>
          {previewError && <p className="error-text">{previewError}</p>}
          {savedRuns.length === 0 && <p>No saved extractions.</p>}
          {savedRuns.map((r) => (
            <div key={r.path} className="pane-header">
              <div>
                <label><input type="checkbox" aria-label={`Compare ${r.title} ${r.path}`}
                  checked={comparePaths.includes(r.path)}
                  disabled={!comparePaths.includes(r.path) && (comparePaths.length >= 2 ||
                    (comparePaths.length > 0 && savedRuns.find((row) => row.path === comparePaths[0])?.epubSha256 !== r.epubSha256))}
                  onChange={(e) => setComparePaths(e.target.checked ? [...comparePaths, r.path] : comparePaths.filter((p) => p !== r.path))}
                /> <strong>{r.title}</strong></label>
                <p>
                  {r.configurations?.length > 1
                    ? `Mixed: ${r.configurations.join(", ")}`
                    : `${r.model} · ${r.promptVersion}`}{" "}
                  · {r.recipes} recipes · {r.completed}/{r.total} chunks ·{" "}
                  {r.status || (r.incomplete ? "Incomplete" : "Complete")}

                </p>
                <p className="caption">
                  {r.createdAt === null
                    ? "Date unknown"
                    : new Date(r.createdAt * 1000).toLocaleString()}{" "}
                  ·{" "}
                  {r.newSpendUsd === null
                    ? "Cost unknown"
                    : `$${r.newSpendUsd.toFixed(4)} estimated new spend`}{" "}
                  · ${r.unresolvedUsd.toFixed(4)} unresolved
                  {r.inheritedReservedUsd != null &&
                    r.inheritedReservedUsd > 0 &&
                    ` · $${r.inheritedReservedUsd.toFixed(4)} inherited spend/reservations`}
                </p>
              </div>
              <div className="actions">
                <button
                  onClick={() => {
                    setShowHistory(false);
    setShowResults(false);
                    void openRecent(r.path);
                  }}
                >
                  Open
                </button>
                <button onClick={() => void revealFile(r.path)}>Reveal</button>
                {r.incomplete && <button onClick={() => {
                  void run("Opening extraction…", () => api.run(r.path), (next) => {
                    accept(next); setResumeRun(true); setModel(!next.feedback || next.feedback.policy.length === 1 ? next.model : "automatic"); setShowExtraction(true);
                  });
                }}>Resume…</button>}
                <button
                  onClick={async () => {
                    const out = await savePath(
                      r.path.split("/").pop() ?? "run.json",
                    );
                    if (out)
                      await run(
                        "Exporting run…",
                        () => api.exportRun(r.path, out),
                        () => {},
                      );
                  }}
                >
                  Export
                </button>
              </div>
            </div>
          ))}
        </section>
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
              <div key={b.path} className="library-entry">
                <button
                  onClick={() => {
                    const latest = b.runs?.[0];
                    if (latest) void openRecent(latest.path);
                    else void openPath(b.path);
                  }}
                >
                  <BookCover path={b.path} />
                  <span>
                    <strong>{b.title}</strong>
                    <span className="caption">{b.authors.join(", ")}</span>
                    {b.error && <span className="error-text">{b.error}</span>}
                    {b.runs?.[0] && (
                      <span className="caption">
                        {b.runs[0].status || (b.runs[0].incomplete ? "Incomplete" : "Complete")} ·{" "}
                        {b.runs[0].recipes} recipes · {b.runs[0].completed}/
                        {b.runs[0].total} chunks ·{" "}
                        {b.runs[0].configurations?.length > 1
                          ? "Mixed models/prompts"
                          : b.runs[0].model}{" "}
                        · {b.runs[0].promptVersion} ·{" "}
                        {b.runs[0].createdAt === null
                          ? "Date unknown"
                          : new Date(
                              b.runs[0].createdAt * 1000,
                            ).toLocaleDateString()}{" "}
                        ·{" "}
                        {b.runs[0].newSpendUsd === null
                          ? "Cost unknown"
                          : `$${b.runs[0].newSpendUsd.toFixed(4)}`}
                      </span>
                    )}
                  </span>
                </button>
                {!!b.runs?.length && (
                  <details>
                    <summary>{b.runs.length} saved extractions</summary>
                    {b.runs.map((r) => (
                      <button
                        key={r.path}
                        onClick={() => {
                          setShowLibrary(false);
                          void openRecent(r.path);
                        }}
                      >
                        {r.configurations?.length > 1
                          ? `Mixed: ${r.configurations.join(", ")}`
                          : `${r.model} · ${r.promptVersion}`}{" "}
                        · {r.recipes} recipes ·{" "}
                        {r.createdAt === null
                          ? "Date unknown"
                          : new Date(r.createdAt * 1000).toLocaleString()}
                      </button>
                    ))}
                  </details>
                )}
              </div>
            ))}
            {!libraryRows.length && <p>No matching books.</p>}
          </div>
        </section>
      )}
      {!book && !showLibrary && !showHistory && !showResults && !loading && (
        <div className="empty">
          <BookOpen size={34} />
          <h2>Your cookbooks</h2>
          <p>
            Browse your Calibre library to extract recipes or inspect previous
            extractions.
          </p>
          <div className="actions">
            <button className="primary" onClick={() => void browseLibrary()}>
              <FolderOpen size={15} />
              Browse library
            </button>
            <button onClick={() => void openBook("epub")}>Open cookbook</button>
          </div>
        </div>
      )}
      {book?.path && !showLibrary && !showHistory && !showResults && (
        <section className="source-checks" aria-label="Extraction feedback">
          <h2>Extraction feedback</h2>
          {audits.length > 0 && <p><strong>AI audit: {auditStatus(audits, book.status, book.incomplete)}</strong> · {audits.length} audit pass{audits.length === 1 ? "" : "es"}</p>}
          {contextExpansions.length > 0 && <details><summary>Expanded audit context ({contextExpansions.length})</summary><ul>{contextExpansions.map((expansion, index) => <li key={`${expansion.group}-${index}`}>{expansion.reason} · group {expansion.group + 1}</li>)}</ul><p className="caption">Context expansion is audit evidence only; it does not establish acceptance or apply corrections.</p></details>}
          {audits.some((audit) => audit.corrections.length > 0) && <details><summary>Proposed corrections ({audits.reduce((count, audit) => count + audit.corrections.length, 0)})</summary><ul>{audits.flatMap((audit) => audit.corrections).map((correction, index) => <li key={index}>{correctionText(correction)}</li>)}</ul></details>}
          {appliedCorrections.length > 0 && <details><summary>Applied corrections ({appliedCorrections.length})</summary><ul>{appliedCorrections.map((entry, index) => <li key={index}>{entry.reason} · {correctionText(entry.correction)}</li>)}</ul></details>}
          {book.feedback ? <>
            <p><strong>{book.feedback.phase}</strong> · {book.feedback.stopReason || (book.feedback.phase === "Complete" ? "All automated checks passed" : "Checking source coverage and fidelity")}</p>
            <p className="caption">Extraction/recovery: ${book.feedback.extractionUsd.toFixed(4)} · Verification: ${book.feedback.verificationUsd.toFixed(4)} · ${book.feedback.unresolvedUsd.toFixed(4)} unresolved reservations</p>
            <details><summary>Recovery policy and AI feedback ({book.feedback.findings.length})</summary>
              <p>{book.feedback.policy.join(" → ")}</p>
              <ul>{book.feedback.checks.map((check, i) => <li key={i}>{check}</li>)}</ul>
              <ul>{book.feedback.findings.map((f, i) => <li key={i}>
                <strong>{f.resolved ? "Recovered" : "Unresolved"} · {f.category}</strong>
                <p>{f.message}</p>
                <button onClick={() => { const index = book.documents.findIndex(d => d.path === f.source); if (index >= 0) void selectDocument(index); }}>{f.source}{f.chunk ? ` · ${f.chunk}` : ""}{f.lines.length > 0 ? ` · source lines ${f.lines.map(n => n + 1).join(", ")}` : ""}</button>
                <p className="caption">Feedback from {f.model}</p>
              </li>)}</ul>
            </details>
          </> : <p>Automated verification: Not assessed. This historical extraction has not run the new source checks.</p>}
        </section>
      )}
      {book &&
        !showLibrary &&
        !showHistory && !showResults &&
        (view !== "Review" ? (
          <section className="tool-view">
            <button className="back" onClick={() => setView("Review")}>
              <ArrowLeft size={15} />
              Back to source
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
                Back to source
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
            Saved automatically in your cookbook library. Cached outputs are
            reused; clicking Extract authorizes the estimated network work
            below.
          </p>
          <p>{model === "automatic" ? "Automatic extraction starts with GLM 5.3 Flash, verifies source coverage, and recovers unresolved groups with other models." : "Your selected model extracts the source; a separate model verifies coverage and fidelity."}</p>
          <details>
            <summary>Advanced options</summary>
            {preview?.policy && <p className="caption">Policy: {preview.policy.join(" → ")}<br />
              Remaining: ${preview.extractionRemainingUsd?.toFixed(2)} extraction/recovery · ${preview.verificationRemainingUsd?.toFixed(2)} verification
            </p>}
          <label>
            Model
            <select
              value={model || book?.model}
              disabled={resumeRun}
              onChange={(e) => setModel(e.target.value)}
            >
              {!modelChoices.some((m) => m.id === (model || book?.model)) && (
                <option value={model || book?.model}>
                  {model || book?.model} (saved configuration)
                </option>
              )}
              {modelChoices.map((m) => (
                <option key={m.id} value={m.id} disabled={!m.enabled}>
                  {m.label}
                </option>
              ))}
            </select>
          </label>
          <label>
            Extraction strategy
            <select
              aria-label="Extraction strategy"
              value={strategy}
              disabled={resumeRun}
              onChange={(e) => setStrategy(e.target.value as "indexed" | "hybrid")}
            >
              <option value="indexed">Indexed (default)</option>
              <option value="hybrid">Hybrid (experimental)</option>
            </select>
          </label>
          {allowNetwork && (
            <label>
              Spending limit (USD)
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
          <label>
            Concurrent requests
            <select
              aria-label="Concurrent requests"
              value={concurrency}
              onChange={(e) => setConcurrency(Number(e.target.value))}
            >
              {Array.from({ length: 8 }, (_, index) => index + 1).map((value) => (
                <option key={value} value={value}>
                  {value}
                </option>
              ))}
            </select>
          </label>

            <label className="check-label">
              <input
                type="checkbox"
                checked={!allowNetwork}
                onChange={(e) => {
                  setAllowNetwork(!e.target.checked);
                  setRefresh(false);
                }}
              />
              Cache only (no network requests)
            </label>
            <label className="check-label">
              <input
                type="checkbox"
                checked={refresh}
                disabled={!allowNetwork || resumeRun}
                onChange={(e) => setRefresh(e.target.checked)}
              />
              Re-extract selected chunks into a new run
            </label>
          </details>
          <p className="caption">
            {chunkSelection.length
              ? `${chunkSelection.length} selected chunks`
              : "All eligible chunks"}{" "}
            · {allowNetwork ? "Network enabled" : "Cache only"}
          </p>
          {book?.path && book.incomplete && (
            <label className="check-label">
              <input
                type="checkbox"
                checked={resumeRun}
                onChange={(e) => {
                  setResumeRun(e.target.checked);
                  setRefresh(false);
                  if (e.target.checked) setModel(!book.feedback || book.feedback.policy.length === 1 ? book.model : "automatic");
                }}
              />
              Resume this extraction
            </label>
          )}
          <div aria-live="polite">
            {previewError ? (
              <p className="error-text">{previewError}</p>
            ) : preview ? (
              <>
                <p>
                  {preview.cached} reused · {preview.pending} pending ·
                  estimated additional cost{" "}
                  {preview.lowUsd !== null && preview.highUsd !== null
                    ? range([preview.lowUsd, preview.highUsd])
                    : "unknown"}
                </p>
                <p className="caption">{preview.basis}</p>
                {(preview.extraction.uncalibrated || preview.verification.uncalibrated) && (
                  <p className="caption">Estimate is uncalibrated; compatible completed samples are insufficient.</p>
                )}
                {(["extraction", "verification"] as const).map((operation) => (
                  <p className="caption" key={operation}>
                    {operation === "extraction" ? "Extraction" : "Verification"} {range(preview[operation].usd)}
                    {preview[operation].basis && ` · ${preview[operation].basis}`}
                  </p>
                ))}
                <p className="caption">
                  Conservative reservation:{" "}
                  {preview.reservationUsd === null
                    ? "unknown"
                    : `$${preview.reservationUsd.toFixed(4)}`}
                </p>
              </>
            ) : (
              <p>Calculating estimate…</p>
            )}
          </div>
          <footer>
            <button onClick={() => setShowExtraction(false)}>Cancel</button>
            <button
              className="primary"
              disabled={
                !!loading ||
                !preview ||
                !!previewError ||
                (allowNetwork && preview.reservationUsd === null) ||
                !Number.isFinite(budget) ||
                budget < 0
              }
              onClick={() => void extract()}
            >
              {resumeRun ? "Resume extraction" : "Extract"}
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
