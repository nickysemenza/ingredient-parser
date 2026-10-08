import { Laptop, Play, X } from "lucide-react";
import { useEffect, useState } from "react";
import { toast } from "sonner";
import {
  api,
  can,
  message,
  type BackendOptions,
  type BackendStatus,
  type Estimate,
  type GatewayStatus,
  type OpenedBook,
  type Progress,
} from "../../api";
import { Badge, Button, Facts, Select, Spinner, type Tone } from "../../components/ui";
import { CatalogPanel } from "./Catalog";
import { formatCost, formatCostRange, formatCount, formatDuration, formatDurationRange, formatEta } from "./format";
import { Card, Cover, Notes, plural, RunList, subscription } from "./parts";

const VERDICT: Record<string, Tone> = { cookbook: "ok", not_cookbook: "bad", ambiguous: "warn" };
const BACKENDS = [
  ["gateway", "AI Gateway"],
  ["claude-cli", "Claude Code subscription"],
  ["codex-cli", "Codex subscription"],
] as const;
const MODELS: Record<string, [string, string][]> = {
  "claude-cli": [["opus", "Opus"]],
  "codex-cli": [
    ["gpt-5.6-sol", "Sol"],
    ["gpt-6-astra", "Astra"],
  ],
};

export function ExtractionProgress({ progress, onCancel }: { progress: Progress | null; onCancel: () => void }) {
  const fraction = progress && progress.total ? progress.done / progress.total : 0;
  const sub = progress ? subscription(progress.active_models) : false;
  return (
    <div role="status" aria-label="Extraction progress" className="space-y-3">
      <div className="flex items-center justify-between gap-3">
        <span className="flex items-center gap-2 font-medium">
          <Spinner className="size-3.5" />
          {progress ? `${progress.phase} · ${progress.done}/${progress.total} chunks` : "Starting the extraction…"}
        </span>
        <Button size="sm" onClick={onCancel}>
          <X />
          Cancel
        </Button>
      </div>
      <div className="h-1.5 overflow-hidden rounded-full bg-control">
        <div className="h-full rounded-full bg-accent transition-[width] duration-500" style={{ width: `${fraction * 100}%` }} />
      </div>
      {progress && (
        <div className="grid grid-cols-3 gap-2 text-[11.5px]">
          {(
            [
              ["in flight", progress.in_flight],
              ["failed", progress.failed],
              ["cached", progress.cached],
              ["recipes", progress.recipes_so_far],
              ["elapsed", formatDuration(progress.elapsed_ms)],
              ["remaining", formatEta(progress.eta)],
              ["spent", sub ? "subscription" : formatCost(progress.cost_so_far_usd)],
              ["projected", sub ? "—" : formatCost(progress.eta.projected_cost_usd)],
            ] as const
          ).map(([label, value]) => (
            <div key={label} className="rounded-control bg-mantle px-2.5 py-1.5">
              <div className="text-faint">{label}</div>
              <div className="font-medium tabular-nums">{value}</div>
            </div>
          ))}
          {progress.active_models.length > 0 && <div className="col-span-3 truncate font-mono text-faint">{progress.active_models.join(", ")}</div>}
        </div>
      )}
    </div>
  );
}

function Extract({
  book,
  extracting,
  progress,
  cataloging,
  onExtract,
  onCancel,
}: {
  book: OpenedBook;
  extracting: boolean;
  progress: Progress | null;
  cataloging: boolean;
  onExtract: (options: BackendOptions) => void;
  onCancel: () => void;
}) {
  const [backend, setBackend] = useState("gateway");
  const [model, setModel] = useState("opus");
  const [useCatalog, setUseCatalog] = useState(false);
  const [backends, setBackends] = useState<BackendStatus[]>([]);
  const [estimate, setEstimate] = useState<Estimate | null>(null);
  const [gateway, setGateway] = useState<GatewayStatus | null>(null);
  useEffect(() => {
    api.backends().then(setBackends, (e: unknown) => toast.error(message(e)));
  }, []);
  useEffect(() => {
    let live = true;
    setEstimate(null);
    api.estimate(book.path, { backend, model: backend === "gateway" ? null : model, use_catalog: useCatalog }).then(
      (value) => live && setEstimate(value),
      (e: unknown) => live && toast.error(message(e)),
    );
    api.gateway().then(
      (value) => live && setGateway(value),
      (e: unknown) => live && toast.error(message(e)),
    );
    return () => {
      live = false;
    };
  }, [book.path, backend, model, useCatalog]);
  const status = backends.find((b) => b.backend === backend);
  const unconfigured = backend === "gateway" ? gateway !== null && !gateway.configured : !status?.ready;
  const options = { backend, model: backend === "gateway" ? null : model, use_catalog: useCatalog };
  return (
    <Card title="Extract recipes" label="Extraction">
      <div className="flex flex-wrap items-center gap-3">
        <Select aria-label="Backend" disabled={extracting} value={backend} onChange={(e) => {
          setBackend(e.target.value);
          setModel(MODELS[e.target.value]?.[0][0] ?? "opus");
        }}>
          {BACKENDS.map(([value, label]) => (
            <option key={value} value={value}>
              {label}
            </option>
          ))}
        </Select>
        {backend !== "gateway" && (
          <Select aria-label="Model" disabled={extracting} value={model} onChange={(e) => setModel(e.target.value)}>
            {MODELS[backend].map(([value, label]) => (
              <option key={value} value={value}>
                {label}
              </option>
            ))}
          </Select>
        )}
        <label className="flex items-center gap-2 text-muted">
          <input type="checkbox" className="accent-(--accent)" disabled={extracting} checked={useCatalog} onChange={(e) => setUseCatalog(e.target.checked)} />
          Use saved catalog
        </label>
      </div>
      {backend !== "gateway" && (
        <p className={status?.ready ? "text-muted" : "text-warn"}>
          {status ? (status.error ?? "Ready · subscription usage · stops on reported allowance limits") : "Checking CLI readiness…"}
        </p>
      )}
      {backend === "gateway" && gateway && !gateway.configured && (
        <p className="text-warn">
          Gateway credentials are not configured, so no model can be called. Put them in <span className="font-mono">{gateway.configPath ?? "the gateway configuration file"}</span>
          {gateway.error ? `: ${gateway.error}` : "."}
        </p>
      )}
      {backend === "gateway" && gateway?.configured && (
        <p className="font-mono text-[11.5px] text-muted">
          {[gateway.baseUrl, gateway.ladder.join(" → ")].filter(Boolean).join(" · ")}
        </p>
      )}
      <div aria-label="Extraction estimate" role="region" className="rounded-control border border-line bg-mantle/50 p-3">
        {estimate ? (
          <>
            <Facts
              items={[
                ["Chunks", `${estimate.chunks} (${estimate.cache_hits} cached)`],
                ["Tokens", `${formatCount(estimate.input_tokens)} in · ${formatCount(estimate.output_tokens)} out`],
                ["Calls", `${estimate.calls_low}–${estimate.calls_high}`],
                ["Cost", backend !== "gateway" ? "Subscription usage" : formatCostRange(estimate.cost_usd_low, estimate.cost_usd_high)],
                ["Time", formatDurationRange(estimate.wall_ms_low, estimate.wall_ms_high)],
                ["Ladder", <span key="ladder" className="font-mono text-[11.5px]">{estimate.ladder.join(" → ")}</span>],
                ["Concurrency", estimate.concurrency],
              ]}
            />
            <div className="mt-2">
              <Notes items={estimate.assumptions} />
            </div>
          </>
        ) : (
          <p className="flex items-center gap-2 text-muted">
            <Spinner className="size-3.5" /> Measuring the book…
          </p>
        )}
      </div>
      {extracting ? (
        <ExtractionProgress progress={progress} onCancel={onCancel} />
      ) : (
        <Button variant="primary" size="lg" className="w-full" disabled={unconfigured || cataloging} onClick={() => onExtract(options)}>
          <Play />
          Extract
        </Button>
      )}
    </Card>
  );
}

export function BookPanel({
  book,
  extracting,
  progress,
  cataloging,
  onCataloging,
  onExtract,
  onCancel,
  onOpenRun,
}: {
  book: OpenedBook;
  extracting: boolean;
  progress: Progress | null;
  cataloging: boolean;
  onCataloging: (busy: boolean) => void;
  onExtract: (options: BackendOptions) => void;
  onCancel: () => void;
  onOpenRun: (path: string) => void;
}) {
  const { outline, classified } = book;
  return (
    <div className="min-h-0 flex-1 overflow-auto">
      <div className="mx-auto grid max-w-6xl gap-4 p-5 lg:grid-cols-[minmax(0,1fr)_minmax(0,1fr)]">
        <div className="space-y-4">
          <section aria-label="Book outline" className="flex gap-4 rounded-panel border border-line bg-evidence p-4">
            <Cover path={book.path} className="h-36 w-24 shrink-0" />
            <div className="min-w-0 flex-1 space-y-2">
              <div>
                <h2 className="text-[16px] leading-tight font-semibold">{outline.source.title}</h2>
                <p className="text-[12.5px] text-muted">{outline.source.authors.join(", ") || "Unknown author"}</p>
              </div>
              <Facts
                items={[
                  ["Contents recipes", outline.nav_recipe_titles],
                  ["Lines", formatCount(outline.lines)],
                  ["Chunks", outline.chunks],
                  ["Documents", outline.source.spine_docs],
                  ["Opened in", book.openMs ? formatDuration(book.openMs) : "already open"],
                ]}
              />
            </div>
          </section>
          <Card title="Classification" label="Cookbook classification" aside={<Badge tone={VERDICT[classified.classification]}>{classified.classification.replace("_", " ")}</Badge>}>
            <div className="flex items-center gap-3">
              <div className="h-1.5 flex-1 overflow-hidden rounded-full bg-control">
                <div className="h-full rounded-full bg-accent" style={{ width: `${classified.score * 100}%` }} />
              </div>
              <span className="text-[11.5px] text-muted tabular-nums">score {classified.score.toFixed(2)}</span>
            </div>
            <p className="text-[11.5px] text-muted">
              Decided by {classified.method} · {plural(classified.quantity_lines, "quantity line")} · {plural(classified.ingredient_runs, "ingredient run")} · {plural(classified.solid_runs, "solid run")}
            </p>
            <Notes items={classified.reasons} />
          </Card>
          {outline.chapters.length > 0 && (
            <Card title="Contents" label="Chapters" aside={<span className="text-[11.5px] text-faint">{plural(outline.chapters.length, "chapter")}</span>}>
              <ol className="columns-2 gap-6 text-[12.5px]">
                {outline.chapters.map((chapter, i) => (
                  <li key={i} className="truncate py-0.5">
                    <span className="mr-2 text-faint tabular-nums">{i + 1}</span>
                    {chapter}
                  </li>
                ))}
              </ol>
            </Card>
          )}
        </div>
        <div className="space-y-4">
          {can.extract ? (
            <>
              <Extract book={book} extracting={extracting} progress={progress} cataloging={cataloging} onExtract={onExtract} onCancel={onCancel} />
              <CatalogPanel disabled={extracting} paths={[book.path]} onBusy={onCataloging} />
            </>
          ) : (
            <Card title="Extract recipes" label="Extraction">
              <div className="flex gap-3">
                <Laptop className="mt-0.5 size-5 shrink-0 text-muted" />
                <p className="text-muted">
                  Extraction calls language models, so it runs in the desktop app. Open a saved run of this book here (drop its <span className="font-mono">.json</span>) to review the recipes, photos and diagnostics offline.
                </p>
              </div>
            </Card>
          )}
          <RunList runs={book.runs} label="Runs of this book" onOpen={onOpenRun} empty={can.extract ? "This book has no saved runs yet." : "No run of this book is open. Drop a run file to add one."} />
        </div>
      </div>
    </div>
  );
}
