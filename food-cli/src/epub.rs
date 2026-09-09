//! Headless cookbook review. Rendering is owned by the binary; exit 3 denotes incomplete extraction,
//! exit 4 denotes expectation mismatches, and exit 1 denotes an operational error.
use clap::Subcommand;
use recipe_epub::review::{ReviewRun, RunOptions};
use std::path::PathBuf;

#[derive(Subcommand)]
pub enum Command {
    /// List registered cookbook extraction runs.
    Runs {
        #[arg(long)]
        book: Option<PathBuf>,
    },
    /// List curated extraction models and compatibility status.
    Models,
    /// Inspect cleaned source chunks without AI or credentials.
    Inspect {
        book: PathBuf,
        #[arg(long)]
        chunk: Option<String>,
    },
    /// Extract to a durable run. Cache-only unless network is explicitly enabled.
    Extract {
        book: PathBuf,
        #[arg(long)]
        out: Option<PathBuf>,
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        resume: bool,
        /// Start a new run from saved chunk outputs, preserving the parent.
        #[arg(long, conflicts_with = "resume")]
        from: Option<PathBuf>,
        #[arg(long)]
        allow_network: bool,
        #[arg(long, requires = "allow_network")]
        refresh: bool,
        #[arg(long, default_value = recipe_epub::models::DEFAULT_MODEL)]
        model: String,
        #[arg(long)]
        cache_dir: Option<PathBuf>,
        #[arg(long)]
        chunk: Vec<String>,
        #[arg(long, default_value_t = 10.0)]
        budget_usd: f64,
    },
    /// Inspect a saved run or select one source chunk.
    Show {
        run: PathBuf,
        #[arg(long, conflicts_with = "chunk")]
        summary: bool,
        #[arg(long, conflicts_with_all = ["chunk", "summary"])]
        recipe: Option<usize>,
        #[arg(long)]
        chunk: Option<String>,
    },
    /// Reassemble and parse saved model outputs, entirely offline.
    Replay {
        run: PathBuf,
        /// Source image captions transcribed visually or by OCR, checked against this EPUB.
        #[arg(long)]
        image_text: Option<PathBuf>,
        /// Refresh source inspection from the identical EPUB without model calls.
        #[arg(long)]
        source: Option<PathBuf>,
        #[arg(long)]
        out: PathBuf,
    },
    /// Compare a run with separately authored source expectations.
    Evaluate {
        run: PathBuf,
        #[arg(long)]
        expectations: PathBuf,
    },
    /// Inspect source-to-result matches, unassigned blocks, images, and links.
    Audit { run: PathBuf },
    /// Compare saved extraction and parser outputs.
    #[command(alias = "compare")]
    Diff { before: PathBuf, after: PathBuf },
}

pub async fn execute(
    command: &Command,
) -> Result<(serde_json::Value, i32), Box<dyn std::error::Error>> {
    let mut code = 0;
    let value = match command {
        Command::Runs { book } => {
            serde_json::to_value(recipe_epub::review::store::list(book.as_deref())?)?
        }
        Command::Models => serde_json::to_value(recipe_epub::models::catalog())?,
        Command::Inspect { book, chunk } => {
            let run =
                recipe_epub::review::store::inspect(book, recipe_epub::models::DEFAULT_MODEL)?;
            select(&run, chunk)?
        }
        Command::Show {
            run,
            chunk,
            summary,
            recipe,
        } => {
            let run = ReviewRun::read(run)?;
            if *summary {
                run_summary(&run, None)
            } else if let Some(index) = recipe {
                serde_json::json!({"recipe":run.recipes.get(*index).ok_or("recipe index out of range")?,"parsed":run.parsed.get(*index)})
            } else {
                select(&run, chunk)?
            }
        }
        Command::Replay {
            run,
            out,
            source,
            image_text,
        } => {
            let outcome = recipe_epub::review::replay_to_run(recipe_epub::review::ReplayRequest {
                run: run.clone(),
                out: out.clone(),
                source: source.clone(),
                image_text: image_text.clone(),
            })?;
            let saved = outcome.run;
            if saved.incomplete() {
                code = 3;
            }
            run_summary(&saved, Some(out))
        }
        Command::Extract {
            book,
            out,
            resume,
            from,
            allow_network,
            refresh,
            model,
            cache_dir,
            chunk,
            budget_usd,
            dry_run,
        } => {
            let request = recipe_epub::review::ExtractionRequest {
                book: book.clone(),
                out: out.clone().unwrap_or_default(),
                model: model.clone(),
                resume: *resume,
                from: from.clone(),
                options: RunOptions {
                    allow_network: *allow_network,
                    refresh: *refresh,
                    cache_dir: cache_dir.clone(),
                    chunks: chunk.clone(),
                    budget_usd: *budget_usd,
                },
            };
            if *dry_run {
                let run = recipe_epub::review::prepare(&request)?;
                return Ok((
                    serde_json::to_value(recipe_epub::review::preflight::plan(
                        &run,
                        &request.options,
                    )?)?,
                    0,
                ));
            }
            let control = recipe_epub::review::ExtractionControl::default();
            let extraction = recipe_epub::review::extract_to_run_controlled(
                request,
                &control,
                |p| {
                    eprintln!(
                        "EPUB: {}/{} chunks · {} active · {} failed · {} recipes · {}s · {} estimated · ${:.4} spent/reserved{}",
                        p.completed,
                        p.total,
                        p.active,
                        p.failed,
                        p.recipes,
                        p.elapsed_seconds,
                        p.estimated_usd
                            .map(|v| format!("${v:.4}"))
                            .unwrap_or_else(|| "unknown".into()),
                        p.reserved_usd,
                        if p.stopping {
                            " · stopping; saving active requests"
                        } else {
                            ""
                        }
                    );
                },
            );
            tokio::pin!(extraction);
            let outcome = tokio::select! {
                result = &mut extraction => result?,
                signal = tokio::signal::ctrl_c() => {
                    signal?;
                    control.cancel();
                    eprintln!("Stopping new requests; waiting for active requests to save. Resume with --resume --out <saved path>.");
                    extraction.await?
                }
            };
            if control.is_cancelled() {
                code = 130;
            }
            eprintln!(
                "Extraction {}: {}",
                outcome.run.status(),
                outcome.path.display()
            );
            let run = outcome.run;
            if run.incomplete() && code == 0 {
                code = 3;
            }
            run_summary(&run, Some(&outcome.path))
        }
        Command::Evaluate { run, expectations } => {
            let expected: serde_json::Value =
                serde_json::from_slice(&std::fs::read(expectations)?)?;
            let run = ReviewRun::read(run)?;
            let result = recipe_epub::review::evaluate_document(&run, expected)?;
            if run.incomplete() {
                code = 3;
            } else if recipe_epub::review::evaluation_failed(&result) {
                code = 4;
            }

            result
        }
        Command::Audit { run } => recipe_epub::review::source_audit(&ReviewRun::read(run)?),
        Command::Diff { before, after } => {
            recipe_epub::review::diff(&ReviewRun::read(before)?, &ReviewRun::read(after)?)?
        }
    };
    Ok((value, code))
}

fn select(
    run: &ReviewRun,
    chunk: &Option<String>,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    match chunk {
        None => Ok(serde_json::to_value(run)?),
        Some(id) => Ok(serde_json::to_value(
            run.chunks
                .iter()
                .find(|c| &c.id == id)
                .ok_or("unknown chunk")?,
        )?),
    }
}

fn run_summary(run: &ReviewRun, path: Option<&std::path::Path>) -> serde_json::Value {
    serde_json::json!({"quality_issues":recipe_epub::review::quality::issues(run),"status":run.status(),"run":path,"source":run.source,"epub_sha256":run.epub_sha256,"chunks":run.chunks.len(),"completed_chunks":run.chunks.iter().filter(|c|c.output.is_some()).count(),"recipes":run.recipes.len(),"cached":run.chunks.iter().filter(|c| c.cached).count(),"incomplete":run.incomplete(),"reserved_usd":run.reserved_usd,"failures":run.chunks.iter().filter(|c| c.error.is_some()).map(|c| serde_json::json!({"id":c.id,"error":c.error})).collect::<Vec<_>>()})
}

/// Terminal presentation only; history and comparison policy remain shared Rust.
pub fn print_human(command: &Command, value: &serde_json::Value) -> bool {
    let money = |value: &serde_json::Value| {
        value
            .as_f64()
            .map(|v| format!("${v:.4}"))
            .unwrap_or_else(|| "unknown".into())
    };
    match command {
        Command::Runs { .. } => {
            let rows = value.as_array().map(Vec::as_slice).unwrap_or_default();
            if rows.is_empty() {
                println!("No cookbook extractions yet.");
            }
            for row in rows {
                let text = |key: &str| row[key].as_str().unwrap_or("unknown");
                println!(
                    "{} · {} · {} recipes · {}/{} chunks",
                    text("title"),
                    text("status"),
                    row["recipes"],
                    row["completed"],
                    row["total"]
                );
                let date = row["created_at"]
                    .as_u64()
                    .map(|at| {
                        let hours = recipe_epub::review::store::now().saturating_sub(at) / 3600;
                        if hours == 0 {
                            "within the last hour".into()
                        } else {
                            format!("{hours} hours ago")
                        }
                    })
                    .unwrap_or_else(|| "date unknown".into());
                println!(
                    "  {} · {} · {} · estimated {} · unresolved {}",
                    text("model"),
                    text("prompt_version"),
                    date,
                    money(&row["new_spend_usd"]),
                    money(&row["unresolved_usd"])
                );
                println!("  Source review flags: {}", row["quality_flags"]);
                println!("  {}", text("path"));
            }
            println!(
                "Compare: cookbook compare <before> <after>. Resume: cookbook extract <epub> --resume --out <path> --model <model> --allow-network"
            );
            true
        }
        Command::Extract { .. } | Command::Show { summary: true, .. }
            if value.get("run").is_some() =>
        {
            println!(
                "Extraction {} · {} recipes · {}/{} chunks",
                value["status"].as_str().unwrap_or("unknown"),
                value["recipes"],
                value["completed_chunks"],
                value["chunks"]
            );
            if let Some(path) = value["run"].as_str() {
                println!("Saved: {path}");
            }
            let issues = value["quality_issues"]
                .as_array()
                .map(Vec::as_slice)
                .unwrap_or_default();
            let unknown = issues
                .iter()
                .filter(|i| i["kind"] == "unextracted_chunk")
                .count();
            println!(
                "Source checks: {unknown} unextracted chunks · {} review flags",
                issues.len() - unknown
            );
            for issue in issues
                .iter()
                .filter(|i| i["kind"] != "unextracted_chunk")
                .take(10)
            {
                println!(
                    "  {}: {}",
                    issue["source"].as_str().unwrap_or("source"),
                    issue["message"].as_str().unwrap_or("Review source")
                );
            }
            println!("Use cookbook audit <run> for review details.");
            true
        }
        Command::Audit { .. } => {
            for issue in value["quality_issues"].as_array().into_iter().flatten() {
                println!(
                    "{} · {}",
                    issue["source"].as_str().unwrap_or("source"),
                    issue["message"].as_str().unwrap_or("Review source")
                );
                if let Some(detail) = issue["detail"].as_str() {
                    println!("  {detail}");
                }
            }
            println!(
                "Use --format json for source block attribution and structured review flags. These signals do not prove complete recipe coverage."
            );
            true
        }
        Command::Diff { .. } => {
            println!(
                "{} ({}) → {} ({})",
                value["before_model"],
                value["before_status"],
                value["after_model"],
                value["after_status"]
            );
            println!(
                "Recipes: {} → {} · estimated cost: {} → {}",
                value["before_recipes"],
                value["after_recipes"],
                money(&value["before_cost"]),
                money(&value["after_cost"])
            );
            for source in value["changed_sources"].as_array().into_iter().flatten() {
                println!(
                    "Changed: {}",
                    source["source"].as_str().unwrap_or("unknown")
                );
            }
            println!("Use --format json for recipe and parser differences.");
            true
        }
        _ => false,
    }
}
