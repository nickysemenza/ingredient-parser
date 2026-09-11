import { useEffect, useState } from "react";
import { Play, X } from "lucide-react";
import {
  api,
  type Estimate,
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
          <span>{formatCost(progress.cost_so_far_usd)} so far</span>
          <span>{formatDuration(progress.elapsed_ms)} elapsed</span>
          <span>{formatEta(progress.eta)}</span>
          <span>{formatCost(progress.eta.projected_cost_usd)} projected</span>
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
  onCancel,
  onOpenRun,
  onError,
}: {
  book: OpenedBook;
  extracting: boolean;
  progress: Progress | null;
  onExtract: () => void;
  onCancel: () => void;
  onOpenRun: (path: string) => void;
  onError: (value: string) => void;
}) {
  const [estimate, setEstimate] = useState<Estimate | null>(null);
  const [gateway, setGateway] = useState<GatewayStatus | null>(null);
  const { outline, classified } = book;
  useEffect(() => {
    let active = true;
    setEstimate(null);
    void api
      .estimate(book.path)
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
  }, [book.path, onError]);
  const unconfigured = gateway !== null && !gateway.configured;
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
                  formatCostRange(
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
              disabled={unconfigured}
              onClick={onExtract}
            >
              <Play size={14} />
              Extract
            </button>
            {gateway && !gateway.configured && (
              <p className="stale">
                Gateway credentials are not configured, so no model can be
                called. Put them in{" "}
                <span className="mono">
                  {gateway.configPath ?? "the gateway configuration file"}
                </span>
                {gateway.error ? `: ${gateway.error}` : "."}
              </p>
            )}
            {gateway?.configured && (
              <p className="caption">
                {gateway.baseUrl} · ladder {gateway.ladder.join(" → ")}
              </p>
            )}
          </>
        )}
      </section>
      <RunHistory
        runs={book.runs}
        label="Runs of this book"
        onOpen={onOpenRun}
        empty="This book has no saved runs yet."
      />
    </div>
  );
}
