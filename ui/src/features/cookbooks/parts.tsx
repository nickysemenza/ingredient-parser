import { BookOpen, ChevronRight, FolderOpen, Trash2 } from "lucide-react";
import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import { api, type RunSummary } from "../../api";
import { Badge, Button, IconButton } from "../../components/ui";
import { cn } from "../../lib/cn";
import { formatCost, formatDate, formatDuration, formatPercent } from "./format";

/** Subscription CLIs report no per-call price. */
export const subscription = (models: string[]) => models.some((model) => model.includes("-cli/"));
export const costOf = (models: string[], usd: number | null) => (subscription(models) ? "subscription" : formatCost(usd));
export const plural = (n: number, word: string) => `${n.toLocaleString("en-US")} ${word}${n === 1 ? "" : "s"}`;

export interface Row {
  key: string;
  cells: ReactNode[];
  tone?: "bad" | "warn";
}

/** A report table whose first cell of each row is its header. */
export function Table({ head, rows, label }: { head: string[]; rows: Row[]; label?: string }) {
  return (
    <div className="overflow-x-auto">
      <table aria-label={label} className="w-full border-collapse text-[12.5px]">
        <thead>
          <tr className="border-b border-line text-left text-[11px] text-muted">
            {head.map((h) => (
              <th key={h} className="px-2 py-1.5 font-medium whitespace-nowrap first:pl-0">
                {h}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {rows.map((row) => (
            <tr key={row.key} className={cn("border-b border-line/60 align-top last:border-0", row.tone === "bad" && "bg-bad/6", row.tone === "warn" && "bg-warn/6")}>
              {row.cells.map((cell, index) =>
                index ? (
                  <td key={index} className="px-2 py-1.5 tabular-nums">
                    {cell}
                  </td>
                ) : (
                  <th key={index} scope="row" className="py-1.5 pr-2 text-left font-medium whitespace-nowrap">
                    {cell}
                  </th>
                ),
              )}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

/** Covers are read from the archive one at a time, so they load when they
 *  scroll into view and are remembered for the session. */
const covers = new Map<string, string | null>();
export function Cover({ path, className }: { path: string; className?: string }) {
  const [url, setUrl] = useState<string | null>(() => covers.get(path) ?? null);
  const ref = useRef<HTMLSpanElement>(null);
  const load = useCallback(() => {
    if (covers.has(path)) return;
    covers.set(path, null);
    api.cover(path).then(
      (image) => {
        covers.set(path, image?.dataUrl ?? null);
        setUrl(image?.dataUrl ?? null);
      },
      () => covers.set(path, null),
    );
  }, [path]);
  useEffect(() => {
    const node = ref.current;
    if (!node || typeof IntersectionObserver === "undefined") return;
    const observer = new IntersectionObserver((entries) => {
      if (entries.some((e) => e.isIntersecting)) {
        load();
        observer.disconnect();
      }
    });
    observer.observe(node);
    return () => observer.disconnect();
  }, [load]);
  return (
    <span ref={ref} onPointerEnter={load} className={cn("flex items-center justify-center overflow-hidden rounded-md border border-line bg-control text-faint", className)}>
      {url ? <img src={url} alt="" className="size-full object-cover" /> : <BookOpen className="size-5" />}
    </span>
  );
}

export function RunList({
  runs,
  label,
  onOpen,
  onDelete,
  onReveal,
  empty,
}: {
  runs: RunSummary[];
  label: string;
  onOpen: (path: string) => void;
  onDelete?: (run: RunSummary) => void;
  onReveal?: (run: RunSummary) => void;
  empty: ReactNode;
}) {
  return (
    <section aria-label={label}>
      <header className="mb-2 flex items-baseline justify-between">
        <h2 className="text-[12.5px] font-semibold">{label}</h2>
        <span className="text-[11.5px] text-faint">{plural(runs.length, "run")}</span>
      </header>
      {runs.length ? (
        <ul className="divide-y divide-line overflow-hidden rounded-panel border border-line bg-evidence">
          {runs.map((run) => (
            <li key={run.path} className="group flex items-center gap-3 px-3 py-2.5 hover:bg-hover/40">
              <button type="button" aria-label={`Open run of ${run.book} from ${formatDate(run.started_at)}`} onClick={() => onOpen(run.path)} className="flex min-w-0 flex-1 flex-col text-left">
                <span className="flex items-center gap-2">
                  <span className="truncate text-[13px] font-medium">{run.book}</span>
                  {run.incomplete && <Badge tone="warn">incomplete</Badge>}
                </span>
                <span className="mt-0.5 truncate text-[11.5px] text-muted tabular-nums">
                  {formatDate(run.started_at)} · {plural(run.recipes, "recipe")} · {plural(run.items, "item")} · recall {formatPercent(run.recall)} · {costOf(run.ladder, run.cost_usd)} · {formatDuration(run.wall_ms)}
                </span>
                <span className="truncate font-mono text-[11px] text-faint">{run.ladder.join(" → ") || "default ladder"}</span>
              </button>
              <span className="flex items-center gap-0.5 opacity-60 transition-opacity group-hover:opacity-100">
                {onReveal && (
                  <IconButton label="Reveal in Finder" onClick={() => onReveal(run)}>
                    <FolderOpen />
                  </IconButton>
                )}
                {onDelete && (
                  <IconButton label="Delete run" onClick={() => onDelete(run)}>
                    <Trash2 />
                  </IconButton>
                )}
                <Button size="sm" variant="ghost" onClick={() => onOpen(run.path)}>
                  Open
                  <ChevronRight />
                </Button>
              </span>
            </li>
          ))}
        </ul>
      ) : (
        <p className="rounded-panel border border-dashed border-line p-5 text-center text-[12.5px] text-muted">{empty}</p>
      )}
    </section>
  );
}

/** A content card in the book and run views. */
export function Card({ title, aside, children, label, className }: { title: ReactNode; aside?: ReactNode; children: ReactNode; label?: string; className?: string }) {
  return (
    <section aria-label={label ?? (typeof title === "string" ? title : undefined)} className={cn("rounded-panel border border-line bg-evidence", className)}>
      <header className="flex min-h-10 items-center justify-between gap-3 border-b border-line px-4 py-2">
        <h2 className="text-[12.5px] font-semibold">{title}</h2>
        {aside && <div className="flex items-center gap-2">{aside}</div>}
      </header>
      <div className="space-y-3 p-4 text-[12.5px]">{children}</div>
    </section>
  );
}

export function Notes({ items, tone }: { items: string[]; tone?: "warn" }) {
  if (!items.length) return null;
  return (
    <ul className={cn("list-disc space-y-1 pl-4 text-[12px]", tone === "warn" ? "text-warn" : "text-muted")}>
      {items.map((item, i) => (
        <li key={i}>{item}</li>
      ))}
    </ul>
  );
}
