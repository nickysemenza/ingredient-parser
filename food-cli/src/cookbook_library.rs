//! `food-cli cookbook library DIR`: extract a library's cookbooks and write
//! the worst-first report. Books are hashed and deduplicated, classified
//! structurally (a cheap cached model call decides the ambiguous ones),
//! skipped when the store already holds a run of the same file, optionally
//! sampled, and extracted a few at a time under a cost ceiling. Every
//! outcome, including the books not touched, ends up as a row in the report.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use cookbook::cache::ChunkCache;
use cookbook::classify::{Classification, classify, classify_structure};
use cookbook::library::{LibraryReport, RowStatus, ShaCache, build_report, scan};
use cookbook::native::runs;
use cookbook::{Book, CancelToken, ExtractOptions, Progress, Transport};
use futures::StreamExt;
use rand::seq::SliceRandom;

/// What the sweep decided for one distinct book before extracting.
struct Candidate {
    path: PathBuf,
    sha256: String,
    title: String,
    authors: Vec<String>,
}

pub struct SweepArgs<'a> {
    pub dir: &'a Path,
    /// Books extracted at once.
    pub books: usize,
    /// Extract even when the store holds a run of the same file.
    pub force: bool,
    /// Stop starting books once the sweep's projected spend would pass this.
    pub max_cost: Option<f64>,
    pub max_books: Option<usize>,
    /// A seeded sample of this many books, spread across authors.
    pub sample: Option<usize>,
    pub seed: u64,
    /// Title or path substrings; a book must match one.
    pub only: &'a [String],
    /// Classify and estimate only; spend nothing.
    pub dry_run: bool,
    pub options: ExtractOptions,
    pub out: Option<&'a Path>,
}

pub struct SweepOutcome {
    pub report: LibraryReport,
    pub written: Option<(PathBuf, PathBuf)>,
    /// Spent by this sweep's extractions.
    pub cost_usd: f64,
    /// Estimated cost of the books a dry run would extract.
    pub projected_low_usd: f64,
    pub projected_high_usd: f64,
    pub cancelled: bool,
}

pub async fn sweep(
    args: SweepArgs<'_>,
    transport: &(impl Transport + Sync),
    cache: &(impl ChunkCache + Sync),
) -> Result<SweepOutcome, String> {
    let mut shas = ShaCache::open_default();
    let scanned = scan(args.dir, &mut shas);
    let _ = shas.save_default();
    eprintln!(
        "{} distinct books, {} duplicate files, {} unreadable",
        scanned.books.len(),
        scanned.duplicates.len(),
        scanned.unreadable.len()
    );
    let ladder =
        cookbook::models::resolve_ladder(&args.options.ladder).map_err(|e| e.to_string())?;
    let classifier = ladder.first().copied().ok_or("the ladder is empty")?;
    let cancel = CancelToken::new();

    // 1. Classify every distinct book by structure, in parallel; the model
    //    only sees the ambiguous ones, one at a time.
    let mut statuses: HashMap<String, RowStatus> = HashMap::new();
    let mut candidates: Vec<Candidate> = Vec::new();
    struct Structural {
        classification: Classification,
        title: String,
        authors: Vec<String>,
    }
    let structural: Vec<Result<Structural, String>> = {
        use rayon::prelude::*;
        scanned
            .books
            .par_iter()
            .map(|book| {
                let bytes = std::fs::read(&book.path).map_err(|e| e.to_string())?;
                let opened = Book::open(bytes, label_for(&book.path)).map_err(|e| e.to_string())?;
                let source = opened.source();
                Ok(Structural {
                    classification: classify_structure(&opened).classification,
                    title: source.title.clone(),
                    authors: source.authors.clone(),
                })
            })
            .collect()
    };
    for (i, (book, structural)) in scanned.books.iter().zip(structural).enumerate() {
        let Structural {
            mut classification,
            title,
            authors,
        } = match structural {
            Ok(v) => v,
            Err(error) => {
                eprintln!(
                    "[skip {}/{}] {}: {error}",
                    i + 1,
                    scanned.books.len(),
                    book.path.display()
                );
                statuses.insert(book.sha256.clone(), RowStatus::Failed { error });
                continue;
            }
        };
        if classification == Classification::Ambiguous
            && let Ok(bytes) = std::fs::read(&book.path)
            && let Ok(opened) = Book::open(bytes, label_for(&book.path))
        {
            classification = classify(&opened, classifier, transport, cache, &cancel)
                .await
                .classification;
        }
        match classification {
            Classification::Cookbook => candidates.push(Candidate {
                path: book.path.clone(),
                sha256: book.sha256.clone(),
                title,
                authors,
            }),
            Classification::NotCookbook => {
                statuses.insert(book.sha256.clone(), RowStatus::NotCookbook);
            }
            Classification::Ambiguous => {
                statuses.insert(
                    book.sha256.clone(),
                    RowStatus::Skipped {
                        reason: "ambiguous".into(),
                    },
                );
            }
        }
    }
    eprintln!(
        "{} cookbooks, {} not, {} ambiguous or unreadable",
        candidates.len(),
        statuses
            .values()
            .filter(|s| matches!(s, RowStatus::NotCookbook))
            .count(),
        statuses
            .values()
            .filter(|s| !matches!(s, RowStatus::NotCookbook))
            .count()
    );

    // 2. Existing runs, `--only`, the sample, `--max-books`.
    let mut pending: Vec<Candidate> = Vec::new();
    for candidate in candidates {
        let existing = runs::latest_for_sha(&candidate.sha256).map_err(|e| e.to_string())?;
        if existing.is_some() && !args.force {
            statuses.insert(candidate.sha256.clone(), RowStatus::Existing);
            continue;
        }
        if !args.only.is_empty() {
            let title = candidate.title.to_lowercase();
            let path = candidate.path.to_string_lossy().to_lowercase();
            if !args
                .only
                .iter()
                .map(|o| o.to_lowercase())
                .any(|o| title.contains(&o) || path.contains(&o))
            {
                statuses.insert(
                    candidate.sha256.clone(),
                    RowStatus::Skipped {
                        reason: "not selected".into(),
                    },
                );
                continue;
            }
        }
        pending.push(candidate);
    }
    if let Some(n) = args.sample {
        let (chosen, rest) = sample_across_authors(pending, n, args.seed);
        for c in rest {
            statuses.insert(
                c.sha256,
                RowStatus::Skipped {
                    reason: "not sampled".into(),
                },
            );
        }
        pending = chosen;
    }
    if let Some(max) = args.max_books {
        for c in pending.drain(max.min(pending.len())..) {
            statuses.insert(
                c.sha256,
                RowStatus::Skipped {
                    reason: "over --max-books".into(),
                },
            );
        }
    }
    eprintln!("{} books to extract", pending.len());

    // 3. Estimate; a dry run stops here.
    let mut projected = (0.0f64, 0.0f64);
    let mut estimates: HashMap<String, (f64, f64, usize)> = HashMap::new();
    for c in &pending {
        let bytes = std::fs::read(&c.path).map_err(|e| e.to_string())?;
        let book = Book::open(bytes, label_for(&c.path)).map_err(|e| e.to_string())?;
        let estimate = book
            .estimate(&args.options, cache)
            .map_err(|e| e.to_string())?;
        projected.0 += estimate.cost_usd_low;
        projected.1 += estimate.cost_usd_high;
        estimates.insert(
            c.sha256.clone(),
            (
                estimate.cost_usd_low,
                estimate.cost_usd_high,
                estimate.chunks,
            ),
        );
    }
    if args.dry_run {
        eprintln!(
            "dry run: {} books, ${:.2}–${:.2} projected",
            pending.len(),
            projected.0,
            projected.1
        );
        for c in pending {
            statuses.insert(
                c.sha256,
                RowStatus::Skipped {
                    reason: "dry run".into(),
                },
            );
        }
        let report =
            build_report(Some(args.dir), Some(&scanned), &statuses).map_err(|e| e.to_string())?;
        let written = args
            .out
            .map(|out| write_report(&report, Some(out)))
            .transpose()?;
        return Ok(SweepOutcome {
            report,
            written,
            cost_usd: 0.0,
            projected_low_usd: projected.0,
            projected_high_usd: projected.1,
            cancelled: false,
        });
    }

    // 4. Extract, a few books at once, under the cost ceiling.
    let spent = Arc::new(Mutex::new(Spend::default()));
    let total = pending.len();
    let counter = Arc::new(Mutex::new(0usize));
    let cancel_all = cancel.clone();
    let ctrl_c = tokio::spawn({
        let cancel = cancel.clone();
        async move {
            if tokio::signal::ctrl_c().await.is_ok() {
                eprintln!("cancelling…");
                cancel.cancel();
            }
        }
    });
    let results: Vec<(String, RowStatus, f64)> = futures::stream::iter(pending)
        .map(|c| {
            let spent = Arc::clone(&spent);
            let counter = Arc::clone(&counter);
            let estimate = estimates.get(&c.sha256).copied().unwrap_or((0.0, 0.0, 0));
            let options = ExtractOptions {
                label: label_for(&c.path),
                ..args.options.clone()
            };
            let cancel = cancel_all.clone();
            async move {
                let n = {
                    let mut k = counter.lock().unwrap_or_else(|e| e.into_inner());
                    *k += 1;
                    *k
                };
                if let Some(max) = args.max_cost {
                    let projected = spent.lock().unwrap_or_else(|e| e.into_inner()).projected();
                    if projected + estimate.1 > max {
                        eprintln!(
                            "[skip {n}/{total}] {} · would pass --max-cost (${projected:.2} + ${:.2})",
                            c.title, estimate.1
                        );
                        return (
                            c.sha256,
                            RowStatus::Skipped {
                                reason: "budget".into(),
                            },
                            0.0,
                        );
                    }
                }
                if cancel.is_cancelled() {
                    return (
                        c.sha256,
                        RowStatus::Skipped {
                            reason: "cancelled".into(),
                        },
                        0.0,
                    );
                }
                eprintln!(
                    "[start {n}/{total}] {} · {} chunks · est ${:.2}–${:.2}",
                    c.title, estimate.2, estimate.0, estimate.1
                );
                let bytes = match std::fs::read(&c.path) {
                    Ok(b) => b,
                    Err(e) => return (c.sha256, RowStatus::Failed { error: e.to_string() }, 0.0),
                };
                let book = match Book::open(bytes, label_for(&c.path)) {
                    Ok(b) => b,
                    Err(e) => return (c.sha256, RowStatus::Failed { error: e.to_string() }, 0.0),
                };
                spent.lock().unwrap_or_else(|e| e.into_inner()).start(&c.sha256);
                let progress = {
                    let spent = Arc::clone(&spent);
                    let sha = c.sha256.clone();
                    move |p: Progress| {
                        spent
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .in_flight
                            .insert(sha.clone(), p.cost_so_far_usd);
                    }
                };
                let started = std::time::Instant::now();
                let result = book.extract(&options, transport, cache, &cancel, progress).await;
                let mut ledger = spent.lock().unwrap_or_else(|e| e.into_inner());
                ledger.in_flight.remove(&c.sha256);
                match result {
                    Ok(extraction) => {
                        let cost = extraction.report.total_cost_usd;
                        ledger.finished += cost;
                        drop(ledger);
                        let report = &extraction.report;
                        let saved = runs::save(&extraction, None)
                            .map(|p| p.display().to_string())
                            .unwrap_or_else(|e| format!("not saved: {e}"));
                        eprintln!(
                            "[done {n}/{total}] {} · {} recall · {} missing · {} phantom{} · ${cost:.2} · {:.0} s · {saved}",
                            c.title,
                            report
                                .crosscheck
                                .recall
                                .map(|r| format!("{:.0}%", r * 100.0))
                                .unwrap_or_else(|| "n/a".into()),
                            report.crosscheck.missing.len(),
                            report.crosscheck.phantom.len(),
                            if report.incomplete { " · incomplete" } else { "" },
                            started.elapsed().as_secs_f64()
                        );
                        (c.sha256, RowStatus::Extracted, cost)
                    }
                    Err(cookbook::Error::Cancelled(report)) => {
                        ledger.finished += report.total_cost_usd;
                        drop(ledger);
                        eprintln!("[cancelled {n}/{total}] {} · ${:.2}", c.title, report.total_cost_usd);
                        (
                            c.sha256,
                            RowStatus::Skipped {
                                reason: "cancelled".into(),
                            },
                            report.total_cost_usd,
                        )
                    }
                    Err(e) => {
                        drop(ledger);
                        eprintln!("[failed {n}/{total}] {} · {e}", c.title);
                        (c.sha256, RowStatus::Failed { error: e.to_string() }, 0.0)
                    }
                }
            }
        })
        .buffer_unordered(args.books.max(1))
        .collect()
        .await;
    ctrl_c.abort();
    let cancelled = cancel.is_cancelled();
    let mut cost = 0.0;
    for (sha, status, spent_usd) in results {
        cost += spent_usd;
        statuses.insert(sha, status);
    }

    // 5. The report over everything.
    let report =
        build_report(Some(args.dir), Some(&scanned), &statuses).map_err(|e| e.to_string())?;
    let written = write_report(&report, args.out)?;
    Ok(SweepOutcome {
        report,
        written: Some(written),
        cost_usd: cost,
        projected_low_usd: projected.0,
        projected_high_usd: projected.1,
        cancelled,
    })
}

/// Money spent so far: finished books plus the live cost of the ones in
/// flight.
#[derive(Default)]
struct Spend {
    finished: f64,
    in_flight: BTreeMap<String, f64>,
}

impl Spend {
    fn start(&mut self, sha: &str) {
        self.in_flight.insert(sha.to_string(), 0.0);
    }

    fn projected(&self) -> f64 {
        self.finished + self.in_flight.values().sum::<f64>()
    }
}

/// `n` books drawn with `seed`, taking one per author in turn so one
/// publisher's house style does not fill the sample. Returns the chosen and
/// the rest.
fn sample_across_authors(
    pending: Vec<Candidate>,
    n: usize,
    seed: u64,
) -> (Vec<Candidate>, Vec<Candidate>) {
    use rand::SeedableRng;
    let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
    let mut by_author: BTreeMap<String, Vec<Candidate>> = BTreeMap::new();
    for c in pending {
        let author = c
            .authors
            .first()
            .cloned()
            .unwrap_or_default()
            .to_lowercase();
        by_author.entry(author).or_default().push(c);
    }
    let mut groups: Vec<Vec<Candidate>> = by_author.into_values().collect();
    for g in &mut groups {
        g.shuffle(&mut rng);
    }
    groups.shuffle(&mut rng);
    let mut chosen = Vec::new();
    while chosen.len() < n && groups.iter().any(|g| !g.is_empty()) {
        for g in &mut groups {
            if chosen.len() >= n {
                break;
            }
            if let Some(c) = g.pop() {
                chosen.push(c);
            }
        }
    }
    let rest: Vec<Candidate> = groups.into_iter().flatten().collect();
    (chosen, rest)
}

fn label_for(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Write `<stem>.json` and `<stem>.md`; returns the two paths.
pub fn write_report(
    report: &LibraryReport,
    out: Option<&Path>,
) -> Result<(PathBuf, PathBuf), String> {
    let stem = match out {
        Some(p) => p.with_extension(""),
        None => cookbook::library::reports_dir()
            .ok_or("no data directory for this platform")?
            .join(format!(
                "library-{}",
                jiff::Timestamp::now().strftime("%Y%m%dT%H%MZ")
            )),
    };
    if let Some(parent) = stem.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let json_path = stem.with_extension("json");
    let md_path = stem.with_extension("md");
    std::fs::write(
        &json_path,
        serde_json::to_string_pretty(report).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    std::fs::write(&md_path, report.render_markdown()).map_err(|e| e.to_string())?;
    Ok((json_path, md_path))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn candidate(title: &str, author: &str) -> Candidate {
        Candidate {
            path: PathBuf::from(format!("{title}.epub")),
            sha256: title.to_string(),
            title: title.to_string(),
            authors: vec![author.to_string()],
        }
    }

    /// One book per author before a second from anyone; the same seed
    /// draws the same sample.
    #[test]
    fn sample_spreads_across_authors_and_is_reproducible() {
        let pending = || {
            vec![
                candidate("a1", "Ann"),
                candidate("a2", "Ann"),
                candidate("a3", "Ann"),
                candidate("b1", "Bob"),
                candidate("c1", "Cy"),
            ]
        };
        let (chosen, rest) = sample_across_authors(pending(), 3, 7);
        let authors: std::collections::BTreeSet<&str> =
            chosen.iter().map(|c| c.authors[0].as_str()).collect();
        assert_eq!(
            authors.len(),
            3,
            "{chosen:?}",
            chosen = chosen.iter().map(|c| &c.title).collect::<Vec<_>>()
        );
        assert_eq!(rest.len(), 2);
        let (again, _) = sample_across_authors(pending(), 3, 7);
        assert_eq!(
            chosen.iter().map(|c| &c.title).collect::<Vec<_>>(),
            again.iter().map(|c| &c.title).collect::<Vec<_>>()
        );
        let (all, none) = sample_across_authors(pending(), 10, 1);
        assert_eq!(all.len(), 5);
        assert!(none.is_empty());
    }
}
