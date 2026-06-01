// CLI application - panics are acceptable for fatal errors
#![allow(clippy::unwrap_used)]

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[clap(author, version, about, long_about = None)]
#[clap(propagate_version = true)]
struct Cli {
    #[clap(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Scrape {
        #[clap(value_parser)]
        url: String,
        #[arg(short, long)]
        json: bool,
        #[arg(short, long)]
        parse: bool,
    },
    /// Scrape every recipe from a local EPUB cookbook file (AI-assisted)
    ScrapeEpub {
        /// Path to the .epub file
        #[clap(value_parser)]
        path: String,
        #[arg(short, long)]
        json: bool,
        #[arg(short, long)]
        parse: bool,
        /// Model id override (default: gemini-2.5-flash; claude-* / gpt-* also work)
        #[arg(long)]
        model: Option<String>,
        /// Bypass the on-disk extraction cache
        #[arg(long)]
        no_cache: bool,
    },
    /// Scan a Calibre/EPUB library, ranking ingredient lines the parser misses
    /// (have a number but yield no amount) as accuracy-corpus candidates.
    ScanCookbooks {
        /// Directory to scan recursively for .epub files
        #[clap(value_parser)]
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
        #[clap(value_parser)]
        name: String,
        /// Enable debug trace output showing which parsers were used
        #[arg(short, long)]
        debug: bool,
        /// Export trace to Jaeger JSON format and write to file
        #[arg(long)]
        jaeger_output: Option<String>,
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

/// Recursively collect `.epub` files under `dir`.
fn find_epubs(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else {
            continue;
        };
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                stack.push(p);
            } else if p
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("epub"))
            {
                out.push(p);
            }
        }
    }
    out
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    match &cli.command {
        Commands::Scrape { url, json, parse } => {
            let s = recipe_scraper_fetcher::Fetcher::new();
            let scraped = s.scrape_url(url).await.unwrap();
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
            model,
            no_cache,
        } => {
            let bytes = std::fs::read(path).unwrap();
            let opts = recipe_epub::Options {
                model: model.clone(),
                use_cache: !no_cache,
                ..Default::default()
            };
            match recipe_epub::extract_cookbook(&bytes, path, &opts).await {
                Ok((recipes, stats)) => {
                    // Cost/cache summary goes to stderr so --json stdout stays clean.
                    eprintln!("[{}] {}", stats.model, stats.summary());
                    if *parse {
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
        Commands::ScanCookbooks {
            dir,
            limit,
            bottom,
            concurrency,
        } => {
            let mut epubs = find_epubs(std::path::Path::new(dir));
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
            for (line, n) in ranked.into_iter().take(*bottom) {
                println!("{n}\t{line}");
            }
        }
        Commands::ParseIngredient {
            name,
            debug,
            jaeger_output,
        } => {
            if *debug || jaeger_output.is_some() {
                // Use parse_with_trace for debug output or Jaeger export
                let parser = ingredient::IngredientParser::new();
                let result = parser.parse_with_trace(name);

                // Export to Jaeger JSON if requested
                if let Some(output_path) = jaeger_output {
                    let jaeger_json = result.trace.to_jaeger_json();
                    if let Err(e) = std::fs::write(output_path, &jaeger_json) {
                        eprintln!("Failed to write Jaeger JSON to {output_path}: {e}");
                        std::process::exit(1);
                    }
                    eprintln!("Wrote Jaeger trace to: {output_path}");
                }

                // Print the trace tree if debug is enabled
                if *debug {
                    let use_color = std::io::IsTerminal::is_terminal(&std::io::stdout());
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
                let res = ingredient::from_str(name);
                println!("{}", serde_json::to_string_pretty(&res).unwrap());
                println!("{res}")
            }
        }
        Commands::ParseAmount { text, json } => {
            let parser = ingredient::IngredientParser::new();
            match parser.parse_amount(text) {
                Ok(amounts) => {
                    if *json {
                        println!("{}", serde_json::to_string_pretty(&amounts).unwrap());
                    } else {
                        for amount in amounts {
                            println!("{amount}");
                        }
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

            // Try to parse a simple amount with the unit and check if the parsed unit matches
            let test_input = format!("1 {unit}");
            let is_valid = parser
                .parse_amount(&test_input)
                .map(|amounts| {
                    !amounts.is_empty()
                        && amounts[0].unit().to_str().to_lowercase() == unit.to_lowercase()
                })
                .unwrap_or(false);

            println!("{}", if is_valid { "valid" } else { "invalid" });
            std::process::exit(if is_valid { 0 } else { 1 });
        }
    }
}
