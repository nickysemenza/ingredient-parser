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
        #[arg(long)]
        paths: bool,
    },
    /// Compare latest processing results per book, model, and prompt.
    Results {
        #[arg(long)]
        book: Option<PathBuf>,
    },
    /// Show AI verification findings and recovery attempts.
    Feedback { run: PathBuf },
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
        #[arg(long, default_value = recipe_epub::recovery::AUTOMATIC)]
        model: String,
        #[arg(long)]
        cache_dir: Option<PathBuf>,
        #[arg(long)]
        chunk: Vec<String>,
        #[arg(long, default_value_t = 10.0)]
        budget_usd: f64,
        #[arg(long, default_value_t = 4, value_parser = clap::value_parser!(u8).range(1..=4))]
        concurrency: u8,
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
    Audit {
        run: PathBuf,
        #[arg(long)]
        attribution: bool,
    },
    /// Compare saved extraction and parser outputs.
    #[command(alias = "compare")]
    Diff { before: PathBuf, after: PathBuf },
}

pub async fn execute(
    command: &Command,
) -> Result<(serde_json::Value, i32), Box<dyn std::error::Error>> {
    let mut code = 0;
    let value = match command {
        Command::Runs { book, .. } => {
            serde_json::to_value(recipe_epub::review::store::list(book.as_deref())?)?
        }
        Command::Results { book } => {
            serde_json::to_value(recipe_epub::review::results::list(book.as_deref())?)?
        }
        Command::Feedback { run } => serde_json::to_value(ReviewRun::read(run)?.recovery)?,
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
            concurrency,
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
                    concurrency: usize::from(*concurrency),
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
                        "{}: {}/{} chunks · {} active · {} failed attempts · {} recipes · {}s · {} estimated · ${:.4} reserved (not confirmed spend) · {}{}",
                        p.phase,
                        p.completed,
                        p.total,
                        p.active,
                        p.failed,
                        p.recipes,
                        p.elapsed_seconds,
                        p.estimated_usd
                            .map(|v| format!("${v:.4}"))
                            .unwrap_or_else(|| "unknown".into()),
                        p.unresolved_usd,
                        p.active_models.join(", "),
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
        Command::Audit { run, attribution } => {
            let run = ReviewRun::read(run)?;
            if *attribution {
                recipe_epub::review::source_audit(&run)
            } else {
                serde_json::json!({"epub_sha256":run.epub_sha256,"quality_issues":recipe_epub::review::quality::issues(&run)})
            }
        }
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
    serde_json::json!({"extraction_feedback":run.recovery,"quality_issues":recipe_epub::review::quality::issues(run),"status":run.status(),"run":path,"source":run.source,"epub_sha256":run.epub_sha256,"chunks":run.chunks.len(),"completed_chunks":run.chunks.iter().filter(|c|c.output.is_some()).count(),"recipes":run.recipes.len(),"cached":run.chunks.iter().filter(|c| c.cached).count(),"incomplete":run.incomplete(),"reserved_usd":run.reserved_usd,"failures":run.chunks.iter().filter(|c| c.error.is_some()).map(|c| serde_json::json!({"id":c.id,"error":c.error})).collect::<Vec<_>>()})
}

/// Terminal presentation only; history and comparison policy remain shared Rust.
pub fn print_human(command: &Command, value: &serde_json::Value) -> bool {
    let feedback = if matches!(command, Command::Feedback { .. }) {
        value
    } else {
        &value["extraction_feedback"]
    };
    if feedback.is_object() {
        println!(
            "Extraction feedback: {}",
            feedback["phase"].as_str().unwrap_or("Not assessed")
        );
        if let Some(reason) = feedback["stop_reason"].as_str() {
            println!("{reason}");
        }
        let mut rows = vec![];
        let mut extraction = 0.0;
        let mut verification = 0.0;
        let mut unresolved = 0.0;
        for attempt in feedback["attempts"].as_array().into_iter().flatten() {
            let reservation = attempt["reservation_usd"].as_f64().unwrap_or(0.0);
            let charge = attempt["estimated_usd"].as_f64();
            if charge.is_none() {
                unresolved += reservation;
            }
            if attempt["verification"] == true {
                verification += charge.unwrap_or(0.0);
            } else {
                extraction += charge.unwrap_or(0.0);
            }
        }
        println!(
            "Extraction/recovery: ${extraction:.4} · Verification: ${verification:.4} · ${unresolved:.4} unresolved reservations"
        );
        for group in feedback["groups"].as_array().into_iter().flatten() {
            let resolved = group["accepted"].is_number();
            for candidate in group["candidates"].as_array().into_iter().flatten() {
                if candidate["verified"] == true {
                    rows.push(vec![
                        "Verification".into(),
                        candidate["model"].as_str().unwrap_or("unknown").into(),
                        group["chunks"].to_string(),
                        if resolved {
                            "All automated group checks passed".into()
                        } else {
                            "Source issues detected".into()
                        },
                    ]);
                }
                for finding in candidate["feedback"].as_array().into_iter().flatten() {
                    rows.push(vec![
                        if resolved {
                            "Recovered".into()
                        } else {
                            "Unresolved".into()
                        },
                        finding["model"].as_str().unwrap_or("unknown").into(),
                        format!("chunk {} lines {}", finding["chunk"], finding["lines"]),
                        finding["message"].as_str().unwrap_or("").into(),
                    ]);
                }
            }
        }
        if food_cli::tables::interactive() {
            println!(
                "{}",
                food_cli::tables::terminal_table(
                    &["Result", "Model", "Source", "AI / source feedback"],
                    &rows
                )
            );
        } else {
            for row in rows {
                println!("{}", row.join(" · "));
            }
        }
    }
    if matches!(command, Command::Feedback { .. }) {
        if feedback.is_null() {
            println!("Automated verification: Not assessed");
        }
        return true;
    }
    if matches!(command, Command::Results { .. }) {
        let rows: Vec<Vec<String>> = value["rows"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|row| {
                let r = &row["latest"];
                let text = |v: &serde_json::Value| {
                    v.as_str()
                        .map(str::to_owned)
                        .unwrap_or_else(|| v.to_string())
                };
                vec![
                    text(&r["title"]),
                    if let Some(configurations) =
                        r["configurations"].as_array().filter(|c| c.len() > 1)
                    {
                        format!(
                            "Mixed:\n{}",
                            configurations
                                .iter()
                                .map(text)
                                .collect::<Vec<_>>()
                                .join("\n")
                        )
                    } else {
                        format!("{}\n{}", text(&r["model"]), text(&r["prompt_version"]))
                    },
                    row["processing_success_rate"]
                        .as_f64()
                        .map(|p| format!("{:.1}%", p * 100.0))
                        .unwrap_or_else(|| "unknown".into()),
                    format!(
                        "{}/{} complete; {} failed; {} pending",
                        r["completed"], r["total"], row["failed_chunks"], row["pending_chunks"]
                    ),
                    format!("{} recipes", r["recipes"]),
                    row["attempts"]
                        .as_u64()
                        .map(|n| format!("{n} calls; {} failed", row["failed_attempts"]))
                        .unwrap_or_else(|| "unknown".into()),
                    format!(
                        "{} new / ${:.4} unresolved / {} inherited reservation",
                        r["new_spend_usd"]
                            .as_f64()
                            .map(|v| format!("${v:.4}"))
                            .unwrap_or_else(|| "unknown".into()),
                        r["unresolved_usd"].as_f64().unwrap_or(0.0),
                        r["inherited_reserved_usd"]
                            .as_f64()
                            .map(|v| format!("${v:.4}"))
                            .unwrap_or_else(|| "unknown".into())
                    ),
                ]
            })
            .collect();
        println!(
            "Latest run per book/model/prompt. Success = completed / (completed + failed) chunks, including cache reuse. Pending chunks excluded. This does not verify recipe fidelity."
        );
        if food_cli::tables::interactive() {
            println!(
                "{}",
                food_cli::tables::terminal_table(
                    &[
                        "Book",
                        "Model / prompt",
                        "Success",
                        "Book coverage",
                        "Recipes",
                        "Attempts",
                        "Estimate / unresolved"
                    ],
                    &rows
                )
            );
        } else {
            for row in rows {
                println!("{}", row.join(" · ").replace('\n', " / "));
            }
        }
        for warning in value["unreadable"].as_array().into_iter().flatten() {
            eprintln!("Unreadable run: {warning}");
        }
        return true;
    }
    if food_cli::tables::interactive() && print_table(command, value) {
        return true;
    }
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
                println!("  Source checks: {}", row["quality_flags"]);
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
                "Source checks: {unknown} unextracted chunks · {} source checks",
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
            println!("Use cookbook feedback <run> for AI verification and recovery details.");
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
                "Use --attribution --format json for source block attribution and structured source checks. These signals do not prove complete recipe coverage."
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

fn print_table(command: &Command, value: &serde_json::Value) -> bool {
    let text = |v: &serde_json::Value| -> String {
        match v {
            serde_json::Value::Null => "unknown".into(),
            serde_json::Value::String(s) => s.clone(),
            _ => v.to_string(),
        }
    };
    let money = |v: &serde_json::Value| {
        v.as_f64()
            .map(|n| format!("${n:.4}"))
            .unwrap_or_else(|| "unknown".into())
    };
    let (headers, rows): (Vec<&str>, Vec<Vec<String>>) = match command {
        Command::Runs { paths, .. } => {
            let mut headers = vec![
                "Book / identity",
                "Status",
                "Recipes / chunks",
                "Model / prompt",
                "Age",
                "Estimated / reserved",
                "Flags",
            ];
            if *paths {
                headers.push("Path");
            }
            let rows = value
                .as_array()
                .into_iter()
                .flatten()
                .map(|r| {
                    let identity = std::path::Path::new(r["path"].as_str().unwrap_or(""))
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .and_then(|s| s.rsplit("--").next())
                        .unwrap_or("unknown");
                    let mut row = vec![
                        format!("{}\n{}", text(&r["title"]), identity),
                        text(&r["status"]),
                        format!(
                            "{} recipes\n{}/{} chunks",
                            r["recipes"], r["completed"], r["total"]
                        ),
                        format!("{}\n{}", text(&r["model"]), text(&r["prompt_version"])),
                        r["created_at"]
                            .as_u64()
                            .map(|t| {
                                format!(
                                    "{}h",
                                    recipe_epub::review::store::now().saturating_sub(t) / 3600
                                )
                            })
                            .unwrap_or_else(|| "unknown".into()),
                        format!(
                            "{} estimated\n{} unresolved",
                            money(&r["new_spend_usd"]),
                            money(&r["unresolved_usd"])
                        ),
                        text(&r["quality_flags"]),
                    ];
                    if *paths {
                        row.push(text(&r["path"]));
                    }
                    row
                })
                .collect();
            (headers, rows)
        }
        Command::Models => (
            vec!["Model", "Transport", "Availability"],
            value
                .as_array()
                .into_iter()
                .flatten()
                .map(|r| {
                    vec![
                        format!("{}\n{}", text(&r["label"]), text(&r["id"])),
                        text(&r["transport"]),
                        format!(
                            "{}\n{}",
                            if r["enabled"].as_bool() == Some(true) {
                                "Available"
                            } else {
                                "Unavailable"
                            },
                            text(&r["status"])
                        ),
                    ]
                })
                .collect(),
        ),
        Command::Audit { .. } => (
            vec!["Kind", "Source / chunk", "Finding"],
            value["quality_issues"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|r| {
                    vec![
                        text(&r["kind"]),
                        format!(
                            "{}\n{}",
                            text(&r["source"]),
                            r["chunk"].as_str().unwrap_or("")
                        ),
                        format!(
                            "{}\n{}",
                            text(&r["message"]),
                            r["detail"].as_str().unwrap_or("")
                        ),
                    ]
                })
                .collect(),
        ),
        Command::Extract { dry_run: true, .. } => (
            vec!["Preflight", "Value"],
            [
                "total",
                "cached",
                "pending",
                "estimated_low_usd",
                "estimated_high_usd",
                "reservation_usd",
                "basis",
            ]
            .iter()
            .map(|key| {
                vec![
                    key.replace('_', " "),
                    if key.ends_with("usd") {
                        money(&value[key])
                    } else {
                        text(&value[key])
                    },
                ]
            })
            .collect(),
        ),
        Command::Diff { .. } => (
            vec!["Comparison", "Before", "After"],
            ["model", "status", "recipes", "cost"]
                .iter()
                .map(|key| {
                    vec![
                        key.to_string(),
                        text(&value[format!("before_{key}")]),
                        text(&value[format!("after_{key}")]),
                    ]
                })
                .chain(std::iter::once(vec![
                    "Changed sources".into(),
                    value["changed_sources"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .map(&text)
                        .collect::<Vec<_>>()
                        .join("\n"),
                    String::new(),
                ]))
                .collect(),
        ),
        Command::Show { summary: true, .. } => (
            vec!["Extraction", "Value"],
            vec![
                vec!["Status".into(), text(&value["status"])],
                vec!["Recipes".into(), text(&value["recipes"])],
                vec![
                    "Chunks".into(),
                    format!("{}/{}", value["completed_chunks"], value["chunks"]),
                ],
                vec!["Spent/reserved".into(), money(&value["reserved_usd"])],
                vec!["Source".into(), text(&value["source"])],
            ]
            .into_iter()
            .chain(value["failures"].as_array().into_iter().flatten().map(|f| {
                vec![
                    text(&f["id"]),
                    f["error"]
                        .as_str()
                        .unwrap_or("unknown")
                        .split("; selection=")
                        .next()
                        .unwrap_or("unknown")
                        .into(),
                ]
            }))
            .collect(),
        ),
        _ => return false,
    };
    println!("{}", food_cli::tables::terminal_table(&headers, &rows));
    true
}
