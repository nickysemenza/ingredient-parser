//! Native run discovery. JSON runs remain authoritative; index entries are disposable.
use super::{ReviewRun, error, hash};
use crate::EpubError;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
static SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunMetadata {
    #[serde(default)]
    pub parent_id: Option<String>,
    #[serde(default)]
    pub prompt_fingerprint: String,
    pub id: String,
    pub title: String,
    pub created_at: u64,
    #[serde(default)]
    pub updated_at: Option<u64>,
    pub operation: String,
    #[serde(default)]
    pub inherited_reserved_usd: f64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallRecord {
    #[serde(default)]
    pub failure: Option<serde_json::Value>,
    /// Provider details, including reasoning/cached token breakdowns when reported.
    #[serde(default)]
    pub raw_usage: Option<serde_json::Value>,
    pub source_key: String,
    pub request_fingerprint: String,
    pub usage: Option<crate::Usage>,
    pub estimated_usd: Option<f64>,
    pub error: Option<String>,
}
/// One chunk dispatch, including the transport's bounded retry usage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Charge {
    #[serde(default)]
    pub attempts: Vec<CallRecord>,
    pub chunk: String,
    pub model: String,
    pub started_at: u64,
    pub reservation_usd: f64,
    pub usage: Option<crate::Usage>,
    pub estimated_usd: Option<f64>,
    pub input_rate: Option<f64>,
    pub output_rate: Option<f64>,
    #[serde(default)]
    pub cache_read_rate: Option<f64>,
    #[serde(default)]
    pub cache_creation_rate: Option<f64>,
    pub rate_date: String,
    #[serde(default)]
    pub rate_source: Option<String>,
    pub status: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunSummary {
    #[serde(default)]
    pub summary_version: u32,
    #[serde(default)]
    pub quality_flags: Option<usize>,
    #[serde(default)]
    pub status: String,
    pub path: PathBuf,
    #[serde(default)]
    pub source: String,
    pub title: String,
    pub epub_sha256: String,
    pub model: String,
    #[serde(default)]
    pub configurations: Vec<String>,
    pub prompt_version: String,
    pub created_at: Option<u64>,
    pub recipes: usize,
    pub completed: usize,
    pub total: usize,
    pub incomplete: bool,
    pub reserved_usd: f64,
    #[serde(default)]
    pub new_spend_usd: Option<f64>,
    #[serde(default)]
    pub inherited_reserved_usd: Option<f64>,
    #[serde(default)]
    pub unresolved_usd: f64,
}
pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
pub(crate) fn new_id() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!(
        "{nanos:x}-{}-{}",
        std::process::id(),
        SEQUENCE.fetch_add(1, Ordering::Relaxed)
    )
}
pub fn root() -> Result<PathBuf, EpubError> {
    if let Some(path) = std::env::var_os("RECIPE_EPUB_RUNS_DIR") {
        return Ok(path.into());
    }
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let base = if cfg!(target_os = "macos") {
        home.ok_or_else(|| error("HOME is unavailable"))?
            .join("Library/Application Support")
    } else if cfg!(target_os = "windows") {
        std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .ok_or_else(|| error("LOCALAPPDATA is unavailable"))?
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| home.map(|h| h.join(".local/share")))
            .ok_or_else(|| error("HOME and XDG_DATA_HOME are unavailable"))?
    };
    Ok(base.join("ingredient-parser/cookbook-runs"))
}
fn slug(value: &str) -> String {
    let text: String = value
        .chars()
        .take(80)
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect();
    let text = text.trim_matches('-');
    if text.is_empty() {
        "cookbook".into()
    } else {
        text.into()
    }
}
pub fn inspect(path: &Path, model: &str) -> Result<ReviewRun, EpubError> {
    ReviewRun::inspect(
        &std::fs::read(path).map_err(|e| error(e.to_string()))?,
        &path.to_string_lossy(),
        model,
    )
}
pub fn destination(book: &Path, model: &str) -> Result<PathBuf, EpubError> {
    let directory = root()?.join(source_hash(book)?);
    std::fs::create_dir_all(&directory).map_err(|e| error(e.to_string()))?;
    let title = crate::book_metadata(book)?.title;
    Ok(directory.join(format!(
        "{}--{}--{}--{}.json",
        slug(&title),
        slug(model),
        now(),
        new_id()
    )))
}
pub fn summary(run: &ReviewRun, path: &Path) -> RunSummary {
    RunSummary {
        summary_version: 3,
        quality_flags: Some(
            super::quality::issues(run)
                .iter()
                .filter(|i| i.kind != "unextracted_chunk")
                .count(),
        ),
        status: if run.incomplete()
            && std::fs::File::open(path.with_extension("run-lock")).is_ok_and(|file| {
                fs2::FileExt::try_lock_shared(&file)
                    .is_err_and(|error| error.kind() == std::io::ErrorKind::WouldBlock)
            }) {
            "running".into()
        } else {
            run.status().into()
        },
        path: path.into(),
        source: run.source.clone(),
        title: run
            .metadata
            .as_ref()
            .map(|m| m.title.clone())
            .unwrap_or_else(|| {
                Path::new(&run.source)
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned()
            }),
        epub_sha256: run.epub_sha256.clone(),
        model: run.model.clone(),
        configurations: run
            .chunks
            .iter()
            .filter(|c| c.output.is_some())
            .map(|c| {
                format!(
                    "{} / {}",
                    c.model.as_deref().unwrap_or("unknown model"),
                    c.prompt_version.as_deref().unwrap_or("unknown prompt")
                )
            })
            // Requested configuration matters even if all its new calls failed:
            // inherited successes must not look like successes from this model.
            .chain(std::iter::once(format!(
                "{} / {}",
                run.model, run.prompt_version
            )))
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect(),
        prompt_version: run.prompt_version.clone(),
        created_at: run.metadata.as_ref().map(|m| m.created_at),
        recipes: run.recipes.len(),
        completed: run.chunks.iter().filter(|c| c.output.is_some()).count(),
        total: run.chunks.len(),
        incomplete: run.incomplete(),
        reserved_usd: run.reserved_usd,
        new_spend_usd: run.metadata.as_ref().map(|_| {
            run.charges
                .iter()
                .filter_map(|c| c.estimated_usd)
                .sum::<f64>()
                .max(0.0)
        }),
        inherited_reserved_usd: run.metadata.as_ref().map(|m| m.inherited_reserved_usd),
        unresolved_usd: run
            .charges
            .iter()
            .filter(|c| {
                c.estimated_usd.is_none()
                    || c.status == "failed"
                    || c.attempts.iter().any(|a| a.usage.is_none())
            })
            .map(|c| (c.reservation_usd - c.estimated_usd.unwrap_or(0.0)).max(0.0))
            .sum::<f64>()
            .max(0.0),
    }
}
pub fn register(run: &ReviewRun, path: &Path) -> Result<(), EpubError> {
    let path = path.canonicalize().map_err(|e| error(e.to_string()))?;
    let directory = root()?.join("index");
    std::fs::create_dir_all(&directory).map_err(|e| error(e.to_string()))?;
    let key = hash(path.to_string_lossy().as_bytes());
    let target = directory.join(format!("{key}.json"));
    let tmp = directory.join(format!(
        "{key}-{}-{}.tmp",
        std::process::id(),
        SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::write(&tmp, serde_json::to_vec(&summary(run, &path))?)
        .map_err(|e| error(e.to_string()))?;
    std::fs::rename(tmp, target).map_err(|e| error(e.to_string()))
}
pub fn list(book: Option<&Path>) -> Result<Vec<RunSummary>, EpubError> {
    let wanted = book.map(source_hash).transpose()?;
    let directory = root()?;
    let directory = directory.canonicalize().unwrap_or(directory);
    let mut found = std::collections::BTreeMap::new();
    for entry in walkdir::WalkDir::new(directory.join("index"))
        .max_depth(1)
        .into_iter()
        .filter_map(Result::ok)
    {
        if let Ok(bytes) = std::fs::read(entry.path())
            && let Ok(item) = serde_json::from_slice::<RunSummary>(&bytes)
            && item.path.exists()
        {
            let item = if item.summary_version != 3
                || item.status.is_empty()
                || item.status == "running"
                || item.quality_flags.is_none()
            {
                if let Ok(run) = ReviewRun::read(&item.path) {
                    let _ = register(&run, &item.path);
                    summary(&run, &item.path)
                } else {
                    item
                }
            } else {
                item
            };
            found.insert(item.path.clone(), item);
        }
    }
    for entry in walkdir::WalkDir::new(&directory)
        .max_depth(2)
        .into_iter()
        .filter_map(Result::ok)
    {
        let path = entry.path();
        if path.parent() == Some(directory.join("index").as_path())
            || path.extension().is_none_or(|e| e != "json")
            || found.contains_key(path)
        {
            continue;
        }
        if let Ok(run) = ReviewRun::read(path) {
            // A concurrent writer/removal must not make browsing fail.
            let _ = register(&run, path);
            found.insert(path.into(), summary(&run, path));
        }
    }
    let mut rows: Vec<_> = found
        .into_values()
        .filter(|r| wanted.as_ref().is_none_or(|h| h == &r.epub_sha256))
        .collect();
    rows.sort_by(|a, b| {
        b.created_at
            .cmp(&a.created_at)
            .then_with(|| a.path.cmp(&b.path))
    });
    Ok(rows)
}
/// Hash once per file metadata revision, including relocation-safe content identity.
pub fn source_hash(path: &Path) -> Result<String, EpubError> {
    #[derive(Serialize, Deserialize)]
    struct Identity {
        length: u64,
        modified: u128,
        digest: String,
    }
    let path = path.canonicalize().map_err(|e| error(e.to_string()))?;
    let meta = path.metadata().map_err(|e| error(e.to_string()))?;
    let modified = meta
        .modified()
        .map_err(|e| error(e.to_string()))?
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let dir = root()?.join("source-identities");
    let cached = dir.join(format!("{}.json", hash(path.to_string_lossy().as_bytes())));
    if let Ok(bytes) = std::fs::read(&cached)
        && let Ok(identity) = serde_json::from_slice::<Identity>(&bytes)
        && identity.length == meta.len()
        && identity.modified == modified
    {
        return Ok(identity.digest);
    }
    let digest = hash(&std::fs::read(&path).map_err(|e| error(e.to_string()))?);
    std::fs::create_dir_all(dir).map_err(|e| error(e.to_string()))?;
    std::fs::write(
        cached,
        serde_json::to_vec(&Identity {
            length: meta.len(),
            modified,
            digest: digest.clone(),
        })?,
    )
    .map_err(|e| error(e.to_string()))?;
    Ok(digest)
}
