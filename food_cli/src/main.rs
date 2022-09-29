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
    },
    ParseIngredient {
        #[clap(value_parser)]
        name: String,
    },
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    match &cli.command {
        Commands::Scrape { url } => {
            let s = recipe_scraper::Scraper::new();
            let res = s.scrape_url(url).await.unwrap();
            println!("{}", serde_json::to_string_pretty(&res).unwrap());
            println!("{:#?}", res.parse())
        }
        Commands::ParseIngredient { name } => {
            let res = ingredient::from_str(name);
            println!("{}", serde_json::to_string_pretty(&res).unwrap());
        }
    }
}
