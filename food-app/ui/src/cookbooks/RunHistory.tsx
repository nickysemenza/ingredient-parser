import type { RunSummary } from "../bridge";
import {
  formatCost,
  formatDate,
  formatDuration,
  formatPercent,
} from "./format";

export function RunHistory({
  runs,
  label,
  onOpen,
  onDelete,
  empty = "No saved runs yet.",
}: {
  runs: RunSummary[];
  label: string;
  onOpen: (path: string) => void;
  onDelete?: (run: RunSummary) => void;
  empty?: string;
}) {
  return (
    <section className="run-history" aria-label={label}>
      <header className="pane-header">
        <h2>{label}</h2>
        <span className="caption">
          {runs.length} run{runs.length === 1 ? "" : "s"}
        </span>
      </header>
      {runs.length ? (
        <ul className="run-rows">
          {runs.map((run) => (
            <li key={run.path}>
              <div className="run-row-main">
                <strong>{run.book}</strong>
                <span className="caption">
                  {formatDate(run.started_at)} · {run.recipes} recipes ·{" "}
                  {run.items} items · recall {formatPercent(run.recall)} ·{" "}
                  {formatCost(run.cost_usd)} · {formatDuration(run.wall_ms)}
                </span>
                <span className="caption">
                  {run.ladder.join(" → ") || "default ladder"}
                </span>
              </div>
              <div className="actions">
                {run.incomplete && (
                  <span className="badge warn">incomplete</span>
                )}
                <button onClick={() => onOpen(run.path)}>Open</button>
                {onDelete && (
                  <button onClick={() => onDelete(run)}>Delete</button>
                )}
              </div>
            </li>
          ))}
        </ul>
      ) : (
        <p className="empty-inline">{empty}</p>
      )}
    </section>
  );
}
