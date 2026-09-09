import { RecipeScale } from "@ingredient-parser/recipe-ui";
import {
  useEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
} from "react";
import { FileText, FlaskConical, Play } from "lucide-react";
import {
  api,
  openSourceUrl,
  display,
  type CorpusResult,
  type IngredientResult,
  type IngredientInspection,
  type RecipeResult,
} from "./bridge";
import { JsonView, Split, Tabs, useStored, VirtualList } from "./components";
import { Inspector } from "./Inspector";
export function Parser({
  onError,
  onBusy,
}: {
  onError: (v: string) => void;
  onBusy: (v: boolean) => void;
}) {
  const [mode, setMode] = useStored<"Ingredients" | "Recipe URL" | "Corpus">(
    "v1:parser-mode",
    "Ingredients",
    ["Ingredients", "Recipe URL", "Corpus"],
  );
  const [input, setInput] = useStored(
    "v1:input",
    "2 cups all-purpose flour, sifted",
  );
  const [url, setUrl] = useStored("v1:url", "");
  const [path, setPath] = useStored("v1:corpus-path", "");
  const [rows, setRows] = useState<IngredientResult[]>([]);
  const [selected, setSelected] = useState<number | null>(null);
  const [parsedInput, setParsedInput] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [recipe, setRecipe] = useState<RecipeResult | null>(null);
  const [corpus, setCorpus] = useState<CorpusResult | null>(null);
  const [filter, setFilter] = useState("All");
  const [search, setSearch] = useState("");
  const [corpusLine, setCorpusLine] = useState<number | null>(null);
  const [sourceView, setSourceView] = useState<
    "Ingredients" | "Recipe & source"
  >("Ingredients");
  const [sort, setSort] = useState<{ key: string; ascending: boolean } | null>(
    null,
  );
  const request = useRef(0);
  const run = async <T,>(fn: () => Promise<T>, accept: (value: T) => void) => {
    const id = ++request.current;
    setLoading(true);
    onBusy(true);
    try {
      const result = await fn();
      if (request.current === id) accept(result);
    } catch (e) {
      if (request.current === id) onError(String(e));
    } finally {
      if (request.current === id) {
        setLoading(false);
        onBusy(false);
      }
    }
  };
  const parse = (text = input) =>
    void run(
      () => api.parse(text),
      (result) => {
        setRows(result);
        setSelected(result[0]?.lineNumber ?? null);
        setParsedInput(text);
      },
    );
  const filtered = useMemo(
    () =>
      corpus?.cases.filter(
        (r) =>
          (filter === "All" ||
            r.status.toLowerCase() === filter.toLowerCase()) &&
          r.input.toLowerCase().includes(search.toLowerCase()),
      ) ?? [],
    [corpus, filter, search],
  );
  const corpusIndex = filtered.findIndex(
    (row) => row.lineNumber === corpusLine,
  );
  const sorted = useMemo(() => {
    if (!sort) return rows;
    return [...rows].sort(
      (a, b) =>
        display(a[sort.key as keyof IngredientResult]).localeCompare(
          display(b[sort.key as keyof IngredientResult]),
        ) * (sort.ascending ? 1 : -1),
    );
  }, [rows, sort]);
  const current = sorted.find((row) => row.lineNumber === selected);
  const [inspection, setInspection] = useState<IngredientInspection | null>(
    null,
  );
  useEffect(() => {
    let active = true;
    setInspection(null);
    if (current)
      api
        .inspect(current.input)
        .then((value) => {
          if (active) setInspection(value);
        })
        .catch((e) => {
          if (active) onError(String(e));
        });
    return () => {
      active = false;
    };
  }, [current, onError]);
  const [corpusInspection, setCorpusInspection] =
    useState<IngredientInspection | null>(null);
  const invalidate = () => {
    request.current++;
    setRows([]);
    setSelected(null);
    setInspection(null);
    setCorpusInspection(null);
    setLoading(false);
    onBusy(false);
  };
  const inspectCorpus = async () => {
    const row = filtered[corpusIndex];
    if (row) await run(() => api.inspect(row.input), setCorpusInspection);
  };
  const [columnText, setColumnText] = useStored(
    "v1:parser-columns",
    "30,22,17,18,13",
  );
  const parsedColumns = columnText.split(",").map(Number);
  const columns =
    parsedColumns.length === 5 &&
    parsedColumns.every((n) => Number.isFinite(n) && n >= 5 && n <= 80)
      ? parsedColumns
      : [30, 22, 17, 18, 13];
  const resizeStart = useRef<{ x: number; value: number } | null>(null);
  const setColumn = (index: number, value: number) =>
    setColumnText(
      columns
        .map((n, i) => (i === index ? Math.min(80, Math.max(5, value)) : n))
        .join(","),
    );
  const results = (
    <section
      className="result-table"
      style={
        {
          "--columns": columns
            .map((n, i) => `minmax(${i === 0 ? 100 : 60}px, ${n}fr)`)
            .join(" "),
        } as CSSProperties
      }
    >
      <div className="table-header">
        {["Input", "Name", "Amounts", "Modifier", "Confidence"].map(
          (label, index) => (
            <div key={label} className="table-header-cell">
              <button
                aria-label={label}
                onClick={() => {
                  setSort({
                    key: label.toLowerCase(),
                    ascending:
                      sort?.key === label.toLowerCase()
                        ? !sort.ascending
                        : true,
                  });
                }}
              >
                {label}
                {sort?.key === label.toLowerCase()
                  ? sort.ascending
                    ? " ↑"
                    : " ↓"
                  : ""}
              </button>
              {index < 4 && (
                <span
                  className="column-handle"
                  role="separator"
                  aria-label={`Resize ${label} column`}
                  aria-orientation="vertical"
                  aria-valuenow={columns[index]}
                  tabIndex={0}
                  onClick={(e) => e.stopPropagation()}
                  onPointerDown={(e) => {
                    e.stopPropagation();
                    resizeStart.current = {
                      x: e.clientX,
                      value: columns[index],
                    };
                    e.currentTarget.setPointerCapture(e.pointerId);
                  }}
                  onPointerMove={(e) => {
                    if (resizeStart.current)
                      setColumn(
                        index,
                        resizeStart.current.value +
                          (e.clientX - resizeStart.current.x) / 8,
                      );
                  }}
                  onPointerUp={() => {
                    resizeStart.current = null;
                  }}
                  onKeyDown={(e) => {
                    if (e.key === "ArrowLeft" || e.key === "ArrowRight") {
                      e.preventDefault();
                      e.stopPropagation();
                      setColumn(
                        index,
                        columns[index] + (e.key === "ArrowLeft" ? -2 : 2),
                      );
                    }
                  }}
                />
              )}
            </div>
          ),
        )}
      </div>
      <VirtualList
        rows={sorted}
        selected={sorted.findIndex((row) => row.lineNumber === selected)}
        onSelect={(index) => setSelected(sorted[index].lineNumber)}
        label="Parsed ingredients"
        render={(row) => (
          <>
            <span className="mono" title={row.input}>
              {row.input}
            </span>
            <span className="ingredient-name">{row.name}</span>
            <span className="amount">{row.amounts.join(" / ")}</span>
            <span>{row.modifier ?? "—"}</span>
            <span>{row.confidence}</span>
          </>
        )}
      />
    </section>
  );
  return (
    <div className="workspace parser">
      <header className="workspace-heading">
        <h1>Parser</h1>
        <Tabs
          value={mode}
          items={["Ingredients", "Recipe URL", "Corpus"]}
          onChange={(next) => {
            invalidate();
            setMode(next);
            setRecipe(null);
            setCorpus(null);
          }}
          label="Parser source"
        />
      </header>
      {mode === "Ingredients" && (
        <>
          <div className="input-bar">
            <label className="grow">
              <span className="sr-only">Ingredient lines</span>
              <textarea
                className="mono"
                value={input}
                rows={3}
                onChange={(e) => {
                  setInput(e.target.value);
                  invalidate();
                }}
                onKeyDown={(e) => {
                  if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
                    e.preventDefault();
                    if (!loading && input.trim()) parse();
                  }
                }}
                placeholder="One ingredient per line"
              />
            </label>
            <div className="input-actions">
              <button
                className="primary"
                disabled={loading || !input.trim()}
                onClick={() => parse()}
              >
                <Play size={14} />
                {loading ? "Parsing…" : "Parse"}
              </button>
              <span className="caption">⌘ Enter</span>
            </div>
          </div>
          {parsedInput !== null && parsedInput !== input && (
            <p role="status" className="stale">
              Input changed. Parse again to update these results.
            </p>
          )}
          {rows.length ? (
            <>
              <div className="section-summary">
                {rows.length} ingredient{rows.length === 1 ? "" : "s"}
                <span>Select a row to inspect its evidence</span>
              </div>
              <Split
                id="parser"
                initial={65}
                min={40}
                max={75}
                className="parser-split"
                left={results}
                right={inspection ? <Inspector data={inspection} /> : null}
              />
            </>
          ) : (
            <div className="empty">
              <FlaskConical size={32} />
              <h2>Follow an ingredient through the parser</h2>
              <p>
                Paste one or more lines, then parse to compare fields and
                inspect each stage.
              </p>
            </div>
          )}
        </>
      )}
      {mode === "Recipe URL" && (
        <>
          <form
            className="source-bar"
            onSubmit={(e) => {
              e.preventDefault();
              void run(
                () => api.recipe(url),
                (r) => {
                  setRecipe(r);
                  setRows(r.ingredients);
                  setSelected(r.ingredients[0]?.lineNumber ?? null);
                },
              );
            }}
          >
            <label htmlFor="recipe-url">Recipe URL</label>
            <input
              id="recipe-url"
              type="url"
              value={url}
              onChange={(e) => {
                setUrl(e.target.value);
                invalidate();
                setRecipe(null);
              }}
              placeholder="https://…"
              required
            />
            <button className="primary" disabled={loading || !url.trim()}>
              {loading ? "Loading…" : "Load recipe"}
            </button>
          </form>
          {recipe ? (
            <>
              <div className="section-heading">
                <h2>{recipe.title}</h2>
                <Tabs
                  value={sourceView}
                  items={["Ingredients", "Recipe & source"]}
                  onChange={setSourceView}
                  label="Recipe view"
                />
              </div>
              {sourceView === "Ingredients" ? (
                <Split
                  id="recipe"
                  initial={60}
                  left={results}
                  right={
                    inspection ? (
                      <Inspector data={inspection} />
                    ) : (
                      <p>No ingredient traces.</p>
                    )
                  }
                />
              ) : (
                <WebRecipe recipe={recipe} onError={onError} />
              )}
            </>
          ) : (
            <div className="empty">
              <FileText size={32} />
              <h2>Inspect a web recipe</h2>
              <p>
                Load a URL to compare the source recipe with its parsed
                ingredients.
              </p>
            </div>
          )}
        </>
      )}
      {mode === "Corpus" && (
        <>
          <form
            className="source-bar"
            onSubmit={(e) => {
              e.preventDefault();
              void run(
                () => api.corpus(path || null),
                (r) => {
                  setCorpus(r);
                  setCorpusLine(r.cases[0]?.lineNumber ?? null);
                },
              );
            }}
          >
            <label htmlFor="corpus-path">Corpus path</label>
            <input
              id="corpus-path"
              value={path}
              onChange={(e) => {
                setPath(e.target.value);
                invalidate();
                setCorpus(null);
              }}
              placeholder="Default repository corpus"
            />
            <button className="primary" disabled={loading}>
              {loading ? "Scoring…" : "Load & score"}
            </button>
          </form>
          {corpus ? (
            <>
              <div className="section-summary">
                {Object.entries(
                  corpus.cases.reduce<Record<string, number>>(
                    (t, r) => ({ ...t, [r.status]: (t[r.status] ?? 0) + 1 }),
                    {},
                  ),
                ).map(([k, v]) => (
                  <span key={k}>
                    {v} {k}
                  </span>
                ))}
              </div>
              <div className="filter-bar">
                <label>
                  Show{" "}
                  <select
                    value={filter}
                    onChange={(e) => {
                      setFilter(e.target.value);
                      invalidate();
                    }}
                  >
                    {["All", "Exact", "Regression", "Xfail", "Promote"].map(
                      (s) => (
                        <option key={s}>{s}</option>
                      ),
                    )}
                  </select>
                </label>
                <input
                  aria-label="Find corpus input"
                  placeholder="Find input…"
                  value={search}
                  onChange={(e) => {
                    setSearch(e.target.value);
                    invalidate();
                  }}
                />
              </div>
              <Split
                id="corpus"
                initial={55}
                left={
                  <VirtualList
                    label="Corpus rows"
                    rows={filtered}
                    selected={corpusIndex}
                    onSelect={(i) => {
                      request.current++;
                      setLoading(false);
                      onBusy(false);
                      setCorpusLine(filtered[i].lineNumber);
                      setCorpusInspection(null);
                    }}
                    render={(r) => (
                      <>
                        <span className={`status ${r.status.toLowerCase()}`}>
                          {r.status}
                        </span>
                        <span className="mono">{r.input}</span>
                      </>
                    )}
                  />
                }
                right={
                  corpusInspection ? (
                    <div className="corpus-evidence">
                      <button onClick={() => setCorpusInspection(null)}>
                        Back to field comparison
                      </button>
                      <Inspector
                        data={corpusInspection}
                        onClose={() => setCorpusInspection(null)}
                      />
                    </div>
                  ) : filtered[corpusIndex] ? (
                    <section className="inspector">
                      <h2>Field comparison</h2>
                      <p className="mono">{filtered[corpusIndex].input}</p>
                      {filtered[corpusIndex].reason && (
                        <p>{filtered[corpusIndex].reason}</p>
                      )}
                      <div className="scroll">
                        <table className="corpus-fields">
                          <thead>
                            <tr>
                              <th>Field</th>
                              <th>Expected</th>
                              <th>Actual</th>
                            </tr>
                          </thead>
                          <tbody>
                            {filtered[corpusIndex].fields.map((field) => (
                              <tr
                                key={field.field}
                                className={field.matches ? "" : "mismatch"}
                              >
                                <th scope="row">{field.field}</th>
                                <td>{field.expected}</td>
                                <td>
                                  {field.actual}
                                  {!field.matches && (
                                    <span className="sr-only"> (mismatch)</span>
                                  )}
                                </td>
                              </tr>
                            ))}
                          </tbody>
                        </table>
                      </div>
                      <button onClick={() => void inspectCorpus()}>
                        Inspect ingredient
                      </button>
                    </section>
                  ) : (
                    <p className="empty-inline">Select a corpus row.</p>
                  )
                }
              />
            </>
          ) : (
            <div className="empty">
              <h2>Check the regression corpus</h2>
              <p>
                Score labeled inputs using the same comparisons as the parser’s
                accuracy tests.
              </p>
            </div>
          )}
        </>
      )}
    </div>
  );
}

function WebRecipe({
  recipe,
  onError,
}: {
  recipe: RecipeResult;
  onError: (v: string) => void;
}) {
  const [factor, setFactor] = useState(1);
  const [scaled, setScaled] = useState(recipe);
  const [loading, setLoading] = useState(false);
  const generation = useRef(0);
  const scale = async (value: number) => {
    const id = ++generation.current;
    setLoading(true);
    try {
      const result = await api.scaleWeb(recipe.source, value);
      if (id === generation.current) {
        setScaled(result);
        setFactor(value);
      }
    } catch (e) {
      onError(String(e));
    } finally {
      if (id === generation.current) setLoading(false);
    }
  };
  const source = recipe.source as Record<string, unknown>;
  const sections = scaled.sections;
  return (
    <article className="web-recipe scroll">
      <button
        className="source-link"
        onClick={() =>
          void openSourceUrl(recipe.url).catch((e) => onError(String(e)))
        }
      >
        Open original recipe in browser
      </button>
      {typeof source.description === "string" && <p>{source.description}</p>}
      <RecipeScale
        className="scale"
        label="Web recipe scale"
        disabled={loading}
        value={factor}
        onChange={(value) => void scale(value)}
      />
      {sections.length ? (
        sections.map((section, i) => (
          <section key={i}>
            {section.name && <h3>{section.name}</h3>}
            <div className="web-recipe-columns">
              <ul className="ingredient-lines">
                {section.ingredients.map((line, n) => (
                  <li key={n}>{line}</li>
                ))}
              </ul>
              <ol className="instructions">
                {section.instructions.map((line, n) => (
                  <li key={n}>{line}</li>
                ))}
              </ol>
            </div>
          </section>
        ))
      ) : (
        <ul className="ingredient-lines">
          {scaled.ingredients.map((r) => (
            <li key={r.lineNumber}>
              {r.amounts.join(" / ")} {r.name}
              {r.modifier ? `, ${r.modifier}` : ""}
            </li>
          ))}
        </ul>
      )}
      {recipe.diagnostics.length > 0 && (
        <details>
          <summary>Diagnostics</summary>
          {recipe.diagnostics.map((d, i) => (
            <p key={i}>{d}</p>
          ))}
        </details>
      )}
      <details>
        <summary>Scraped source</summary>
        <JsonView value={recipe.source} />
      </details>
    </article>
  );
}
