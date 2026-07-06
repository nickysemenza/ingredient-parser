import { useCallback, useMemo, useState } from "react";
import { wasm } from "./wasm";
import { DecompositionView } from "./DecompositionView";
import { Scraper } from "./components/Scraper";
import { TextInput } from "./components/ui";
import { useQueryState } from "nuqs";
import { fmtAmount, formatRichText, safeParseRichText } from "./lib/format";
import {
  DEFAULT_INGREDIENT,
  DEFAULT_RICH_TEXT,
  DEMO_INGREDIENT_NAMES,
  EXAMPLE_INGREDIENTS,
} from "./config";
import { IconGitHub, IconGlobe, IconLink, IconText } from "./icons";

export const Demo: React.FC = () => {
  const [text, setText] = useQueryState("i", { defaultValue: DEFAULT_INGREDIENT });
  const [richText, setRichText] = useQueryState("rt", {
    defaultValue: DEFAULT_RICH_TEXT,
  });

  const parsed = useMemo(
    () => (text ? wasm.parse_ingredient(text) : undefined),
    [text]
  );
  const decomp = useMemo(
    () => (text ? wasm.decompose_ingredient(text) : undefined),
    [text]
  );
  const parsedRich = useMemo(
    () =>
      richText ? safeParseRichText(richText, DEMO_INGREDIENT_NAMES) : undefined,
    [richText]
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
            <TextInput
              placeholder="e.g. 2 cups flour, sifted"
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
              <DecompositionView decomp={decomp} />
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
            {DEMO_INGREDIENT_NAMES.map((n) => (
              <span
                key={n}
                className="rounded-md border border-zinc-200 bg-zinc-50 px-2 py-0.5 font-mono text-xs text-zinc-600"
              >
                {n}
              </span>
            ))}
          </div>

          <div className="mt-8 rounded-2xl border border-zinc-200 bg-white p-6 shadow-sm">
            <TextInput
              placeholder="Enter recipe instructions…"
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

// Two-element nav cluster shares one pill style (button + anchor).
const NAV_ITEM =
  "inline-flex items-center gap-1.5 rounded-lg px-3 py-1.5 text-sm text-zinc-600 transition hover:bg-zinc-100";

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
          <button onClick={copyLink} className={NAV_ITEM}>
            <IconLink />
            {copied ? "Copied!" : "Copy link"}
          </button>
          <a
            href="https://github.com/nickysemenza/ingredient-parser"
            target="_blank"
            rel="noreferrer"
            className={NAV_ITEM}
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

// Parse-fidelity chip. `confidence` is the rollup; the "needs review" pill is
// keyed off the discrete `fell_back` / `unparsed_digit` booleans — not a
// confidence threshold — so a name-only fallback (which can still be Medium)
// is flagged for review just like a missed quantity.
const CONFIDENCE_STYLES: Record<"high" | "medium" | "low", string> = {
  high: "bg-emerald-50 text-emerald-700",
  medium: "bg-zinc-100 text-zinc-600",
  low: "bg-red-50 text-red-700",
};

const ParseNotesBadges: React.FC<{
  notes: ReturnType<typeof wasm.parse_ingredient>["parse_notes"];
}> = ({ notes }) => {
  const reviewReasons = [
    notes.fell_back && "Fell back to a name-only ingredient",
    notes.unparsed_digit &&
      "Contains a digit that produced no amount (likely missed quantity)",
  ].filter(Boolean) as string[];

  return (
    <div className="mt-0.5 flex shrink-0 items-center gap-1.5">
      <span
        className={`rounded-md px-2 py-0.5 text-xs font-medium capitalize ${CONFIDENCE_STYLES[notes.confidence]}`}
        title={`Parse confidence: ${notes.confidence}`}
      >
        {notes.confidence}
      </span>
      {reviewReasons.length > 0 && (
        <span
          className="cursor-help rounded-md bg-amber-100 px-2 py-0.5 text-xs font-medium text-amber-800"
          title={reviewReasons.join("\n")}
        >
          ⚠ review
        </span>
      )}
    </div>
  );
};

// `usage` is a declared role from the line (e.g. "oil, for frying" →
// "frying_medium"); "normal" is the unmarked default and gets no chip. The
// snake_case wire value is humanized for display.
const formatUsage = (usage: string) => usage.replace(/_/g, " ");

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
  const showUsage = parsed.usage !== "normal";
  return (
    <div>
      <div className="flex items-start justify-between gap-3">
        <h4 className="text-lg font-semibold text-zinc-900">
          {parsed.name || <span className="italic text-zinc-400">no name</span>}
        </h4>
        <ParseNotesBadges notes={parsed.parse_notes} />
      </div>

      {(showUsage || parsed.optional) && (
        <div className="mt-2 flex flex-wrap items-center gap-1.5">
          {showUsage && (
            <span
              className="rounded-md bg-accent-100 px-2 py-0.5 text-xs font-medium capitalize text-accent-700"
              title={`Declared usage: ${parsed.usage}`}
            >
              {formatUsage(parsed.usage)}
            </span>
          )}
          {parsed.optional && (
            <span
              className="rounded-md bg-sky-50 px-2 py-0.5 text-xs font-medium text-sky-700"
              title="This ingredient is marked optional"
            >
              optional
            </span>
          )}
        </div>
      )}

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
