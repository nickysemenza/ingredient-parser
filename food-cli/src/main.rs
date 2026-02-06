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
