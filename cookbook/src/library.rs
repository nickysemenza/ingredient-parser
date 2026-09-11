//! Finding EPUBs in a library directory.

/// Every `.epub` under `dir`, sorted by path. Hidden directories are skipped.
#[cfg(feature = "native")]
pub fn find_epubs(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut out: Vec<std::path::PathBuf> = walkdir::WalkDir::new(dir)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| e.depth() == 0 || !e.file_name().to_string_lossy().starts_with('.'))
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .filter(|e| {
            e.path()
                .extension()
                .is_some_and(|x| x.eq_ignore_ascii_case("epub"))
        })
        .map(|e| e.into_path())
        .collect();
    out.sort();
    out
}

/// sha256 per library file, remembered by size and modification time so a
/// rescan reads only files that changed. Persisted as JSON.
#[cfg(feature = "native")]
#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct ShaCache {
    entries: std::collections::BTreeMap<String, ShaEntry>,
    #[serde(skip)]
    dirty: bool,
}

#[cfg(feature = "native")]
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct ShaEntry {
    len: u64,
    modified_ms: u128,
    sha256: String,
}

#[cfg(feature = "native")]
impl ShaCache {
    /// `<data dir>/ingredient-parser/cookbook/library/sha-cache.json`.
    pub fn default_path() -> Option<std::path::PathBuf> {
        directories::BaseDirs::new().map(|b| {
            b.data_dir()
                .join("ingredient-parser")
                .join("cookbook")
                .join("library")
                .join("sha-cache.json")
        })
    }

    /// The cache at `path`, empty when it is missing or unreadable.
    pub fn load(path: Option<&std::path::Path>) -> ShaCache {
        path.and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    /// The cache at its default location.
    pub fn open_default() -> ShaCache {
        Self::load(Self::default_path().as_deref())
    }

    /// Write the cache to `path` if anything was hashed since it was loaded.
    pub fn save(&self, path: Option<&std::path::Path>) -> std::io::Result<()> {
        let Some(path) = path else {
            return Ok(());
        };
        if !self.dirty {
            return Ok(());
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(
            path,
            serde_json::to_string(self).map_err(std::io::Error::other)?,
        )
    }

    pub fn save_default(&self) -> std::io::Result<()> {
        self.save(Self::default_path().as_deref())
    }

    /// The remembered sha256 when the file's size and mtime still match.
    fn cached_sha(&self, path: &std::path::Path) -> Option<String> {
        let meta = std::fs::metadata(path).ok()?;
        let entry = self.entries.get(path.to_string_lossy().as_ref())?;
        (entry.len == meta.len() && entry.modified_ms == modified_ms(&meta))
            .then(|| entry.sha256.clone())
    }

    fn remember(&mut self, path: &std::path::Path, len: u64, modified_ms: u128, sha256: String) {
        self.entries.insert(
            path.to_string_lossy().into_owned(),
            ShaEntry {
                len,
                modified_ms,
                sha256,
            },
        );
        self.dirty = true;
    }

    /// The file's sha256, hashing it only when its size or mtime changed.
    pub fn sha_for(&mut self, path: &std::path::Path) -> std::io::Result<String> {
        if let Some(sha) = self.cached_sha(path) {
            return Ok(sha);
        }
        let (len, modified_ms, sha256) = hash_file(path)?;
        self.remember(path, len, modified_ms, sha256.clone());
        Ok(sha256)
    }
}

#[cfg(feature = "native")]
fn modified_ms(meta: &std::fs::Metadata) -> u128 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

/// `(len, mtime ms, sha256)` of a file, read once.
#[cfg(feature = "native")]
fn hash_file(path: &std::path::Path) -> std::io::Result<(u64, u128, String)> {
    let meta = std::fs::metadata(path)?;
    let sha256 = crate::epub::open::sha256_hex(&std::fs::read(path)?);
    Ok((meta.len(), modified_ms(&meta), sha256))
}

/// One distinct EPUB in a library.
#[cfg(feature = "native")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScannedBook {
    pub path: std::path::PathBuf,
    pub sha256: String,
}

/// A library directory hashed and deduplicated.
#[cfg(feature = "native")]
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct LibraryScan {
    /// Distinct books, sorted by path; the first path of each sha wins.
    pub books: Vec<ScannedBook>,
    /// `(duplicate, kept)` pairs: byte-identical copies of a book kept above.
    pub duplicates: Vec<(std::path::PathBuf, std::path::PathBuf)>,
    /// Files that could not be read, with the error.
    pub unreadable: Vec<(std::path::PathBuf, String)>,
}

/// Find, hash and deduplicate every EPUB under `dir`. Files the cache does
/// not know are hashed in parallel.
#[cfg(feature = "native")]
pub fn scan(dir: &std::path::Path, shas: &mut ShaCache) -> LibraryScan {
    use rayon::prelude::*;
    let paths = find_epubs(dir);
    let hashed: Vec<std::io::Result<String>> = {
        let known: Vec<Option<String>> = paths.iter().map(|p| shas.cached_sha(p)).collect();
        let fresh: Vec<Option<std::io::Result<(u64, u128, String)>>> = paths
            .par_iter()
            .zip(known.par_iter())
            .map(|(path, known)| known.is_none().then(|| hash_file(path)))
            .collect();
        paths
            .iter()
            .zip(known)
            .zip(fresh)
            .map(|((path, known), fresh)| match (known, fresh) {
                (Some(sha), _) => Ok(sha),
                (None, Some(Ok((len, modified_ms, sha256)))) => {
                    shas.remember(path, len, modified_ms, sha256.clone());
                    Ok(sha256)
                }
                (None, Some(Err(e))) => Err(e),
                (None, None) => Err(std::io::Error::other("unreachable: not cached, not hashed")),
            })
            .collect()
    };
    let mut out = LibraryScan::default();
    let mut seen: std::collections::HashMap<String, std::path::PathBuf> =
        std::collections::HashMap::new();
    for (path, hashed) in paths.into_iter().zip(hashed) {
        match hashed {
            Ok(sha256) => {
                if let Some(kept) = seen.get(&sha256) {
                    out.duplicates.push((path, kept.clone()));
                } else {
                    seen.insert(sha256.clone(), path.clone());
                    out.books.push(ScannedBook { path, sha256 });
                }
            }
            Err(e) => out.unreadable.push((path, e.to_string())),
        }
    }
    out
}

/// Why a library book has, or has not, a run in this report.
#[cfg(feature = "native")]
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum RowStatus {
    /// Extracted by this sweep.
    Extracted,
    /// A run from an earlier sweep was reused.
    Existing,
    /// Not extracted; `reason` says why (`not sampled`, `budget`, `ambiguous`).
    Skipped { reason: String },
    /// The structural classifier (or the model) says it is not a cookbook.
    NotCookbook,
    /// Extraction returned an error.
    Failed { error: String },
}

/// One library book in the report, worst-first.
#[cfg(feature = "native")]
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct BookRow {
    pub title: String,
    pub authors: Vec<String>,
    pub path: Option<String>,
    pub sha256: String,
    pub classification: Option<crate::classify::Classification>,
    pub classify_method: Option<String>,
    #[serde(flatten)]
    pub status: RowStatus,
    pub run: Option<String>,
    pub started_at: Option<String>,
    pub chunks: usize,
    pub lines: usize,
    pub recipes: usize,
    pub techniques: usize,
    pub essays: usize,
    pub nav_titles: usize,
    pub recall: Option<f32>,
    pub missing: Vec<String>,
    pub phantoms: Vec<String>,
    /// Soft flags by kind.
    pub flags: std::collections::BTreeMap<String, usize>,
    pub flagged_chunks: usize,
    pub failed_chunks: usize,
    pub second_opinions: usize,
    pub escalated: bool,
    pub incomplete: bool,
    pub unresolved_refs: usize,
    pub ingredient_lines: usize,
    /// Share of ingredient lines the parser read with an amount.
    pub ingredient_parse_rate: Option<f32>,
    pub transport_errors: usize,
    pub calls: usize,
    pub cached_calls: usize,
    pub cost_usd: f64,
    pub wall_ms: u64,
    /// `|wall − estimate midpoint| / wall`.
    pub eta_error: Option<f32>,
    pub problem_score: f32,
}

#[cfg(feature = "native")]
impl BookRow {
    /// A row for a book without a run.
    pub fn without_run(
        title: &str,
        authors: &[String],
        path: Option<&str>,
        sha256: &str,
        status: RowStatus,
    ) -> BookRow {
        let mut row = BookRow {
            title: title.to_string(),
            authors: authors.to_vec(),
            path: path.map(str::to_string),
            sha256: sha256.to_string(),
            classification: None,
            classify_method: None,
            status,
            run: None,
            started_at: None,
            chunks: 0,
            lines: 0,
            recipes: 0,
            techniques: 0,
            essays: 0,
            nav_titles: 0,
            recall: None,
            missing: Vec::new(),
            phantoms: Vec::new(),
            flags: Default::default(),
            flagged_chunks: 0,
            failed_chunks: 0,
            second_opinions: 0,
            escalated: false,
            incomplete: false,
            unresolved_refs: 0,
            ingredient_lines: 0,
            ingredient_parse_rate: None,
            transport_errors: 0,
            calls: 0,
            cached_calls: 0,
            cost_usd: 0.0,
            wall_ms: 0,
            eta_error: None,
            problem_score: 0.0,
        };
        row.problem_score = problem_score(&row);
        row
    }

    /// A row summarizing a saved run.
    pub fn from_extraction(
        extraction: &crate::report::Extraction,
        run_path: Option<&str>,
        book_path: Option<&str>,
        status: RowStatus,
    ) -> BookRow {
        use crate::model::Item;
        use crate::report::{CallOutcome, ChunkStatus};
        let cookbook = &extraction.cookbook;
        let report = &extraction.report;
        let mut row = BookRow::without_run(
            &cookbook.source.title,
            &cookbook.source.authors,
            book_path,
            &cookbook.source.sha256,
            status,
        );
        row.classification = Some(crate::classify::Classification::Cookbook);
        row.run = run_path.map(str::to_string);
        row.started_at = Some(report.started_at.clone());
        row.chunks = report.chunks.len();
        row.lines = cookbook.source.lines;
        for item in cookbook.items() {
            match item {
                Item::Recipe(_) => row.recipes += 1,
                Item::Technique(_) => row.techniques += 1,
                Item::Essay(_) => row.essays += 1,
            }
        }
        row.nav_titles = report.crosscheck.nav_titles;
        row.recall = report.crosscheck.recall;
        row.missing = report.crosscheck.missing.clone();
        row.phantoms = report.crosscheck.phantom.clone();
        for chunk in &report.chunks {
            if !chunk.flags.is_empty() {
                row.flagged_chunks += 1;
            }
            for flag in &chunk.flags {
                *row.flags.entry(flag.kind().to_string()).or_insert(0) += 1;
            }
            if chunk.status == ChunkStatus::Failed {
                row.failed_chunks += 1;
            }
            if chunk.second_opinion.is_some() {
                row.second_opinions += 1;
            }
        }
        row.escalated = report.escalation.is_some();
        row.incomplete = report.incomplete;
        row.unresolved_refs = report.unresolved_refs.len();
        let (mut lines, mut high) = (0usize, 0usize);
        for recipe in cookbook.recipes() {
            for line in recipe.sections.iter().flat_map(|s| s.ingredients.iter()) {
                lines += 1;
                if line.confidence == ingredient::Confidence::High {
                    high += 1;
                }
            }
        }
        row.ingredient_lines = lines;
        row.ingredient_parse_rate = (lines > 0).then(|| high as f32 / lines as f32);
        row.transport_errors = report
            .calls
            .iter()
            .filter(|c| matches!(c.outcome, CallOutcome::Transport { .. }))
            .count();
        row.calls = report.calls.len();
        row.cached_calls = report.calls.iter().filter(|c| c.cached).count();
        row.cost_usd = report.total_cost_usd;
        row.wall_ms = report.wall_ms;
        let midpoint = (report.estimate.wall_ms_low + report.estimate.wall_ms_high) as f32 / 2.0;
        row.eta_error = (report.wall_ms > 0)
            .then(|| (report.wall_ms as f32 - midpoint).abs() / report.wall_ms as f32);
        row.problem_score = problem_score(&row);
        row
    }
}

/// How badly a book went, for sorting: lost contents titles weigh most,
/// then failures, phantoms, flagged chunks, unresolved references and weak
/// ingredient parses. A failed or incomplete run sits above every finished
/// one. Tune the weights here; the Markdown legend quotes them.
#[cfg(feature = "native")]
pub fn problem_score(row: &BookRow) -> f32 {
    let recall_loss = row.recall.map(|r| 1.0 - r).unwrap_or(0.0) * 100.0;
    let flagged = if row.chunks > 0 {
        row.flagged_chunks as f32 / row.chunks as f32 * 10.0
    } else {
        0.0
    };
    let parse_loss = row.ingredient_parse_rate.map(|r| 1.0 - r).unwrap_or(0.0) * 20.0;
    let failed = matches!(row.status, RowStatus::Failed { .. }) as u8 as f32 * 100.0;
    recall_loss
        + row.phantoms.len() as f32 * 2.0
        + row.failed_chunks as f32 * 10.0
        + flagged
        + row.unresolved_refs as f32 * 0.1
        + parse_loss
        + row.incomplete as u8 as f32 * 50.0
        + failed
}

/// Totals across the report's rows.
#[cfg(feature = "native")]
#[derive(Debug, Default, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Totals {
    pub books: usize,
    pub duplicates: usize,
    pub with_runs: usize,
    pub extracted: usize,
    pub existing: usize,
    pub skipped: usize,
    pub not_cookbooks: usize,
    pub failed: usize,
    pub cost_usd: f64,
    pub wall_ms: u64,
    pub mean_recall: Option<f32>,
    pub missing: usize,
    pub phantoms: usize,
    pub failed_chunks: usize,
    pub unresolved_refs: usize,
    pub flags: std::collections::BTreeMap<String, usize>,
}

#[cfg(feature = "native")]
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct LibraryReport {
    pub generated_at: String,
    pub library_dir: Option<String>,
    pub totals: Totals,
    /// Worst first.
    pub rows: Vec<BookRow>,
}

#[cfg(feature = "native")]
impl LibraryReport {
    /// Sort worst-first and total the rows.
    pub fn new(
        library_dir: Option<&std::path::Path>,
        duplicates: usize,
        mut rows: Vec<BookRow>,
    ) -> LibraryReport {
        rows.sort_by(|a, b| {
            b.problem_score
                .partial_cmp(&a.problem_score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.title.cmp(&b.title))
        });
        let mut totals = Totals {
            books: rows.len(),
            duplicates,
            ..Totals::default()
        };
        let mut recalls = Vec::new();
        for row in &rows {
            match &row.status {
                RowStatus::Extracted => totals.extracted += 1,
                RowStatus::Existing => totals.existing += 1,
                RowStatus::Skipped { .. } => totals.skipped += 1,
                RowStatus::NotCookbook => totals.not_cookbooks += 1,
                RowStatus::Failed { .. } => totals.failed += 1,
            }
            if row.run.is_some() {
                totals.with_runs += 1;
                totals.cost_usd += row.cost_usd;
                totals.wall_ms += row.wall_ms;
                totals.missing += row.missing.len();
                totals.phantoms += row.phantoms.len();
                totals.failed_chunks += row.failed_chunks;
                totals.unresolved_refs += row.unresolved_refs;
                for (kind, n) in &row.flags {
                    *totals.flags.entry(kind.clone()).or_insert(0) += n;
                }
                if let Some(r) = row.recall {
                    recalls.push(r);
                }
            }
        }
        totals.mean_recall =
            (!recalls.is_empty()).then(|| recalls.iter().sum::<f32>() / recalls.len() as f32);
        LibraryReport {
            generated_at: jiff::Timestamp::now().to_string(),
            library_dir: library_dir.map(|p| p.to_string_lossy().into_owned()),
            totals,
            rows,
        }
    }

    /// Totals, the worst-first table, details for the worst rows, and the
    /// signal-to-module legend.
    pub fn render_markdown(&self) -> String {
        use std::fmt::Write;
        let t = &self.totals;
        let mut out = String::new();
        let _ = writeln!(out, "# Cookbook library report\n");
        let _ = writeln!(
            out,
            "Generated {}{}.\n",
            self.generated_at,
            self.library_dir
                .as_deref()
                .map(|d| format!(" from `{d}`"))
                .unwrap_or_default()
        );
        let _ = writeln!(
            out,
            "{} books ({} duplicate files skipped): {} with runs ({} extracted now, {} reused), {} skipped, {} not cookbooks, {} failed. Runs cost ${:.2} over {:.0} s of wall time; mean contents recall {}; {} missing titles, {} phantoms, {} failed chunks, {} unresolved references.\n",
            t.books,
            t.duplicates,
            t.with_runs,
            t.extracted,
            t.existing,
            t.skipped,
            t.not_cookbooks,
            t.failed,
            t.cost_usd,
            t.wall_ms as f64 / 1000.0,
            t.mean_recall
                .map(|r| format!("{:.1}%", r * 100.0))
                .unwrap_or_else(|| "n/a".into()),
            t.missing,
            t.phantoms,
            t.failed_chunks,
            t.unresolved_refs,
        );
        if !t.flags.is_empty() {
            let flags: Vec<String> = t.flags.iter().map(|(k, n)| format!("{k} {n}")).collect();
            let _ = writeln!(out, "Flags: {}.\n", flags.join(", "));
        }
        let _ = writeln!(out, "## Books, worst first\n");
        let _ = writeln!(
            out,
            "| # | book | status | recall | missing | phantom | failed | flagged | refs | parse | cost | wall | score |"
        );
        let _ = writeln!(out, "|---|---|---|---|---|---|---|---|---|---|---|---|---|");
        for (i, row) in self.rows.iter().enumerate() {
            let status = match &row.status {
                RowStatus::Extracted => "extracted".to_string(),
                RowStatus::Existing => "existing".to_string(),
                RowStatus::Skipped { reason } => format!("skipped: {reason}"),
                RowStatus::NotCookbook => "not a cookbook".to_string(),
                RowStatus::Failed { error } => format!("failed: {}", truncate(error, 60)),
            };
            let _ = writeln!(
                out,
                "| {} | {} | {} | {} | {} | {} | {} | {}/{} | {} | {} | ${:.2} | {:.0} s | {:.0} |",
                i + 1,
                row.title.replace('|', "\\|"),
                status,
                row.recall
                    .map(|r| format!("{:.0}%", r * 100.0))
                    .unwrap_or_else(|| "-".into()),
                row.missing.len(),
                row.phantoms.len(),
                row.failed_chunks,
                row.flagged_chunks,
                row.chunks,
                row.unresolved_refs,
                row.ingredient_parse_rate
                    .map(|r| format!("{:.0}%", r * 100.0))
                    .unwrap_or_else(|| "-".into()),
                row.cost_usd,
                row.wall_ms as f64 / 1000.0,
                row.problem_score,
            );
        }
        let worst: Vec<&BookRow> = self
            .rows
            .iter()
            .filter(|r| r.run.is_some() && r.problem_score > 0.0)
            .take(15)
            .collect();
        if !worst.is_empty() {
            let _ = writeln!(out, "\n## Details of the worst {}\n", worst.len());
            for row in worst {
                let _ = writeln!(out, "### {}\n", row.title);
                if let Some(run) = &row.run {
                    let _ = writeln!(
                        out,
                        "Run `{run}`; {} chunks, {} recipes, {} techniques, {} essays; {}.\n",
                        row.chunks,
                        row.recipes,
                        row.techniques,
                        row.essays,
                        if row.incomplete {
                            "incomplete"
                        } else {
                            "complete"
                        }
                    );
                }
                if !row.missing.is_empty() {
                    let _ = writeln!(
                        out,
                        "Missing contents titles: {}\n",
                        row.missing
                            .iter()
                            .map(|m| format!("“{m}”"))
                            .collect::<Vec<_>>()
                            .join(", ")
                    );
                }
                if !row.phantoms.is_empty() {
                    let _ = writeln!(
                        out,
                        "Phantom titles: {}\n",
                        row.phantoms
                            .iter()
                            .map(|m| format!("“{m}”"))
                            .collect::<Vec<_>>()
                            .join(", ")
                    );
                }
                if !row.flags.is_empty() {
                    let flags: Vec<String> =
                        row.flags.iter().map(|(k, n)| format!("{k} {n}")).collect();
                    let _ = writeln!(out, "Flags: {}\n", flags.join(", "));
                }
            }
        }
        let _ = writeln!(out, "\n## Reading the signals\n");
        let _ = writeln!(
            out,
            "Score = (1 − recall) × 100 + phantoms × 2 + failed chunks × 10 + flagged/chunks × 10 + unresolved refs × 0.1 + (1 − parse rate) × 20 + incomplete × 50 + failed × 100.\n"
        );
        let _ = writeln!(out, "| Signal | Look in |\n|---|---|");
        for (signal, module) in SIGNAL_LEGEND {
            let _ = writeln!(out, "| {signal} | {module} |");
        }
        out
    }
}

/// Which module each report signal points at.
#[cfg(feature = "native")]
pub const SIGNAL_LEGEND: &[(&str, &str)] = &[
    (
        "missing titles, `missing_nav_title`",
        "`chunk.rs` (a title cut from its ingredient run), `lines.rs` title detection, `crosscheck.rs` title matching and nav resolution",
    ),
    (
        "phantoms, `phantom_title`, `caption_as_title`",
        "`validate.rs` (labels, captions, section headings), `crosscheck.rs` thin-recipe rule, `assemble.rs` (variation attach, retitle)",
    ),
    (
        "failed chunks, incomplete, transport errors",
        "`contract.rs` lowering faults, `validate.rs` hard faults, chunk budget, timeouts and rate limits in `run.rs`",
    ),
    (
        "`unassigned_lines`, `ingredient_like_ignored`, `low_amount_parse_rate`",
        "`validate.rs` thresholds, `lines.rs` quantity detection, parser gaps (corpus)",
    ),
    (
        "`prose_ingredients`, `recipe_without_steps`",
        "`validate.rs`, `assemble.rs`",
    ),
    (
        "unresolved references",
        "`refs.rs` (anchor → page → title), `lines.rs` link extraction",
    ),
    (
        "ingredient parse rate",
        "`parse.rs` and the `ingredient` crate",
    ),
    ("eta error", "`eta.rs`, catalog priors in `models.rs`"),
];

#[cfg(feature = "native")]
fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let cut: String = s.chars().take(max).collect();
        format!("{cut}…")
    }
}

/// The latest run of every distinct book in the store, joined to a scan
/// for paths and to the books the scan found but never extracted.
#[cfg(feature = "native")]
pub fn build_report(
    library_dir: Option<&std::path::Path>,
    scan: Option<&LibraryScan>,
    statuses: &std::collections::HashMap<String, RowStatus>,
) -> crate::error::Result<LibraryReport> {
    use crate::native::runs;
    let mut latest: std::collections::BTreeMap<String, runs::RunSummary> = Default::default();
    for summary in runs::list()? {
        latest.entry(summary.sha256.clone()).or_insert(summary);
    }
    let paths: std::collections::HashMap<&str, &std::path::Path> = scan
        .map(|s| {
            s.books
                .iter()
                .map(|b| (b.sha256.as_str(), b.path.as_path()))
                .collect()
        })
        .unwrap_or_default();
    let mut rows = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for (sha, summary) in &latest {
        if scan.is_some() && !paths.contains_key(sha.as_str()) {
            continue;
        }
        let extraction = runs::load(std::path::Path::new(&summary.path))?;
        let status = statuses.get(sha).cloned().unwrap_or(RowStatus::Existing);
        let path = paths
            .get(sha.as_str())
            .map(|p| p.to_string_lossy().into_owned());
        rows.push(BookRow::from_extraction(
            &extraction,
            Some(&summary.path),
            path.as_deref(),
            status,
        ));
        seen.insert(sha.clone());
    }
    if let Some(scan) = scan {
        for book in &scan.books {
            if seen.contains(&book.sha256) {
                continue;
            }
            let status = statuses
                .get(&book.sha256)
                .cloned()
                .unwrap_or(RowStatus::Skipped {
                    reason: "no run".into(),
                });
            let (title, authors) = crate::epub::open::Package::parse_file(&book.path)
                .map(|p| {
                    (
                        if p.title.is_empty() {
                            file_stem(&book.path)
                        } else {
                            p.title
                        },
                        p.authors,
                    )
                })
                .unwrap_or_else(|_| (file_stem(&book.path), Vec::new()));
            rows.push(BookRow::without_run(
                &title,
                &authors,
                Some(&book.path.to_string_lossy()),
                &book.sha256,
                status,
            ));
        }
    }
    Ok(LibraryReport::new(
        library_dir,
        scan.map(|s| s.duplicates.len()).unwrap_or(0),
        rows,
    ))
}

/// `<data dir>/ingredient-parser/cookbook/library/`, where sweep reports go.
#[cfg(feature = "native")]
pub fn reports_dir() -> Option<std::path::PathBuf> {
    ShaCache::default_path().and_then(|p| p.parent().map(std::path::Path::to_path_buf))
}

#[cfg(feature = "native")]
fn file_stem(path: &std::path::Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

#[cfg(all(test, feature = "native"))]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn finds_epubs_recursively_and_sorted() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("b/Book")).unwrap();
        std::fs::create_dir_all(dir.path().join(".hidden")).unwrap();
        std::fs::write(dir.path().join("b/Book/z.EPUB"), b"").unwrap();
        std::fs::write(dir.path().join("a.epub"), b"").unwrap();
        std::fs::write(dir.path().join("a.pdf"), b"").unwrap();
        std::fs::write(dir.path().join(".hidden/x.epub"), b"").unwrap();
        let found = find_epubs(dir.path());
        assert_eq!(
            found,
            [dir.path().join("a.epub"), dir.path().join("b/Book/z.EPUB")]
        );
    }

    /// Byte-identical copies collapse to the first path; the sha cache
    /// rehashes only when a file changes.
    #[test]
    fn scan_deduplicates_by_sha_and_remembers_hashes() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("copy")).unwrap();
        std::fs::write(dir.path().join("a.epub"), b"same bytes").unwrap();
        std::fs::write(dir.path().join("copy/a.epub"), b"same bytes").unwrap();
        std::fs::write(dir.path().join("b.epub"), b"other bytes").unwrap();
        let cache_path = dir.path().join("sha-cache.json");
        let mut shas = ShaCache::load(Some(&cache_path));
        let scan1 = scan(dir.path(), &mut shas);
        assert_eq!(scan1.books.len(), 2);
        assert_eq!(
            scan1.duplicates,
            vec![(dir.path().join("copy/a.epub"), dir.path().join("a.epub"))]
        );
        assert!(scan1.unreadable.is_empty());
        shas.save(Some(&cache_path)).unwrap();
        let mut reloaded = ShaCache::load(Some(&cache_path));
        assert_eq!(reloaded.entries.len(), 3);
        let scan2 = scan(dir.path(), &mut reloaded);
        assert_eq!(scan1, scan2);
        assert!(!reloaded.dirty, "nothing changed, nothing rehashed");
        std::fs::write(dir.path().join("b.epub"), b"other bytes, longer").unwrap();
        let scan3 = scan(dir.path(), &mut reloaded);
        assert!(reloaded.dirty);
        assert_ne!(scan3.books[1].sha256, scan2.books[1].sha256);
    }

    /// Lost contents titles outrank phantoms; a failed run outranks both;
    /// the Markdown lists rows in that order.
    #[test]
    fn problem_score_orders_rows_and_renders() {
        let mut fine = BookRow::without_run("Fine", &[], None, "a", RowStatus::Existing);
        fine.run = Some("fine.json".into());
        fine.recall = Some(1.0);
        fine.chunks = 10;
        fine.problem_score = problem_score(&fine);
        let mut lossy = fine.clone();
        lossy.title = "Lossy".into();
        lossy.recall = Some(0.9);
        lossy.missing = vec!["Soup".into()];
        lossy.problem_score = problem_score(&lossy);
        let mut phantoms = fine.clone();
        phantoms.title = "Phantoms".into();
        phantoms.phantoms = vec!["Wine".into(), "Note".into()];
        phantoms.problem_score = problem_score(&phantoms);
        let failed = BookRow::without_run(
            "Broken",
            &[],
            None,
            "d",
            RowStatus::Failed {
                error: "zip".into(),
            },
        );
        let report = LibraryReport::new(None, 1, vec![fine, phantoms, lossy, failed]);
        let order: Vec<&str> = report.rows.iter().map(|r| r.title.as_str()).collect();
        assert_eq!(order, ["Broken", "Lossy", "Phantoms", "Fine"]);
        assert_eq!(report.totals.with_runs, 3);
        assert_eq!(report.totals.failed, 1);
        assert_eq!(report.totals.missing, 1);
        assert_eq!(report.totals.phantoms, 2);
        let md = report.render_markdown();
        assert!(md.contains("| 1 | Broken | failed: zip |"));
        assert!(md.contains("### Lossy"));
        assert!(md.contains("Missing contents titles: “Soup”"));
        assert!(md.contains("Look in"));
    }
}
