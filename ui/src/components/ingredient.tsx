// How a parsed ingredient looks everywhere: the authored line with its fields
// underlined, the structured result, and the evidence behind it.
import { ArrowRight, Braces, ChevronRight, CircleAlert, Copy, Layers, ListTree, Sparkles, X } from "lucide-react";
import { useMemo, useState, type ReactNode } from "react";
import { toast } from "sonner";
import type { Confidence, IngredientRow, Inspection, Segment, SegmentField, Stages, TraceNode } from "../api";
import { copyText } from "../api/platform";
import { cn } from "../lib/cn";
import { ActionMenu, Badge, IconButton, JsonView, Tabs, type Tone } from "./ui";

const FIELD_CLASS: Record<SegmentField, string> = {
  amount: "field-amount",
  name: "field-name",
  modifier: "field-modifier",
};

/** The authored line, each region underlined in the color of the field it became. */
export function SourceText({ segments, className, wrap = true }: { segments: Segment[]; className?: string; wrap?: boolean }) {
  return (
    <span className={cn("font-mono", wrap ? "whitespace-pre-wrap" : "whitespace-pre", className)}>
      {segments.map((segment, i) =>
        segment.field ? (
          <span key={i} className={FIELD_CLASS[segment.field]} data-field={segment.field}>
            {segment.text}
          </span>
        ) : (
          <span key={i} className="text-faint">
            {segment.text}
          </span>
        ),
      )}
    </span>
  );
}

export function FieldLegend({ segments }: { segments?: Segment[] }) {
  const present = new Set(segments?.map((s) => s.field));
  const fields: [SegmentField, string][] = [
    ["amount", "bg-amount"],
    ["name", "bg-accent"],
    ["modifier", "bg-violet"],
  ];
  return (
    <div className="flex flex-wrap items-center gap-3 text-[11.5px] text-muted">
      {fields.map(([field, color]) => (
        <span key={field} className={cn("inline-flex items-center gap-1.5", segments && !present.has(field) && "opacity-40")}>
          <span className={cn("size-2 rounded-full", color)} />
          {field}
        </span>
      ))}
    </div>
  );
}

const CONFIDENCE: Record<Confidence, Tone> = { high: "ok", medium: "neutral", low: "bad" };

export function ConfidenceBadge({ row }: { row: Pick<IngredientRow, "confidence" | "reviewReasons"> }) {
  const reasons = row.reviewReasons.map((r) => r.message).join("\n");
  return (
    <span className="inline-flex items-center gap-1">
      <Badge tone={CONFIDENCE[row.confidence]}>{row.confidence}</Badge>
      {row.reviewReasons.length > 0 && (
        <span title={reasons} aria-label={`Needs review: ${reasons}`} className="text-warn">
          <CircleAlert className="size-3.5" />
        </span>
      )}
    </span>
  );
}

export function Amounts({ amounts, className }: { amounts: string[]; className?: string }) {
  if (!amounts.length) return <span className="text-faint">—</span>;
  return (
    <span className={cn("flex flex-wrap gap-1", className)}>
      {amounts.map((amount, i) => (
        <span key={i} className="rounded bg-amount/10 px-1.5 py-px font-medium text-amount tabular-nums">
          {amount}
        </span>
      ))}
    </span>
  );
}

const humanize = (value: string) => value.replaceAll("_", " ");

/** The structured result of one line. */
export function ResultSummary({ row }: { row: IngredientRow }) {
  const { ingredient } = row;
  return (
    <div className="space-y-3">
      <div className="flex items-start justify-between gap-3">
        <div className="min-w-0">
          <div className="text-[11px] font-medium tracking-wide text-muted uppercase">Name</div>
          <div className="truncate text-[17px] font-semibold text-name">
            {ingredient.name || <span className="text-faint italic">no name</span>}
          </div>
        </div>
        <ConfidenceBadge row={row} />
      </div>
      <dl className="grid grid-cols-[6.5rem_1fr] gap-x-3 gap-y-2 text-[12.5px]">
        <dt className="text-muted">Amounts</dt>
        <dd>
          <Amounts amounts={row.amounts} />
        </dd>
        <dt className="text-muted">Modifier</dt>
        <dd className={ingredient.modifier ? "text-violet" : "text-faint"}>{ingredient.modifier ?? "—"}</dd>
        <dt className="text-muted">Usage</dt>
        <dd>{ingredient.usage === "normal" ? <span className="text-faint">normal</span> : <Badge tone="accent">{humanize(ingredient.usage)}</Badge>}</dd>
        <dt className="text-muted">Optional</dt>
        <dd className={ingredient.optional ? "text-fg" : "text-faint"}>{ingredient.optional ? "Yes" : "No"}</dd>
      </dl>
      {row.reviewReasons.length > 0 && (
        <ul className="space-y-1.5">
          {row.reviewReasons.map((reason) => (
            <li key={reason.tag} className="flex gap-2 rounded-control border border-warn/30 bg-warn/8 px-2.5 py-2 text-[12px] text-fg">
              <CircleAlert className="mt-px size-3.5 shrink-0 text-warn" />
              <span>
                {reason.message} <span className="font-mono text-[11px] text-muted">{reason.tag}</span>
              </span>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

function Step({ title, fired, children }: { title: string; fired: boolean; children: ReactNode }) {
  return (
    <li className="relative pb-4 pl-6 last:pb-0">
      <span className={cn("absolute top-1 left-0 size-2.5 rounded-full border-2", fired ? "border-accent bg-accent" : "border-line-strong bg-evidence")} />
      <span className="absolute top-4 bottom-0 left-[4.5px] w-px bg-line [li:last-child>&]:hidden" />
      <div className={cn("text-[11px] font-semibold tracking-wide uppercase", fired ? "text-fg" : "text-muted")}>{title}</div>
      <div className="mt-1 space-y-1 text-[12.5px]">{children}</div>
    </li>
  );
}

function Rewrites({ steps, empty }: { steps: Stages["normalize"]; empty: string }) {
  if (!steps.length) return <p className="text-faint">{empty}</p>;
  return (
    <>
      {steps.map((step, i) => (
        <div key={i} className="rounded-control border border-line bg-mantle px-2.5 py-1.5">
          <div className="font-mono text-[11px] text-accent">{step.name}</div>
          <div className="flex flex-wrap items-center gap-1.5 font-mono text-[12px]">
            <span className="text-muted line-through decoration-faint">{step.before}</span>
            <ArrowRight className="size-3 text-faint" />
            <span>{step.after}</span>
          </div>
        </div>
      ))}
    </>
  );
}

/** The pipeline that shaped a line: normalize → recognize → grammar → segment → refine. */
export function StagePipeline({ stages }: { stages: Stages | null }) {
  if (!stages) return <p className="text-muted">No stage report for this line.</p>;
  const matched = stages.recognizers.find((r) => r.output !== null);
  return (
    <ol>
      <Step title="Normalize" fired={stages.normalize.length > 0}>
        <Rewrites steps={stages.normalize} empty="No rewrites fired." />
      </Step>
      <Step title="Recognize" fired={matched !== undefined}>
        <div className="flex flex-wrap gap-1">
          {stages.recognizers.map((r) => (
            <Badge key={r.name} tone={r.output !== null ? "ok" : "neutral"} title={r.output ?? "no match"}>
              <span className="font-mono">{r.name}</span>
              {r.output !== null ? " ✓" : ""}
            </Badge>
          ))}
          {!stages.recognizers.length && <span className="text-faint">No recognizers attempted.</span>}
        </div>
        {matched && <p className="font-mono text-[12px] text-ok">{matched.output}</p>}
      </Step>
      <Step title="Grammar" fired={stages.grammar.outcome === "parsed"}>
        {stages.grammar.outcome === "parsed" ? (
          <p>
            Parsed name <span className="font-mono text-name">“{stages.grammar.name}”</span>
          </p>
        ) : stages.grammar.outcome === "fell_back" ? (
          <p className="text-warn">The grammar failed; the whole line became the name.</p>
        ) : (
          <p className="text-faint">Skipped.</p>
        )}
      </Step>
      <Step title="Segment" fired={stages.segment.length > 0}>
        <Rewrites steps={stages.segment} empty="No clauses to classify." />
      </Step>
      <Step title="Refine" fired={stages.refine.length > 0}>
        <Rewrites steps={stages.refine} empty="No passes changed it." />
      </Step>
      <Step title="Result" fired>
        {stages.result !== null ? (
          <p>
            name <span className="font-mono text-name">“{stages.result}”</span>
          </p>
        ) : (
          <p className="text-warn">Name-only fallback.</p>
        )}
      </Step>
    </ol>
  );
}

function countFailures(node: TraceNode): number {
  return (node.outcome === "failure" ? 1 : 0) + node.children.reduce((n, c) => n + countFailures(c), 0);
}

function prune(node: TraceNode, keepFailures: boolean): TraceNode | null {
  if (!keepFailures && node.outcome === "failure") return null;
  return {
    ...node,
    children: node.children.map((c) => prune(c, keepFailures)).filter((c): c is TraceNode => c !== null),
  };
}

function TreeNode({ node, depth }: { node: TraceNode; depth: number }) {
  const [open, setOpen] = useState(depth < 2);
  const dot = node.outcome === "success" ? "bg-ok" : node.outcome === "failure" ? "bg-bad" : "bg-faint";
  return (
    <li>
      <button
        type="button"
        onClick={() => setOpen(!open)}
        aria-expanded={node.children.length ? open : undefined}
        className="group flex w-full items-start gap-1.5 rounded px-1 py-0.5 text-left hover:bg-hover/60"
      >
        <ChevronRight
          className={cn("mt-0.5 size-3 shrink-0 text-faint transition-transform", open && "rotate-90", !node.children.length && "invisible")}
        />
        <span className={cn("mt-1.5 size-1.5 shrink-0 rounded-full", dot)} />
        <span className="min-w-0">
          <span className="font-mono text-[12px] text-fg">{node.name}</span>
          <span className="ml-2 font-mono text-[11px] text-faint">{node.input}</span>
          {node.detail && <span className={cn("block font-mono text-[11px]", node.outcome === "failure" ? "text-bad/80" : "text-muted")}>{node.detail}</span>}
        </span>
      </button>
      {open && node.children.length > 0 && (
        <ul className="ml-[11px] border-l border-line pl-2">
          {node.children.map((child, i) => (
            <TreeNode key={i} node={child} depth={depth + 1} />
          ))}
        </ul>
      )}
    </li>
  );
}

/** The grammar trace. Failed branches (backtracking) are hidden by default. */
export function TraceTree({ trace }: { trace: TraceNode | null }) {
  const [failures, setFailures] = useState(false);
  const failed = useMemo(() => (trace ? countFailures(trace) : 0), [trace]);
  const shown = useMemo(() => (trace ? prune(trace, failures) : null), [trace, failures]);
  if (!shown) return <p className="text-muted">No trace available.</p>;
  return (
    <div>
      <label className="mb-2 flex items-center gap-2 text-[12px] text-muted">
        <input type="checkbox" checked={failures} onChange={(e) => setFailures(e.target.checked)} className="accent-(--accent)" />
        Show {failed} failed branch{failed === 1 ? "" : "es"}
      </label>
      <ul>
        <TreeNode node={shown} depth={0} />
      </ul>
    </div>
  );
}

type View = "Overview" | "Stages" | "Trace" | "JSON";
const VIEW_ICONS: Record<View, ReactNode> = {
  Overview: <Sparkles />,
  Stages: <Layers />,
  Trace: <ListTree />,
  JSON: <Braces />,
};

async function copy(label: string, text: string) {
  try {
    await copyText(text);
    toast.success(`Copied ${label}`);
  } catch (error) {
    toast.error(`Could not copy: ${String(error)}`);
  }
}

/** Everything the parser recorded for one line. */
export function Inspector({ data, onClose, title = "Ingredient" }: { data: Inspection; onClose?: () => void; title?: string }) {
  const [view, setView] = useState<View>("Overview");
  return (
    <section aria-label="Ingredient inspector" className="flex min-h-0 flex-1 flex-col bg-mantle/40">
      <header className="flex h-11 shrink-0 items-center justify-between gap-2 border-b border-line px-4">
        <h2 className="text-[12.5px] font-semibold">{title}</h2>
        <div className="flex items-center gap-1">
          <ActionMenu
            label="Copy"
            trigger={<Copy />}
            items={[
              { label: "Copy input", onSelect: () => void copy("input", data.row.input) },
              { label: "Copy JSON", onSelect: () => void copy("JSON", JSON.stringify(data.row.ingredient, null, 2)) },
              { label: "Copy stage view", onSelect: () => void copy("stage view", data.stagesText) },
              { label: "Copy Jaeger JSON", onSelect: () => void copy("Jaeger JSON", data.jaegerJson) },
            ]}
          />
          {onClose && (
            <IconButton label="Close inspector" onClick={onClose}>
              <X />
            </IconButton>
          )}
        </div>
      </header>
      <div className="shrink-0 space-y-2 border-b border-line bg-evidence px-4 py-3">
        <p className="text-[14px] leading-relaxed">
          <SourceText segments={data.row.segments} />
        </p>
        <FieldLegend segments={data.row.segments} />
      </div>
      <Tabs
        className="shrink-0 px-4"
        value={view}
        items={["Overview", "Stages", "Trace", "JSON"] as const}
        onChange={setView}
        label="Ingredient evidence"
        render={(item) => (
          <>
            {VIEW_ICONS[item]}
            {item}
          </>
        )}
      />
      <div className="min-h-0 flex-1 overflow-auto p-4">
        {view === "Overview" ? (
          <ResultSummary row={data.row} />
        ) : view === "Stages" ? (
          <StagePipeline stages={data.stages} />
        ) : view === "Trace" ? (
          <TraceTree trace={data.trace} />
        ) : (
          <JsonView value={data.row.ingredient} />
        )}
      </div>
    </section>
  );
}
