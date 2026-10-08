import type { CallOutcome, CallRecord, RunReport } from "../../api";
import { VirtualList } from "../../components/layout";
import { Badge, Facts, JsonView } from "../../components/ui";
import { cn } from "../../lib/cn";
import { useState } from "react";
import { describeFlag, formatCost, formatCount, formatDate, formatDuration, formatPercent, orderChunks } from "./format";
import { Card, costOf, plural, Table } from "./parts";

function outcomeText(outcome: CallOutcome): string {
  switch (outcome.outcome) {
    case "ok":
      return "ok";
    case "invalid":
      return `invalid: ${outcome.faults.join(", ")}`;
    case "transport":
      return `${outcome.kind}: ${outcome.message}`;
  }
}

const CALL_GRID = "grid grid-cols-[2.5rem_5rem_minmax(8rem,1.4fr)_6rem_3.5rem_4.5rem_3rem_3.5rem_5rem_minmax(8rem,2fr)] gap-2";
const CALL_COLUMNS = ["Seq", "Chunk", "Model", "Purpose", "Try", "Latency", "HTTP", "Cache", "Cost", "Outcome"];

function CallRow({ call }: { call: CallRecord }) {
  return (
    <div className={cn(CALL_GRID, "w-full items-center text-[12px] tabular-nums")}>
      <span className="text-faint">{call.seq}</span>
      <span className="truncate font-mono">{call.chunk_id}</span>
      <span className="truncate font-mono" title={call.actual_model ?? call.model}>
        {call.model}
      </span>
      <span>{call.purpose.replace("_", " ")}</span>
      <span>#{call.attempt}</span>
      <span>{formatDuration(call.latency_ms)}</span>
      <span>{call.status ?? "—"}</span>
      <span className={call.cached ? "text-ok" : "text-muted"}>{call.cached ? "hit" : "live"}</span>
      <span>{call.billing === "subscription" ? "sub" : formatCost(call.cost_usd)}</span>
      <span className={cn("truncate", call.outcome.outcome === "ok" ? "text-ok" : "text-bad")} title={outcomeText(call.outcome)}>
        {outcomeText(call.outcome)}
        {call.truncated ? " · truncated" : ""}
      </span>
    </div>
  );
}

function Stat({ label, value, tone }: { label: string; value: React.ReactNode; tone?: "ok" | "warn" | "bad" }) {
  return (
    <div className="rounded-panel border border-line bg-evidence px-4 py-3">
      <div className="text-[11px] font-medium text-muted">{label}</div>
      <div className={cn("mt-0.5 text-[18px] font-semibold tracking-tight tabular-nums", tone === "ok" && "text-ok", tone === "warn" && "text-warn", tone === "bad" && "text-bad")}>{value}</div>
    </div>
  );
}

/** Everything the run recorded about how it reached its result. */
export function Diagnostics({ report }: { report: RunReport }) {
  const [call, setCall] = useState(0);
  const check = report.crosscheck;
  const escalation = report.escalation;
  const failed = report.chunks.filter((c) => c.status === "failed").length;
  const maxStage = Math.max(1, ...report.stages.map((s) => s.ms));
  return (
    <div className="min-h-0 flex-1 overflow-auto">
      <div className="mx-auto max-w-6xl space-y-4 p-5">
        <div className="grid grid-cols-2 gap-3 md:grid-cols-4">
          <Stat label="Recall" value={formatPercent(check.recall)} tone={check.recall === null ? undefined : check.recall >= 0.95 ? "ok" : "warn"} />
          <Stat label="Wall time" value={formatDuration(report.wall_ms)} />
          <Stat label="Cost" value={costOf(report.options.ladder, report.total_cost_usd)} />
          <Stat label="Chunks" value={`${report.chunks.length - failed}/${report.chunks.length}`} tone={failed ? "bad" : "ok"} />
        </div>
        <Card title={`Run ${report.run_id}`} label="Run summary" aside={<Badge tone={report.cancelled || report.incomplete ? "warn" : "ok"}>{report.cancelled ? "cancelled" : report.incomplete ? "incomplete" : "complete"}</Badge>}>
          <Facts
            items={[
              ["Started", formatDate(report.started_at)],
              ["Cost", `${costOf(report.options.ladder, report.total_cost_usd)}${report.cost_complete ? "" : " (incomplete: an unpriced model answered)"}`],
              escalation ? ["Escalation", `${escalation.reason} · ${formatPercent(escalation.flagged_fraction)} flagged · ${escalation.from_model} → ${escalation.to_model}`] : null,
              ["Crosscheck", `${check.matched}/${check.nav_titles} contents titles matched · recall ${formatPercent(check.recall)}`],
              ["Missing", check.missing.join(", ") || "none"],
              ["Phantom", check.phantom.join(", ") || "none"],
            ]}
          />
        </Card>
        <div className="grid gap-4 lg:grid-cols-2">
          <Card title="Stages" label="Stage timings">
            <ul className="space-y-1.5">
              {report.stages.map((stage) => (
                <li key={stage.stage} className="grid grid-cols-[7rem_1fr_4.5rem] items-center gap-3 text-[12px]">
                  <span className="text-muted">{stage.stage.replace("_", " ")}</span>
                  <span className="h-1.5 overflow-hidden rounded-full bg-control">
                    <span className="block h-full rounded-full bg-accent" style={{ width: `${(stage.ms / maxStage) * 100}%` }} />
                  </span>
                  <span className="text-right tabular-nums">{formatDuration(stage.ms)}</span>
                </li>
              ))}
            </ul>
          </Card>
          <Card title="Models" label="Usage by model">
            <Table
              head={["Model", "Calls", "Input", "Output", "Cached", "Cost"]}
              rows={report.usage_by_model.map((u) => ({
                key: u.model,
                cells: [<span key="model" className="font-mono">{u.model}</span>, u.calls, formatCount(u.usage.input_tokens), formatCount(u.usage.output_tokens), formatCount(u.usage.cache_read_input_tokens), u.model.includes("-cli/") ? "subscription" : formatCost(u.cost_usd)],
              }))}
            />
          </Card>
        </div>
        <Card title="Chunks" label="Chunks" aside={<span className="text-[11.5px] text-faint">failed and flagged first</span>}>
          <Table
            head={["Chunk", "Lines", "Status", "Final model", "Attempts", "Second opinion", "Recipes", "Flags"]}
            rows={orderChunks(report.chunks).map((chunk) => ({
              key: chunk.id,
              tone: chunk.status === "failed" ? "bad" : chunk.flags.length ? "warn" : undefined,
              cells: [
                <span key="id" className="font-mono">{chunk.id}</span>,
                `${chunk.start}–${chunk.end}`,
                `${chunk.status}${chunk.cached ? " · cached" : ""}`,
                <span key="model" className="font-mono">{chunk.final_model ?? "—"}</span>,
                chunk.attempts,
                chunk.second_opinion ? `${chunk.second_opinion.model} → ${chunk.second_opinion.chosen} (${chunk.second_opinion.reason})` : "—",
                chunk.recipes,
                chunk.flags.map(describeFlag).join("; ") || "—",
              ],
            }))}
          />
        </Card>
        {report.catalog_id && (
          <Card title="Catalog completeness" label="Catalog completeness">
            <p className="text-muted">Catalog {report.catalog_id}</p>
            {report.catalog_missing.length ? (
              <ul className="list-disc pl-4">
                {report.catalog_missing.map((t) => (
                  <li key={t}>Missing: {t}</li>
                ))}
              </ul>
            ) : (
              <p>All unambiguous catalog recipe titles were found.</p>
            )}
          </Card>
        )}
        <section aria-label="Call log" className="flex h-[420px] flex-col overflow-hidden rounded-panel border border-line bg-evidence">
          <header className="flex min-h-10 items-center justify-between border-b border-line px-4">
            <h2 className="text-[12.5px] font-semibold">Calls</h2>
            <span className="text-[11.5px] text-faint">{plural(report.calls.length, "call")}</span>
          </header>
          <div className={cn(CALL_GRID, "shrink-0 border-b border-line bg-mantle/60 px-3 py-1.5 text-[11px] font-medium text-muted")}>
            {CALL_COLUMNS.map((c) => (
              <span key={c}>{c}</span>
            ))}
          </div>
          <VirtualList label="Model calls" rows={report.calls} selected={call} onSelect={setCall} rowHeight={32} render={(row) => <CallRow call={row} />} empty="No model calls." />
        </section>
        <div className="grid gap-4 lg:grid-cols-2">
          <Card title="Unresolved references" label="Unresolved references">
            {report.unresolved_refs.length ? (
              <ul className="space-y-1">
                {report.unresolved_refs.map((r, i) => (
                  <li key={i}>
                    <span className="font-mono text-muted">{r.item_id}</span> line {r.line}: “{r.text}” <span className="text-faint">(tried {r.attempted.join(", ")})</span>
                  </li>
                ))}
              </ul>
            ) : (
              <p className="text-muted">Every reference resolved.</p>
            )}
          </Card>
          <Card title="ETA trace" label="Estimate accuracy">
            <Table
              head={["Elapsed", "Remaining (low)", "Remaining (high)"]}
              rows={report.eta_trace.map((s, i) => ({ key: String(i), cells: [formatDuration(s.elapsed_ms), formatDuration(s.remaining_low_ms), formatDuration(s.remaining_high_ms)] }))}
            />
          </Card>
        </div>
        <details className="group rounded-panel border border-line bg-evidence">
          <summary className="cursor-pointer px-4 py-2.5 text-[12.5px] font-medium select-none">Raw report</summary>
          <JsonView value={report} className="m-3 mt-0 max-h-[480px]" />
        </details>
      </div>
    </div>
  );
}
