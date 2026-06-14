import { useEffect, useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { useQueryState, parseAsFloat } from "nuqs";
import { wasm, ScrapedRecipe } from "../wasm";
import { Spinner } from "../Spinner";
import { CORS_PROXY, DEFAULT_SCRAPE_URL, SCALE_OPTIONS } from "../config";
import {
  fmtAmount,
  formatRichText,
  safeParseRichText,
  scaleAmount,
} from "../lib/format";
import { FOCUS_RING } from "./ui";

export const Scraper: React.FC = () => {
  const [url, setUrl] = useQueryState("url", { defaultValue: DEFAULT_SCRAPE_URL });
  // parseAsFloat coerces invalid `?scale=` values to the default; clearOnDefault
  // (nuqs v2 default) drops `scale=1` from the URL, matching the old null write.
  const [scaleFactor, setScaleFactor] = useQueryState(
    "scale",
    parseAsFloat.withDefault(1)
  );

  // Debounce the URL so typing doesn't key a fetch per keystroke; the
  // debounced value drives the query, which handles abort + caching.
  const [debouncedUrl, setDebouncedUrl] = useState(url);
  useEffect(() => {
    const timer = setTimeout(() => setDebouncedUrl(url), 500);
    return () => clearTimeout(timer);
  }, [url]);

  const {
    data: scrapedRecipe,
    isFetching: loading,
    error,
  } = useQuery<ScrapedRecipe>({
    queryKey: ["scrape", debouncedUrl],
    enabled: !!debouncedUrl,
    queryFn: async ({ signal }) => {
      const res = await fetch(CORS_PROXY + encodeURIComponent(debouncedUrl), {
        signal,
      });
      if (!res.ok) throw new Error(`Fetch failed (HTTP ${res.status})`);
      const body = await res.text();
      return wasm.scrape(body, debouncedUrl);
    },
  });
  // wasm.scrape rejections are raw strings (Result<_, String>), not Errors —
  // `(error as Error).message` would render "undefined".
  const errorMessage = error
    ? `Couldn't scrape this page: ${
        error instanceof Error ? error.message : String(error)
      }`
    : null;

  const parsedIngredients = useMemo(
    () =>
      scrapedRecipe
        ? scrapedRecipe.sections
            .flatMap((s) => s.ingredients)
            .map((i) => wasm.parse_ingredient(i))
        : [],
    [scrapedRecipe]
  );
  const ingredientNames = useMemo(
    () => parsedIngredients.map((p) => p.name),
    [parsedIngredients]
  );

  return (
    <div className="space-y-8">
      <div className="relative">
        <input
          type="text"
          placeholder="Enter a recipe URL (e.g. from NYTimes Cooking)"
          className={`w-full rounded-xl border border-zinc-300 bg-white px-5 py-4 pr-12 text-lg transition ${FOCUS_RING}`}
          value={url}
          onChange={(e) => setUrl(e.target.value)}
        />
        {loading && (
          <div className="absolute top-1/2 right-4 -translate-y-1/2 text-accent-600">
            <Spinner />
          </div>
        )}
      </div>

      {errorMessage && (
        <div className="rounded-xl border border-red-200 bg-red-50 px-4 py-3 text-sm text-red-700">
          {errorMessage}
        </div>
      )}

      {scrapedRecipe && (
        <>
          <div className="flex flex-col gap-6 lg:flex-row">
            <div className="flex-1 rounded-xl border border-zinc-100 bg-zinc-50 p-6">
              <h3 className="text-2xl font-bold text-zinc-900">
                {scrapedRecipe.name}
              </h3>
              {scrapedRecipe.category && (
                <p className="mt-1 text-sm italic text-zinc-500">
                  {scrapedRecipe.category}
                </p>
              )}
              {scrapedRecipe.times && (
                <div className="mt-2 flex flex-wrap gap-3 text-sm text-zinc-600">
                  {(
                    [
                      ["active", scrapedRecipe.times.active],
                      ["total", scrapedRecipe.times.total],
                      ["prep", scrapedRecipe.times.prep],
                      ["cook", scrapedRecipe.times.cook],
                    ] as const
                  )
                    .filter(([, value]) => value)
                    .map(([label, value]) => (
                      <span key={label}>
                        ⏱ {label}: {value}
                      </span>
                    ))}
                </div>
              )}
              {scrapedRecipe.description && (
                <p className="mt-2 italic text-zinc-700">
                  {scrapedRecipe.description}
                </p>
              )}
              {scrapedRecipe.equipment && scrapedRecipe.equipment.length > 0 && (
                <ul className="mt-2 text-sm text-zinc-600">
                  {scrapedRecipe.equipment.map((e, i) => (
                    <li key={i}>🔧 {e}</li>
                  ))}
                </ul>
              )}
              {scrapedRecipe.notes && scrapedRecipe.notes.length > 0 && (
                <ul className="mt-2 text-sm text-zinc-500">
                  {scrapedRecipe.notes.map((n, i) => (
                    <li key={i}>📝 {n}</li>
                  ))}
                </ul>
              )}

              <div className="mt-4 flex items-center gap-2">
                <span className="text-sm font-medium text-zinc-600">Scale:</span>
                {SCALE_OPTIONS.map((scale) => (
                  <button
                    key={scale}
                    onClick={() => setScaleFactor(scale)}
                    className={`rounded-lg px-3 py-1 text-sm font-medium transition ${
                      scaleFactor === scale
                        ? "bg-accent-600 text-white shadow-sm"
                        : "border border-zinc-300 bg-white text-zinc-700 hover:bg-zinc-100"
                    }`}
                  >
                    {scale === 0.5 ? "½" : scale}x
                  </button>
                ))}
                <input
                  type="number"
                  min="0.1"
                  step="0.1"
                  value={scaleFactor}
                  onChange={(e) =>
                    setScaleFactor(parseFloat(e.target.value) || 1)
                  }
                  className={`w-16 rounded-lg border border-zinc-300 px-2 py-1 text-center text-sm ${FOCUS_RING}`}
                />
              </div>
            </div>

            {scrapedRecipe.image && (
              <div className="lg:w-1/3">
                <img
                  className="h-64 w-full rounded-xl object-cover shadow-sm lg:h-full"
                  src={scrapedRecipe.image}
                  alt={scrapedRecipe.name}
                />
              </div>
            )}
          </div>

          <div className="grid gap-8 md:grid-cols-2">
            <div className="rounded-2xl border border-zinc-100 bg-white p-6">
              <h4 className="mb-5 text-xl font-bold text-zinc-900">
                Ingredients
              </h4>
              <div className="space-y-2.5">
                {parsedIngredients.map((p, index) => (
                  <div
                    key={index}
                    className="flex items-center justify-between gap-4 rounded-xl bg-zinc-50 px-4 py-3 transition hover:bg-zinc-100"
                  >
                    <div className="flex-1">
                      <div className="font-semibold text-zinc-800 underline decoration-accent-400 decoration-2 underline-offset-2">
                        {p.name}
                      </div>
                      {p.modifier && (
                        <div className="mt-1 text-sm italic text-zinc-500">
                          {p.modifier}
                        </div>
                      )}
                    </div>
                    <div className="text-right font-medium text-accent-700">
                      {p.amounts
                        .filter((a) => a.unit !== "$" && a.unit !== "kcal")
                        .map((a) => scaleAmount(a, scaleFactor))
                        .map((a) => fmtAmount(a))
                        .join(" / ")}
                    </div>
                  </div>
                ))}
              </div>
            </div>

            <div className="rounded-2xl border border-zinc-100 bg-white p-6">
              <h4 className="mb-5 text-xl font-bold text-zinc-900">
                Instructions
              </h4>
              <ol className="space-y-3">
                {scrapedRecipe.sections
                  .flatMap((s) => s.instructions)
                  .map((instruction, index) => (
                    <li
                      key={index}
                      className="flex items-start gap-4 rounded-xl bg-zinc-50 p-4 transition hover:bg-zinc-100"
                    >
                      <div className="flex h-8 w-8 flex-shrink-0 items-center justify-center rounded-full bg-accent-600 text-sm font-bold text-white">
                        {index + 1}
                      </div>
                      <div className="flex-1 leading-relaxed text-zinc-700">
                        {formatRichText(
                          safeParseRichText(instruction, ingredientNames)
                        )}
                      </div>
                    </li>
                  ))}
              </ol>
            </div>
          </div>
        </>
      )}
    </div>
  );
};
