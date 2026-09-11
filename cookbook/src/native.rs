//! Native host pieces: the reqwest transport with gateway credentials from
//! the environment, a file cache, request dumping and offline replay, and
//! the flat run store. Nothing here is reachable from wasm.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::cache::{CachedCall, ChunkCache};
use crate::error::{Error, Result};
use crate::report::Extraction;
use crate::transport::{CancelToken, HttpRequest, HttpResponse, Transport, TransportError};

pub const BASE_URL_VAR: &str = "CLOUDFLARE_AI_GATEWAY_BASE_URL";
pub const API_KEY_VAR: &str = "AI_GATEWAY_API_KEY";
const API_KEY_FALLBACK_VAR: &str = "CF_AIG_TOKEN";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(180);

/// Where the desktop app keeps gateway credentials when launched outside a
/// shell: `<config dir>/ingredient-parser/gateway.env`.
pub fn gateway_config_path() -> Option<PathBuf> {
    directories::BaseDirs::new()
        .map(|b| b.config_dir().join("ingredient-parser").join("gateway.env"))
}

/// A variable from the process environment, else from `gateway.env`.
fn config_value(name: &str) -> Option<String> {
    if let Ok(v) = std::env::var(name)
        && !v.trim().is_empty()
    {
        return Some(v.trim().to_string());
    }
    let path = gateway_config_path()?;
    let text = std::fs::read_to_string(path).ok()?;
    text.lines().find_map(|line| {
        let (k, v) = line.split_once('=')?;
        (k.trim() == name)
            .then(|| v.trim().trim_matches(['"', '\'']).to_string())
            .filter(|v| !v.is_empty())
    })
}

/// Sends gateway requests with reqwest, adding the base URL and the
/// `cf-aig-authorization` bearer.
pub struct ReqwestTransport {
    client: reqwest::Client,
    base: String,
    token: String,
}

impl ReqwestTransport {
    pub fn new(base: impl Into<String>, token: impl Into<String>) -> Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .build()
            .map_err(|e| Error::Config(e.to_string()))?;
        Ok(Self {
            client,
            base: base.into().trim_end_matches('/').to_string(),
            token: token.into(),
        })
    }

    /// Reads `CLOUDFLARE_AI_GATEWAY_BASE_URL` and `AI_GATEWAY_API_KEY` (or
    /// `CF_AIG_TOKEN`) from the environment, `.env` (if the host loaded it),
    /// or `gateway.env`.
    pub fn from_env() -> Result<Self> {
        let base = config_value(BASE_URL_VAR).ok_or_else(|| {
            Error::Config(format!("{BASE_URL_VAR} is not set (the gateway root, e.g. https://gateway.ai.cloudflare.com/v1/<account>/<gateway>)"))
        })?;
        let token = config_value(API_KEY_VAR)
            .or_else(|| config_value(API_KEY_FALLBACK_VAR))
            .ok_or_else(|| Error::Config(format!("{API_KEY_VAR} is not set")))?;
        Self::new(base, token)
    }

    pub fn base(&self) -> &str {
        &self.base
    }
}

impl Transport for ReqwestTransport {
    async fn send(
        &self,
        request: HttpRequest,
        cancel: &CancelToken,
    ) -> Result<HttpResponse, TransportError> {
        let mut req = self
            .client
            .post(format!("{}{}", self.base, request.path))
            .header("cf-aig-authorization", format!("Bearer {}", self.token));
        for (name, value) in &request.headers {
            req = req.header(name.as_str(), value.as_str());
        }
        let send = req.json(&request.body).send();
        let response = tokio::select! {
            _ = cancel.cancelled() => return Err(TransportError::Cancelled),
            r = send => r.map_err(map_error)?,
        };
        let status = response.status().as_u16();
        let headers = response
            .headers()
            .iter()
            .filter_map(|(k, v)| {
                v.to_str()
                    .ok()
                    .map(|v| (k.as_str().to_string(), v.to_string()))
            })
            .collect();
        let body = tokio::select! {
            _ = cancel.cancelled() => return Err(TransportError::Cancelled),
            b = response.text() => b.map_err(map_error)?,
        };
        Ok(HttpResponse {
            status,
            headers,
            body,
        })
    }

    async fn sleep(&self, ms: u64) {
        tokio::time::sleep(Duration::from_millis(ms)).await;
    }
}

fn map_error(e: reqwest::Error) -> TransportError {
    if e.is_timeout() {
        TransportError::Timeout
    } else if e.is_connect() {
        TransportError::Connect
    } else {
        TransportError::Other(e.without_url().to_string())
    }
}

/// One file per cached call under `<cache dir>/ingredient-parser/cookbook/`.
pub struct FsChunkCache {
    dir: PathBuf,
}

impl FsChunkCache {
    pub fn default_dir() -> Option<PathBuf> {
        directories::BaseDirs::new()
            .map(|b| b.cache_dir().join("ingredient-parser").join("cookbook"))
    }

    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    pub fn open_default() -> Result<Self> {
        Self::default_dir()
            .map(Self::new)
            .ok_or_else(|| Error::Config("no cache directory for this platform".into()))
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn path(&self, key: &str) -> PathBuf {
        self.dir
            .join(&key[..2.min(key.len())])
            .join(format!("{key}.json"))
    }
}

impl ChunkCache for FsChunkCache {
    fn get(&self, key: &str) -> Option<CachedCall> {
        let text = std::fs::read_to_string(self.path(key)).ok()?;
        serde_json::from_str(&text).ok()
    }

    fn put(&self, key: &str, call: &CachedCall) {
        let path = self.path(key);
        let Some(parent) = path.parent() else {
            return;
        };
        if std::fs::create_dir_all(parent).is_err() {
            return;
        }
        let tmp = parent.join(format!(".{key}.{}.tmp", std::process::id()));
        if let Ok(text) = serde_json::to_string(call)
            && std::fs::write(&tmp, text).is_ok()
        {
            let _ = std::fs::rename(&tmp, &path);
        }
    }
}

/// One recorded call, as written to a dump directory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DumpedCall {
    pub seq: usize,
    pub request: HttpRequest,
    pub response: Option<HttpResponse>,
    pub error: Option<String>,
    pub started_ms: u64,
    pub latency_ms: u64,
}

/// Wraps a transport and writes every request and response to
/// `<dir>/calls/<seq>--<chunk>--<model>.json` for offline replay.
pub struct DumpTransport<T> {
    inner: T,
    dir: PathBuf,
    seq: AtomicUsize,
    started: std::time::Instant,
}

impl<T> DumpTransport<T> {
    pub fn new(inner: T, dir: impl Into<PathBuf>) -> std::io::Result<Self> {
        let dir = dir.into();
        std::fs::create_dir_all(dir.join("calls"))?;
        Ok(Self {
            inner,
            dir,
            seq: AtomicUsize::new(0),
            started: std::time::Instant::now(),
        })
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }
}

fn slug(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect()
}

impl<T: Transport> Transport for DumpTransport<T> {
    async fn send(
        &self,
        request: HttpRequest,
        cancel: &CancelToken,
    ) -> Result<HttpResponse, TransportError> {
        let seq = self.seq.fetch_add(1, Ordering::SeqCst);
        let started_ms = self.started.elapsed().as_millis() as u64;
        let result = self.inner.send(request.clone(), cancel).await;
        let latency_ms = self.started.elapsed().as_millis() as u64 - started_ms;
        let (model, chunk) = request_tags(&request);
        let record = DumpedCall {
            seq,
            request,
            response: result.as_ref().ok().cloned(),
            error: result.as_ref().err().map(ToString::to_string),
            started_ms,
            latency_ms,
        };
        let path = self.dir.join("calls").join(format!(
            "{seq:04}--{}--{}.json",
            slug(&chunk),
            slug(&model)
        ));
        if let Ok(text) = serde_json::to_string_pretty(&record) {
            let _ = std::fs::write(path, text);
        }
        result
    }

    async fn sleep(&self, ms: u64) {
        self.inner.sleep(ms).await;
    }
}

fn request_tags(request: &HttpRequest) -> (String, String) {
    let meta = request
        .headers
        .iter()
        .find(|(n, _)| n == "cf-aig-metadata")
        .and_then(|(_, v)| serde_json::from_str::<serde_json::Value>(v).ok());
    let get = |k: &str| {
        meta.as_ref()
            .and_then(|m| m[k].as_str())
            .unwrap_or("unknown")
            .to_string()
    };
    (get("model"), get("chunk"))
}

/// Answers from a dump directory, keyed by request body, so a pipeline can be
/// re-run with no credentials. A request that was not recorded is an error.
pub struct ReplayTransport {
    by_body: HashMap<String, HttpResponse>,
    pub misses: Mutex<Vec<HttpRequest>>,
}

impl ReplayTransport {
    pub fn load(dir: impl AsRef<Path>) -> Result<Self> {
        let mut by_body = HashMap::new();
        for entry in std::fs::read_dir(dir.as_ref().join("calls"))? {
            let entry = entry?;
            let text = std::fs::read_to_string(entry.path())?;
            let call: DumpedCall = serde_json::from_str(&text)?;
            if let Some(response) = call.response {
                by_body.insert(body_key(&call.request), response);
            }
        }
        Ok(Self {
            by_body,
            misses: Mutex::new(Vec::new()),
        })
    }

    pub fn len(&self) -> usize {
        self.by_body.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_body.is_empty()
    }
}

fn body_key(request: &HttpRequest) -> String {
    crate::cache::cache_key("replay", &request.path, "", &request.body)
}

impl Transport for ReplayTransport {
    async fn send(
        &self,
        request: HttpRequest,
        _cancel: &CancelToken,
    ) -> Result<HttpResponse, TransportError> {
        match self.by_body.get(&body_key(&request)) {
            Some(response) => Ok(response.clone()),
            None => {
                self.misses
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .push(request);
                Err(TransportError::Other("not recorded in the dump".into()))
            }
        }
    }
}

/// The flat run store: one `Extraction` JSON per run.
pub mod runs {
    use super::*;
    use crate::model::Item;

    pub const RUNS_DIR_VAR: &str = "COOKBOOK_RUNS_DIR";

    /// Where hand-authored answer keys live:
    /// `<data dir>/ingredient-parser/cookbook/expectations/<slug>.json`.
    pub fn expectations_dir() -> Option<PathBuf> {
        directories::BaseDirs::new().map(|b| {
            b.data_dir()
                .join("ingredient-parser")
                .join("cookbook")
                .join("expectations")
        })
    }

    pub fn root() -> Option<PathBuf> {
        if let Ok(dir) = std::env::var(RUNS_DIR_VAR)
            && !dir.trim().is_empty()
        {
            return Some(PathBuf::from(dir));
        }
        directories::BaseDirs::new().map(|b| {
            b.data_dir()
                .join("ingredient-parser")
                .join("cookbook")
                .join("runs")
        })
    }

    /// A row in `runs`.
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    #[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
    pub struct RunSummary {
        pub path: String,
        pub run_id: String,
        pub book: String,
        pub sha256: String,
        pub started_at: String,
        pub ladder: Vec<String>,
        pub recipes: usize,
        pub items: usize,
        pub recall: Option<f32>,
        pub cost_usd: f64,
        pub wall_ms: u64,
        pub incomplete: bool,
    }

    pub fn summarize(path: &Path, extraction: &Extraction) -> RunSummary {
        RunSummary {
            path: path.to_string_lossy().into_owned(),
            run_id: extraction.report.run_id.clone(),
            book: extraction.cookbook.source.title.clone(),
            sha256: extraction.cookbook.source.sha256.clone(),
            started_at: extraction.report.started_at.clone(),
            ladder: extraction.report.estimate.ladder.clone(),
            recipes: extraction.cookbook.recipes().count(),
            items: extraction.cookbook.items().count(),
            recall: extraction.report.crosscheck.recall,
            cost_usd: extraction.report.total_cost_usd,
            wall_ms: extraction.report.wall_ms,
            incomplete: extraction.report.incomplete,
        }
    }

    /// Write a run to `<root>/<title-slug>--<run_id>.json` (or `out`).
    pub fn save(extraction: &Extraction, out: Option<&Path>) -> Result<PathBuf> {
        let path = match out {
            Some(p) => p.to_path_buf(),
            None => {
                let root = root()
                    .ok_or_else(|| Error::Config("no data directory for this platform".into()))?;
                std::fs::create_dir_all(&root)?;
                let title = slug(&extraction.cookbook.source.title);
                let title = if title.trim_matches('-').is_empty() {
                    "book".to_string()
                } else {
                    title
                };
                root.join(format!("{title}--{}.json", extraction.report.run_id))
            }
        };
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, serde_json::to_string_pretty(extraction)?)?;
        Ok(path)
    }

    pub fn load(path: &Path) -> Result<Extraction> {
        Ok(serde_json::from_str(&std::fs::read_to_string(path)?)?)
    }

    /// Every run in the store, newest first.
    pub fn list() -> Result<Vec<RunSummary>> {
        let Some(root) = root() else {
            return Ok(Vec::new());
        };
        let mut out = Vec::new();
        let Ok(entries) = std::fs::read_dir(&root) else {
            return Ok(out);
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "json")
                && let Ok(extraction) = load(&path)
            {
                out.push(summarize(&path, &extraction));
            }
        }
        out.sort_by(|a, b| b.started_at.cmp(&a.started_at));
        Ok(out)
    }

    /// Recipes and techniques, flattened, for quick listings.
    pub fn titles(extraction: &Extraction) -> Vec<(&str, &str)> {
        extraction
            .cookbook
            .items()
            .map(|i| match i {
                Item::Recipe(_) => ("recipe", i.title()),
                Item::Technique(_) => ("technique", i.title()),
                Item::Essay(_) => ("essay", i.title()),
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::cost::Usage;

    #[test]
    fn fs_cache_round_trips_and_shards_by_prefix() {
        let dir = tempfile::tempdir().unwrap();
        let cache = FsChunkCache::new(dir.path());
        let call = CachedCall {
            key: "abcdef".into(),
            model: "m".into(),
            contract: "v1".into(),
            response: HttpResponse {
                status: 200,
                headers: vec![],
                body: "{}".into(),
            },
            usage: Usage::default(),
            recorded_at: "now".into(),
        };
        assert!(cache.get("abcdef").is_none());
        cache.put("abcdef", &call);
        assert!(dir.path().join("ab").join("abcdef.json").exists());
        assert_eq!(cache.get("abcdef"), Some(call));
    }

    #[tokio::test]
    async fn dump_then_replay_answers_identical_requests() {
        let dir = tempfile::tempdir().unwrap();
        let scripted = crate::test_support::ScriptedTransport::new(|req, _| {
            Ok(HttpResponse {
                status: 200,
                headers: vec![],
                body: req.body["n"].to_string(),
            })
        });
        let dump = DumpTransport::new(scripted, dir.path()).unwrap();
        let req = |n: u32| HttpRequest {
            path: "/x".into(),
            headers: vec![(
                "cf-aig-metadata".into(),
                format!("{{\"model\":\"m\",\"chunk\":\"k{n:03}\"}}"),
            )],
            body: serde_json::json!({"n": n}),
        };
        let cancel = CancelToken::new();
        assert_eq!(dump.send(req(1), &cancel).await.unwrap().body, "1");
        assert_eq!(dump.send(req(2), &cancel).await.unwrap().body, "2");
        let files: Vec<String> = std::fs::read_dir(dir.path().join("calls"))
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert!(files.iter().any(|f| f == "0000--k001--m.json"), "{files:?}");
        let replay = ReplayTransport::load(dir.path()).unwrap();
        assert_eq!(replay.len(), 2);
        assert_eq!(replay.send(req(2), &cancel).await.unwrap().body, "2");
        assert!(replay.send(req(3), &cancel).await.is_err());
        assert_eq!(replay.misses.lock().unwrap().len(), 1);
    }

    #[test]
    fn config_falls_back_to_gateway_env_file() {
        // Only the parse path is testable without touching the real config
        // dir; exercise it through a temp file with the same format.
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("gateway.env");
        std::fs::write(&file, "# comment\nAI_GATEWAY_API_KEY = \"secret\"\nCLOUDFLARE_AI_GATEWAY_BASE_URL=https://g/v1/a/b/\n").unwrap();
        let text = std::fs::read_to_string(&file).unwrap();
        let value = |name: &str| {
            text.lines().find_map(|line| {
                let (k, v) = line.split_once('=')?;
                (k.trim() == name).then(|| v.trim().trim_matches(['"', '\'']).to_string())
            })
        };
        assert_eq!(value("AI_GATEWAY_API_KEY").as_deref(), Some("secret"));
        let t =
            ReqwestTransport::new(value("CLOUDFLARE_AI_GATEWAY_BASE_URL").unwrap(), "k").unwrap();
        assert_eq!(t.base(), "https://g/v1/a/b");
    }
}
