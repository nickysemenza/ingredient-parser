//! The web build's backend. The frontend's worker calls [`dispatch`] with the
//! same command names and JSON arguments the desktop app sends through Tauri,
//! so both hosts answer from one implementation (`food-core`).
//!
//! The browser has no filesystem: files the user opens are handed over with
//! [`put_file`] and addressed by name afterwards. Run files are summarized on
//! arrival so an opened book can list them.

use std::collections::HashMap;
use std::sync::Mutex;

use cookbook::{Extraction, RunSummary};
use food_core::{AppResult, Host};
use wasm_bindgen::prelude::*;

#[derive(Default)]
struct Files {
    bytes: HashMap<String, (u64, Vec<u8>)>,
    runs: Vec<RunSummary>,
    version: u64,
}

static FILES: Mutex<Option<Files>> = Mutex::new(None);

fn with_files<T>(f: impl FnOnce(&mut Files) -> AppResult<T>) -> AppResult<T> {
    let mut guard = FILES.lock().map_err(|_| "File store is unavailable")?;
    f(guard.get_or_insert_with(Files::default))
}

struct WebHost;

impl Host for WebHost {
    fn read(&self, path: &str) -> AppResult<Vec<u8>> {
        with_files(|files| {
            files
                .bytes
                .get(path)
                .map(|(_, bytes)| bytes.clone())
                .ok_or_else(|| format!("{path} is not open. Open the file again."))
        })
    }
    fn stamp(&self, path: &str) -> AppResult<String> {
        with_files(|files| {
            files
                .bytes
                .get(path)
                .map(|(version, _)| version.to_string())
                .ok_or_else(|| format!("{path} is not open. Open the file again."))
        })
    }
    fn runs_for(&self, sha256: &str) -> AppResult<Vec<RunSummary>> {
        with_files(|files| {
            Ok(files
                .runs
                .iter()
                .rev()
                .filter(|run| run.sha256 == sha256)
                .cloned()
                .collect())
        })
    }
}

#[wasm_bindgen(start)]
pub fn init() {
    console_error_panic_hook::set_once();
}

/// Run a portable command: `args` and the result are JSON text.
#[wasm_bindgen]
pub fn dispatch(command: &str, args: &str) -> Result<String, String> {
    let value = serde_json::from_str(args).map_err(|e| format!("Invalid arguments: {e}"))?;
    let result = food_core::dispatch(&WebHost, command, value)?;
    serde_json::to_string(&result).map_err(|e| e.to_string())
}

/// Store a file the user opened under `path`, replacing an earlier one. A saved
/// extraction run is also summarized so its book lists it.
#[wasm_bindgen]
pub fn put_file(path: &str, bytes: Vec<u8>) -> Result<(), String> {
    let run = serde_json::from_slice::<Extraction>(&bytes)
        .ok()
        .map(|extraction| RunSummary::of(path.to_owned(), &extraction));
    with_files(|files| {
        files.version += 1;
        files.runs.retain(|r| r.path != path);
        files.runs.extend(run);
        files.bytes.insert(path.to_owned(), (files.version, bytes));
        Ok(())
    })
}

/// Summaries of every run file opened this session, newest first (JSON).
#[wasm_bindgen]
pub fn list_runs() -> Result<String, String> {
    with_files(|files| {
        let runs: Vec<_> = files.runs.iter().rev().collect();
        serde_json::to_string(&runs).map_err(|e| e.to_string())
    })
}

/// The command names `dispatch` accepts (JSON array).
#[wasm_bindgen]
pub fn commands() -> String {
    serde_json::to_string(food_core::COMMANDS).unwrap_or_default()
}
