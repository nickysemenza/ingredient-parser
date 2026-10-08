import { Image as ImageIcon, Link2, Search } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { api, type Chapter, type ImageRef, type Item, type Recipe, type RecipeRef, type SourceLine, type Step } from "../../api";
import { Split } from "../../components/layout";
import { Badge, Input, Segmented, Spinner, Tip, type Tone } from "../../components/ui";
import { cn } from "../../lib/cn";
import { InspectPane } from "../parser/InspectPane";
import { findItem, flattenChapters, formatAmounts, formatSpan } from "./format";

const KIND: Record<Item["kind"], Tone> = { recipe: "accent", technique: "violet", essay: "neutral" };

/** Chapters and their items, filtered by title: the run's navigation. */
export function BookTree({ chapters, selected, onSelect }: { chapters: Chapter[]; selected: string | null; onSelect: (id: string) => void }) {
  const [filter, setFilter] = useState("");
  const rows = useMemo(() => flattenChapters(chapters, filter), [chapters, filter]);
  const items = rows.flatMap((r) => (r.kind === "item" ? [r.key] : []));
  const names = useMemo(() => new Map(chapters.flatMap((c) => c.items.map((i) => [i.id, i.title] as const))), [chapters]);
  const list = useRef<HTMLDivElement>(null);
  useEffect(() => {
    list.current?.querySelector('[aria-current="true"]')?.scrollIntoView({ block: "nearest" });
  }, [selected]);
  return (
    <section aria-label="Book contents" className="flex min-h-0 flex-1 flex-col bg-mantle/40">
      <div className="relative shrink-0 border-b border-line p-2.5">
        <Search className="pointer-events-none absolute top-1/2 left-5 size-3.5 -translate-y-1/2 text-faint" />
        <Input aria-label="Find item" className="h-7 pl-8 text-[12.5px]" placeholder="Find recipe…" value={filter} onChange={(e) => setFilter(e.target.value)} />
      </div>
      {/* Arrow keys move between the item buttons inside. */}
      {/* oxlint-disable-next-line jsx-a11y/no-static-element-interactions */}
      <div
        ref={list}
        className="min-h-0 flex-1 overflow-auto px-1.5 pb-3 outline-none"
        onKeyDown={(e) => {
          const at = selected ? items.indexOf(selected) : -1;
          const next = e.key === "ArrowDown" ? items[Math.min(items.length - 1, at + 1)] : e.key === "ArrowUp" ? items[Math.max(0, at - 1)] : undefined;
          if (next) {
            e.preventDefault();
            onSelect(next);
          }
        }}
      >
        {rows.map((row) =>
          row.kind === "chapter" ? (
            <h3 key={`chapter-${row.key}`} className="sticky top-0 z-1 flex items-baseline justify-between bg-mantle/95 px-2 pt-3 pb-1 text-[11px] font-semibold tracking-wide text-muted uppercase backdrop-blur">
              <span className="truncate">{row.title}</span>
              <span className="font-normal text-faint tabular-nums">{row.items}</span>
            </h3>
          ) : (
            <button
              key={row.key}
              type="button"
              aria-current={selected === row.key ? "true" : undefined}
              onClick={() => onSelect(row.key)}
              className={cn(
                "flex w-full flex-col gap-0.5 rounded-control px-2 py-1.5 text-left transition-colors",
                selected === row.key ? "bg-evidence shadow-sm ring-1 ring-line" : "hover:bg-hover/60",
              )}
            >
              <span className="flex items-center gap-2">
                <span className="min-w-0 flex-1 truncate text-[12.5px] font-medium">{row.item.title}</span>
                {row.item.kind !== "recipe" && <Badge tone={KIND[row.item.kind]}>{row.item.kind}</Badge>}
              </span>
              <span className="truncate text-[11px] text-faint">
                {row.item.kind === "recipe"
                  ? `${row.counts.ingredients} lines · ${row.counts.steps} steps${row.counts.photos ? ` · ${row.counts.photos} photos` : ""}${row.counts.refs ? ` · ${row.counts.refs} refs` : ""}`
                  : `${row.counts.steps} steps · ${row.counts.photos} photos`}
                {row.item.kind === "recipe" && row.item.variant_of && ` · variation of ${names.get(row.item.variant_of) ?? row.item.variant_of}`}
              </span>
            </button>
          ),
        )}
        {!rows.length && <p className="p-4 text-center text-[12.5px] text-muted">No matching items.</p>}
      </div>
    </section>
  );
}

function RefChip({ reference, title, onJump }: { reference: RecipeRef; title: string | undefined; onJump: (id: string) => void }) {
  return (
    <Tip label={`${reference.kind} reference · matched by ${reference.method}`}>
      <button
        type="button"
        onClick={() => onJump(reference.target_id)}
        className="ml-1.5 inline-flex h-5 items-center gap-1 rounded-full border border-accent/30 bg-accent/8 px-2 align-middle text-[11px] font-medium text-accent hover:bg-accent/15"
      >
        <Link2 className="size-3" />
        {title ?? reference.text}
      </button>
    </Tip>
  );
}

function Photo({ image, source, onNeed }: { image: ImageRef; source: string | undefined; onNeed: (path: string) => void }) {
  useEffect(() => onNeed(image.path), [image.path, onNeed]);
  return (
    <figure className="overflow-hidden rounded-panel border border-line bg-evidence">
      {source ? (
        <img src={source} alt={image.alt ?? ""} className="max-h-[420px] w-full object-cover" />
      ) : (
        <p className="flex h-32 items-center justify-center gap-2 text-[12px] text-faint">
          {source === undefined ? <Spinner className="size-3.5" /> : <ImageIcon className="size-4" />}
          {source === "" ? "Image unavailable" : image.path}
        </p>
      )}
      {image.caption && <figcaption className="px-3 py-2 text-[12px] text-muted">{image.caption}</figcaption>}
    </figure>
  );
}

const confidenceTone: Record<string, Tone> = { high: "ok", medium: "neutral", low: "bad" };

function RecipeBody({ recipe, titleOf, inspecting, onInspect, onJump }: { recipe: Recipe; titleOf: (id: string) => string | undefined; inspecting: string | null; onInspect: (raw: string) => void; onJump: (id: string) => void }) {
  const times = recipe.meta.times;
  const timings = (["active", "total", "prep", "cook"] as const).flatMap((k) => (times?.[k] ? [`${k} ${times[k]}`] : []));
  return (
    <>
      {(recipe.meta.recipe_yield || timings.length > 0 || recipe.meta.category || recipe.meta.page) && (
        <div className="flex flex-wrap gap-1.5">
          {recipe.meta.recipe_yield && <Badge>{recipe.meta.recipe_yield}</Badge>}
          {timings.map((t) => (
            <Badge key={t}>{t}</Badge>
          ))}
          {recipe.meta.category && <Badge>{recipe.meta.category}</Badge>}
          {recipe.meta.page && <Badge>page {recipe.meta.page}</Badge>}
        </div>
      )}
      {recipe.meta.description.map((p, i) => (
        <p key={i} className="text-[13.5px] leading-relaxed text-muted">
          {p}
        </p>
      ))}
      {recipe.meta.equipment.length > 0 && <p className="text-[12.5px] text-muted">Equipment: {recipe.meta.equipment.join(", ")}</p>}
      {recipe.sections.map((section, index) => (
        <section key={section.name ?? index} className="space-y-3">
          {section.name && <h3 className="text-[11px] font-semibold tracking-wide text-muted uppercase">{section.name}</h3>}
          <ul aria-label={`${section.name ?? "Recipe"} ingredients`} className="-mx-2 space-y-0.5">
            {section.ingredients.map((line) => (
              <li key={line.line} className="flex items-center">
                <button
                  type="button"
                  onClick={() => onInspect(line.raw)}
                  aria-pressed={inspecting === line.raw}
                  className={cn(
                    "grid min-w-0 flex-1 grid-cols-[minmax(0,1.3fr)_minmax(0,1fr)_auto] items-baseline gap-3 rounded-control px-2 py-1.5 text-left text-[13px]",
                    inspecting === line.raw ? "bg-selection shadow-[inset_2px_0_0_var(--accent)]" : "hover:bg-hover/60",
                  )}
                >
                  <span className="truncate font-mono text-[12.5px]">{line.raw}</span>
                  <span className="flex min-w-0 items-baseline gap-2 truncate">
                    <span className="truncate font-medium text-name">{line.parsed.name}</span>
                    {line.parsed.amounts.length > 0 && <span className="shrink-0 text-[12px] text-amount tabular-nums">{formatAmounts(line.parsed.amounts)}</span>}
                    {line.parsed.modifier && <span className="truncate text-[12px] text-violet">{line.parsed.modifier}</span>}
                  </span>
                  <Badge tone={confidenceTone[line.confidence]}>{line.confidence}</Badge>
                </button>
                {line.ref && <RefChip reference={line.ref} title={titleOf(line.ref.target_id)} onJump={onJump} />}
              </li>
            ))}
          </ul>
          <Steps steps={section.steps} titleOf={titleOf} onJump={onJump} />
        </section>
      ))}
      {recipe.notes.length > 0 && (
        <section className="space-y-2 rounded-panel border border-line bg-mantle/50 p-4">
          <h3 className="text-[11px] font-semibold tracking-wide text-muted uppercase">Notes</h3>
          {recipe.notes.map((note) => (
            <p key={note.line} className="text-[13px] leading-relaxed">
              {note.label && <strong>{note.label}: </strong>}
              {note.text}
              {note.refs.map((r, n) => (
                <RefChip key={n} reference={r} title={titleOf(r.target_id)} onJump={onJump} />
              ))}
            </p>
          ))}
        </section>
      )}
    </>
  );
}

function Steps({ steps, titleOf, onJump }: { steps: Step[]; titleOf?: (id: string) => string | undefined; onJump?: (id: string) => void }) {
  if (!steps.length) return null;
  return (
    <ol className="space-y-2.5">
      {steps.map((step, n) => (
        <li key={step.line} className="flex gap-3 text-[13.5px] leading-relaxed">
          <span className="mt-0.5 flex size-5 shrink-0 items-center justify-center rounded-full bg-control text-[11px] font-semibold text-muted tabular-nums">{n + 1}</span>
          <span>
            {step.text}
            {onJump && step.refs.map((r, i) => <RefChip key={i} reference={r} title={titleOf?.(r.target_id)} onJump={onJump} />)}
          </span>
        </li>
      ))}
    </ol>
  );
}

/** The book's own lines for an item, so a reviewer can check the extraction
 *  against its source. */
function SourceView({ bookPath, item }: { bookPath: string; item: Item }) {
  const [lines, setLines] = useState<SourceLine[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    let live = true;
    api.source(bookPath, item.span.start, item.span.end).then(
      (value) => live && setLines(value),
      (e: unknown) => live && setError(String(e)),
    );
    return () => {
      live = false;
    };
  }, [bookPath, item.span.start, item.span.end]);
  if (error) return <p className="text-bad">{error}</p>;
  if (!lines) return <Spinner />;
  return (
    <pre aria-label="Source lines" className="overflow-auto rounded-panel border border-line bg-evidence p-3 font-mono text-[12px] leading-relaxed whitespace-pre-wrap">
      {lines.map((line) => (
        <div key={line.line} className="flex">
          <span className="mr-4 w-12 shrink-0 text-right text-faint select-none tabular-nums">{line.line}</span>
          <span>{line.text}</span>
        </div>
      ))}
    </pre>
  );
}

/** The selected item, and the ingredient inspector it can open. */
export function ItemView({ item, chapters, bookPath, onJump }: { item: Item; chapters: Chapter[]; bookPath: string | null; onJump: (id: string) => void }) {
  const [photos, setPhotos] = useState<Record<string, string>>({});
  const [inspecting, setInspecting] = useState<string | null>(null);
  const [view, setView] = useState<"Extracted" | "Source">("Extracted");
  const requested = useRef(new Set<string>());
  const titleOf = useCallback((id: string) => findItem(chapters, id)?.title, [chapters]);
  const onNeedPhoto = useCallback(
    (path: string) => {
      if (requested.current.has(path)) return;
      requested.current.add(path);
      if (!bookPath) return setPhotos((was) => ({ ...was, [path]: "" }));
      api.image(bookPath, path).then(
        (image) => setPhotos((was) => ({ ...was, [path]: image.dataUrl })),
        () => setPhotos((was) => ({ ...was, [path]: "" })),
      );
    },
    [bookPath],
  );
  let body: ReactNode;
  if (view === "Source" && bookPath) body = <SourceView bookPath={bookPath} item={item} />;
  else if (item.kind === "recipe") body = <RecipeBody recipe={item} titleOf={titleOf} inspecting={inspecting} onInspect={setInspecting} onJump={onJump} />;
  else
    body = (
      <>
        {(item.kind === "technique" ? item.description : item.text).map((p, i) => (
          <p key={i} className="text-[13.5px] leading-relaxed">
            {p}
          </p>
        ))}
        {item.kind === "technique" && <Steps steps={item.steps} />}
      </>
    );
  const article = (
    <article aria-label={item.title} className="min-h-0 flex-1 overflow-auto">
      <header className="sticky top-0 z-1 flex items-start justify-between gap-4 border-b border-line bg-base/95 px-6 py-3.5 backdrop-blur">
        <div className="min-w-0">
          <h2 className="text-[18px] leading-tight font-semibold tracking-tight">{item.title}</h2>
          <p className="mt-1 truncate text-[11.5px] text-faint">
            {item.kind} · {item.id}
            {item.name !== item.title ? ` · ${item.name}` : ""} · {formatSpan(item.span)}
          </p>
        </div>
        {bookPath && <Segmented size="sm" label="Item view" value={view} items={["Extracted", "Source"] as const} onChange={setView} />}
      </header>
      <div className="mx-auto max-w-3xl space-y-5 px-6 py-5">
        {body}
        {view === "Extracted" && item.photos.map((image) => <Photo key={image.path} image={image} source={photos[image.path]} onNeed={onNeedPhoto} />)}
        {!bookPath && item.photos.length > 0 && <p className="text-[12px] text-faint">Open this run's EPUB to load its photos and source.</p>}
      </div>
    </article>
  );
  return (
    <Split
      id="run-item"
      initial={62}
      left={article}
      right={
        inspecting ? (
          <InspectPane input={inspecting} onClose={() => setInspecting(null)} />
        ) : null
      }
    />
  );
}

