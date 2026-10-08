import { ChevronRight, ExternalLink, Globe } from "lucide-react";
import { useState } from "react";
import { api, isDesktop, type IngredientRow, type RecipeView } from "../../api";
import { openUrl } from "../../api/platform";
import { param, setParams } from "../../app/router";
import { useReportStatus } from "../../app/status";
import { Amounts, SourceText } from "../../components/ingredient";
import { Split } from "../../components/layout";
import { RichText, ScaleControl } from "../../components/recipe";
import { Badge, Button, EmptyState, Input, JsonView } from "../../components/ui";
import { cn } from "../../lib/cn";
import { RECIPE_URL } from "../../lib/examples";
import { useAction } from "../../lib/query";
import { useStored } from "../../lib/stored";
import { InspectPane } from "./InspectPane";

function Disclosure({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <details className="group rounded-panel border border-line bg-evidence">
      <summary className="flex h-10 cursor-pointer list-none items-center gap-2 px-4 text-[12.5px] font-medium select-none">
        <ChevronRight className="size-3.5 text-muted transition-transform group-open:rotate-90" />
        {title}
      </summary>
      <div className="border-t border-line p-4">{children}</div>
    </details>
  );
}

function IngredientLine({ row, selected, onSelect }: { row: IngredientRow; selected: boolean; onSelect: () => void }) {
  return (
    <li>
      <button
        type="button"
        onClick={onSelect}
        aria-pressed={selected}
        className={cn(
          "flex w-full items-baseline gap-3 rounded-control px-2.5 py-1.5 text-left text-[13px] transition-colors",
          selected ? "bg-selection shadow-[inset_2px_0_0_var(--accent)]" : "hover:bg-hover/60",
        )}
      >
        <Amounts amounts={row.amounts} className="w-28 shrink-0 text-[12px]" />
        <span className="min-w-0 flex-1">
          <SourceText segments={row.segments} className="font-sans" />
        </span>
        {row.reviewReasons.length > 0 && <Badge tone="warn">review</Badge>}
      </button>
    </li>
  );
}

function RecipeArticle({ recipe, onRecipe, selected, onSelect }: { recipe: RecipeView; onRecipe: (r: RecipeView) => void; selected: string | null; onSelect: (input: string) => void }) {
  const scaling = useAction();
  const times = recipe.times && Object.entries(recipe.times).filter(([key, value]) => value && !key.endsWith("minutes"));
  return (
    <article aria-label="Recipe" className="min-h-0 flex-1 overflow-auto">
      <header className="flex gap-5 border-b border-line p-5">
        {recipe.image && <img src={recipe.image} alt="" className="size-28 shrink-0 rounded-panel border border-line object-cover" />}
        <div className="min-w-0 flex-1">
          <h2 className="text-[19px] leading-tight font-semibold tracking-tight">{recipe.title}</h2>
          {recipe.description && <p className="mt-1.5 line-clamp-3 text-[13px] text-muted">{recipe.description}</p>}
          <div className="mt-3 flex flex-wrap items-center gap-1.5">
            {recipe.recipeYield && <Badge>Makes {recipe.recipeYield}</Badge>}
            {recipe.category && <Badge>{recipe.category}</Badge>}
            {times?.map(([key, value]) => (
              <Badge key={key}>
                {key} {value}
              </Badge>
            ))}
            <Button size="sm" variant="ghost" onClick={() => void openUrl(recipe.url)}>
              <ExternalLink />
              Original
            </Button>
          </div>
        </div>
      </header>
      <div className="flex items-center justify-between gap-3 border-b border-line bg-mantle/40 px-5 py-2.5">
        <span className="text-[12px] text-muted">Scale every amount</span>
        <ScaleControl
          value={recipe.factor}
          disabled={scaling.busy}
          onChange={(factor) => void scaling.run(() => api.scaleRecipe(recipe.source, factor), onRecipe)}
        />
      </div>
      <div className="space-y-6 p-5">
        {recipe.sections.map((section, i) => (
          <section key={i} className="grid gap-6 lg:grid-cols-[minmax(0,1fr)_minmax(0,1.2fr)]">
            <div>
              <h3 className="mb-2 text-[11px] font-semibold tracking-wide text-muted uppercase">{section.name ?? "Ingredients"}</h3>
              <ul aria-label={`${section.name ?? "Recipe"} ingredients`} className="-mx-2.5 space-y-0.5">
                {section.ingredients.map((row) => (
                  <IngredientLine key={row.lineNumber} row={row} selected={selected === row.input} onSelect={() => onSelect(row.input)} />
                ))}
              </ul>
            </div>
            {section.instructions.length > 0 && (
              <div>
                <h3 className="mb-2 text-[11px] font-semibold tracking-wide text-muted uppercase">Method</h3>
                <ol className="space-y-3">
                  {section.instructions.map((step, n) => (
                    <li key={n} className="flex gap-3 text-[13.5px]">
                      <span className="mt-0.5 flex size-5 shrink-0 items-center justify-center rounded-full bg-control text-[11px] font-semibold text-muted tabular-nums">
                        {n + 1}
                      </span>
                      <RichText chunks={step} />
                    </li>
                  ))}
                </ol>
              </div>
            )}
          </section>
        ))}
        {(recipe.notes.length > 0 || recipe.equipment.length > 0) && (
          <Disclosure title="Notes and equipment">
            <ul className="list-disc space-y-1 pl-5 text-[13px]">
              {[...recipe.equipment.map((e) => `Equipment: ${e}`), ...recipe.notes].map((note, i) => (
                <li key={i}>{note}</li>
              ))}
            </ul>
          </Disclosure>
        )}
        {recipe.diagnostics.length > 0 && (
          <Disclosure title={`Diagnostics (${recipe.diagnostics.length})`}>
            <ul className="space-y-1 text-[12.5px] text-warn">
              {recipe.diagnostics.map((d, i) => (
                <li key={i}>{d}</li>
              ))}
            </ul>
          </Disclosure>
        )}
        <Disclosure title="Scraped source">
          <JsonView value={recipe.source} className="max-h-96" />
        </Disclosure>
      </div>
    </article>
  );
}

export function Recipe() {
  const [url, setUrl] = useStored("v2:recipe-url", "");
  const [draft, setDraft] = useState(() => param("url") ?? url);
  const [recipe, setRecipe] = useState<RecipeView | null>(null);
  const [selected, setSelected] = useState<string | null>(null);
  const loading = useAction();
  const load = (target: string) => {
    setUrl(target);
    setDraft(target);
    setParams({ url: target });
    void loading.run(() => api.recipe(target), (value) => {
      setRecipe(value);
      setSelected(value.sections[0]?.ingredients[0]?.input ?? null);
    });
  };
  useReportStatus("parser", {
    message: loading.busy ? "Loading recipe…" : recipe ? recipe.title : "Ready",
    detail: recipe ? `${recipe.sections.reduce((n, s) => n + s.ingredients.length, 0)} ingredients` : undefined,
    busy: loading.busy,
  });
  return (
    <>
      <form
        className="flex shrink-0 items-center gap-2 border-b border-line px-4 py-2.5"
        onSubmit={(e) => {
          e.preventDefault();
          if (draft.trim()) load(draft.trim());
        }}
      >
        <Globe className="size-4 shrink-0 text-faint" />
        <label htmlFor="recipe-url" className="sr-only">
          Recipe URL
        </label>
        <Input id="recipe-url" type="url" required value={draft} onChange={(e) => setDraft(e.target.value)} placeholder="Paste a recipe page URL…" />
        <Button type="submit" variant="primary" busy={loading.busy} disabled={!draft.trim()}>
          Load recipe
        </Button>
      </form>
      {recipe ? (
        <Split
          id="recipe"
          initial={58}
          left={<RecipeArticle recipe={recipe} onRecipe={setRecipe} selected={selected} onSelect={setSelected} />}
          right={<InspectPane input={selected} />}
        />
      ) : (
        <EmptyState
          icon={<Globe />}
          title="Parse a recipe from the web"
          actions={
            <Button variant="secondary" onClick={() => load(RECIPE_URL)} disabled={loading.busy}>
              Try an example recipe
            </Button>
          }
        >
          Load any page with recipe markup to see every ingredient parsed, the method annotated, and the whole recipe scaled.
          {!isDesktop && " Pages are fetched through a CORS proxy."}
        </EmptyState>
      )}
    </>
  );
}
