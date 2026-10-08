import { Check, FileUp, Search, X } from "lucide-react";
import { useMemo, useRef, useState } from "react";
import { api, can, openFiles, type CorpusCase, type CorpusResult } from "../../api";
import { useReportStatus } from "../../app/status";
import { Split, VirtualList } from "../../components/layout";
import { Badge, Button, EmptyState, Input, Segmented, type Tone } from "../../components/ui";
import { cn } from "../../lib/cn";
import { useAction } from "../../lib/query";
import { useStored } from "../../lib/stored";
import { InspectPane } from "./InspectPane";

const STATUSES = ["EXACT", "REGRESSION", "XFAIL", "PROMOTE", "INVALID"] as const;
const TONE: Record<string, Tone> = { EXACT: "ok", REGRESSION: "bad", XFAIL: "neutral", PROMOTE: "accent", INVALID: "warn" };
const DESCRIPTION: Record<string, string> = {
  EXACT: "Every field matches",
  REGRESSION: "A field that used to match no longer does",
  XFAIL: "A known failure, still failing",
  PROMOTE: "A known failure that now passes",
  INVALID: "The case could not be scored",
};

function Comparison({ row }: { row: CorpusCase }) {
  return (
    <div className="min-h-0 flex-1 overflow-auto p-4">
      <div className="flex items-center gap-2">
        <Badge tone={TONE[row.status]}>{row.status}</Badge>
        <span className="text-[12px] text-muted">{DESCRIPTION[row.status]}</span>
      </div>
      <p className="mt-3 rounded-control bg-evidence p-3 font-mono text-[13px]">{row.input}</p>
      {row.reason && <p className="mt-2 text-[12.5px] text-muted">{row.reason}</p>}
      <p className="mt-1 text-[11.5px] text-faint">
        {row.section} · line {row.lineNumber}
      </p>
      <table aria-label="Field comparison" className="mt-4 w-full border-collapse text-[12.5px]">
        <thead>
          <tr className="border-b border-line text-left text-[11px] text-muted">
            <th className="w-6 py-1.5 font-medium">
              <span className="sr-only">Match</span>
            </th>
            <th className="py-1.5 font-medium">Field</th>
            <th className="py-1.5 font-medium">Expected</th>
            <th className="py-1.5 font-medium">Actual</th>
          </tr>
        </thead>
        <tbody>
          {row.fields.map((field) => (
            <tr key={field.field} className={cn("border-b border-line/60 align-top", !field.matches && "bg-bad/6")}>
              <td className="py-2">{field.matches ? <Check className="size-3.5 text-ok" /> : <X className="size-3.5 text-bad" aria-label="mismatch" />}</td>
              <th scope="row" className="py-2 pr-3 text-left font-medium text-muted">
                {field.field}
              </th>
              <td className="py-2 pr-3 font-mono text-[12px]">{field.expected || <span className="text-faint">—</span>}</td>
              <td className={cn("py-2 font-mono text-[12px]", !field.matches && "text-bad")}>{field.actual || <span className="text-faint">—</span>}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

function CaseView({ row }: { row: CorpusCase }) {
  const [view, setView] = useState<"Fields" | "Inspect">("Fields");
  return (
    <>
      <div className="flex h-11 shrink-0 items-center justify-between border-b border-line px-4">
        <h2 className="text-[12.5px] font-semibold">Corpus case</h2>
        <Segmented size="sm" label="Case view" value={view} items={["Fields", "Inspect"] as const} onChange={setView} />
      </div>
      {view === "Fields" ? <Comparison row={row} /> : <InspectPane input={row.input} title="Parser evidence" />}
    </>
  );
}

export function Corpus() {
  const [path, setPath] = useStored("v2:corpus-path", "");
  const [result, setResult] = useState<CorpusResult | null>(null);
  const [filter, setFilter] = useState<string | null>(null);
  const [search, setSearch] = useState("");
  const [selected, setSelected] = useState<number | null>(null);
  const scoring = useAction();
  const file = useRef<HTMLInputElement>(null);
  const score = (target: string | null) =>
    void scoring.run(() => api.corpus(target), (value) => {
      setResult(value);
      setSelected(null);
    });

  const counts = useMemo(
    () => (result?.cases ?? []).reduce<Record<string, number>>((all, c) => ({ ...all, [c.status]: (all[c.status] ?? 0) + 1 }), {}),
    [result],
  );
  const rows = useMemo(
    () =>
      (result?.cases ?? []).filter(
        (c) => (!filter || c.status === filter) && (!search || c.input.toLowerCase().includes(search.toLowerCase())),
      ),
    [result, filter, search],
  );
  const current = rows.find((r) => r.lineNumber === selected) ?? rows[0];
  const total = result?.cases.length ?? 0;
  useReportStatus("parser", {
    message: scoring.busy ? "Scoring corpus…" : result ? `${total} cases scored` : "Ready",
    detail: result ? `${counts.EXACT ?? 0} exact · ${counts.REGRESSION ?? 0} regressions` : undefined,
    busy: scoring.busy,
  });

  return (
    <>
      <div className="flex shrink-0 flex-wrap items-center gap-2 border-b border-line px-4 py-2.5">
        <Button variant="primary" busy={scoring.busy} onClick={() => score(can.paths && path.trim() ? path.trim() : null)}>
          {result ? "Score again" : "Score corpus"}
        </Button>
        {can.paths ? (
          <>
            <label htmlFor="corpus-path" className="sr-only">
              Corpus path
            </label>
            <Input id="corpus-path" className="max-w-md" value={path} onChange={(e) => setPath(e.target.value)} placeholder="Built-in corpus (or a corpus.jsonl path)" />
          </>
        ) : (
          <>
            <span className="text-[12px] text-muted">{result?.path ? result.path : "Built-in corpus"}</span>
            <input
              ref={file}
              type="file"
              accept=".jsonl,application/jsonl"
              hidden
              aria-label="Corpus file"
              onChange={async (e) => {
                const [name] = await openFiles(e.target.files ?? []);
                e.target.value = "";
                if (name) score(name);
              }}
            />
            <Button variant="ghost" onClick={() => file.current?.click()}>
              <FileUp />
              Score my corpus.jsonl
            </Button>
          </>
        )}
      </div>
      {result ? (
        <>
          <div className="flex shrink-0 flex-wrap items-center gap-2 border-b border-line px-4 py-2">
            <div role="group" aria-label="Filter by status" className="flex flex-wrap gap-1">
              {[null, ...STATUSES].map((status) =>
                status && !counts[status] ? null : (
                  <button
                    key={status ?? "all"}
                    type="button"
                    aria-pressed={filter === status}
                    onClick={() => setFilter(status)}
                    className={cn(
                      "flex h-6 items-center gap-1.5 rounded-full border px-2.5 text-[11.5px] font-medium transition-colors",
                      filter === status ? "border-accent/40 bg-accent/10 text-fg" : "border-line text-muted hover:bg-hover",
                    )}
                  >
                    {status ? status.toLowerCase() : "all"}
                    <span className="text-faint tabular-nums">{status ? counts[status] : total}</span>
                  </button>
                ),
              )}
            </div>
            <div className="relative ml-auto w-56">
              <Search className="pointer-events-none absolute top-1/2 left-2.5 size-3.5 -translate-y-1/2 text-faint" />
              <Input aria-label="Find corpus input" className="h-7 pl-8 text-[12.5px]" placeholder="Find input…" value={search} onChange={(e) => setSearch(e.target.value)} />
            </div>
          </div>
          <Split
            id="corpus"
            initial={52}
            left={
              <VirtualList
                label="Corpus rows"
                rows={rows}
                selected={current ? rows.indexOf(current) : -1}
                onSelect={(i) => setSelected(rows[i].lineNumber)}
                render={(row) => (
                  <span className="flex min-w-0 items-center gap-3">
                    <Badge tone={TONE[row.status]} className="w-[5.5rem] justify-center">
                      {row.status.toLowerCase()}
                    </Badge>
                    <span className="truncate font-mono text-[12.5px]">{row.input}</span>
                  </span>
                )}
              />
            }
            right={current ? <CaseView key={current.lineNumber} row={current} /> : <EmptyState title="No case selected" />}
          />
        </>
      ) : (
        <EmptyState icon={<Check />} title="Check the regression corpus">
          Score labeled lines with the same comparisons as the parser's accuracy tests, then inspect any case that does not match.
        </EmptyState>
      )}
    </>
  );
}
