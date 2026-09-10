import { useEffect, useState } from "react";
import { api } from "./bridge";
import type { ModelBookResults } from "./generated";

/** Latest comparable snapshots; processing completion never implies recipe fidelity. */
export function ModelResults({ onOpen }: { onOpen: (path: string) => void }) {
  const [report, setReport] = useState<ModelBookResults | null>(null);
  const [error, setError] = useState("");
  const [query, setQuery] = useState("");
  const [revision, setRevision] = useState(0);
  useEffect(() => {
    let cancelled = false;
    setReport(null);
    setError("");
    api.results(null).then((data) => { if (!cancelled) setReport(data); })
      .catch((cause: unknown) => { if (!cancelled) setError(String(cause)); });
    return () => { cancelled = true; };
  }, [revision]);
  const rows = report?.rows.filter(({ latest: r }) =>
    `${r.title} ${r.model} ${r.promptVersion} ${r.configurations.join(" ")}`.toLocaleLowerCase().includes(query.toLocaleLowerCase())) ?? [];
  const money = (value: number | null) => value === null ? "Unknown" : `$${value.toFixed(4)}`;
  return <section className="library model-results" aria-label="Model results">
    <div className="pane-header">
      <h2>Model results</h2>
      <button onClick={() => setRevision((n) => n + 1)}>Refresh results</button>
    </div>
    <p>Latest extraction for each book, model, and prompt. Success counts completed chunks divided by completed and failed chunks, including cache reuse. Pending chunks are excluded.</p>
    <p className="caption">Processing success does not verify recipe fidelity. Source verification is reported in Extraction feedback. Attempts and costs belong to the displayed run; historical unknowns stay unknown.</p>
    <label>Filter books or models <input type="search" value={query} onChange={(e) => setQuery(e.target.value)} /></label>
    {error && <p role="alert">Could not load results: {error}. Use Refresh results to retry.</p>}
    {!report && !error && <p role="status">Loading model results…</p>}
    {report && rows.length === 0 && <p>{query ? "No matching books or models." : "No extraction results yet."}</p>}
    {report?.unreadable.map((message) => <p role="alert" key={message}>Unreadable extraction: {message}</p>)}
    {rows.length > 0 && <div className="model-results-scroll" tabIndex={0} role="region" aria-label="Model comparison table">
      <table>
        <caption>Processing success by book and model</caption>
        <thead><tr><th scope="col">Book / model</th><th scope="col">Success</th><th scope="col">Book coverage</th><th scope="col">Recipes</th><th scope="col">Attempts</th><th scope="col">New estimated spend</th><th scope="col">Extraction</th></tr></thead>
        <tbody>{rows.map((row) => {
          const r = row.latest;
          return <tr key={r.path}>
            <th scope="row"><strong>{r.title}</strong><span>{r.configurations.length > 1 ? `Mixed: ${r.configurations.join(", ")}` : r.model}</span><span className="caption">{r.promptVersion}</span></th>
            <td>{row.processingSuccessRate === null ? "Unknown" : `${(row.processingSuccessRate * 100).toFixed(1)}%`}</td>
            <td>{r.completed}/{r.total} complete<span>{row.failedChunks} failed · {row.pendingChunks} pending</span></td>
            <td>{r.recipes} recipes</td>
            <td>{row.attempts === null ? "Unknown" : `${row.attempts} ${row.attempts === 1 ? "call" : "calls"}`}<span>{row.failedAttempts === null ? "Failures unknown" : `${row.failedAttempts} failed`}</span></td>
            <td>{money(r.newSpendUsd)}<span>{money(r.unresolvedUsd)} unresolved</span>{(r.inheritedReservedUsd ?? 0) > 0 && <span>{money(r.inheritedReservedUsd)} inherited reservation</span>}</td>
            <td><button onClick={() => onOpen(r.path)} aria-label={`Open ${r.title} ${r.model} ${r.promptVersion}`}>Open extraction</button><span className="caption">{row.runs} saved {row.runs === 1 ? "extraction" : "extractions"}</span></td>
          </tr>;
        })}</tbody>
      </table>
    </div>}
  </section>;
}
