import { useState } from "react";
import type { CallOutcome, CallRecord, RunReport } from "../bridge";
import { JsonView, VirtualList } from "../components";
import { Facts, Table } from "./parts";
import {
  describeFlag,
  formatCost,
  formatCount,
  formatDate,
  formatDuration,
  formatPercent,
  orderChunks,
} from "./format";

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

function CallRow({ call }: { call: CallRecord }) {
  return (
    <>
      <span>{call.seq}</span>
      <span>{call.chunk_id}</span>
      <span>{call.model}</span>
      <span>{call.purpose}</span>
      <span>#{call.attempt}</span>
      <span>{formatDuration(call.latency_ms)}</span>
      <span>{call.status ?? "—"}</span>
      <span>{call.cached ? "cached" : "live"}</span>
      <span>{formatCost(call.cost_usd)}</span>
      <span
        className={call.outcome.outcome === "ok" ? "success" : "error-text"}
      >
        {outcomeText(call.outcome)}
        {call.truncated ? " · truncated" : ""}
      </span>
    </>
  );
}

const CALL_COLUMNS = [
  "Seq",
  "Chunk",
  "Model",
  "Purpose",
  "Attempt",
  "Latency",
  "Status",
  "Cache",
  "Cost",
  "Outcome",
];

/** Everything the run recorded about how it reached its result. */
export function Diagnostics({ report }: { report: RunReport }) {
  const [call, setCall] = useState(0);
  const check = report.crosscheck;
  const escalation = report.escalation;
  return (
    <div className="diagnostics scroll">
      <section className="card" aria-label="Run summary">
        <h2>Run {report.run_id}</h2>
        <Facts
          items={[
            ["started", formatDate(report.started_at)],
            ["wall time", formatDuration(report.wall_ms)],
            [
              "total cost",
              `${formatCost(report.total_cost_usd)}${report.cost_complete ? "" : " (incomplete: an unpriced model answered)"}`,
            ],
            [
              "state",
              report.cancelled
                ? "cancelled"
                : report.incomplete
                  ? "incomplete"
                  : "complete",
            ],
            escalation
              ? [
                  "escalation",
                  `${escalation.reason} · ${formatPercent(escalation.flagged_fraction)} flagged · ${escalation.from_model} → ${escalation.to_model}`,
                ]
              : null,
            [
              "crosscheck",
              `${check.matched}/${check.nav_titles} contents titles matched · recall ${formatPercent(check.recall)}`,
            ],
            ["missing", check.missing.join(", ") || "none"],
            ["phantom", check.phantom.join(", ") || "none"],
          ]}
        />
      </section>
      <section className="card" aria-label="Stage timings">
        <h2>Stages</h2>
        <Table
          head={["Stage", "Time"]}
          rows={report.stages.map((stage) => ({
            key: stage.stage,
            cells: [stage.stage, formatDuration(stage.ms)],
          }))}
        />
      </section>
      <section className="card" aria-label="Usage by model">
        <h2>Models</h2>
        <Table
          head={["Model", "Calls", "Input", "Output", "Cached read", "Cost"]}
          rows={report.usage_by_model.map((usage) => ({
            key: usage.model,
            cells: [
              usage.model,
              usage.calls,
              formatCount(usage.usage.input_tokens),
              formatCount(usage.usage.output_tokens),
              formatCount(usage.usage.cache_read_input_tokens),
              formatCost(usage.cost_usd),
            ],
          }))}
        />
      </section>
      <section className="card" aria-label="Chunks">
        <h2>Chunks</h2>
        <Table
          head={[
            "Chunk",
            "Lines",
            "Status",
            "Final model",
            "Attempts",
            "Second opinion",
            "Recipes",
            "Flags",
          ]}
          rows={orderChunks(report.chunks).map((chunk) => ({
            key: chunk.id,
            className: chunk.status,
            cells: [
              chunk.id,
              `${chunk.start}–${chunk.end}`,
              `${chunk.status}${chunk.cached ? " · cached" : ""}`,
              chunk.final_model ?? "—",
              chunk.attempts,
              chunk.second_opinion
                ? `${chunk.second_opinion.model} → ${chunk.second_opinion.chosen} (${chunk.second_opinion.reason})`
                : "—",
              chunk.recipes,
              chunk.flags.map(describeFlag).join("; ") || "—",
            ],
          }))}
        />
      </section>
      <section className="card call-log" aria-label="Call log">
        <h2>Calls</h2>
        <div className="call-header">
          {CALL_COLUMNS.map((label) => (
            <span key={label}>{label}</span>
          ))}
        </div>
        <VirtualList
          label="Model calls"
          rows={report.calls}
          selected={call}
          onSelect={setCall}
          render={(row) => <CallRow call={row} />}
        />
      </section>
      <section className="card" aria-label="Unresolved references">
        <h2>Unresolved references</h2>
        {report.unresolved_refs.length ? (
          <ul className="reasons">
            {report.unresolved_refs.map((reference, index) => (
              <li key={index}>
                {reference.item_id} line {reference.line}: “{reference.text}”
                (tried {reference.attempted.join(", ")})
              </li>
            ))}
          </ul>
        ) : (
          <p className="caption">Every reference resolved.</p>
        )}
      </section>
      <section className="card" aria-label="Estimate accuracy">
        <h2>ETA trace</h2>
        <Table
          head={["Elapsed", "Remaining (low)", "Remaining (high)"]}
          rows={report.eta_trace.map((sample, index) => ({
            key: String(index),
            cells: [
              formatDuration(sample.elapsed_ms),
              formatDuration(sample.remaining_low_ms),
              formatDuration(sample.remaining_high_ms),
            ],
          }))}
        />
      </section>
      <details className="card">
        <summary>Raw report</summary>
        <JsonView value={report} />
      </details>
    </div>
  );
}
