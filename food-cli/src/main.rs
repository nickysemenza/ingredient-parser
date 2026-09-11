// CLI application - panics are acceptable for fatal errors
#![allow(clippy::unwrap_used)]

use clap::{Parser, Subcommand};
mod cookbook;
mod cookbook_library;

// The corpus/diagnostic verbs live in the library half so tests and other
// crates can call them; this binary is argument parsing, printing and exit
// codes. See src/lib.rs.
use food_cli::{corpus_lint, corpus_table, explain, tables};

/// Default path to the accuracy corpus, relative to this crate's manifest.
const DEFAULT_CORPUS_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../ingredient-parser/tests/corpus/corpus.jsonl"
);

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
#[command(propagate_version = true)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
    /// Output representation; jsonl is supported by ingredient batch and cookbook stats.
    #[arg(long, global = true, value_enum, default_value = "human")]
    format: OutputFormat,
}

#[derive(Clone, Copy, PartialEq, clap::ValueEnum)]
enum OutputFormat {
    Human,
    Json,
    Jsonl,
}

#[derive(Subcommand)]
enum Commands {
    #[command(subcommand)]
    Ingredient(IngredientCommand),
    #[command(subcommand)]
    Amount(AmountCommand),
    #[command(subcommand)]
    Text(TextCommand),
    #[command(subcommand)]
    Recipe(RecipeCommand),
    #[command(subcommand)]
    /// EPUB cookbook extraction (also available as `epub` for scripts).
    #[command(alias = "epub")]
    Cookbook(cookbook::Command),
    #[command(subcommand)]
    Corpus(CorpusCommand),
}

#[derive(Subcommand)]
enum IngredientCommand {
    Parse {
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
    Batch {
        /// Path to a file with one ingredient line per line (use - for stdin; blank lines reported)
        file: String,
    },
}

#[derive(Subcommand)]
enum AmountCommand {
    Parse {
        /// The amount to parse (e.g., "2 cups", "1/2 tsp")
        text: String,
    },
    Validate {
        /// The unit to validate (e.g., "cup", "tablespoon")
        unit: String,
        /// Additional custom units (comma-separated)
        #[arg(short = 'e', long)]
        extra_units: Option<String>,
    },
}

#[derive(Subcommand)]
enum TextCommand {
    Parse {
        /// The text to parse (e.g., "Add 1 cup flour and mix")
        text: String,
        /// Ingredient names to recognize (comma-separated)
        #[arg(short, long)]
        ingredients: String,
    },
}

#[derive(Subcommand)]
enum RecipeCommand {
    Scrape {
        url: String,
        #[arg(short, long)]
        parse: bool,
    },
}

#[derive(Subcommand)]
enum CorpusCommand {
    /// Compare two frozen evaluator outputs.
    Compare {
        before: std::path::PathBuf,
        after: std::path::PathBuf,
        #[arg(long)]
        book_id: Vec<String>,
    },
    /// Reproduce source-selected cookbook rows (does not overwrite labels).
    Sample {
        library: std::path::PathBuf,
        #[arg(long)]
        corpus: std::path::PathBuf,
    },
    /// Verify frozen source records and the benchmark against local books.
    Verify {
        library: std::path::PathBuf,
        #[arg(long)]
        corpus: std::path::PathBuf,
    },
    /// Render the corpus as an HTML review table.
    Table {
        #[arg(long, default_value = DEFAULT_CORPUS_PATH)]
        corpus: String,
        #[arg(long)]
        out: Option<String>,
    },
    /// Score independently authored cookbook labels.
    Evaluate {
        directory: std::path::PathBuf,
        #[arg(long, default_value = "development")]
        split: String,
    },
    /// Validate the accuracy corpus, and (with --report-stages) print a
    /// per-pass coverage report that flags parser passes firing on zero rows.
    Lint {
        /// Corpus file to lint (defaults to the repo's corpus.jsonl)
        #[arg(long, default_value = DEFAULT_CORPUS_PATH)]
        corpus: String,
        /// Print rows-per-pass coverage tables (normalize/recognize/refine) and a
        /// zero-coverage "possible dead pass" section. Without this flag, `lint`
        /// only validates that rows parse as JSON and prints the row count.
        #[arg(long)]
        report_stages: bool,
    },
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
    let ing = ip.from_str(input);
    // Refuse whatever the parser says needs review, rather than re-deriving the
    // rule here — this used to test the same booleans in its own words.
    if let Some(reason) = ing.parse_notes.review_reasons().first() {
        return Err(reason.to_string());
    }
    if ing.name.trim().is_empty() {
        return Err("parse produced an empty name".to_string());
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

/// Read a file or exit — the binary owns process termination, so library verbs
/// never do.
fn read_or_exit(path: &str) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| {
        eprintln!("failed to read {path}: {e}");
        std::process::exit(1);
    })
}

#[tokio::main]
async fn main() {
    // Surface the extractor's tracing (chunk skips, escalation, truncation) on
    // stderr. Off unless RUST_LOG is set, so normal --format json stdout stays clean;
    // try `RUST_LOG=cookbook=info`. Without this, those warns went nowhere.
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();
    // Load AI gateway creds (AI_GATEWAY_API_KEY, CLOUDFLARE_AI_GATEWAY_BASE_URL)
    // from a repo-root .env. Missing file is fine; real exported vars take precedence.
    let _ = dotenvy::dotenv();
    let cli = Cli::parse();

    let format = cli.format;
    if format == OutputFormat::Jsonl
        && !matches!(
            &cli.command,
            Commands::Ingredient(IngredientCommand::Batch { .. })
        )
    {
        eprintln!("--format jsonl is supported only by ingredient batch");
        std::process::exit(2);
    }
    if format != OutputFormat::Human
        && matches!(&cli.command, Commands::Corpus(CorpusCommand::Table { .. }))
    {
        eprintln!("corpus table writes HTML; --format is not supported");
        std::process::exit(2);
    }
    match &cli.command {
        Commands::Cookbook(command) => {
            match cookbook::execute(command.clone(), format != OutputFormat::Human).await {
                Ok(code) => {
                    if code != 0 {
                        std::process::exit(code);
                    }
                }
                Err(error) => {
                    eprintln!("{error}");
                    std::process::exit(1);
                }
            }
        }
        Commands::Recipe(RecipeCommand::Scrape { url, parse }) => {
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
                if format == OutputFormat::Json {
                    println!("{}", serde_json::to_string_pretty(&parsed).unwrap());
                } else {
                    print_value(&serde_json::to_value(&parsed).unwrap(), format)
                }
            } else if format == OutputFormat::Json {
                println!("{}", serde_json::to_string_pretty(&scraped).unwrap());
            } else {
                print_value(&serde_json::to_value(&scraped).unwrap(), format)
            }
        }
        Commands::Ingredient(IngredientCommand::Parse {
            name,
            debug,
            explain,
            jaeger_output,
            emit_corpus_row,
        }) => {
            if *emit_corpus_row {
                // Authoring helper: one JSONL row for the corpus, or a refusal.
                let ip = ingredient::IngredientParser::new();
                match build_corpus_row(&ip, name) {
                    Ok(row) => println!("{row}"),
                    Err(reason) => {
                        eprintln!(
                            "refusing to emit corpus row for {name:?}: {reason}\n\
                             (inspect with `ingredient parse {name:?} --explain`)"
                        );
                        std::process::exit(1);
                    }
                }
                return;
            }
            if *debug || *explain || jaeger_output.is_some() {
                let parser = ingredient::IngredientParser::new();
                let full_trace = *debug || jaeger_output.is_some();
                let execution = parser.parse_line(
                    name,
                    ingredient::ParseOptions {
                        decomposition: *explain,
                        trace: if full_trace {
                            ingredient::TraceDetail::Full
                        } else {
                            ingredient::TraceDetail::Stages
                        },
                    },
                );
                let use_color = std::io::IsTerminal::is_terminal(&std::io::stderr());

                // Export to Jaeger JSON if requested
                if let Some(output_path) = jaeger_output
                    && let Some(trace) = execution.trace.as_ref()
                {
                    let jaeger_json = trace.to_jaeger_json();
                    if let Err(e) = std::fs::write(output_path, &jaeger_json) {
                        eprintln!("Failed to write Jaeger JSON to {output_path}: {e}");
                        std::process::exit(1);
                    }
                    eprintln!("Wrote Jaeger trace to: {output_path}");
                }

                // Compact stage report — the routing view. The miette header
                // labels which text became each final field (amount/name/modifier)
                // and carets any digit that produced no amount; the stage view
                // below shows which pipeline stage shaped the line.
                if *explain
                    && let (Some(decomposition), Some(stages)) =
                        (execution.decomposition.as_ref(), execution.stages.as_ref())
                {
                    eprint!(
                        "{}",
                        explain::render(
                            decomposition,
                            &execution.ingredient.parse_notes,
                            use_color,
                        )
                    );
                    eprintln!();
                    eprintln!("{}", stages.format(use_color));
                }

                // Print the full trace tree if debug is enabled
                if *debug && let Some(trace) = execution.trace.as_ref() {
                    eprintln!("{}", trace.format_tree(use_color));
                }
                if format == OutputFormat::Json {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&execution.ingredient).unwrap()
                    );
                } else {
                    println!("{}", execution.ingredient);
                }
            } else {
                let res = ingredient::from_str(name);
                if format == OutputFormat::Json {
                    println!("{}", serde_json::to_string_pretty(&res).unwrap());
                } else {
                    println!("{res}");
                }
            }
        }
        Commands::Ingredient(IngredientCommand::Batch { file }) => {
            use std::io::Read;
            let contents = if file == "-" {
                let mut contents = String::new();
                if let Err(error) = std::io::stdin().read_to_string(&mut contents) {
                    eprintln!("failed to read stdin: {error}");
                    std::process::exit(1);
                }
                contents
            } else {
                read_or_exit(file)
            };
            let ip = ingredient::IngredientParser::new();
            let rows: Vec<_> = contents.lines().enumerate().map(|(index, line)| {
                let parsed = ip.from_str(line);
                let error = if line.trim().is_empty() { Some("empty ingredient line".to_owned()) }
                    else { parsed.parse_notes.review_reasons().first().map(ToString::to_string) };
                serde_json::json!({"line_number":index + 1,"line":line,"name":parsed.name,"amounts":parsed.amounts,"modifier":parsed.modifier,"optional":parsed.optional,"usage":parsed.usage,"error":error})
            }).collect();
            if format == OutputFormat::Jsonl {
                for row in &rows {
                    println!("{row}");
                }
            } else {
                print_value(&serde_json::json!(rows), format);
            }
        }
        Commands::Corpus(CorpusCommand::Compare {
            before,
            after,
            book_id,
        }) => {
            let result = (|| -> Result<_, Box<dyn std::error::Error>> {
                let before = serde_json::from_slice(&std::fs::read(before)?)?;
                let after = serde_json::from_slice(&std::fs::read(after)?)?;
                ingredient_corpus::cookbooks::compare(&before, &after, book_id)
            })();
            print_result(result, format);
        }
        Commands::Corpus(CorpusCommand::Sample { library, corpus }) => {
            print_result(
                ingredient_corpus::sampling::sample(library, corpus).map(
                    |(rows, manifest)| serde_json::json!({"rows": rows, "manifest": manifest}),
                ),
                format,
            );
        }
        Commands::Corpus(CorpusCommand::Verify { library, corpus }) => {
            print_result(ingredient_corpus::sampling::verify(library, corpus), format);
        }
        Commands::Corpus(CorpusCommand::Evaluate { directory, split }) => {
            match ingredient_corpus::cookbooks::evaluate(directory, Some(split)) {
                Ok(value) => print_value(&value, format),
                Err(error) => {
                    eprintln!("{error}");
                    std::process::exit(1);
                }
            }
        }
        Commands::Corpus(CorpusCommand::Lint {
            corpus,
            report_stages,
        }) => {
            let contents = read_or_exit(corpus);
            let outcome = corpus_lint::lint(&ingredient_corpus::parse(&contents), *report_stages);
            if !outcome.problems.is_empty() {
                // Sanity mode treats a malformed row as fatal; the report is
                // still useful mid-edit, so there it is only a warning.
                let label = if *report_stages {
                    "warning: skipping"
                } else {
                    "malformed"
                };
                eprintln!("{label} {} corpus row(s):", outcome.problems.len());
                for p in &outcome.problems {
                    eprintln!("  {p}");
                }
                if !*report_stages {
                    std::process::exit(1);
                }
            }
            if format == OutputFormat::Json {
                print_value(
                    &serde_json::json!({"report":outcome.report,"problems":outcome.problems}),
                    format,
                );
            } else {
                print!("{}", outcome.report);
            }
        }
        Commands::Corpus(CorpusCommand::Table { corpus, out }) => {
            let contents = match std::fs::read_to_string(corpus) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("failed to read {corpus}: {e}");
                    std::process::exit(1);
                }
            };
            let (html, rows) = corpus_table::render_html(&ingredient_corpus::parse(&contents));
            match out.as_deref() {
                Some("-") => print!("{html}"),
                Some(path) => {
                    write_or_exit(std::path::Path::new(path), &html);
                    eprintln!("wrote {path} ({rows} rows)");
                }
                None => {
                    let path = std::env::temp_dir().join("ingredient-corpus.html");
                    write_or_exit(&path, &html);
                    eprintln!("wrote {} ({rows} rows)", path.display());
                    // Best-effort: open in the default browser. Headless/SSH
                    // environments have no opener — just leave the path printed.
                    if let Err(e) = open::that(&path) {
                        eprintln!("(couldn't open a browser: {e} — open the file above manually)");
                    }
                }
            }
        }
        Commands::Amount(AmountCommand::Parse { text }) => {
            let parser = ingredient::IngredientParser::new();
            match parser.parse_amount(text) {
                Ok(amounts) => {
                    if format == OutputFormat::Json {
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
        Commands::Text(TextCommand::Parse { text, ingredients }) => {
            let ingredient_names: Vec<String> = ingredients
                .split(',')
                .map(|s| s.trim().to_string())
                .collect();

            let parser = ingredient::rich_text::RichParser::new(ingredient_names);
            match parser.parse(text) {
                Ok(rich) => {
                    if format == OutputFormat::Json {
                        println!("{}", serde_json::to_string_pretty(&rich).unwrap());
                    } else {
                        print_value(&serde_json::to_value(&rich).unwrap(), format);
                    }
                }
                Err(e) => {
                    eprintln!("Parse error: {e}");
                    std::process::exit(1);
                }
            }
        }
        Commands::Amount(AmountCommand::Validate { unit, extra_units }) => {
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

            if format == OutputFormat::Json {
                println!("{}", serde_json::json!({"unit":unit,"valid":is_valid}));
            } else {
                println!("{}", if is_valid { "valid" } else { "invalid" });
            }
            std::process::exit(if is_valid { 0 } else { 1 });
        }
    }
}

fn print_result(
    result: Result<serde_json::Value, Box<dyn std::error::Error>>,
    format: OutputFormat,
) {
    match result {
        Ok(value) => print_value(&value, format),
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
}

/// Compact, deterministic terminal rendering without debug structs or JSON punctuation.
fn print_value(value: &serde_json::Value, format: OutputFormat) {
    if format == OutputFormat::Json {
        println!("{}", serde_json::to_string_pretty(value).unwrap());
        return;
    }
    fn render(value: &serde_json::Value, indent: usize) {
        let pad = " ".repeat(indent);
        match value {
            serde_json::Value::Object(fields) => {
                for (key, value) in fields {
                    if value.is_object() || value.is_array() {
                        println!("{pad}{key}:");
                        render(value, indent + 2);
                    } else {
                        println!(
                            "{pad}{key}: {}",
                            value
                                .as_str()
                                .map(str::to_owned)
                                .unwrap_or_else(|| value.to_string())
                        );
                    }
                }
            }
            serde_json::Value::Array(items) => {
                for (index, item) in items.iter().enumerate() {
                    println!("{pad}[{}]", index + 1);
                    render(item, indent + 2);
                }
            }
            value => println!(
                "{pad}{}",
                value
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| value.to_string())
            ),
        }
    }
    render(value, 0);
}

fn write_or_exit(path: &std::path::Path, contents: &str) {
    if let Err(error) = std::fs::write(path, contents) {
        eprintln!("failed to write {}: {error}", path.display());
        std::process::exit(1);
    }
}
