import { ChevronRight, Map as MapIcon } from "lucide-react";
import { useEffect, useState } from "react";
import { toast } from "sonner";
import { api, message, type Catalog, type CatalogProgress, type Entry, type SourceLine } from "../../api";
import { Badge, Button, Select, type Tone } from "../../components/ui";
import { cn } from "../../lib/cn";
import { Card, Notes, plural } from "./parts";

const READERS = [
  ["claude-cli/opus", "Opus · Claude Code"],
  ["codex-cli/gpt-5.6-sol", "Sol · Codex"],
  ["codex-cli/gpt-6-astra", "Astra · Codex"],
] as const;
const AUDITOR = "codex-cli/gpt-5.6-sol";
const STATUS_TONE: Record<string, Tone> = { complete: "ok", uncertain: "warn", failed: "bad", stale: "warn", building: "accent" };

function SourceRange({ path, entry }: { path: string; entry: Entry }) {
  const [lines, setLines] = useState<SourceLine[] | null>(null);
  useEffect(() => {
    let live = true;
    api.source(path, entry.start, entry.end).then(
      (value) => live && setLines(value),
      (error: unknown) => toast.error(message(error)),
    );
    return () => {
      live = false;
    };
  }, [path, entry.start, entry.end]);
  if (!lines) return <p className="p-3 text-muted">Reading the source…</p>;
  return (
    <pre aria-label="Catalog source" className="max-h-72 overflow-auto bg-mantle p-3 font-mono text-[11.5px] leading-relaxed">
      {lines.map((line) => (
        <div key={line.line} className={cn(line.line === entry.title_line && "font-semibold text-accent")}>
          <span className="mr-3 inline-block w-12 text-right text-faint select-none">{line.line}</span>
          {line.text}
        </div>
      ))}
    </pre>
  );
}

/** Map a book's structure once (recipes, variations, continuations) so
 *  extraction can be guided by it. Experimental; desktop only. */
export function CatalogPanel({ paths, disabled = false, onBusy }: { paths: string[]; disabled?: boolean; onBusy?: (busy: boolean) => void }) {
  const [catalog, setCatalog] = useState<Catalog | null>(null);
  const [busy, setBusy] = useState(false);
  const [progress, setProgress] = useState<CatalogProgress | null>(null);
  const [reader, setReader] = useState<string>(READERS[0][0]);
  const [audit, setAudit] = useState(false);
  const [open, setOpen] = useState<number | null>(null);
  const path = paths.length === 1 ? paths[0] : undefined;
  useEffect(() => {
    let live = true;
    setCatalog(null);
    setOpen(null);
    if (path)
      api.catalogStatus(path).then(
        (value) => live && setCatalog(value),
        (error: unknown) => live && toast.error(message(error)),
      );
    return () => {
      live = false;
    };
  }, [path]);
  const build = async (force: boolean) => {
    setBusy(true);
    onBusy?.(true);
    setProgress(null);
    try {
      const results = await api.catalog(paths, { reader, auditor: audit && reader !== AUDITOR ? AUDITOR : null, force }, setProgress);
      setCatalog(results.length === 1 ? (results[0] ?? null) : null);
      toast.success(`${plural(results.length, "catalog")} saved`);
    } catch (error) {
      toast.error(message(error));
      if (path) setCatalog(await api.catalogStatus(path).catch(() => null));
    } finally {
      setBusy(false);
      onBusy?.(false);
      setProgress(null);
    }
  };
  const status = busy ? "building" : (catalog?.status ?? "missing");
  return (
    <Card
      title="Structural catalog"
      label="Structural catalog"
      aside={
        <>
          <Badge tone="violet">experimental</Badge>
          <Badge tone={STATUS_TONE[status] ?? "neutral"}>{status}</Badge>
        </>
      }
    >
      <p className="text-muted">Read the source once to map recipes, variations and continuations. Uses subscription allowance and stops on reported limits.</p>
      <div className="flex flex-wrap items-center gap-3">
        <label className="flex items-center gap-2">
          <span className="text-muted">Reader</span>
          <Select aria-label="Reader" value={reader} disabled={busy} onChange={(e) => setReader(e.target.value)}>
            {READERS.map(([value, name]) => (
              <option key={value} value={value}>
                {name}
              </option>
            ))}
          </Select>
        </label>
        <label className="flex items-center gap-2 text-muted">
          <input type="checkbox" className="accent-(--accent)" checked={audit} disabled={busy || reader === AUDITOR} onChange={(e) => setAudit(e.target.checked)} />
          Audit uncertain regions with Sol
        </label>
        <span className="ml-auto flex gap-2">
          {busy ? (
            <Button onClick={() => void api.cancelExtraction().catch((e: unknown) => toast.error(message(e)))}>Cancel catalog</Button>
          ) : (
            <>
              {catalog && (
                <Button variant="ghost" disabled={disabled} onClick={() => void build(true)}>
                  Rebuild
                </Button>
              )}
              <Button disabled={disabled || !paths.length} onClick={() => void build(false)}>
                {catalog?.status === "failed" ? "Retry catalog" : "Build catalog"}
                {paths.length > 1 ? ` (${paths.length} books)` : ""}
              </Button>
            </>
          )}
        </span>
      </div>
      {progress && (
        <div role="status" className="space-y-1.5">
          <div className="flex justify-between text-[11.5px] text-muted">
            <span>
              {progress.book} · {progress.phase}
            </span>
            <span className="tabular-nums">
              {progress.done}/{progress.total} windows
            </span>
          </div>
          <div className="h-1.5 overflow-hidden rounded-full bg-control">
            <div className="h-full bg-accent transition-[width]" style={{ width: `${progress.total ? (progress.done / progress.total) * 100 : 0}%` }} />
          </div>
        </div>
      )}
      {catalog && (
        <>
          <p className="text-[11.5px] text-muted">
            Reader {catalog.reader}
            {catalog.auditor ? ` · auditor ${catalog.auditor}` : ""} · {plural(catalog.windows.length, "window")} · {plural(catalog.entries.length, "mapped item")}
          </p>
          {catalog.error && <p className="text-warn">{catalog.error}</p>}
          <Notes items={catalog.concerns} tone="warn" />
          {path && catalog.entries.length > 0 && (
            <ul aria-label="Catalog map" className="divide-y divide-line overflow-hidden rounded-control border border-line">
              {catalog.entries.map((entry, index) => (
                <li key={entry.title_line}>
                  <button type="button" aria-expanded={open === index} onClick={() => setOpen(open === index ? null : index)} className="flex w-full items-center gap-2 px-3 py-1.5 text-left hover:bg-hover/50">
                    <ChevronRight className={cn("size-3 text-faint transition-transform", open === index && "rotate-90")} />
                    <MapIcon className="size-3.5 text-muted" />
                    <span className="font-medium">{entry.kind}</span>
                    <span className="text-muted tabular-nums">
                      lines {entry.start}–{entry.end - 1}
                    </span>
                    {entry.uncertain && <Badge tone="warn">uncertain</Badge>}
                  </button>
                  {open === index && <SourceRange path={path} entry={entry} />}
                </li>
              ))}
            </ul>
          )}
        </>
      )}
    </Card>
  );
}
