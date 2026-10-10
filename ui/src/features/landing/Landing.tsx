// The public face of the web build: a live parser up front, then what it
// does and how to use it. Everything on the page runs the real Rust parser,
// compiled to WebAssembly, in a worker.
import { ArrowRight, BookOpen, CodeXml, Copy, Cpu, FlaskConical, Globe, Layers, Monitor, Moon, Ruler, Scale, Sparkles, Split as SplitIcon, Sun } from "lucide-react";
import { useEffect, useState, type ReactNode } from "react";
import { toast } from "sonner";
import { api, type IngredientRow } from "../../api";
import { copyText } from "../../api/platform";
import { navigate } from "../../app/router";
import { useTheme, APPEARANCES } from "../../app/theme";
import { Amounts, ConfidenceBadge, FieldLegend, SourceText, StagePipeline } from "../../components/ingredient";
import { RichText } from "../../components/recipe";
import { Badge, Button, Spinner, Textarea } from "../../components/ui";
import { cn } from "../../lib/cn";
import { HERO_EXAMPLES, RECIPE_URL, RICH_TEXT, RICH_TEXT_NAMES } from "../../lib/examples";
import { useQuery } from "../../lib/query";

const GITHUB = "https://github.com/nickysemenza/ingredient-parser";
const CRATE = "https://crates.io/crates/ingredient";
const DOCS = "https://docs.rs/ingredient";

function Link({ to, children, className }: { to: "parser" | "cookbooks"; children: ReactNode; className?: string }) {
  return (
    <a
      href={`${import.meta.env.BASE_URL}${to}`}
      onClick={(e) => {
        if (e.metaKey || e.ctrlKey) return;
        e.preventDefault();
        navigate(to);
      }}
      className={className}
    >
      {children}
    </a>
  );
}

function ThemeButton() {
  const { appearance, setAppearance } = useTheme();
  const next = APPEARANCES[(APPEARANCES.indexOf(appearance) + 1) % APPEARANCES.length];
  const Icon = appearance === "dark" ? Moon : appearance === "light" ? Sun : Monitor;
  return (
    <Button variant="ghost" size="icon" aria-label={`Appearance: ${appearance}. Switch to ${next}.`} onClick={() => setAppearance(next)}>
      <Icon />
    </Button>
  );
}

function Nav() {
  return (
    <nav aria-label="Site" className="sticky top-0 z-30 border-b border-line/70 bg-base/80 backdrop-blur-md">
      <div className="mx-auto flex h-14 max-w-6xl items-center gap-6 px-5">
        <span className="flex items-center gap-2">
          <span className="flex size-6 items-center justify-center rounded-md bg-accent text-[12px] font-bold text-accent-ink">ip</span>
          <span className="text-[14px] font-semibold tracking-tight">ingredient-parser</span>
        </span>
        <div className="hidden items-center gap-5 text-[13px] text-muted sm:flex">
          <Link to="parser" className="hover:text-fg">
            Parser
          </Link>
          <Link to="cookbooks" className="hover:text-fg">
            Cookbooks
          </Link>
          <a href={DOCS} className="hover:text-fg">
            Docs
          </a>
        </div>
        <div className="ml-auto flex items-center gap-1">
          <ThemeButton />
          <a href={GITHUB} aria-label="GitHub repository" className="flex size-8 items-center justify-center rounded-control text-muted hover:bg-hover hover:text-fg">
            <CodeXml className="size-4" />
          </a>
          <Link to="parser" className="ml-2 inline-flex h-8 items-center gap-1.5 rounded-control bg-fg px-3 text-[13px] font-medium text-base hover:opacity-90">
            Open workbench
            <ArrowRight className="size-3.5" />
          </Link>
        </div>
      </div>
    </nav>
  );
}

/** Parse as you type, and show where every part of the line went. */
function Hero() {
  const [input, setInput] = useState(HERO_EXAMPLES[0]);
  const [ms, setMs] = useState<number | null>(null);
  const parsed = useQuery(
    input.trim()
      ? async () => {
          const start = performance.now();
          const [row] = await api.parseLines(input);
          setMs(performance.now() - start);
          return row;
        }
      : null,
    [input],
    60,
  );
  const row: IngredientRow | undefined = parsed.value;
  const inspection = useQuery(input.trim() ? () => api.inspect(input) : null, [input], 250);
  return (
    <header className="relative overflow-hidden">
      <div aria-hidden className="pointer-events-none absolute inset-x-0 -top-40 h-[520px] bg-[radial-gradient(60%_60%_at_50%_0%,color-mix(in_oklab,var(--accent)_16%,transparent),transparent)]" />
      <div className="relative mx-auto max-w-6xl px-5 pt-20 pb-14 sm:pt-24">
        <div className="mx-auto max-w-3xl text-center">
          <Badge tone="accent" className="mb-5">
            <Cpu />
            Rust, running in your browser via WebAssembly
          </Badge>
          <h1 className="text-[40px] leading-[1.05] font-semibold tracking-[-0.03em] sm:text-[56px]">
            Recipe ingredients,
            <br />
            <span className="text-accent">parsed into structure.</span>
          </h1>
          <p className="mx-auto mt-5 max-w-xl text-[16px] leading-relaxed text-muted">
            Amounts, units, ranges, names and modifiers from the way people actually write ingredient lines. Type one below.
          </p>
        </div>

        <div className="mx-auto mt-10 max-w-3xl">
          <div className="rounded-[14px] border border-line bg-evidence p-1.5 shadow-[0_1px_0_var(--line),0_20px_60px_-20px_color-mix(in_oklab,var(--fg)_25%,transparent)]">
            <label htmlFor="hero-input" className="sr-only">
              Ingredient line
            </label>
            <input
              id="hero-input"
              value={input}
              onChange={(e) => setInput(e.target.value)}
              spellCheck={false}
              autoComplete="off"
              placeholder="e.g. 2 cups flour, sifted"
              className="h-14 w-full rounded-[10px] bg-transparent px-4 font-mono text-[17px] outline-none placeholder:text-faint"
            />
            <div className="rounded-[10px] border border-line bg-mantle/60 p-4" aria-live="polite">
              {row ? (
                <div className={cn("space-y-4 transition-opacity", parsed.loading && "opacity-70")}>
                  <div className="flex items-start justify-between gap-4">
                    <p data-testid="hero-decomposition" className="text-[17px] leading-relaxed">
                      <SourceText segments={row.segments} />
                    </p>
                    <span className="shrink-0 text-[11px] text-faint tabular-nums">{ms !== null ? `${ms < 1 ? "<1" : ms.toFixed(1)} ms` : ""}</span>
                  </div>
                  <dl aria-label="Parsed ingredient" className="grid gap-3 sm:grid-cols-[1.1fr_1fr_1fr_auto]">
                    <div>
                      <dt className="text-[11px] font-medium text-muted">Name</dt>
                      <dd className="mt-0.5 truncate text-[15px] font-semibold text-name">{row.ingredient.name || "—"}</dd>
                    </div>
                    <div>
                      <dt className="text-[11px] font-medium text-muted">Amounts</dt>
                      <dd className="mt-1">
                        <Amounts amounts={row.amounts} className="text-[13px]" />
                      </dd>
                    </div>
                    <div>
                      <dt className="text-[11px] font-medium text-muted">Modifier</dt>
                      <dd className={cn("mt-0.5 truncate text-[14px]", row.ingredient.modifier ? "text-violet" : "text-faint")}>{row.ingredient.modifier ?? "—"}</dd>
                    </div>
                    <div>
                      <dt className="text-[11px] font-medium text-muted">Confidence</dt>
                      <dd className="mt-1">
                        <ConfidenceBadge row={row} />
                      </dd>
                    </div>
                  </dl>
                  <FieldLegend segments={row.segments} />
                </div>
              ) : (
                <div className="flex h-24 items-center justify-center text-[13px] text-muted">{parsed.error ?? (input.trim() ? <Spinner /> : "Type an ingredient line.")}</div>
              )}
            </div>
          </div>
          <div className="mt-4 flex flex-wrap justify-center gap-2">
            {HERO_EXAMPLES.map((example) => (
              <button
                key={example}
                type="button"
                onClick={() => setInput(example)}
                className={cn(
                  "h-7 rounded-full border px-3 font-mono text-[12px] transition-colors",
                  example === input ? "border-accent/40 bg-accent/10 text-fg" : "border-line bg-evidence text-muted hover:border-line-strong hover:text-fg",
                )}
              >
                {example}
              </button>
            ))}
          </div>
        </div>
      </div>
      <Pipeline stages={inspection.value?.stages ?? null} input={input} />
    </header>
  );
}

function Pipeline({ stages, input }: { stages: Parameters<typeof StagePipeline>[0]["stages"]; input: string }) {
  return (
    <section aria-label="How it reads a line" className="border-y border-line bg-mantle/50">
      <div className="mx-auto grid max-w-6xl gap-10 px-5 py-16 md:grid-cols-[1fr_1.2fr]">
        <div>
          <h2 className="text-[26px] leading-tight font-semibold tracking-tight">Every decision is on the record.</h2>
          <p className="mt-3 text-[15px] leading-relaxed text-muted">
            A line is normalized, matched by recognizers, read by a grammar, split into clauses and refined. Each stage reports what it changed, so a surprising result is one click from its cause.
          </p>
          <Link to="parser" className="mt-6 inline-flex items-center gap-1.5 text-[14px] font-medium text-accent hover:underline">
            Inspect any line in the workbench
            <ArrowRight className="size-3.5" />
          </Link>
        </div>
        <div className="rounded-panel border border-line bg-evidence p-5 shadow-sm">
          <p className="mb-4 truncate font-mono text-[12.5px] text-muted">{input || "—"}</p>
          {stages ? <StagePipeline stages={stages} /> : <Spinner />}
        </div>
      </div>
    </section>
  );
}

function Prose() {
  const [text, setText] = useState(RICH_TEXT);
  const chunks = useQuery(() => api.richText(text, RICH_TEXT_NAMES), [text], 120);
  return (
    <div className="rounded-panel border border-line bg-evidence p-5">
      <label htmlFor="prose-input" className="text-[12px] font-medium text-muted">
        Instructions
      </label>
      <Textarea id="prose-input" rows={3} value={text} onChange={(e) => setText(e.target.value)} className="mt-2 text-[14px]" />
      <p data-testid="rich-text" className="mt-4 min-h-12 text-[15px]">
        {chunks.value ? <RichText chunks={chunks.value} /> : <Spinner />}
      </p>
      <p className="mt-3 text-[11.5px] text-faint">Measures are recognized in prose and ingredient names are matched against the recipe.</p>
    </div>
  );
}

const FEATURES: { icon: ReactNode; title: string; body: string }[] = [
  { icon: <Ruler />, title: "Units that convert", body: "Volume, weight, temperature, time and counts, with fractions, unicode vulgar fractions and metric pairs like “1 cup / 120 g”." },
  { icon: <SplitIcon />, title: "Ranges and alternatives", body: "“2-3 cloves”, “1 to 2 tbsp”, “1 (14-ounce) can”: ranges, nested sizes and multiple amounts stay distinct." },
  { icon: <Sparkles />, title: "Names, not noise", body: "Size words, preparation and usage (“for frying”, “to taste”) are kept apart from the ingredient's name." },
  { icon: <Scale />, title: "Scale whole recipes", body: "Load a recipe from the web, double it, and every amount and every measure in the method moves together." },
  { icon: <Layers />, title: "A regression ratchet", body: "Hundreds of labeled lines score every change. The corpus runs here too, against this exact build." },
  { icon: <BookOpen />, title: "Whole cookbooks", body: "The desktop app extracts EPUB cookbooks into linked recipes with photos. Review any saved run here, offline." },
];

function Install() {
  const snippet = `use ingredient::from_str;

let ingredient = from_str("2 cups all-purpose flour, sifted");
assert_eq!(ingredient.name, "all-purpose flour");
assert_eq!(ingredient.modifier, Some("sifted".to_string()));`;
  return (
    <div className="overflow-hidden rounded-panel border border-line bg-evidence">
      <div className="flex items-center justify-between border-b border-line px-4 py-2.5">
        <code className="font-mono text-[13px]">
          <span className="text-faint">$</span> cargo add ingredient
        </code>
        <Button
          size="sm"
          variant="ghost"
          onClick={() => void copyText("cargo add ingredient").then(() => toast.success("Copied"), (e: unknown) => toast.error(String(e)))}
        >
          <Copy />
          Copy
        </Button>
      </div>
      <pre className="overflow-x-auto bg-mantle/60 p-4 font-mono text-[12.5px] leading-relaxed">{snippet}</pre>
    </div>
  );
}

export default function Landing() {
  useEffect(() => {
    document.title = "ingredient-parser · recipe ingredients, parsed";
  }, []);
  return (
    <div className="min-h-dvh bg-base text-fg">
      <Nav />
      <main>
        <Hero />
        <section aria-label="Recipes and prose" className="mx-auto max-w-6xl px-5 py-20">
          <div>
            <div className="grid items-start gap-10 md:grid-cols-[1fr_1.2fr]">
              <div>
                <h2 className="text-[26px] leading-tight font-semibold tracking-tight">Beyond the ingredient list.</h2>
                <p className="mt-3 text-[15px] leading-relaxed text-muted">
                  Methods mention amounts too. The same parser finds them in prose, links ingredients by name, and scales the whole recipe at once.
                </p>
                <div className="mt-6 flex flex-wrap gap-2">
                  <Button variant="secondary" onClick={() => navigate("parser", `?mode=recipe&url=${encodeURIComponent(RECIPE_URL)}`)}>
                    <Globe />
                    Parse a web recipe
                  </Button>
                  <Button variant="ghost" onClick={() => navigate("parser", "?mode=corpus")}>
                    <FlaskConical />
                    Score the corpus
                  </Button>
                </div>
              </div>
              <Prose />
            </div>
          </div>
        </section>
        <section aria-label="Features" className="border-t border-line">
          <div className="mx-auto max-w-6xl px-5 py-20">
            <div>
              <h2 className="text-[26px] font-semibold tracking-tight">Built for real recipes.</h2>
              <div className="mt-8 grid gap-px overflow-hidden rounded-panel border border-line bg-line sm:grid-cols-2 lg:grid-cols-3">
                {FEATURES.map((f) => (
                  <div key={f.title} className="bg-evidence p-5">
                    <span className="flex size-8 items-center justify-center rounded-lg bg-accent/10 text-accent [&_svg]:size-4">{f.icon}</span>
                    <h3 className="mt-3 text-[14px] font-semibold">{f.title}</h3>
                    <p className="mt-1.5 text-[13px] leading-relaxed text-muted">{f.body}</p>
                  </div>
                ))}
              </div>
            </div>
          </div>
        </section>
        <section aria-label="Install" className="border-t border-line bg-mantle/50">
          <div className="mx-auto grid max-w-6xl items-center gap-10 px-5 py-20 md:grid-cols-[1fr_1.2fr]">
            <div>
              <h2 className="text-[26px] font-semibold tracking-tight">Use it from Rust.</h2>
              <p className="mt-3 text-[15px] leading-relaxed text-muted">One function for the common case; a configurable parser, rich-text parsing and unit conversion when you need them.</p>
              <div className="mt-6 flex gap-4 text-[14px] font-medium">
                <a href={DOCS} className="text-accent hover:underline">
                  Documentation
                </a>
                <a href={CRATE} className="text-accent hover:underline">
                  crates.io
                </a>
                <a href={GITHUB} className="text-accent hover:underline">
                  Source
                </a>
              </div>
            </div>
            <Install />
          </div>
        </section>
      </main>
      <footer className="border-t border-line">
        <div className="mx-auto flex max-w-6xl flex-wrap items-center justify-between gap-4 px-5 py-8 text-[12.5px] text-muted">
          <span>ingredient-parser · MIT licensed</span>
          <span className="flex items-center gap-3">
            <a href={GITHUB} className="hover:text-fg">
              GitHub
            </a>
            <a href={CRATE} className="hover:text-fg">
              crates.io
            </a>
            <a href={DOCS} className="hover:text-fg">
              docs.rs
            </a>
          </span>
        </div>
      </footer>
    </div>
  );
}
