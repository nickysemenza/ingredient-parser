//! `food-cli cookbook`: inspect, estimate, extract, explain, replay, sample,
//! runs, and models. Every command prints JSON with `--format json`; human
//! output is tables and summaries. Extraction needs gateway credentials
//! (`CLOUDFLARE_AI_GATEWAY_BASE_URL`, `AI_GATEWAY_API_KEY`); everything else
//! runs offline.

use std::io::IsTerminal;
use std::path::{Path, PathBuf};

use clap::{Args, Subcommand};
use cookbook::cache::{ChunkCache, NoCache};
use cookbook::classify::{Classification, classify_structure};
use cookbook::eval::{self, Expectations};
use cookbook::native::runs::{self, RunSummary};
use cookbook::native::{DumpTransport, FsChunkCache, ReplayTransport, ReqwestTransport};
use cookbook::{
    Book, CancelToken, ExtractOptions, Extraction, HttpRequest, HttpResponse, Item, Progress,
    Transport, TransportError,
};
use rand::SeedableRng;
use rand::seq::SliceRandom;
use serde_json::{Value, json};

use crate::tables::terminal_table;

/// Exit code when some chunk failed every model.
pub const EXIT_INCOMPLETE: i32 = 3;

#[derive(Subcommand, Clone)]
pub enum Command {
    /// Open a book offline: metadata, contents, chunks, classification.
    Inspect {
        book: PathBuf,
        /// List every chunk.
        #[arg(long)]
        chunks: bool,
        /// Print one chunk exactly as the model sees it.
        #[arg(long)]
        chunk: Option<String>,
        /// Print cleaned lines with provenance, e.g. `120..160`.
        #[arg(long)]
        lines: Option<String>,
        /// Include the table of contents.
        #[arg(long)]
        nav: bool,
        /// Include the printed-page map.
        #[arg(long)]
        pages: bool,
    },
    /// Cost and time before spending anything.
    Estimate {
        book: PathBuf,
        #[command(flatten)]
        flags: RunFlags,
        #[arg(long)]
        no_cache: bool,
    },
    /// Extract the book. Saves the run to the run store (or `--out`).
    Extract {
        book: PathBuf,
        #[arg(long)]
        out: Option<PathBuf>,
        /// Record every request and response here for `replay`.
        #[arg(long)]
        dump: Option<PathBuf>,
        #[arg(long)]
        no_cache: bool,
        #[command(flatten)]
        flags: RunFlags,
    },
    /// How one item came to be: its lines, chunk, calls, and references.
    Explain {
        run: PathBuf,
        /// Item id, e.g. `003.0042`.
        #[arg(long)]
        recipe: Option<String>,
        /// Case-insensitive title substring.
        #[arg(long)]
        title: Option<String>,
        /// The EPUB, to show the source lines with what claimed them.
        #[arg(long)]
        book: Option<PathBuf>,
    },
    /// Re-run the whole pipeline from a dump, with no credentials.
    Replay {
        #[arg(long)]
        dump: PathBuf,
        #[arg(long)]
        out: Option<PathBuf>,
        #[command(flatten)]
        flags: RunFlags,
    },
    /// Random recipes from saved runs, or random cookbooks from a library.
    Sample {
        /// Saved run files to draw recipes from.
        runs: Vec<PathBuf>,
        /// Draw books from this directory instead (outline only).
        #[arg(long)]
        library: Option<PathBuf>,
        #[arg(long, default_value_t = 5)]
        n: usize,
        #[arg(long, default_value_t = 1)]
        seed: u64,
    },
    /// Score extractions against hand-authored answer keys; exit 4 when the
    /// gate fails.
    Eval {
        /// Directory of `<slug>.json` answer keys (default: the data dir).
        #[arg(long)]
        expectations: Option<PathBuf>,
        /// Only these slugs.
        #[arg(long)]
        book: Vec<String>,
        /// Resolve relative `book` paths here, by file name if needed.
        #[arg(long)]
        library: Option<PathBuf>,
        /// Answer from `<dir>/<slug>` dumps instead of the network.
        #[arg(long)]
        replay: Option<PathBuf>,
        /// Record each book's requests under `<dir>/<slug>`.
        #[arg(long)]
        dump: Option<PathBuf>,
        /// Write the full report here.
        #[arg(long)]
        out: Option<PathBuf>,
        #[arg(long)]
        no_cache: bool,
        #[command(flatten)]
        flags: RunFlags,
    },
    /// Write an answer-key skeleton for a book (titles from its contents).
    Expect {
        book: PathBuf,
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Saved runs, newest first.
    Runs {
        /// Title substring filter.
        #[arg(long)]
        book: Option<String>,
        #[arg(long, default_value_t = 30)]
        limit: usize,
    },
    /// The model catalog with rates and throughput priors.
    Models,
}

#[derive(Args, Clone, Default)]
pub struct RunFlags {
    /// Chunks in flight at once (default 16).
    #[arg(long)]
    pub concurrency: Option<usize>,
    /// Model ids, cheapest first (default: the catalog ladder).
    #[arg(long, value_delimiter = ',')]
    pub ladder: Vec<String>,
    #[arg(long)]
    pub no_second_opinion: bool,
    #[arg(long)]
    pub no_escalation: bool,
    /// Label in reports and gateway metadata (default: the file name).
    #[arg(long)]
    pub label: Option<String>,
}

impl RunFlags {
    fn options(&self, book: &Path) -> ExtractOptions {
        let default = ExtractOptions::default();
        ExtractOptions {
            label: self.label.clone().unwrap_or_else(|| {
                book.file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default()
            }),
            concurrency: self.concurrency.unwrap_or(default.concurrency),
            ladder: self.ladder.clone(),
            second_opinion: !self.no_second_opinion,
            whole_book_escalation: !self.no_escalation,
            max_output_tokens: default.max_output_tokens,
        }
    }
}

enum AnyTransport {
    Live(ReqwestTransport),
    Dump(DumpTransport<ReqwestTransport>),
    Replay(ReplayTransport),
}

impl Transport for AnyTransport {
    async fn send(
        &self,
        request: HttpRequest,
        cancel: &CancelToken,
    ) -> Result<HttpResponse, TransportError> {
        match self {
            AnyTransport::Live(t) => t.send(request, cancel).await,
            AnyTransport::Dump(t) => t.send(request, cancel).await,
            AnyTransport::Replay(t) => t.send(request, cancel).await,
        }
    }

    async fn sleep(&self, ms: u64) {
        match self {
            AnyTransport::Live(t) => t.sleep(ms).await,
            AnyTransport::Dump(t) => t.sleep(ms).await,
            AnyTransport::Replay(t) => t.sleep(ms).await,
        }
    }
}

fn open(path: &Path, label: &str) -> Result<Book, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    Book::open(bytes, label).map_err(|e| format!("open {}: {e}", path.display()))
}

fn cache(no_cache: bool) -> Result<Box<dyn ChunkCache>, String> {
    if no_cache {
        Ok(Box::new(NoCache))
    } else {
        Ok(Box::new(
            FsChunkCache::open_default().map_err(|e| e.to_string())?,
        ))
    }
}

fn label_for(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn emit(value: &Value, json: bool) {
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(value).unwrap_or_default()
        );
    }
}

/// Run one command. Returns the process exit code.
pub async fn execute(command: Command, json: bool) -> Result<i32, String> {
    match command {
        Command::Inspect {
            book,
            chunks,
            chunk,
            lines,
            nav,
            pages,
        } => inspect(
            &book,
            chunks,
            chunk.as_deref(),
            lines.as_deref(),
            nav,
            pages,
            json,
        ),
        Command::Estimate {
            book,
            flags,
            no_cache,
        } => {
            let b = open(&book, &label_for(&book))?;
            let cache = cache(no_cache)?;
            let estimate = b
                .estimate(&flags.options(&book), &cache)
                .map_err(|e| e.to_string())?;
            if json {
                emit(&json!(estimate), true);
            } else {
                print_estimate(&estimate);
            }
            Ok(0)
        }
        Command::Extract {
            book,
            out,
            dump,
            no_cache,
            flags,
        } => {
            let b = open(&book, &label_for(&book))?;
            let live = ReqwestTransport::from_env().map_err(|e| e.to_string())?;
            let transport = match &dump {
                Some(dir) => {
                    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
                    std::fs::copy(&book, dir.join("book.epub"))
                        .map_err(|e| format!("copy epub into dump: {e}"))?;
                    let manifest = json!({"book": book.display().to_string(), "sha256": b.source().sha256, "label": b.source().label, "started_at": jiff::Timestamp::now().to_string()});
                    std::fs::write(
                        dir.join("manifest.json"),
                        serde_json::to_string_pretty(&manifest).unwrap_or_default(),
                    )
                    .map_err(|e| e.to_string())?;
                    AnyTransport::Dump(DumpTransport::new(live, dir).map_err(|e| e.to_string())?)
                }
                None => AnyTransport::Live(live),
            };
            let cache = cache(no_cache)?;
            let options = flags.options(&book);
            run_and_report(&b, &options, &transport, &cache, out.as_deref(), json).await
        }
        Command::Replay { dump, out, flags } => {
            let book = dump.join("book.epub");
            let b = open(&book, &label_for(&dump))?;
            let transport =
                AnyTransport::Replay(ReplayTransport::load(&dump).map_err(|e| e.to_string())?);
            let options = flags.options(&book);
            run_and_report(&b, &options, &transport, &NoCache, out.as_deref(), json).await
        }
        Command::Explain {
            run,
            recipe,
            title,
            book,
        } => explain(
            &run,
            recipe.as_deref(),
            title.as_deref(),
            book.as_deref(),
            json,
        ),
        Command::Sample {
            runs,
            library,
            n,
            seed,
        } => sample(&runs, library.as_deref(), n, seed, json),
        Command::Eval {
            expectations,
            book,
            library,
            replay,
            dump,
            out,
            no_cache,
            flags,
        } => {
            evaluate(
                expectations.as_deref(),
                &book,
                library.as_deref(),
                replay.as_deref(),
                dump.as_deref(),
                out.as_deref(),
                no_cache,
                &flags,
                json,
            )
            .await
        }
        Command::Expect { book, out } => {
            let b = open(&book, &label_for(&book))?;
            let skeleton = eval::skeleton(&b, &book.display().to_string());
            let text = serde_json::to_string_pretty(&skeleton).map_err(|e| e.to_string())?;
            match out {
                Some(path) => {
                    std::fs::write(&path, text).map_err(|e| e.to_string())?;
                    eprintln!(
                        "wrote {} ({} contents titles); fill in samples and not_recipes by hand",
                        path.display(),
                        skeleton.titles.len()
                    );
                }
                None => println!("{text}"),
            }
            Ok(0)
        }
        Command::Runs { book, limit } => {
            let mut list = runs::list().map_err(|e| e.to_string())?;
            if let Some(filter) = &book {
                let f = filter.to_lowercase();
                list.retain(|r| {
                    r.book.to_lowercase().contains(&f) || r.path.to_lowercase().contains(&f)
                });
            }
            list.truncate(limit);
            if json {
                emit(&json!(list), true);
            } else {
                print_runs(&list);
            }
            Ok(0)
        }
        Command::Models => {
            let models = cookbook::models::catalog();
            if json {
                emit(&json!(models), true);
            } else {
                let rows: Vec<Vec<String>> = models
                    .iter()
                    .map(|m| {
                        let r = m
                            .rates
                            .map(|r| format!("{:.2} / {:.2}", r.input, r.output))
                            .unwrap_or_else(|| "-".into());
                        vec![
                            m.id.to_string(),
                            m.provider.as_str().to_string(),
                            if m.enabled { "yes".into() } else { "no".into() },
                            r,
                            format!(
                                "{:.1}s / {:.0} tok/s ({})",
                                m.priors.ttft_ms_p50 as f64 / 1000.0,
                                m.priors.output_tps,
                                m.priors.measured
                            ),
                            m.status.to_string(),
                        ]
                    })
                    .collect();
                println!(
                    "{}",
                    terminal_table(
                        &[
                            "model",
                            "provider",
                            "enabled",
                            "$/M in / out",
                            "priors",
                            "status"
                        ],
                        &rows
                    )
                );
                println!(
                    "default ladder: {}",
                    cookbook::models::DEFAULT_LADDER.join(" → ")
                );
            }
            Ok(0)
        }
    }
}

fn parse_range(spec: &str, len: usize) -> Result<(usize, usize), String> {
    let (a, b) = spec
        .split_once("..")
        .ok_or_else(|| format!("--lines wants A..B, got {spec:?}"))?;
    let a: usize = a.trim().parse().map_err(|_| format!("bad start {a:?}"))?;
    let b: usize = if b.trim().is_empty() {
        len
    } else {
        b.trim().parse().map_err(|_| format!("bad end {b:?}"))?
    };
    Ok((a.min(len), b.min(len)))
}

fn line_json(book: &Book, idx: usize) -> Value {
    let l = &book.lines().lines[idx];
    json!({
        "idx": idx,
        "id": l.id(),
        "doc": book.lines().doc_path(idx),
        "page": l.page,
        "block": l.clean.block_tag,
        "classes": l.clean.classes,
        "heading": l.clean.heading,
        "in_figure": l.clean.in_figure,
        "anchors": l.clean.anchors,
        "links": l.clean.links.iter().map(|k| json!({"text": k.text, "href": k.href})).collect::<Vec<_>>(),
        "images": l.clean.images.iter().map(|i| &i.path).collect::<Vec<_>>(),
        "title_like": book.lines().title_like(idx),
        "text": l.text(),
    })
}

fn inspect(
    path: &Path,
    list_chunks: bool,
    chunk: Option<&str>,
    lines: Option<&str>,
    nav: bool,
    pages: bool,
    json: bool,
) -> Result<i32, String> {
    let book = open(path, &label_for(path))?;
    if let Some(id) = chunk {
        let c = book
            .chunks()
            .iter()
            .find(|c| c.id == id)
            .ok_or_else(|| format!("no chunk {id}; there are {}", book.chunks().len()))?;
        if json {
            emit(&json!({"chunk": c, "text": c.text(book.lines())}), true);
        } else {
            println!(
                "{} · lines {}..{} · {} chars · {:?}{}",
                c.id,
                c.start,
                c.end,
                c.chars,
                c.boundary,
                c.title_hint
                    .as_ref()
                    .map(|h| format!(" · continues {h:?}"))
                    .unwrap_or_default()
            );
            println!("{}", c.text(book.lines()));
        }
        return Ok(0);
    }
    if let Some(spec) = lines {
        let (a, b) = parse_range(spec, book.lines().len())?;
        if json {
            emit(
                &json!((a..b).map(|i| line_json(&book, i)).collect::<Vec<_>>()),
                true,
            );
        } else {
            for i in a..b {
                let l = &book.lines().lines[i];
                let mut tags = vec![format!("<{}>", l.clean.block_tag)];
                if !l.clean.classes.is_empty() {
                    tags.push(format!(".{}", l.clean.classes.join(".")));
                }
                if let Some(p) = &l.page {
                    tags.push(format!("p{p}"));
                }
                if l.clean.in_figure {
                    tags.push("fig".into());
                }
                if !l.clean.links.is_empty() {
                    tags.push(format!("{} link(s)", l.clean.links.len()));
                }
                if !l.clean.images.is_empty() {
                    tags.push(format!("{} img", l.clean.images.len()));
                }
                if book.lines().title_like(i) {
                    tags.push("title?".into());
                }
                println!("{i:>6} {:<20} {} {}", tags.join(" "), l.id(), l.text());
            }
        }
        return Ok(0);
    }
    let classification = classify_structure(&book);
    let outline = book.outline();
    let chunks: Vec<Value> = book
        .chunks()
        .iter()
        .map(|c| json!({"id": c.id, "start": c.start, "end": c.end, "lines": c.lines(), "chars": c.chars, "doc": book.lines().doc_path(c.start), "title_hint": c.title_hint, "boundary": c.boundary}))
        .collect();
    let mut value = json!({
        "source": book.source(),
        "outline": outline,
        "classification": classification,
        "package": {
            "version": book.package().version,
            "opf_spine": book.package().spine.len(),
            "ncx": book.package().ncx.as_ref().map(|i| i.path.clone()),
            "nav": book.package().nav.as_ref().map(|i| i.path.clone()),
            "nav_entries": book.nav().entries.len(),
            "page_list": book.nav().page_list.len(),
        },
        "docs": book.lines().docs,
        "lines": book.lines().len(),
        "chunks": if list_chunks { json!(chunks) } else { json!(chunks.len()) },
    });
    if nav {
        value["nav"] = json!(book.nav());
    }
    if pages {
        value["pages"] = json!(book.lines().pages());
    }
    if json {
        emit(&value, true);
        return Ok(0);
    }
    let s = book.source();
    println!("{} — {}", s.title, s.authors.join(", "));
    println!(
        "{} spine docs · {} lines · {} chunks · {} contents entries naming recipes",
        s.spine_docs,
        s.lines,
        book.chunks().len(),
        outline.nav_recipe_titles
    );
    println!(
        "classification: {:?} (score {:.2}, {})",
        classification.classification,
        classification.score,
        classification.reasons.join("; ")
    );
    if !outline.chapters.is_empty() {
        println!("chapters: {}", outline.chapters.join(" | "));
    }
    if let Some(cover) = &outline.cover {
        println!("cover: {}", cover.path);
    }
    if list_chunks {
        let rows: Vec<Vec<String>> = book
            .chunks()
            .iter()
            .map(|c| {
                vec![
                    c.id.clone(),
                    format!("{}..{}", c.start, c.end),
                    c.chars.to_string(),
                    format!("{:?}", c.boundary),
                    book.lines().doc_path(c.start).to_string(),
                    c.title_hint.clone().unwrap_or_default(),
                ]
            })
            .collect();
        println!(
            "{}",
            terminal_table(
                &["chunk", "lines", "chars", "boundary", "doc", "continues"],
                &rows
            )
        );
    }
    if nav {
        for e in &book.nav().entries {
            println!(
                "{}{} → {}#{}",
                "  ".repeat(e.depth.saturating_sub(1) as usize),
                e.label,
                e.doc_path,
                e.fragment.clone().unwrap_or_default()
            );
        }
    }
    if pages {
        println!(
            "pages: {}",
            book.lines()
                .pages()
                .iter()
                .map(|(p, l)| format!("{p}@{l}"))
                .collect::<Vec<_>>()
                .join(" ")
        );
    }
    Ok(0)
}

fn print_estimate(e: &cookbook::Estimate) {
    println!(
        "{} chunks ({} lines, {} chars), {} cached · ~{} in / {} out tokens",
        e.chunks, e.lines, e.chars, e.cache_hits, e.input_tokens, e.output_tokens
    );
    println!(
        "calls: {}–{} · time: {}–{} s · cost: ${:.3}–${:.3}",
        e.calls_low,
        e.calls_high,
        e.wall_ms_low / 1000,
        e.wall_ms_high / 1000,
        e.cost_usd_low,
        e.cost_usd_high
    );
    println!(
        "ladder: {} · concurrency {}",
        e.ladder.join(" → "),
        e.concurrency
    );
    for a in &e.assumptions {
        println!("  assuming {a}");
    }
}

async fn run_and_report(
    book: &Book,
    options: &ExtractOptions,
    transport: &AnyTransport,
    cache: &impl ChunkCache,
    out: Option<&Path>,
    json: bool,
) -> Result<i32, String> {
    let bar = if std::io::stderr().is_terminal() {
        indicatif::ProgressBar::new(book.chunks().len() as u64)
    } else {
        indicatif::ProgressBar::hidden()
    };
    bar.set_style(
        indicatif::ProgressStyle::with_template("{spinner} [{bar:30}] {pos}/{len} chunks {msg}")
            .unwrap_or_else(|_| indicatif::ProgressStyle::default_bar())
            .progress_chars("=> "),
    );
    let mut last_phase = None;
    let progress = |p: Progress| {
        bar.set_position(p.done as u64);
        if last_phase != Some(p.phase) {
            last_phase = Some(p.phase);
            bar.println(format!("phase: {:?}", p.phase));
        }
        bar.set_message(format!(
            "· {} recipes · ${:.3} (→ ${:.2}) · {}–{}s left · {}",
            p.recipes_so_far,
            p.cost_so_far_usd,
            p.eta.projected_cost_usd,
            p.eta.remaining_low_ms / 1000,
            p.eta.remaining_high_ms / 1000,
            p.active_models.join(",")
        ));
    };
    let cancel = CancelToken::new();
    let result = book
        .extract(options, transport, cache, &cancel, progress)
        .await;
    bar.finish_and_clear();
    let extraction = match result {
        Ok(e) => e,
        Err(cookbook::Error::Cancelled(report)) => {
            eprintln!(
                "cancelled after {} calls, ${:.3}",
                report.calls.len(),
                report.total_cost_usd
            );
            return Ok(130);
        }
        Err(e) => return Err(e.to_string()),
    };
    let path = runs::save(&extraction, out).map_err(|e| e.to_string())?;
    if json {
        emit(&json!(extraction), true);
        eprintln!("saved {}", path.display());
    } else {
        print_summary(&extraction, &path);
    }
    Ok(if extraction.report.incomplete {
        EXIT_INCOMPLETE
    } else {
        0
    })
}

fn print_summary(extraction: &Extraction, path: &Path) {
    let c = &extraction.cookbook;
    let r = &extraction.report;
    let recipes = c.recipes().count();
    let techniques = c
        .items()
        .filter(|i| matches!(i, Item::Technique(_)))
        .count();
    let essays = c.items().filter(|i| matches!(i, Item::Essay(_))).count();
    println!(
        "{} — {} chapters · {recipes} recipes · {techniques} techniques · {essays} essays · {} edges",
        c.source.title,
        c.chapters.len(),
        c.edges.len()
    );
    println!(
        "{} chunks · {} calls · ${:.3}{} · {:.1}s (estimated {}–{}s, ${:.3}–${:.3})",
        r.chunks.len(),
        r.calls.len(),
        r.total_cost_usd,
        if r.cost_complete {
            ""
        } else {
            " (incomplete pricing)"
        },
        r.wall_ms as f64 / 1000.0,
        r.estimate.wall_ms_low / 1000,
        r.estimate.wall_ms_high / 1000,
        r.estimate.cost_usd_low,
        r.estimate.cost_usd_high
    );
    for m in &r.usage_by_model {
        println!(
            "  {}: {} calls, {} in / {} out tokens, ${:.3}",
            m.model,
            m.calls,
            m.usage.input_tokens,
            m.usage.output_tokens,
            m.cost_usd.unwrap_or(0.0)
        );
    }
    let cc = &r.crosscheck;
    println!(
        "contents: {} recipe titles, {} matched{}; {} missing, {} phantom",
        cc.nav_titles,
        cc.matched,
        cc.recall
            .map(|r| format!(" ({:.0}% recall)", r * 100.0))
            .unwrap_or_default(),
        cc.missing.len(),
        cc.phantom.len()
    );
    let flagged = r.chunks.iter().filter(|c| !c.flags.is_empty()).count();
    let failed = r
        .chunks
        .iter()
        .filter(|c| c.status == cookbook::ChunkStatus::Failed)
        .count();
    let second = r
        .chunks
        .iter()
        .filter(|c| c.second_opinion.is_some())
        .count();
    println!(
        "chunks flagged {flagged} · second opinions {second} · failed {failed} · unresolved refs {}",
        r.unresolved_refs.len()
    );
    if let Some(e) = &r.escalation {
        println!(
            "escalated: {} ({} → {})",
            e.reason,
            e.from_model,
            if e.to_model.is_empty() {
                "nothing left"
            } else {
                &e.to_model
            }
        );
    }
    for chapter in &c.chapters {
        println!("{}", chapter.title.as_deref().unwrap_or("(front matter)"));
        for item in &chapter.items {
            let kind = match item {
                Item::Recipe(r) => format!(
                    "recipe {}i/{}s{}",
                    r.sections
                        .iter()
                        .map(|s| s.ingredients.len())
                        .sum::<usize>(),
                    r.sections.iter().map(|s| s.steps.len()).sum::<usize>(),
                    if r.variant_of.is_some() {
                        " variant"
                    } else {
                        ""
                    }
                ),
                Item::Technique(t) => format!("technique {}s", t.steps.len()),
                Item::Essay(_) => "essay".into(),
            };
            println!(
                "  {} {} [{kind}]{}",
                item.id(),
                item.name(),
                if item.photos().is_empty() {
                    ""
                } else {
                    " 📷"
                }
            );
        }
    }
    println!(
        "saved {}{}",
        path.display(),
        if r.incomplete { " (INCOMPLETE)" } else { "" }
    );
}

fn print_runs(list: &[RunSummary]) {
    let rows: Vec<Vec<String>> = list
        .iter()
        .map(|r| {
            vec![
                r.started_at.chars().take(19).collect(),
                r.book.clone(),
                r.recipes.to_string(),
                r.recall
                    .map(|x| format!("{:.0}%", x * 100.0))
                    .unwrap_or_else(|| "-".into()),
                format!("${:.3}", r.cost_usd),
                format!("{:.0}s", r.wall_ms as f64 / 1000.0),
                r.ladder.join("→"),
                if r.incomplete {
                    "incomplete".into()
                } else {
                    "ok".into()
                },
                r.path.clone(),
            ]
        })
        .collect();
    println!(
        "{}",
        terminal_table(
            &[
                "started", "book", "recipes", "recall", "cost", "wall", "ladder", "status", "path"
            ],
            &rows
        )
    );
}

fn explain(
    run: &Path,
    id: Option<&str>,
    title: Option<&str>,
    book: Option<&Path>,
    json: bool,
) -> Result<i32, String> {
    let extraction = runs::load(run).map_err(|e| e.to_string())?;
    let (chapter, item) = extraction
        .cookbook
        .chapters
        .iter()
        .flat_map(|c| c.items.iter().map(move |i| (c, i)))
        .find(|(_, i)| {
            id.is_some_and(|id| i.id() == id)
                || title.is_some_and(|t| {
                    i.title().to_lowercase().contains(&t.to_lowercase())
                        || i.name().to_lowercase().contains(&t.to_lowercase())
                })
        })
        .ok_or_else(|| "no item matches; pass --recipe ID or --title SUBSTRING".to_string())?;
    let span = item.span();
    let chunk = extraction
        .report
        .chunks
        .iter()
        .find(|c| c.start <= span.start && span.start < c.end);
    let calls: Vec<&cookbook::CallRecord> = chunk
        .map(|c| {
            extraction
                .report
                .calls
                .iter()
                .filter(|k| k.chunk_id == c.id)
                .collect()
        })
        .unwrap_or_default();
    let refs: Vec<Value> = match item {
        Item::Recipe(r) => r
            .sections
            .iter()
            .flat_map(|s| s.ingredients.iter().filter_map(|l| l.reference.as_ref().map(|x| json!({"line": l.line, "text": l.raw, "target": x.target_id, "kind": x.kind, "method": x.method}))))
            .chain(r.sections.iter().flat_map(|s| s.steps.iter().flat_map(|st| st.refs.iter().map(move |x| json!({"line": st.line, "text": st.text, "target": x.target_id, "kind": x.kind, "method": x.method})))))
            .chain(r.notes.iter().flat_map(|n| n.refs.iter().map(move |x| json!({"line": n.line, "text": n.text, "target": x.target_id, "kind": x.kind, "method": x.method}))))
            .collect(),
        _ => Vec::new(),
    };
    let unresolved: Vec<&cookbook::UnresolvedRef> = extraction
        .report
        .unresolved_refs
        .iter()
        .filter(|u| u.item_id == item.id())
        .collect();
    let incoming: Vec<&cookbook::Edge> = extraction
        .cookbook
        .edges
        .iter()
        .filter(|e| e.to == item.id())
        .collect();
    let mut value = json!({
        "chapter": chapter.title,
        "item": item,
        "chunk": chunk,
        "calls": calls,
        "refs": refs,
        "unresolved": unresolved,
        "referenced_by": incoming,
    });
    if let Some(path) = book {
        let b = open(path, &label_for(path))?;
        if b.source().sha256 != extraction.cookbook.source.sha256 {
            return Err("that EPUB is not the one this run extracted (sha256 differs)".into());
        }
        let claimed = claimed_lines(item);
        let lines: Vec<Value> = (span.start..span.end.min(b.lines().len()))
            .map(|i| {
                let role = claimed
                    .iter()
                    .find(|(l, _)| *l == i)
                    .map(|(_, r)| *r)
                    .or_else(|| {
                        let text = b.lines().text(i);
                        text_role(item, text)
                    });
                json!({"idx": i, "role": role.unwrap_or("unclaimed"), "text": b.lines().text(i)})
            })
            .collect();
        value["lines"] = json!(lines);
    }
    if json {
        emit(&value, true);
        return Ok(0);
    }
    println!("{} · {} · {:?}", item.id(), item.name(), chapter.title);
    println!(
        "span {}..{} in {} (page {})",
        span.start,
        span.end,
        span.doc_path,
        span.page.clone().unwrap_or_default()
    );
    if let Some(c) = chunk {
        println!(
            "chunk {} · {:?} · {} attempts · final {} · flags {:?}{}",
            c.id,
            c.status,
            c.attempts,
            c.final_model.clone().unwrap_or_default(),
            c.flags,
            c.second_opinion
                .as_ref()
                .map(|s| format!(
                    " · second opinion {} chose {:?}: {}",
                    s.model, s.chosen, s.reason
                ))
                .unwrap_or_default()
        );
    }
    for k in &calls {
        println!(
            "  call #{} {} {:?} attempt {} {}ms {}{} → {:?}",
            k.seq,
            k.model,
            k.purpose,
            k.attempt,
            k.latency_ms,
            if k.cached { "cached " } else { "" },
            k.cost_usd.map(|c| format!("${c:.4}")).unwrap_or_default(),
            k.outcome
        );
    }
    if let Some(lines) = value.get("lines").and_then(Value::as_array) {
        for l in lines {
            println!(
                "{:>6} {:<11} {}",
                l["idx"],
                l["role"].as_str().unwrap_or(""),
                l["text"].as_str().unwrap_or("")
            );
        }
    }
    for r in &refs {
        println!(
            "ref: {} → {} ({} via {})",
            r["text"].as_str().unwrap_or(""),
            r["target"],
            r["kind"],
            r["method"]
        );
    }
    for u in &unresolved {
        println!("unresolved: {:?} tried {:?}", u.text, u.attempted);
    }
    for e in &incoming {
        println!("referenced by {} ({:?})", e.from, e.kind);
    }
    Ok(0)
}

fn claimed_lines(item: &Item) -> Vec<(usize, &'static str)> {
    let mut out = Vec::new();
    match item {
        Item::Recipe(r) => {
            for s in &r.sections {
                out.extend(s.ingredients.iter().map(|l| (l.line, "ingredient")));
                out.extend(s.steps.iter().map(|l| (l.line, "step")));
            }
            out.extend(r.notes.iter().map(|n| (n.line, "note")));
            out.extend(r.photos.iter().filter_map(|p| p.line.map(|l| (l, "photo"))));
        }
        Item::Technique(t) => out.extend(t.steps.iter().map(|l| (l.line, "step"))),
        Item::Essay(_) => {}
    }
    out
}

fn text_role(item: &Item, text: &str) -> Option<&'static str> {
    if item.title() == text {
        return Some("title");
    }
    match item {
        Item::Recipe(r) => {
            if r.meta.description.iter().any(|d| d == text) {
                Some("description")
            } else if r.meta.recipe_yield.as_deref() == Some(text) {
                Some("yield")
            } else if r.meta.equipment.iter().any(|d| d == text) {
                Some("equipment")
            } else if r.sections.iter().any(|s| s.name.as_deref() == Some(text)) {
                Some("section")
            } else {
                None
            }
        }
        Item::Technique(t) => t
            .description
            .iter()
            .any(|d| d == text)
            .then_some("description"),
        Item::Essay(e) => e.text.iter().any(|d| d == text).then_some("text"),
    }
}

fn sample(
    run_paths: &[PathBuf],
    library: Option<&Path>,
    n: usize,
    seed: u64,
    json: bool,
) -> Result<i32, String> {
    let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
    if let Some(dir) = library {
        let mut books: Vec<Value> = Vec::new();
        for path in cookbook::library::find_epubs(dir) {
            let Ok(bytes) = std::fs::read(&path) else {
                continue;
            };
            let Ok(b) = Book::open(bytes, label_for(&path)) else {
                continue;
            };
            let c = classify_structure(&b);
            if c.classification == Classification::Cookbook {
                books.push(json!({"path": path.display().to_string(), "title": b.source().title, "authors": b.source().authors, "chunks": b.chunks().len(), "nav_recipe_titles": c.nav_recipe_titles, "score": c.score}));
            }
        }
        books.shuffle(&mut rng);
        books.truncate(n);
        if json {
            emit(&json!(books), true);
        } else {
            for b in &books {
                println!(
                    "{} — {} ({} chunks, {} contents recipes)\n  {}",
                    b["title"].as_str().unwrap_or(""),
                    b["authors"],
                    b["chunks"],
                    b["nav_recipe_titles"],
                    b["path"].as_str().unwrap_or("")
                );
            }
        }
        return Ok(0);
    }
    let mut cards: Vec<Value> = Vec::new();
    for path in run_paths {
        let extraction = runs::load(path).map_err(|e| e.to_string())?;
        for r in extraction.cookbook.recipes() {
            cards.push(json!({
                "book": extraction.cookbook.source.title,
                "run": path.display().to_string(),
                "id": r.id,
                "name": r.name,
                "yield": r.meta.recipe_yield,
                "sections": r.sections.iter().map(|s| json!({"name": s.name, "ingredients": s.ingredients.iter().map(|l| &l.raw).collect::<Vec<_>>(), "steps": s.steps.len()})).collect::<Vec<_>>(),
                "refs": r.sections.iter().flat_map(|s| s.ingredients.iter().filter_map(|l| l.reference.as_ref().map(|x| x.target_id.clone()))).collect::<Vec<_>>(),
                "photos": r.photos.len(),
                "span": r.span,
            }));
        }
    }
    cards.shuffle(&mut rng);
    cards.truncate(n);
    if json {
        emit(&json!(cards), true);
    } else {
        for c in &cards {
            println!(
                "== {} · {} · {}",
                c["book"].as_str().unwrap_or(""),
                c["id"].as_str().unwrap_or(""),
                c["name"].as_str().unwrap_or("")
            );
            if let Some(y) = c["yield"].as_str() {
                println!("   {y}");
            }
            for s in c["sections"].as_array().into_iter().flatten() {
                if let Some(name) = s["name"].as_str() {
                    println!("   [{name}]");
                }
                for i in s["ingredients"].as_array().into_iter().flatten() {
                    println!("   - {}", i.as_str().unwrap_or(""));
                }
                println!("   {} steps", s["steps"]);
            }
            println!(
                "   refs {} · photos {} · {}",
                c["refs"],
                c["photos"],
                c["run"].as_str().unwrap_or("")
            );
        }
    }
    Ok(0)
}

/// Exit code when the evaluation gate fails.
pub const EXIT_GATE_FAILED: i32 = 4;

fn resolve_book(spec: &str, base: &Path, library: Option<&Path>) -> Result<PathBuf, String> {
    let direct = PathBuf::from(spec);
    if direct.is_absolute() && direct.exists() {
        return Ok(direct);
    }
    let relative = base.join(spec);
    if relative.exists() {
        return Ok(relative);
    }
    if let Some(lib) = library {
        let name = direct
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if let Some(found) = cookbook::library::find_epubs(lib)
            .into_iter()
            .find(|p| p.file_name().is_some_and(|n| n.to_string_lossy() == name))
        {
            return Ok(found);
        }
    }
    Err(format!(
        "cannot find book {spec:?} (tried {}, and --library)",
        relative.display()
    ))
}

#[allow(clippy::too_many_arguments)]
async fn evaluate(
    expectations: Option<&Path>,
    only: &[String],
    library: Option<&Path>,
    replay: Option<&Path>,
    dump: Option<&Path>,
    out: Option<&Path>,
    no_cache: bool,
    flags: &RunFlags,
    json: bool,
) -> Result<i32, String> {
    let dir = match expectations {
        Some(d) => d.to_path_buf(),
        None => runs::expectations_dir().ok_or("no data directory for this platform")?,
    };
    let mut keys: Vec<(String, PathBuf)> = std::fs::read_dir(&dir)
        .map_err(|e| format!("read {}: {e}", dir.display()))?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .map(|p| {
            (
                p.file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                p,
            )
        })
        .filter(|(slug, _)| only.is_empty() || only.iter().any(|o| o == slug))
        .collect();
    keys.sort();
    if keys.is_empty() {
        return Err(format!("no answer keys in {}", dir.display()));
    }
    let mut scores = Vec::new();
    let mut ladder_used: Vec<String> = Vec::new();
    for (slug, path) in &keys {
        let expected: Expectations =
            serde_json::from_str(&std::fs::read_to_string(path).map_err(|e| e.to_string())?)
                .map_err(|e| format!("{}: {e}", path.display()))?;
        let book_path = resolve_book(&expected.book, &dir, library)?;
        let book = open(&book_path, slug)?;
        if let Some(sha) = &expected.sha256
            && sha != &book.source().sha256
        {
            eprintln!("{slug}: warning: EPUB sha256 differs from the answer key");
        }
        let options = ExtractOptions {
            label: slug.clone(),
            ..flags.options(&book_path)
        };
        eprintln!(
            "== {slug}: {} ({} chunks)",
            book.source().title,
            book.chunks().len()
        );
        let extraction = match replay {
            Some(replay_dir) => {
                let transport = AnyTransport::Replay(
                    ReplayTransport::load(replay_dir.join(slug))
                        .map_err(|e| format!("{slug}: {e}"))?,
                );
                extract_quiet(&book, &options, &transport, &NoCache).await?
            }
            None => {
                let live = ReqwestTransport::from_env().map_err(|e| e.to_string())?;
                let transport = match dump {
                    Some(d) => {
                        let d = d.join(slug);
                        std::fs::create_dir_all(&d).map_err(|e| e.to_string())?;
                        std::fs::copy(&book_path, d.join("book.epub"))
                            .map_err(|e| e.to_string())?;
                        AnyTransport::Dump(DumpTransport::new(live, &d).map_err(|e| e.to_string())?)
                    }
                    None => AnyTransport::Live(live),
                };
                let cache = cache(no_cache)?;
                extract_quiet(&book, &options, &transport, &cache).await?
            }
        };
        let saved = runs::save(&extraction, None).map_err(|e| e.to_string())?;
        eprintln!("   saved {}", saved.display());
        ladder_used = extraction.report.estimate.ladder.clone();
        let score = eval::score(&expected, &extraction);
        eprintln!(
            "   recall {:.0}% ({}/{}), phantoms {}, samples {}, ${:.3}, {:.0}s{}",
            score.title_recall * 100.0,
            score.matched,
            score.titles,
            score.phantoms.len(),
            score
                .sample_pass
                .map(|p| format!("{:.0}%", p * 100.0))
                .unwrap_or_else(|| "-".into()),
            score.cost_usd,
            score.wall_ms as f64 / 1000.0,
            if score.escalated { " (escalated)" } else { "" }
        );
        scores.push(score);
    }
    let report = eval::summarize(ladder_used, scores);
    if let Some(path) = out {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::write(
            path,
            serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
    }
    if json {
        emit(&json!(report), true);
    } else {
        let rows: Vec<Vec<String>> = report
            .books
            .iter()
            .map(|b| {
                vec![
                    b.book.rsplit('/').next().unwrap_or(&b.book).to_string(),
                    format!("{:.0}%", b.title_recall * 100.0),
                    b.phantoms.len().to_string(),
                    b.not_recipe_leaks.len().to_string(),
                    b.sample_pass
                        .map(|p| format!("{:.0}%", p * 100.0))
                        .unwrap_or_else(|| "-".into()),
                    format!("{:.0}%", b.line_coverage * 100.0),
                    format!("${:.3}", b.cost_usd),
                    format!("{:.0}s", b.wall_ms as f64 / 1000.0),
                    b.eta_error
                        .map(|e| format!("{:.0}%", e * 100.0))
                        .unwrap_or_else(|| "-".into()),
                    if b.escalated { "yes".into() } else { "".into() },
                ]
            })
            .collect();
        println!(
            "{}",
            terminal_table(
                &[
                    "book",
                    "recall",
                    "phantom",
                    "leaks",
                    "samples",
                    "coverage",
                    "cost",
                    "wall",
                    "eta err",
                    "escalated"
                ],
                &rows
            )
        );
        for b in &report.books {
            for m in &b.missing {
                println!(
                    "  {}: missing {m:?}",
                    b.book.rsplit('/').next().unwrap_or(&b.book)
                );
            }
            for p in &b.phantoms {
                println!(
                    "  {}: phantom {p:?}",
                    b.book.rsplit('/').next().unwrap_or(&b.book)
                );
            }
            for f in &b.sample_failures {
                println!(
                    "  {}: sample {f}",
                    b.book.rsplit('/').next().unwrap_or(&b.book)
                );
            }
        }
        println!(
            "ladder {} · mean recall {:.1}% · {} phantoms · samples {} · ${:.3} · max {:.0}s · gate {}",
            report.ladder.join(" → "),
            report.mean_recall * 100.0,
            report.total_phantoms,
            report
                .mean_sample_pass
                .map(|p| format!("{:.0}%", p * 100.0))
                .unwrap_or_else(|| "-".into()),
            report.total_cost_usd,
            report.max_wall_ms as f64 / 1000.0,
            if report.gate.pass {
                "PASS".to_string()
            } else {
                format!("FAIL ({})", report.gate.reasons.join("; "))
            }
        );
    }
    Ok(if report.gate.pass {
        0
    } else {
        EXIT_GATE_FAILED
    })
}

/// Extract with progress on stderr but no run summary.
async fn extract_quiet(
    book: &Book,
    options: &ExtractOptions,
    transport: &AnyTransport,
    cache: &impl ChunkCache,
) -> Result<Extraction, String> {
    let bar = if std::io::stderr().is_terminal() {
        indicatif::ProgressBar::new(book.chunks().len() as u64)
    } else {
        indicatif::ProgressBar::hidden()
    };
    let progress = |p: Progress| {
        bar.set_position(p.done as u64);
        bar.set_message(format!(
            "{:?} · ${:.3} · {}–{}s left",
            p.phase,
            p.cost_so_far_usd,
            p.eta.remaining_low_ms / 1000,
            p.eta.remaining_high_ms / 1000
        ));
    };
    let result = book
        .extract(options, transport, cache, &CancelToken::new(), progress)
        .await;
    bar.finish_and_clear();
    match result {
        Ok(e) => Ok(e),
        Err(cookbook::Error::Cancelled(_)) => Err("cancelled".into()),
        Err(e) => Err(e.to_string()),
    }
}
