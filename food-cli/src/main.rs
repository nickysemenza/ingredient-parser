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
                    let use_color = atty::is(atty::Stream::Stdout);
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
    }
}
