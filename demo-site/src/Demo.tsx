import { useCallback, useEffect, useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { RichItem, wasm, ScrapedRecipe, Measure } from "./wasm";
import { Spinner } from "./Spinner";

/* ── URL state helpers ─────────────────────────────────────────
   Inputs are persisted to the query string so demo links are
   shareable. Each writer reads the current params first, so the
   independent effects never clobber each other's keys. */
const getUrlParam = (key: string): string | null =>
  new URLSearchParams(window.location.search).get(key);

const setUrlParam = (key: string, value: string | null) => {
  const params = new URLSearchParams(window.location.search);
  if (value === null || value === "") {
    params.delete(key);
  } else {
    params.set(key, value);
  }
  const qs = params.toString();
  window.history.replaceState(null, "", qs ? `?${qs}` : window.location.pathname);
};

/* Persist a value to the query string, debounced: a replaceState per
   keystroke trips Safari's 100-calls-per-30s rate limit (SecurityError). */
const useUrlParamSync = (key: string, value: string | null) => {
  useEffect(() => {
    const timer = setTimeout(() => setUrlParam(key, value), 500);
    return () => clearTimeout(timer);
  }, [key, value]);
};

const CORS_PROXY =
  import.meta.env.VITE_CORS_PROXY ?? "https://cors.nicky.workers.dev/?target=";

const EXAMPLE_INGREDIENTS = [
  "2 cups all-purpose flour, sifted",
  "1/2 cup butter, softened",
  "3 large eggs, beaten",
  "1 tsp vanilla extract",
  "2 tbsp olive oil, extra virgin",
];

// WASM is guaranteed loaded before this tree mounts (see main.tsx), so every
// call here is synchronous and never guards on the module being ready.
const fmtAmount = (a: Measure): string => {
  try {
    return wasm.format_amount(a);
  } catch {
    return `${a.value} ${a.unit}`.trim();
  }
};

// wasm.parse_rich_text throws (a raw string) on unparseable input; falling
// back to a single Text chunk keeps a render-path call from unmounting the
// page (there's no error boundary above these sections).
const safeParseRichText = (text: string, names: string[]): RichItem[] => {
  try {
    return wasm.parse_rich_text(text, names);
  } catch {
    return [{ kind: "Text", value: text }];
  }
};

export const Demo: React.FC = () => {
  const [text, setText] = useState(
    () => getUrlParam("i") ?? "1 cup / 120 grams flour, sifted"
  );
  const [richText, setRichText] = useState(
    () =>
      getUrlParam("rt") ??
      "Add 1/2 cup / 236 grams water to the bowl with the salt and mix."
  );

  useUrlParamSync("i", text);
  useUrlParamSync("rt", richText);

  const ingredientNames = useMemo(() => ["flour", "water", "salt"], []);

  const parsed = useMemo(
    () => (text ? wasm.parse_ingredient(text) : undefined),
    [text]
  );
  const parsedRich = useMemo(
    () =>
      richText ? safeParseRichText(richText, ingredientNames) : undefined,
    [richText, ingredientNames]
  );

  return (
    <div className="min-h-screen bg-white text-zinc-900">
      <Nav />

      {/* Hero + live parser */}
      <section className="relative overflow-hidden border-b border-zinc-100 bg-gradient-to-b from-accent-50 to-white">
        <div className="mx-auto max-w-4xl px-6 py-16">
          <div className="mb-10 text-center">
            <h1 className="text-4xl font-extrabold tracking-tight text-zinc-900 md:text-5xl">
              ingredient-parser
            </h1>
            <p className="mx-auto mt-4 max-w-xl text-lg leading-relaxed text-zinc-600">
              Turn freeform recipe ingredients into structured data —{" "}
              <span className="font-semibold text-accent-700">
                Rust + WebAssembly
              </span>
              , right in your browser.
            </p>
          </div>

          <div className="animate-fade-in-up mx-auto max-w-xl rounded-2xl border border-zinc-200 bg-white p-6 shadow-sm">
            <label className="mb-2 block text-sm font-medium text-zinc-500">
              Ingredient line
            </label>
            <input
              type="text"
              placeholder="e.g. 2 cups flour, sifted"
              className="w-full rounded-lg border border-zinc-300 bg-white px-4 py-3 text-base transition focus:border-accent-400 focus:ring-2 focus:ring-accent-200 focus:outline-none"
              value={text}
              onChange={(e) => setText(e.target.value)}
            />

            <div className="mt-3 flex flex-wrap gap-1.5">
              {EXAMPLE_INGREDIENTS.map((ingredient) => (
                <button
                  key={ingredient}
                  onClick={() => setText(ingredient)}
                  className="rounded-md border border-zinc-200 bg-zinc-50 px-2.5 py-1 text-xs text-zinc-600 transition hover:border-accent-300 hover:bg-accent-50 hover:text-accent-700"
                >
                  {ingredient}
                </button>
              ))}
            </div>

            <div className="mt-5 border-t border-zinc-100 pt-5">
              <IngredientResult parsed={parsed} />
            </div>
          </div>
        </div>
      </section>

      {/* Recipe scraper */}
      <section className="border-b border-zinc-100 bg-zinc-50">
        <div className="mx-auto max-w-5xl px-6 py-16">
          <SectionHeader
            icon={<IconGlobe />}
            title="Recipe scraper"
            subtitle="Extract structured ingredients and instructions from any recipe URL."
          />
          <div className="mt-8 rounded-2xl border border-zinc-200 bg-white p-6 shadow-sm">
            <Scraper />
          </div>
        </div>
      </section>

      {/* Rich text parser */}
      <section className="bg-white">
        <div className="mx-auto max-w-5xl px-6 py-16">
          <SectionHeader
            icon={<IconText />}
            title="Rich text parser"
            subtitle="Detect ingredient names and amounts inside freeform instructions."
          />

          <div className="mt-4 flex flex-wrap items-center justify-center gap-2 text-sm text-zinc-500">
            <span>Known ingredients:</span>
            {ingredientNames.map((n) => (
              <span
                key={n}
                className="rounded-md border border-zinc-200 bg-zinc-50 px-2 py-0.5 font-mono text-xs text-zinc-600"
              >
                {n}
              </span>
            ))}
          </div>

          <div className="mt-8 rounded-2xl border border-zinc-200 bg-white p-6 shadow-sm">
            <input
              type="text"
              placeholder="Enter recipe instructions…"
              className="w-full rounded-lg border border-zinc-300 bg-white px-4 py-3 text-base transition focus:border-accent-400 focus:ring-2 focus:ring-accent-200 focus:outline-none"
              value={richText}
              onChange={(e) => setRichText(e.target.value)}
            />
            <div className="mt-4 min-h-[72px] rounded-lg border border-zinc-100 bg-zinc-50 p-4 text-base leading-relaxed">
              {parsedRich && formatRichText(parsedRich)}
            </div>
          </div>
        </div>
      </section>

      <Footer />
    </div>
  );
};

const Nav: React.FC = () => {
  const [copied, setCopied] = useState(false);
  const copyLink = useCallback(async () => {
    try {
      await navigator.clipboard.writeText(window.location.href);
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch {
      // clipboard unavailable; no-op
    }
  }, []);

  return (
    <nav className="sticky top-0 z-20 border-b border-zinc-100 bg-white/80 backdrop-blur">
      <div className="mx-auto flex max-w-5xl items-center justify-between px-6 py-3">
        <span className="font-mono text-sm font-semibold tracking-tight text-zinc-900">
          ingredient-parser
        </span>
        <div className="flex items-center gap-1">
          <button
            onClick={copyLink}
            className="inline-flex items-center gap-1.5 rounded-lg px-3 py-1.5 text-sm text-zinc-600 transition hover:bg-zinc-100"
          >
            <IconLink />
            {copied ? "Copied!" : "Copy link"}
          </button>
          <a
            href="https://github.com/nickysemenza/ingredient-parser"
            target="_blank"
            rel="noreferrer"
            className="inline-flex items-center gap-1.5 rounded-lg px-3 py-1.5 text-sm text-zinc-600 transition hover:bg-zinc-100"
          >
            <IconGitHub />
            GitHub
          </a>
        </div>
      </div>
    </nav>
  );
};

const SectionHeader: React.FC<{
  icon: React.ReactNode;
  title: string;
  subtitle: string;
}> = ({ icon, title, subtitle }) => (
  <div className="text-center">
    <div className="mb-3 inline-flex h-11 w-11 items-center justify-center rounded-xl bg-accent-100 text-accent-700">
      {icon}
    </div>
    <h2 className="text-2xl font-bold tracking-tight text-zinc-900 md:text-3xl">
      {title}
    </h2>
    <p className="mx-auto mt-2 max-w-2xl text-zinc-600">{subtitle}</p>
  </div>
);

const IngredientResult: React.FC<{
  parsed: ReturnType<typeof wasm.parse_ingredient> | undefined;
}> = ({ parsed }) => {
  if (!parsed) {
    return (
      <p className="text-sm text-zinc-400">
        Type an ingredient above to see it parsed.
      </p>
    );
  }
  const amounts = parsed.amounts ?? [];
  return (
    <div>
      <h4 className="text-lg font-semibold text-zinc-900">
        {parsed.name || <span className="italic text-zinc-400">no name</span>}
      </h4>

      {amounts.length > 0 && (
        <div className="mt-2.5 flex flex-wrap gap-1.5">
          {amounts.map((a, i) => (
            <span
              key={i}
              className="rounded-md bg-accent-50 px-2.5 py-1 text-sm font-medium text-accent-700"
            >
              {fmtAmount(a)}
            </span>
          ))}
        </div>
      )}

      {parsed.modifier && (
        <p className="mt-2.5 text-sm italic text-zinc-500">{parsed.modifier}</p>
      )}

      <details className="mt-4">
        <summary className="cursor-pointer font-mono text-xs text-zinc-400 transition select-none hover:text-zinc-600">
          raw JSON
        </summary>
        <pre className="mt-2 overflow-auto rounded-lg bg-zinc-900 p-3 font-mono text-xs leading-relaxed text-zinc-100">
          {JSON.stringify(parsed, null, 2)}
        </pre>
      </details>
    </div>
  );
};

const scaleAmount = (amount: Measure, scale: number): Measure => ({
  ...amount,
  value: amount.value * scale,
  upper_value: amount.upper_value ? amount.upper_value * scale : undefined,
});

const Scraper: React.FC = () => {
  const [url, setURL] = useState(
    () =>
      getUrlParam("url") ??
      "https://cooking.nytimes.com/recipes/1020830-caramelized-shallot-pasta"
  );
  const [scaleFactor, setScaleFactor] = useState(() => {
    const fromUrl = parseFloat(getUrlParam("scale") ?? "");
    return Number.isFinite(fromUrl) && fromUrl > 0 ? fromUrl : 1.0;
  });

  useUrlParamSync("url", url);
  useUrlParamSync("scale", scaleFactor === 1 ? null : String(scaleFactor));

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
          className="w-full rounded-xl border border-zinc-300 bg-white px-5 py-4 pr-12 text-lg transition focus:border-accent-400 focus:ring-2 focus:ring-accent-200 focus:outline-none"
          value={url}
          onChange={(e) => setURL(e.target.value)}
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
                {[0.5, 1, 2, 3].map((scale) => (
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
                  className="w-16 rounded-lg border border-zinc-300 px-2 py-1 text-center text-sm focus:border-accent-400 focus:ring-2 focus:ring-accent-200 focus:outline-none"
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

const Footer: React.FC = () => (
  <footer className="bg-zinc-900 text-white">
    <div className="mx-auto flex max-w-5xl flex-col items-center justify-between gap-6 px-6 py-10 md:flex-row">
      <div>
        <h3 className="font-mono text-xl font-bold">ingredient-parser</h3>
        <p className="mt-1 text-sm text-zinc-400">
          © {new Date().getFullYear()} Nicky Semenza
        </p>
      </div>
      <div className="flex flex-wrap items-center gap-3">
        <a
          href="https://github.com/nickysemenza/ingredient-parser"
          target="_blank"
          rel="noreferrer"
          className="rounded-lg bg-white/10 px-4 py-2 transition hover:bg-white/20"
        >
          <img
            alt="GitHub Repo stars"
            src="https://img.shields.io/github/stars/nickysemenza/ingredient-parser?style=social"
            className="invert"
          />
        </a>
        <a
          href="https://crates.io/crates/ingredient"
          target="_blank"
          rel="noreferrer"
          className="rounded-lg bg-white/10 px-4 py-2 transition hover:bg-white/20"
        >
          <img
            alt="Crates.io"
            src="https://img.shields.io/crates/v/ingredient?style=flat&color=white"
          />
        </a>
        <a
          href="https://docs.rs/ingredient"
          target="_blank"
          rel="noreferrer"
          className="rounded-lg bg-white/10 px-4 py-2 transition hover:bg-white/20"
        >
          <img alt="docs.rs" src="https://docs.rs/ingredient/badge.svg" />
        </a>
      </div>
    </div>
  </footer>
);

const formatRichText = (text: RichItem[]) => {
  return text.map((t, index) => {
    switch (t.kind) {
      case "Text":
        return t.value;
      case "Ing":
        return (
          <span
            className="mx-0.5 inline rounded-md border border-accent-200 bg-accent-100 px-1.5 py-0.5 font-semibold text-accent-800"
            key={`ing-${index}`}
          >
            {t.value}
          </span>
        );
      case "Measure": {
        const val = t.value[t.value.length - 1];
        if (!val) {
          return null;
        }
        const displayAmount = val.unit === "whole" ? { ...val, unit: "" } : val;
        return (
          <span
            className="mx-0.5 inline rounded-md border border-blue-200 bg-blue-100 px-1.5 py-0.5 font-semibold text-blue-800"
            key={`measure-${index}`}
          >
            {fmtAmount(displayAmount)}
          </span>
        );
      }
      default:
        return null;
    }
  });
};

/* ── Inline icons (lucide-style, no dependency) ────────────────── */
const IconGlobe: React.FC = () => (
  <svg
    className="h-6 w-6"
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    strokeWidth="2"
    strokeLinecap="round"
    strokeLinejoin="round"
    aria-hidden="true"
  >
    <circle cx="12" cy="12" r="10" />
    <path d="M2 12h20" />
    <path d="M12 2a15.3 15.3 0 0 1 4 10 15.3 15.3 0 0 1-4 10 15.3 15.3 0 0 1-4-10 15.3 15.3 0 0 1 4-10z" />
  </svg>
);

const IconText: React.FC = () => (
  <svg
    className="h-6 w-6"
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    strokeWidth="2"
    strokeLinecap="round"
    strokeLinejoin="round"
    aria-hidden="true"
  >
    <path d="M4 7V4h16v3" />
    <path d="M9 20h6" />
    <path d="M12 4v16" />
  </svg>
);

const IconLink: React.FC = () => (
  <svg
    className="h-4 w-4"
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    strokeWidth="2"
    strokeLinecap="round"
    strokeLinejoin="round"
    aria-hidden="true"
  >
    <path d="M10 13a5 5 0 0 0 7.54.54l3-3a5 5 0 0 0-7.07-7.07l-1.72 1.71" />
    <path d="M14 11a5 5 0 0 0-7.54-.54l-3 3a5 5 0 0 0 7.07 7.07l1.71-1.71" />
  </svg>
);

const IconGitHub: React.FC = () => (
  <svg
    className="h-4 w-4"
    viewBox="0 0 24 24"
    fill="currentColor"
    aria-hidden="true"
  >
    <path d="M12 2C6.48 2 2 6.58 2 12.25c0 4.53 2.87 8.37 6.84 9.73.5.1.68-.22.68-.49v-1.7c-2.78.62-3.37-1.37-3.37-1.37-.46-1.18-1.11-1.5-1.11-1.5-.91-.63.07-.62.07-.62 1 .07 1.53 1.06 1.53 1.06.89 1.56 2.34 1.11 2.91.85.09-.66.35-1.11.63-1.36-2.22-.26-4.56-1.14-4.56-5.05 0-1.12.39-2.03 1.03-2.75-.1-.26-.45-1.3.1-2.71 0 0 .84-.27 2.75 1.05A9.39 9.39 0 0 1 12 6.84c.85 0 1.71.12 2.51.34 1.91-1.32 2.75-1.05 2.75-1.05.55 1.41.2 2.45.1 2.71.64.72 1.03 1.63 1.03 2.75 0 3.92-2.34 4.78-4.57 5.04.36.32.68.94.68 1.9v2.82c0 .27.18.6.69.49A10.26 10.26 0 0 0 22 12.25C22 6.58 17.52 2 12 2z" />
  </svg>
);
