// CLI application - panics are acceptable for fatal errors
#![allow(clippy::unwrap_used)]

use clap::{Parser, Subcommand};
use recipe_epub::CookbookRecipeExt; // .parse() / .low_confidence_lines() on CookbookRecipe

mod corpus_lint;
mod corpus_shadow;
mod explain;
mod tables;

/// Default path to the accuracy corpus, relative to this crate's manifest.
const DEFAULT_CORPUS_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../ingredient-parser/tests/corpus/corpus.jsonl"
);

/// Default path to the rich-text accuracy corpus, relative to this crate's manifest.
const DEFAULT_RICH_CORPUS_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../ingredient-parser/tests/corpus/rich_text.jsonl"
);

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
#[command(propagate_version = true)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Scrape {
        url: String,
        #[arg(short, long)]
        json: bool,
        #[arg(short, long)]
        parse: bool,
    },
    /// Scrape every recipe from a local EPUB cookbook file (AI-assisted)
    ScrapeEpub {
        /// Path to the .epub file
        path: String,
        #[arg(short, long)]
        json: bool,
        #[arg(short, long)]
        parse: bool,
        /// Dump one JSONL object per ingredient line: {line, name, amounts,
        /// modifier}. For corpus harvesting / parse review. Implies neither
        /// --json nor --parse; overrides normal output.
        #[arg(long)]
        dump_parsed: bool,
        /// Model id override (default: gemini-2.5-flash; claude-* / gpt-* also work)
        #[arg(long)]
        model: Option<String>,
        /// Fallback model for chunks the primary can't return parseable output
        /// for (e.g. --escalate-model claude-sonnet-4-6). Different models fail on
        /// different chunks, so a fallback recovers the primary's misses.
        #[arg(long)]
        escalate_model: Option<String>,
        /// Bypass the on-disk extraction cache
        #[arg(long)]
        no_cache: bool,
    },
    /// Debug a single EPUB: re-run every chunk through the model and report any
    /// whose raw payload fails to deserialize, with the offending JSON path. This
    /// is the view `scrape-epub` HIDES — it silently skips bad chunks (and the
    /// wasm cookbook import aborts the whole book on the first one). Bypasses the
    /// cache. Defaults to `claude-haiku-4-5` to mirror cubby's cookbook import.
    DebugEpub {
        /// Path to the .epub file
        path: String,
        /// Model id override (default: claude-haiku-4-5, matching cubby's import)
        #[arg(long)]
        model: Option<String>,
        /// Print the full raw JSON payload of each failed chunk (can be large)
        #[arg(long)]
        raw: bool,
    },
    /// Scan a Calibre/EPUB library, ranking ingredient lines the parser misses
    /// (have a number but yield no amount) as accuracy-corpus candidates.
    ScanCookbooks {
        /// Directory to scan recursively for .epub files
        dir: String,
        /// Max number of books to scan (uses the on-disk cache, so re-runs are free)
        #[arg(long, default_value_t = 10)]
        limit: usize,
        /// How many of the worst (most frequent) miss lines to print
        #[arg(long, default_value_t = 30)]
        bottom: usize,
        /// How many books to extract concurrently (each book still parallelizes
        /// its own chunks internally).
        #[arg(long, default_value_t = 8)]
        concurrency: usize,
    },
    ParseIngredient {
        name: String,
        /// Enable debug trace output showing which parsers were used
        #[arg(short, long)]
        debug: bool,
        /// Show a compact stage-level report (normalize → recognize → grammar →
        /// refine → result) — the view for deciding where a corpus fix belongs
        #[arg(short, long)]
        explain: bool,
        /// Export trace to Jaeger JSON format and write to file
        #[arg(long)]
        jaeger_output: Option<String>,
        /// Print exactly one JSONL corpus row for the parse, ready to append to
        /// tests/corpus/corpus.jsonl. Refuses (stderr + non-zero exit) when the
        /// parse fell back or is low-confidence, so a garbage row can't be
        /// appended blindly. Suppresses the normal JSON output.
        #[arg(long)]
        emit_corpus_row: bool,
    },
    /// Parse a file of ingredient lines (one per line) and emit one JSONL object
    /// per line: {line, name, amounts, modifier} — the same shape as
    /// `scrape-epub --dump-parsed`. For corpus harvesting: re-parse a prior dump
    /// through the current parser (free), or a website's lines via
    /// `scrape <url> --json | jq -r '.sections[].ingredients[]'`.
    ParseLines {
        /// Path to a file with one ingredient line per line (blank lines skipped)
        file: String,
    },
    /// Corpus tooling (lint, coverage reporting). See `corpus lint --help`.
    #[command(subcommand)]
    Corpus(CorpusCommand),
    /// Render the accuracy corpus (tests/corpus/corpus.jsonl) as an HTML table
    /// and open it in the default browser (like `cargo doc --open`). Read-only;
    /// does not touch the corpus.
    CorpusTable {
        /// Corpus file to render (defaults to the repo's corpus.jsonl)
        #[arg(long, default_value = DEFAULT_CORPUS_PATH)]
        corpus: String,
        /// Write the HTML here instead of a temp file, and don't auto-open.
        /// Use "-" for stdout.
        #[arg(long)]
        out: Option<String>,
    },
    /// Parse a measurement/amount string (without ingredient name)
    ParseAmount {
        /// The amount to parse (e.g., "2 cups", "1/2 tsp")
        text: String,
        /// Output as JSON
        #[arg(short, long)]
        json: bool,
    },
    /// Parse rich text (recipe instructions) with embedded measurements
    ParseRichText {
        /// The text to parse (e.g., "Add 1 cup flour and mix")
        text: String,
        /// Ingredient names to recognize (comma-separated)
        #[arg(short, long)]
        ingredients: String,
        /// Output as JSON
        #[arg(short, long)]
        json: bool,
    },
    /// Validate if a unit string is recognized
    ValidateUnit {
        /// The unit to validate (e.g., "cup", "tablespoon")
        unit: String,
        /// Additional custom units (comma-separated)
        #[arg(short = 'e', long)]
        extra_units: Option<String>,
    },
}

#[derive(Subcommand)]
enum CorpusCommand {
    /// Validate the accuracy corpus, and (with --report-stages) print a
    /// per-pass coverage report that flags parser passes firing on zero rows.
    Lint {
        /// Corpus file to lint (defaults to the repo's corpus.jsonl)
        #[arg(long, default_value = DEFAULT_CORPUS_PATH)]
        corpus: String,
        /// Print rows-per-pass coverage tables (normalize/recognize/refine) and a
        /// zero-coverage "possible dead rule" section. Without this flag, `lint`
        /// only validates that rows parse as JSON and prints the row count.
        #[arg(long)]
        report_stages: bool,
    },
    /// A/B the legacy and segmented parse paths over the corpus: report
    /// committed rows where they diverge (full field diff) and rows the
    /// segmented path newly gets right. Exit code = committed rows the
    /// segmented default parses differently from the LABEL (capped at 100),
    /// so zero means no real regressions. (Post-cutover, divergences vs the
    /// repair-less legacy mode are expected and informational.)
    Shadow {
        /// Ingredient corpus to A/B (defaults to the repo's corpus.jsonl)
        #[arg(long, default_value = DEFAULT_CORPUS_PATH)]
        corpus: String,
        /// Rich-text corpus whose inputs are A/B'd informationally
        #[arg(long, default_value = DEFAULT_RICH_CORPUS_PATH)]
        rich_corpus: String,
    },
}

/// Emit one JSONL object for an ingredient line zipped with its parse:
/// `{line, name, amounts, modifier}`. Shared by `scrape-epub --dump-parsed` and
/// `parse-lines` — the corpus-harvest review surface.
fn emit_parsed_line(ip: &ingredient::IngredientParser, line: &str) {
    let p = ip.from_str(line);
    let obj = serde_json::json!({
        "line": line,
        "name": p.name,
        "amounts": p.amounts,
        "modifier": p.modifier,
    });
    println!("{}", serde_json::to_string(&obj).unwrap());
}

/// Render a corpus amount value: an exact fraction *string* (`"2/3"`) for a
/// non-terminating decimal, else a plain JSON number with no trailing `.0`
/// (`2.0` → `2`, `0.5` → `0.5`) — matching the hand-authored corpus convention so
/// ⅔ round-trips exactly rather than as `0.666…`.
fn corpus_value_json(frac: Option<String>, val: f64) -> String {
    match frac {
        Some(s) => serde_json::json!(s).to_string(),
        // An integer-valued f64 writes as a bare int; keep `serde_json`'s float
        // rendering (shortest round-trip) for the rest.
        None if val.fract() == 0.0 && val.abs() < i64::MAX as f64 => (val as i64).to_string(),
        None => serde_json::json!(val).to_string(),
    }
}

/// Serialize one amount as a corpus `{"unit": .., "value": ..}` object string,
/// with keys in corpus order (unit, value, upper_value). `upper_value` is omitted
/// entirely when the amount is not a range. Built as a string (not a
/// `serde_json::Map`, which would alphabetize the keys) to pin that order.
fn corpus_amount_json(m: &ingredient::unit::Measure) -> String {
    let unit = serde_json::json!(m.unit().to_string());
    let value = corpus_value_json(m.value_as_fraction_str(), m.value());
    match m.upper_value() {
        Some(upper) => {
            let upper = corpus_value_json(m.upper_value_as_fraction_str(), upper);
            format!(r#"{{"unit": {unit}, "value": {value}, "upper_value": {upper}}}"#)
        }
        None => format!(r#"{{"unit": {unit}, "value": {value}}}"#),
    }
}

/// Build the one-line JSONL corpus row for `input`'s parse, with keys in corpus
/// order (`input, name, amounts, modifier, optional, usage`) and the optional
/// keys omitted per corpus convention: `modifier` when `None`, `optional` when
/// `false`, `usage` when `Normal`. `amounts` is omitted when empty (a bare
/// name-only row). Returns the row string, or `Err` describing why the parse is
/// unfit to author (fell back, or low confidence) so the caller can refuse it.
fn build_corpus_row(ip: &ingredient::IngredientParser, input: &str) -> Result<String, String> {
    use ingredient::Confidence;

    let ing = ip.from_str(input);
    let notes = ing.parse_notes;
    if notes.fell_back {
        return Err("parse fell back to a name-only ingredient".to_string());
    }
    if notes.confidence == Confidence::Low {
        return Err("low-confidence parse (a digit produced no amount)".to_string());
    }

    // Assemble by hand so key order is stable without the serde_json
    // `preserve_order` feature. Each `to_string` value is valid JSON already.
    let mut parts: Vec<String> = Vec::new();
    let field = |k: &str, v: &serde_json::Value| format!("{}: {}", serde_json::json!(k), v);

    parts.push(field("input", &serde_json::json!(input)));
    parts.push(field("name", &serde_json::json!(ing.name)));
    if !ing.amounts.is_empty() {
        let amounts: Vec<String> = ing.amounts.iter().map(corpus_amount_json).collect();
        parts.push(format!("\"amounts\": [{}]", amounts.join(", ")));
    }
    if let Some(modifier) = &ing.modifier {
        parts.push(field("modifier", &serde_json::json!(modifier)));
    }
    if ing.optional {
        parts.push(field("optional", &serde_json::json!(true)));
    }
    // `usage` is serialized to its snake_case string; omit the `normal` default.
    let usage = serde_json::to_value(ing.usage).map_err(|e| e.to_string())?;
    if usage.as_str() != Some("normal") {
        parts.push(field("usage", &usage));
    }

    Ok(format!("{{{}}}", parts.join(", ")))
}

/// Collect dotted paths of every `null`-valued key in a JSON payload, e.g.
/// `recipes[0].sections[1].instructions`. A `null` in an array-typed recipe
/// field is exactly what serde rejects with "expected a sequence", so this
/// points `debug-epub` straight at the offending field.
fn null_paths(v: &serde_json::Value, path: &str, out: &mut Vec<String>) {
    use serde_json::Value;
    match v {
        Value::Null => out.push(if path.is_empty() {
            "<root>".into()
        } else {
            path.into()
        }),
        Value::Object(m) => {
            for (k, val) in m {
                let p = if path.is_empty() {
                    k.clone()
                } else {
                    format!("{path}.{k}")
                };
                null_paths(val, &p, out);
            }
        }
        Value::Array(a) => {
            for (i, val) in a.iter().enumerate() {
                null_paths(val, &format!("{path}[{i}]"), out);
            }
        }
        _ => {}
    }
}

/// One renderable corpus entry: a parsed JSON row plus the section it falls
/// under (the most recent `// --- … ---` header). `error` is set instead of
/// panicking when a line is malformed, so the viewer survives a bad row.
struct CorpusEntry {
    section: String,
    row: serde_json::Value,
    error: Option<String>,
}

/// Parse the corpus text into entries. Mirrors `accuracy.rs::load`'s line
/// handling (skip `//` comments and blanks) but additionally tracks section
/// headers and tolerates malformed lines.
fn extract_corpus_rows(corpus: &str) -> Vec<CorpusEntry> {
    let mut section = String::from("(ungrouped)");
    let mut out = Vec::new();
    for raw in corpus.lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix("//") {
            // A `// --- Section name --- ` header updates the current section;
            // other `//` comments are ignored.
            if let Some(inner) = rest.trim().strip_prefix("---") {
                let name = inner.trim_end_matches('-').trim();
                if !name.is_empty() {
                    section = name.to_string();
                }
            }
            continue;
        }
        match serde_json::from_str::<serde_json::Value>(line) {
            Ok(row) => out.push(CorpusEntry {
                section: section.clone(),
                row,
                error: None,
            }),
            Err(e) => out.push(CorpusEntry {
                section: section.clone(),
                row: serde_json::json!({ "input": line }),
                error: Some(e.to_string()),
            }),
        }
    }
    out
}

/// Render a corpus amount value: a JSON number trimmed (no trailing `.0`):
/// `2` not `2.0`, `14.5`, `0.5` — or an exact fraction string (`"2/3"`)
/// passed through as-is, since corpus rows may author values in either form.
fn fmt_num(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::Number(n) if n.as_i64().is_some() => n.to_string(),
        serde_json::Value::Number(n) => n.as_f64().map(|f| format!("{f}")).unwrap_or_default(),
        serde_json::Value::String(s) => s.clone(),
        _ => String::new(),
    }
}

/// Format a corpus `amounts` array into compact chips: `2 cup`, `14.5 oz`,
/// range `3–4 oz` when `upper_value` is set, bare count for the `whole` unit.
fn fmt_amounts(amounts: &serde_json::Value) -> String {
    let Some(arr) = amounts.as_array() else {
        return String::new();
    };
    arr.iter()
        .map(|m| {
            let unit = m.get("unit").and_then(|u| u.as_str()).unwrap_or("");
            let value = m.get("value").map(fmt_num).unwrap_or_default();
            let upper = m
                .get("upper_value")
                .filter(|v| !v.is_null())
                .map(fmt_num)
                .filter(|s| !s.is_empty());
            let qty = match upper {
                Some(u) => format!("{value}–{u}"),
                None => value,
            };
            if unit.is_empty() || unit == "whole" {
                qty
            } else {
                format!("{qty} {unit}")
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

const CORPUS_STYLE: &str = "\
body { font-family: -apple-system, system-ui, sans-serif; margin: 2rem; color: #1a1a1a; }
h1 { font-size: 1.4rem; }
h2 { font-size: 1.05rem; margin-top: 2rem; color: #444; border-bottom: 1px solid #ddd; padding-bottom: .2rem; }
.summary { color: #666; }
table { border-collapse: collapse; width: 100%; font-size: .85rem; margin-bottom: 1rem; }
th, td { text-align: left; padding: .3rem .5rem; border-bottom: 1px solid #eee; vertical-align: top; }
thead th { position: sticky; top: 0; background: #fff; border-bottom: 2px solid #ccc; }
tbody tr:nth-child(even) { background: #fafafa; }
td code { font-family: ui-monospace, monospace; white-space: pre-wrap; }
tr.xfail, tr.xfail:nth-child(even) { background: #fff8e1; }
tr.err, tr.err:nth-child(even) { background: #fdecea; }
.opt { text-align: center; color: #2e7d32; }";

/// Render the corpus as a self-contained static HTML doc: one `<h2>` + `<table>`
/// per section. No JS. Returns `(html, row_count)`. `maud` auto-escapes every
/// interpolated value, so no manual escaping is needed.
fn render_corpus_html(corpus: &str) -> (String, usize) {
    use maud::{DOCTYPE, PreEscaped, html};

    let entries = extract_corpus_rows(corpus);
    let total = entries.len();
    let xfail = entries
        .iter()
        .filter(|e| e.row.get("xfail").map(|v| !v.is_null()).unwrap_or(false))
        .count();
    let committed = total - xfail;

    // Group consecutive entries by section (preserving corpus order) so each
    // section renders as one `<h2>` + `<table>`.
    let mut sections: Vec<(&str, Vec<&CorpusEntry>)> = Vec::new();
    for e in &entries {
        match sections.last_mut() {
            Some((name, rows)) if *name == e.section.as_str() => rows.push(e),
            _ => sections.push((e.section.as_str(), vec![e])),
        }
    }

    let markup = html! {
        (DOCTYPE)
        html lang="en" {
            head {
                meta charset="utf-8";
                title { "Ingredient parser corpus" }
                style { (PreEscaped(CORPUS_STYLE)) }
            }
            body {
                h1 { "Ingredient parser corpus" }
                p.summary { (total) " rows · " (committed) " committed · " (xfail) " xfail" }
                @for (name, rows) in &sections {
                    h2 { (name) }
                    table {
                        thead { tr {
                            th { "input" } th { "name" } th { "amounts" }
                            th { "modifier" } th { "optional" } th { "xfail" }
                        } }
                        tbody {
                            @for e in rows {
                                @let g = |k: &str| e.row.get(k).and_then(|v| v.as_str()).unwrap_or("");
                                @let amounts = e.row.get("amounts").map(fmt_amounts).unwrap_or_default();
                                @let optional = e.row.get("optional").and_then(|v| v.as_bool()) == Some(true);
                                @let note: std::borrow::Cow<'_, str> = match &e.error {
                                    Some(err) => format!("malformed: {err}").into(),
                                    None => g("xfail").into(),
                                };
                                @let row_class = if e.error.is_some() {
                                    Some("err")
                                } else if !note.is_empty() {
                                    Some("xfail")
                                } else {
                                    None
                                };
                                tr class=[row_class] {
                                    td { code { (g("input")) } }
                                    td { (g("name")) }
                                    td { (amounts) }
                                    td { (g("modifier")) }
                                    td.opt { @if optional { "✓" } }
                                    td { (note) }
                                }
                            }
                        }
                    }
                }
            }
        }
    };
    (markup.into_string(), total)
}

#[tokio::main]
async fn main() {
    // Surface the extractor's tracing (chunk skips, escalation, truncation) on
    // stderr. Off unless RUST_LOG is set, so normal --json stdout stays clean;
    // try `RUST_LOG=recipe_epub=info`. Without this, those warns went nowhere.
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();
    // Load AI gateway creds (AI_GATEWAY_API_KEY, CLOUDFLARE_AI_GATEWAY_BASE_URL)
    // from a repo-root .env. Missing file is fine; real exported vars take precedence.
    let _ = dotenvy::dotenv();
    let cli = Cli::parse();

    match &cli.command {
        Commands::Scrape { url, json, parse } => {
            let s = recipe_scraper_fetcher::Fetcher::new();
            let scraped = match s.scrape_url(url).await {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("scrape error: {e}");
                    std::process::exit(1);
                }
            };
            if *parse {
                let parsed = scraped.parse();
                if *json {
                    println!("{}", serde_json::to_string_pretty(&parsed).unwrap());
                } else {
                    println!("{parsed:#?}")
                }
            } else if *json {
                println!("{}", serde_json::to_string_pretty(&scraped).unwrap());
            } else {
                println!("{scraped:#?}")
            }
        }
        Commands::ScrapeEpub {
            path,
            json,
            parse,
            dump_parsed,
            model,
            escalate_model,
            no_cache,
        } => {
            let bytes = std::fs::read(path).unwrap();
            let opts = recipe_epub::Options {
                model: model.clone(),
                escalate_model: escalate_model.clone(),
                use_cache: !no_cache,
                ..Default::default()
            };
            match recipe_epub::extract_cookbook(&bytes, path, &opts).await {
                Ok((recipes, stats)) => {
                    // Cost/cache summary goes to stderr so --json stdout stays clean.
                    eprintln!("[{}] {}", stats.model, stats.summary());
                    // A valid EPUB that yields nothing is almost always an
                    // extraction bug (e.g. a content-decode failure), not an empty
                    // book — make it loud instead of exiting 0 with no output.
                    if stats.chunks_total == 0 || recipes.is_empty() {
                        eprintln!(
                            "warning: extracted {} recipe(s) from {} chunk(s) — the book may have failed to decode (check the epub)",
                            recipes.len(),
                            stats.chunks_total
                        );
                    }
                    // Cross-recipe references (recipe A uses recipe B) to stderr too.
                    let with_refs: Vec<_> = recipes
                        .iter()
                        .filter(|r| !r.references.is_empty())
                        .collect();
                    if !with_refs.is_empty() {
                        let total: usize = with_refs.iter().map(|r| r.references.len()).sum();
                        eprintln!(
                            "cross-recipe references: {total} across {} recipe(s)",
                            with_refs.len()
                        );
                        for r in with_refs {
                            let targets: Vec<&str> =
                                r.references.iter().map(|x| x.title.as_str()).collect();
                            eprintln!("  {} → {}", r.meta.title, targets.join(", "));
                        }
                    }
                    if *dump_parsed {
                        // One JSONL object per ingredient line: the verbatim
                        // line zipped with its parsed shape. For corpus harvest.
                        let ip = ingredient::IngredientParser::new();
                        for r in &recipes {
                            for sec in &r.sections {
                                for line in &sec.ingredients {
                                    emit_parsed_line(&ip, line);
                                }
                            }
                        }
                    } else if *parse {
                        let parsed: Vec<_> = recipes.iter().map(|r| r.parse()).collect();
                        if *json {
                            println!("{}", serde_json::to_string_pretty(&parsed).unwrap());
                        } else {
                            println!("{parsed:#?}");
                        }
                    } else if *json {
                        println!("{}", serde_json::to_string_pretty(&recipes).unwrap());
                    } else {
                        println!("{recipes:#?}");
                    }
                }
                Err(e) => {
                    eprintln!("epub scrape error: {e}");
                    std::process::exit(1);
                }
            }
        }
        Commands::DebugEpub { path, model, raw } => {
            let bytes = std::fs::read(path).unwrap_or_else(|e| {
                eprintln!("failed to read {path}: {e}");
                std::process::exit(1);
            });
            // Default to Haiku (cubby's cookbook-import model) so a failure here
            // reproduces the real import; the cache is off because a failed parse
            // is never cached anyway and we want a live payload every run.
            let opts = recipe_epub::Options {
                model: Some(
                    model
                        .clone()
                        .unwrap_or_else(|| "claude-haiku-4-5".to_string()),
                ),
                use_cache: false,
                ..Default::default()
            };
            let mut chunks = match recipe_epub::debug_extract_cookbook(&bytes, path, &opts).await {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("debug-epub error: {e}");
                    std::process::exit(1);
                }
            };
            chunks.sort_by(|a, b| a.doc_path.cmp(&b.doc_path));
            let failures: Vec<&recipe_epub::ChunkDebug> =
                chunks.iter().filter(|c| c.error.is_some()).collect();
            let recipes: usize = chunks.iter().filter_map(|c| c.parsed).sum();
            eprintln!(
                "[{}] {} chunk(s): {} ok ({recipes} recipes), {} FAILED",
                opts.model.as_deref().unwrap_or(""),
                chunks.len(),
                chunks.len() - failures.len(),
                failures.len()
            );
            for c in &failures {
                println!("\n--- FAILED chunk: {} ---", c.doc_path);
                if let Some(h) = &c.title_hint {
                    println!("  title hint: {h}");
                }
                if c.truncated {
                    println!("  ⚠ truncated (hit the {} token limit)", 16000);
                }
                // A "invalid type: string …" serde error inlines the entire
                // offending payload; truncate so the report stays readable
                // (pass --raw for the full payload).
                let err = c.error.as_deref().unwrap_or("");
                let shown = if err.len() > 300 && !*raw {
                    // char-safe truncation (the payload contains °, é, …)
                    let head: String = err.chars().take(300).collect();
                    format!("{head}… ({} bytes total)", err.len())
                } else {
                    err.to_string()
                };
                println!("  error: {shown}");
                if let Some(input) = &c.raw_input {
                    let mut nulls = Vec::new();
                    null_paths(input, "", &mut nulls);
                    println!("  null field(s): {nulls:?}");
                    if *raw {
                        println!(
                            "  raw payload:\n{}",
                            serde_json::to_string_pretty(input).unwrap()
                        );
                    }
                }
            }
            if failures.is_empty() {
                println!(
                    "no parse failures — all {} chunk(s) deserialized cleanly",
                    chunks.len()
                );
            }
            // Non-zero exit when any chunk failed, so this is scriptable in CI.
            std::process::exit(if failures.is_empty() { 0 } else { 2 });
        }
        Commands::ScanCookbooks {
            dir,
            limit,
            bottom,
            concurrency,
        } => {
            let mut epubs = recipe_epub::find_epubs(std::path::Path::new(dir));
            epubs.sort();
            epubs.truncate(*limit);

            // Per-book extraction is independent, so run several books at once
            // (each book already parallelizes its own chunks internally).
            // `buffer_unordered` keeps up to `concurrency` books in flight on a
            // single task; results are aggregated as they complete.
            use futures::stream::{self, StreamExt};
            let opts = recipe_epub::Options::default();
            let mut stream = stream::iter(epubs.clone())
                .map(|path| {
                    let opts = &opts;
                    async move {
                        let p = path.to_string_lossy().to_string();
                        let bytes = match std::fs::read(&path) {
                            Ok(b) => b,
                            Err(e) => return (p, Err(format!("read error: {e}"))),
                        };
                        let result = recipe_epub::extract_cookbook(&bytes, &p, opts)
                            .await
                            .map_err(|e| e.to_string());
                        (p, result)
                    }
                })
                .buffer_unordered((*concurrency).max(1));

            let mut candidates: std::collections::HashMap<String, usize> =
                std::collections::HashMap::new();
            let mut total_recipes = 0usize;
            let mut total_lines = 0usize;
            let mut total_chunks = 0usize;
            let mut total_chunks_cached = 0usize;
            let mut total_cost = 0.0f64;
            let mut cost_known = true;
            while let Some((p, result)) = stream.next().await {
                match result {
                    Ok((recipes, stats)) => {
                        total_recipes += recipes.len();
                        total_chunks += stats.chunks_total;
                        total_chunks_cached += stats.chunks_cached;
                        match stats.cost_usd() {
                            Some(c) => total_cost += c,
                            None => cost_known = false,
                        }
                        for r in &recipes {
                            total_lines += r
                                .sections
                                .iter()
                                .map(|s| s.ingredients.len())
                                .sum::<usize>();
                            for line in r.low_confidence_lines() {
                                *candidates.entry(line).or_default() += 1;
                            }
                        }
                        eprintln!("{p}: {} recipes · {}", recipes.len(), stats.summary());
                    }
                    Err(e) => eprintln!("{p}: error: {e}"),
                }
            }
            let miss_total: usize = candidates.values().sum();
            let mut ranked: Vec<(String, usize)> = candidates.into_iter().collect();
            ranked.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
            let cost_str = if cost_known {
                format!("~${total_cost:.4}")
            } else {
                "n/a".to_string()
            };
            println!(
                "scanned {} book(s): {total_recipes} recipes, {total_lines} ingredient lines, \
                 {miss_total} with a number but no parsed amount\n\
                 cost: {cost_str} · {total_chunks_cached}/{total_chunks} chunks cached",
                epubs.len()
            );
            println!(
                "\ntop {} parser-miss lines (corpus candidates):",
                (*bottom).min(ranked.len())
            );
            // Tab-separated stays grep/copy-friendly for piping into the corpus.
            for (line, n) in ranked.into_iter().take(*bottom) {
                println!("{n}\t{line}");
            }
        }
        Commands::ParseIngredient {
            name,
            debug,
            explain,
            jaeger_output,
            emit_corpus_row,
        } => {
            if *emit_corpus_row {
                // Authoring helper: one JSONL row for the corpus, or a refusal.
                let ip = ingredient::IngredientParser::new();
                match build_corpus_row(&ip, name) {
                    Ok(row) => println!("{row}"),
                    Err(reason) => {
                        eprintln!(
                            "refusing to emit corpus row for {name:?}: {reason}\n\
                             (inspect with `parse-ingredient {name:?} --explain`)"
                        );
                        std::process::exit(1);
                    }
                }
                return;
            }
            if *debug || *explain || jaeger_output.is_some() {
                // Use parse_with_trace for debug output or Jaeger export
                let parser = ingredient::IngredientParser::new();
                let result = parser.parse_with_trace(name);
                let use_color = std::io::IsTerminal::is_terminal(&std::io::stdout());

                // Export to Jaeger JSON if requested
                if let Some(output_path) = jaeger_output {
                    let jaeger_json = result.trace.to_jaeger_json();
                    if let Err(e) = std::fs::write(output_path, &jaeger_json) {
                        eprintln!("Failed to write Jaeger JSON to {output_path}: {e}");
                        std::process::exit(1);
                    }
                    eprintln!("Wrote Jaeger trace to: {output_path}");
                }

                // Compact stage report — the routing view. The miette header
                // labels how the grammar carved the line (amount/name/modifier),
                // or falls back to a caret on a digit that produced no amount;
                // the stage view below shows which pipeline stage shaped the line.
                if *explain {
                    let diag = parser.from_str(name).parse_notes;
                    let decomp = parser.decompose(name);
                    print!("{}", explain::render(&decomp, &diag, use_color));
                    println!();
                    println!("{}", result.trace.format_stages(use_color));
                }

                // Print the full trace tree if debug is enabled
                if *debug {
                    println!("{}", result.trace.format_tree(use_color));
                }

                // Print the result
                match result.result {
                    Ok(ingredient) => {
                        println!("\nResult:");
                        println!("{}", serde_json::to_string_pretty(&ingredient).unwrap());
                    }
                    Err(e) => {
                        eprintln!("\nParse error: {e}");
                    }
                }
            } else {
                // JSON only — stdout must stay pipeable (`… | jq`); the human-
                // readable Display line was breaking that.
                let res = ingredient::from_str(name);
                println!("{}", serde_json::to_string_pretty(&res).unwrap());
            }
        }
        Commands::ParseLines { file } => {
            let contents = match std::fs::read_to_string(file) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("failed to read {file}: {e}");
                    std::process::exit(1);
                }
            };
            let ip = ingredient::IngredientParser::new();
            for line in contents.lines() {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                emit_parsed_line(&ip, line);
            }
        }
        Commands::Corpus(CorpusCommand::Lint {
            corpus,
            report_stages,
        }) => {
            corpus_lint::run(corpus, *report_stages);
        }
        Commands::Corpus(CorpusCommand::Shadow {
            corpus,
            rich_corpus,
        }) => {
            corpus_shadow::run(corpus, rich_corpus);
        }
        Commands::CorpusTable { corpus, out } => {
            let contents = match std::fs::read_to_string(corpus) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("failed to read {corpus}: {e}");
                    std::process::exit(1);
                }
            };
            let (html, rows) = render_corpus_html(&contents);
            match out.as_deref() {
                Some("-") => print!("{html}"),
                Some(path) => {
                    std::fs::write(path, &html).unwrap();
                    eprintln!("wrote {path} ({rows} rows)");
                }
                None => {
                    let path = std::env::temp_dir().join("ingredient-corpus.html");
                    std::fs::write(&path, &html).unwrap();
                    eprintln!("wrote {} ({rows} rows)", path.display());
                    // Best-effort: open in the default browser. Headless/SSH
                    // environments have no opener — just leave the path printed.
                    if let Err(e) = open::that(&path) {
                        eprintln!("(couldn't open a browser: {e} — open the file above manually)");
                    }
                }
            }
        }
        Commands::ParseAmount { text, json } => {
            let parser = ingredient::IngredientParser::new();
            match parser.parse_amount(text) {
                Ok(amounts) => {
                    if *json {
                        println!("{}", serde_json::to_string_pretty(&amounts).unwrap());
                    } else {
                        println!("{}", tables::amount_table(&amounts));
                    }
                }
                Err(e) => {
                    eprintln!("Parse error: {e}");
                    std::process::exit(1);
                }
            }
        }
        Commands::ParseRichText {
            text,
            ingredients,
            json,
        } => {
            let ingredient_names: Vec<String> = ingredients
                .split(',')
                .map(|s| s.trim().to_string())
                .collect();

            let parser = ingredient::rich_text::RichParser::new(ingredient_names);
            match parser.parse(text) {
                Ok(rich) => {
                    if *json {
                        println!("{}", serde_json::to_string_pretty(&rich).unwrap());
                    } else {
                        println!("{rich:?}");
                    }
                }
                Err(e) => {
                    eprintln!("Parse error: {e}");
                    std::process::exit(1);
                }
            }
        }
        Commands::ValidateUnit { unit, extra_units } => {
            // Validate by attempting to parse a simple measurement with this unit
            let mut parser = ingredient::IngredientParser::new();

            // Add extra units if provided
            if let Some(units_str) = extra_units {
                let extra: Vec<&str> = units_str.split(',').map(|s| s.trim()).collect();
                parser = parser.with_units(&extra);
            }

            // Try to parse "1 <unit>". An unknown unit isn't an error — it falls
            // back to the bare-count `whole` — so Whole only counts as valid when
            // the input itself spells it ("whole"/"each"). Comparing the
            // CANONICAL unit (not the raw spelling) keeps aliases and plurals
            // valid: "tablespoon", "cups", "grams".
            use std::str::FromStr;
            let test_input = format!("1 {unit}");
            let input_is_whole =
                ingredient::unit::Unit::from_str(unit) == Ok(ingredient::unit::Unit::Whole);
            let is_valid = parser
                .parse_amount(&test_input)
                .map(|amounts| {
                    amounts.first().is_some_and(|m| {
                        *m.unit() != ingredient::unit::Unit::Whole || input_is_whole
                    })
                })
                .unwrap_or(false);

            println!("{}", if is_valid { "valid" } else { "invalid" });
            std::process::exit(if is_valid { 0 } else { 1 });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"// header comment, ignored
//
// --- basics ---
{"input": "2 cups flour", "name": "flour", "amounts": [{"unit": "cup", "value": 2}]}

{"input": "2-3 cups <broth>", "name": "broth", "amounts": [{"unit": "cup", "value": 2, "upper_value": 3}]}
{"input": "2/3 cup milk", "name": "milk", "amounts": [{"unit": "cup", "value": "2/3"}]}
// --- gaps ---
{"input": "1 pint berries", "name": "berries", "amounts": [{"unit": "pint", "value": 1}], "xfail": "pint range"}
not valid json
"#;

    #[test]
    fn extract_skips_comments_and_tracks_sections() {
        let rows = extract_corpus_rows(SAMPLE);
        // 4 valid rows + 1 malformed = 5 entries; comments/blanks dropped.
        assert_eq!(rows.len(), 5);
        assert_eq!(rows[0].section, "basics");
        assert_eq!(rows[1].section, "basics");
        assert_eq!(rows[2].section, "basics");
        assert_eq!(rows[3].section, "gaps");
        assert!(rows[3].row.get("xfail").is_some());
        // The malformed line is tolerated, not panicked on.
        assert!(rows[4].error.is_some());
    }

    #[test]
    fn render_escapes_and_counts() {
        let (html, rows) = render_corpus_html(SAMPLE);
        assert_eq!(rows, 5);
        assert!(html.contains("<table>"));
        // Summary: 5 entries, 1 has xfail, the malformed one counts as committed.
        assert!(html.contains("5 rows · 4 committed · 1 xfail"));
        // Section headings rendered.
        assert!(html.contains("<h2>basics</h2>"));
        assert!(html.contains("<h2>gaps</h2>"));
        // Angle brackets in input are escaped, not emitted raw.
        assert!(html.contains("&lt;broth&gt;"));
        assert!(!html.contains("<broth>"));
        // Range chip uses an en dash.
        assert!(html.contains("2–3 cup"));
        // A fraction-string value renders as the fraction, not a blank quantity.
        assert!(html.contains("2/3 cup"));
        assert!(!html.contains("> cup<"));
        // xfail reason surfaces.
        assert!(html.contains("pint range"));
    }
}
