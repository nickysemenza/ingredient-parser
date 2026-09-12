import { useEffect, useState } from "react";
import { api, type Catalog, type CatalogProgress } from "../bridge";

export function CatalogPanel({
  paths,
  onError,
  onBusy,
  disabled = false,
}: {
  disabled?: boolean;
  paths: string[];
  onError: (message: string) => void;
  onBusy?: (busy: boolean) => void;
}) {
  const [catalog, setCatalog] = useState<Catalog | null>(null);
  const [busy, setBusy] = useState(false);
  const [progress, setProgress] = useState<CatalogProgress | null>(null);
  const [reader, setReader] = useState("claude-cli/opus");
  const [audit, setAudit] = useState(false);
  const [source, setSource] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const path = paths.length === 1 ? paths[0] : undefined;
  useEffect(() => {
    let active = true;
    setCatalog(null);
    setSource(null);
    if (path)
      void api
        .catalogStatus(path)
        .then((value) => {
          if (active) setCatalog(value);
        })
        .catch((error) => {
          if (active) onError(String(error));
        });
    return () => {
      active = false;
    };
  }, [path, onError]);
  const build = async (force: boolean) => {
    setBusy(true);
    onBusy?.(true);
    setMessage(null);
    setProgress(null);
    try {
      const results = await api.catalog(
        paths,
        {
          reader,
          auditor:
            audit && reader !== "codex-cli/gpt-5.6-sol"
              ? "codex-cli/gpt-5.6-sol"
              : null,
          force,
        },
        setProgress,
      );
      setCatalog(results.length === 1 ? (results[0] ?? null) : null);
      setMessage(
        `${results.length} catalog${results.length === 1 ? "" : "s"} saved`,
      );
    } catch (error) {
      setMessage(String(error));
      onError(String(error));
      if (path) {
        try {
          setCatalog(await api.catalogStatus(path));
        } catch {
          /* Keep the original job error. */
        }
      }
    } finally {
      setBusy(false);
      onBusy?.(false);
    }
  };
  return (
    <section className="card" aria-label="Structural catalog">
      <header className="pane-header">
        <h2>Structural catalog</h2>
        <span className="badge">
          {busy ? "building" : (catalog?.status ?? "missing")}
        </span>
      </header>
      <p className="caption">
        Read the source once to map recipes, variations, and continuations.
        Catalog guidance is experimental and can be enabled for extraction.
      </p>
      <div className="catalog-controls">
        <label>
          Reader{" "}
          <select
            value={reader}
            disabled={busy}
            onChange={(event) => setReader(event.target.value)}
          >
            <option value="claude-cli/opus">Opus · Claude Code</option>
            <option value="codex-cli/gpt-5.6-sol">Sol · Codex</option>
            <option value="codex-cli/gpt-6-astra">Astra · Codex</option>
          </select>
        </label>
        <label>
          <input
            type="checkbox"
            checked={audit}
            disabled={busy || reader === "codex-cli/gpt-5.6-sol"}
            onChange={(event) => setAudit(event.target.checked)}
          />{" "}
          Audit uncertain regions with Sol
        </label>
        {busy ? (
          <button
            onClick={() =>
              void api
                .cancelExtraction()
                .catch((error) => onError(String(error)))
            }
          >
            Cancel catalog
          </button>
        ) : (
          <>
            <button
              disabled={disabled || paths.length === 0}
              onClick={() => void build(false)}
            >
              {catalog?.status === "failed" ? "Retry catalog" : "Build catalog"}
              {paths.length > 1 ? ` (${paths.length} books)` : ""}
            </button>
            {catalog && (
              <button disabled={disabled} onClick={() => void build(true)}>
                Rebuild catalog
              </button>
            )}
          </>
        )}
      </div>
      <p className="caption">
        Uses subscription allowance. Stops on reported limits. Paid-credit
        settings remain controlled by your provider account.
      </p>
      {progress && (
        <p role="status">
          {progress.book} · {progress.phase} · {progress.done}/{progress.total}{" "}
          windows
        </p>
      )}
      {message && <p role="status">{message}</p>}
      {catalog && (
        <>
          <p className="caption">
            Reader: {catalog.reader}
            {catalog.auditor ? ` · Auditor: ${catalog.auditor}` : ""} ·{" "}
            {catalog.windows.length} windows saved · {catalog.entries.length}{" "}
            mapped items
          </p>
          {catalog.error && <p className="stale">{catalog.error}</p>}
          {catalog.concerns.length > 0 && (
            <details>
              <summary>{catalog.concerns.length} unresolved concerns</summary>
              <ul>
                {catalog.concerns.map((concern, index) => (
                  <li key={index}>{concern}</li>
                ))}
              </ul>
            </details>
          )}
          {path && catalog.entries.length > 0 && (
            <details>
              <summary>Inspect source-linked map</summary>
              <div className="catalog-map">
                {catalog.entries.map((entry) => (
                  <button
                    key={entry.title_line}
                    onClick={() =>
                      void api
                        .catalogSource(path, entry.title_line)
                        .then(setSource)
                        .catch((error) => onError(String(error)))
                    }
                  >
                    {entry.kind} · lines {entry.start}–{entry.end - 1}
                    {entry.uncertain ? " · uncertain" : ""}
                  </button>
                ))}
              </div>
              {source && <pre className="catalog-source">{source}</pre>}
            </details>
          )}
        </>
      )}
    </section>
  );
}
