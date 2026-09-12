import { useEffect, useState } from "react";
import { Play, X } from "lucide-react";
import {
  api,
  type Estimate,
  type BackendOptions,
  type BackendStatus,
  type GatewayStatus,
  type OpenedBook,
  type Progress,
} from "../bridge";
import {
  formatCost,
  formatCostRange,
  formatCount,
  formatDuration,
  formatDurationRange,
  formatEta,
} from "./format";
import { CatalogPanel } from "./Catalog";
import { Facts } from "./parts";
import { RunHistory } from "./RunHistory";

export function ExtractionProgress({
  progress,
  onCancel,
}: {
  progress: Progress | null;
  onCancel: () => void;
}) {
  return (
    <div className="progress" role="status" aria-label="Extraction progress">
      <span className="spinner" />
      {progress ? (
        <>
          <span>{progress.phase}</span>
          <span>
            {progress.done}/{progress.total} chunks
          </span>
          <span>{progress.in_flight} in flight</span>
          <span>{progress.failed} failed</span>
          <span>{progress.cached} cached</span>
          <span>{progress.recipes_so_far} recipes</span>
          <span>
            {progress.active_models.some((model) => model.includes("-cli/"))
              ? "Subscription usage"
              : `${formatCost(progress.cost_so_far_usd)} so far`}
          </span>
          <span>{formatDuration(progress.elapsed_ms)} elapsed</span>
          <span>{formatEta(progress.eta)}</span>
          {!progress.active_models.some((model) => model.includes("-cli/")) && (
            <span>{formatCost(progress.eta.projected_cost_usd)} projected</span>
          )}
          {progress.active_models.length > 0 && (
            <span>{progress.active_models.join(", ")}</span>
          )}
        </>
      ) : (
        <span>Starting the extraction…</span>
      )}
      <button onClick={onCancel}>
        <X size={14} />
        Cancel
      </button>
    </div>
  );
}

export function BookPanel({
  book,
  extracting,
  progress,
  onExtract,
  onBusy,
  cataloging = false,
  onCancel,
  onOpenRun,
  onError,
}: {
  book: OpenedBook;
  extracting: boolean;
  progress: Progress | null;
  onExtract: (options: BackendOptions) => void;
  onBusy?: (busy: boolean) => void;
  cataloging?: boolean;
  onCancel: () => void;
  onOpenRun: (path: string) => void;
  onError: (value: string) => void;
}) {
  const [backend, setBackend] = useState("gateway");
  const [model, setModel] = useState("opus");
  const [useCatalog, setUseCatalog] = useState(false);
  const [backends, setBackends] = useState<BackendStatus[]>([]);
  useEffect(() => {
    void api
      .backends()
      .then(setBackends)
      .catch((error) => onError(String(error)));
  }, [onError]);
  const [estimate, setEstimate] = useState<Estimate | null>(null);
  const [gateway, setGateway] = useState<GatewayStatus | null>(null);
  const { outline, classified } = book;
  useEffect(() => {
    let active = true;
    setEstimate(null);
    void api
      .estimate(book.path, {
        backend,
        model: backend === "gateway" ? null : model,
        use_catalog: useCatalog,
      })
      .then((value) => {
        if (active) setEstimate(value);
      })
      .catch((error: unknown) => {
        if (active) onError(String(error));
      });
    void api
      .gateway()
      .then((value) => {
        if (active) setGateway(value);
      })
      .catch((error: unknown) => {
        if (active) onError(String(error));
      });
    return () => {
      active = false;
    };
  }, [book.path, onError, backend, model, useCatalog]);
  const selected = backends.find((item) => item.backend === backend);
  const unconfigured =
    backend === "gateway"
      ? gateway !== null && !gateway.configured
      : !selected?.ready;
  return (
    <div className="book-panel scroll">
      <section className="card" aria-label="Book outline">
        <h2>Outline</h2>
        <p className="caption">
          {outline.source.authors.join(", ") || "Unknown author"}
        </p>
        <Facts
          items={[
            ["chapters", outline.chapters.join(" · ") || "No contents entries"],
            ["contents recipes", outline.nav_recipe_titles],
            ["lines", formatCount(outline.lines)],
            ["chunks", outline.chunks],
            ["documents", outline.source.spine_docs],
            ["opened in", formatDuration(book.openMs)],
          ]}
        />
      </section>
      <section className="card" aria-label="Cookbook classification">
        <header className="pane-header">
          <h2>Classification</h2>
          <span className={`badge verdict ${classified.classification}`}>
            {classified.classification.replace("_", " ")}
          </span>
        </header>
        <p className="caption">
          score {classified.score.toFixed(2)} · decided by {classified.method} ·{" "}
          {classified.quantity_lines} quantity lines ·{" "}
          {classified.ingredient_runs} ingredient runs
        </p>
        <ul className="reasons">
          {classified.reasons.map((reason) => (
            <li key={reason}>{reason}</li>
          ))}
        </ul>
      </section>
      <section className="card" aria-label="Model backend">
        <h2>Model backend</h2>
        <div className="catalog-controls">
          <label>
            Backend{" "}
            <select
              aria-label="Backend"
              disabled={extracting}
              value={backend}
              onChange={(event) => {
                const value = event.target.value;
                setBackend(value);
                setModel(value === "claude-cli" ? "opus" : "gpt-5.6-sol");
              }}
            >
              <option value="gateway">AI Gateway</option>
              <option value="claude-cli">Claude Code subscription</option>
              <option value="codex-cli">Codex subscription</option>
            </select>
          </label>
          {backend !== "gateway" && (
            <label>
              Model{" "}
              <select
                aria-label="Model"
                disabled={extracting}
                value={model}
                onChange={(event) => setModel(event.target.value)}
              >
                {backend === "claude-cli" ? (
                  <option value="opus">Opus</option>
                ) : (
                  <>
                    <option value="gpt-5.6-sol">Sol</option>
                    <option value="gpt-6-astra">Astra</option>
                  </>
                )}
              </select>
            </label>
          )}
          <label>
            <input
              type="checkbox"
              disabled={extracting}
              checked={useCatalog}
              onChange={(event) => setUseCatalog(event.target.checked)}
            />{" "}
            Use saved catalog · experimental
          </label>
        </div>
        {backend !== "gateway" && (
          <p className={selected?.ready ? "caption" : "stale"}>
            {selected
              ? (selected.error ??
                "Ready · subscription usage · stops on reported allowance limits")
              : "Checking CLI readiness…"}
          </p>
        )}
      </section>
      <section className="card" aria-label="Extraction estimate">
        <h2>Estimate</h2>
        {estimate ? (
          <>
            <Facts
              items={[
                [
                  "chunks",
                  `${estimate.chunks} (${estimate.cache_hits} already cached)`,
                ],
                [
                  "tokens",
                  `${formatCount(estimate.input_tokens)} in · ${formatCount(estimate.output_tokens)} out`,
                ],
                ["calls", `${estimate.calls_low}–${estimate.calls_high}`],
                [
                  "cost",
                  backend !== "gateway"
                    ? "Subscription usage"
                    : formatCostRange(
                        estimate.cost_usd_low,
                        estimate.cost_usd_high,
                      ),
                ],
                [
                  "time",
                  formatDurationRange(
                    estimate.wall_ms_low,
                    estimate.wall_ms_high,
                  ),
                ],
                ["ladder", estimate.ladder.join(" → ")],
                ["concurrency", estimate.concurrency],
              ]}
            />
            <ul className="reasons">
              {estimate.assumptions.map((assumption) => (
                <li key={assumption}>{assumption}</li>
              ))}
            </ul>
          </>
        ) : (
          <p className="caption">Measuring the book…</p>
        )}
      </section>
      <section className="card" aria-label="Extraction">
        {extracting ? (
          <ExtractionProgress progress={progress} onCancel={onCancel} />
        ) : (
          <>
            <button
              className="primary"
              disabled={unconfigured || cataloging}
              onClick={() =>
                onExtract({
                  backend,
                  model: backend === "gateway" ? null : model,
                  use_catalog: useCatalog,
                })
              }
            >
              <Play size={14} />
              Extract
            </button>
            {backend === "gateway" && gateway && !gateway.configured && (
              <p className="stale">
                Gateway credentials are not configured, so no model can be
                called. Put them in{" "}
                <span className="mono">
                  {gateway.configPath ?? "the gateway configuration file"}
                </span>
                {gateway.error ? `: ${gateway.error}` : "."}
              </p>
            )}
            {backend === "gateway" && gateway?.configured && (
              <p className="caption">
                {gateway.baseUrl} · ladder {gateway.ladder.join(" → ")}
              </p>
            )}
          </>
        )}
      </section>
      <CatalogPanel
        disabled={extracting}
        paths={[book.path]}
        onError={onError}
        onBusy={onBusy}
      />
      <RunHistory
        runs={book.runs}
        label="Runs of this book"
        onOpen={onOpenRun}
        empty="This book has no saved runs yet."
      />
    </div>
  );
}
