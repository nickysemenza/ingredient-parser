//! Headless cookbook review. Rendering is owned by the binary; exit 3 denotes incomplete extraction,
//! exit 4 denotes expectation mismatches, and exit 1 denotes an operational error.
use clap::Subcommand;
use recipe_epub::review::{ReviewRun, RunOptions};
use std::path::PathBuf;

#[derive(Subcommand)]
pub enum Command {
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
        out: PathBuf,
        #[arg(long)]
        resume: bool,
        /// Start a new run from saved chunk outputs, preserving the parent.
        #[arg(long, conflicts_with = "resume")]
        from: Option<PathBuf>,
        #[arg(long)]
        allow_network: bool,
        #[arg(long, requires = "allow_network")]
        refresh: bool,
        #[arg(long, default_value = "gemini-2.5-flash")]
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
    Diff { before: PathBuf, after: PathBuf },
}

pub async fn execute(
    command: &Command,
) -> Result<(serde_json::Value, i32), Box<dyn std::error::Error>> {
    let mut code = 0;
    let value = match command {
        Command::Inspect { book, chunk } => {
            let run = ReviewRun::inspect(
                &std::fs::read(book)?,
                &book.to_string_lossy(),
                "gemini-2.5-flash",
            )?;
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
        } => {
            let outcome = recipe_epub::review::extract_to_run(
                recipe_epub::review::ExtractionRequest {
                    book: book.clone(),
                    out: out.clone(),
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
                },
                |progress| {
                    eprintln!(
                        "EPUB: {}/{} chunks complete, {} recipes, ${:.4} reserved/spent",
                        progress.completed, progress.total, progress.recipes, progress.reserved_usd
                    );
                },
            )
            .await?;
            let run = outcome.run;
            if run.incomplete() {
                code = 3;
            }
            run_summary(&run, Some(out))
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
    serde_json::json!({"run":path,"source":run.source,"epub_sha256":run.epub_sha256,"chunks":run.chunks.len(),"completed_chunks":run.chunks.iter().filter(|c|c.output.is_some()).count(),"recipes":run.recipes.len(),"cached":run.chunks.iter().filter(|c| c.cached).count(),"incomplete":run.incomplete(),"reserved_usd":run.reserved_usd,"failures":run.chunks.iter().filter(|c| c.error.is_some()).map(|c| serde_json::json!({"id":c.id,"error":c.error})).collect::<Vec<_>>()})
}
