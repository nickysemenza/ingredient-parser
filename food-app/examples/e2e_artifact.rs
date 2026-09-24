//! Seal and verify the offline desktop E2E output.
//! Run `seal QA_DIR PLAYWRIGHT_RESULTS_DIR`, then `verify QA_DIR/e2e-artifact`.

use cookbook::bundle::{BundleManifest, export};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::Command;
use zip::ZipArchive;

type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(Serialize, Deserialize)]
struct Evidence {
    format: String,
    version: u32,
    git_head: String,
    playwright_passed: bool,
    replay: String,
    sha256: BTreeMap<String, String>,
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn file_digest(path: &Path) -> Result<String> {
    Ok(digest(&fs::read(path)?))
}

fn copy_file(from: &Path, to: &Path) -> Result {
    fs::copy(from, to)?;
    Ok(())
}

fn copy_tree(from: &Path, to: &Path) -> Result {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else {
            copy_file(&entry.path(), &target)?;
        }
    }
    Ok(())
}

fn files_in(root: &Path, dir: &Path, files: &mut BTreeMap<String, String>) -> Result {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if entry.file_type()?.is_dir() {
            files_in(root, &path, files)?;
        } else if path != root.join("evidence.json") {
            let relative = path.strip_prefix(root)?.to_string_lossy().into_owned();
            files.insert(relative, file_digest(&path)?);
        }
    }
    Ok(())
}

fn report_passed(path: &Path) -> Result<bool> {
    let report: serde_json::Value = serde_json::from_slice(&fs::read(path)?)?;
    let stats = &report["stats"];
    let unexpected = stats["unexpected"]
        .as_u64()
        .ok_or_else(|| invalid("Playwright report has no unexpected count"))?;
    let expected = stats["expected"]
        .as_u64()
        .ok_or_else(|| invalid("Playwright report has no expected count"))?;
    let interrupted = stats["interrupted"].as_u64().unwrap_or(0);
    Ok(expected > 0 && unexpected == 0 && interrupted == 0)
}

fn git_head() -> Result<String> {
    if let Ok(sha) = std::env::var("GITHUB_SHA") {
        return Ok(sha);
    }
    let output = Command::new("git").args(["rev-parse", "HEAD"]).output()?;
    if !output.status.success() {
        return Err(invalid("git rev-parse HEAD failed").into());
    }
    Ok(String::from_utf8(output.stdout)?.trim().to_owned())
}

fn fixture_path(fixture: &serde_json::Value, field: &str) -> Result<PathBuf> {
    fixture[field]["path"]
        .as_str()
        .map(PathBuf::from)
        .ok_or_else(|| invalid(format!("frontend fixture has no {field}.path")).into())
}

fn seal(qa_dir: &Path, results_dir: &Path) -> Result {
    let artifact = qa_dir.join("e2e-artifact");
    if artifact.exists() {
        fs::remove_dir_all(&artifact)?;
    }
    fs::create_dir_all(&artifact)?;
    let fixture: serde_json::Value =
        serde_json::from_slice(&fs::read(qa_dir.join("frontend-fixture.json"))?)?;
    let run = fixture["runs"][0]["path"]
        .as_str()
        .ok_or_else(|| invalid("frontend fixture has no first saved run"))?;
    copy_file(
        &qa_dir.join("cookbook.epub"),
        &artifact.join("cookbook.epub"),
    )?;
    copy_file(Path::new(run), &artifact.join("run.json"))?;
    copy_file(
        &fixture_path(&fixture, "bundle")?,
        &artifact.join("export.cookbook.zip"),
    )?;
    copy_file(&qa_dir.join("corpus.jsonl"), &artifact.join("corpus.jsonl"))?;
    copy_tree(results_dir, &artifact.join("test-results"))?;
    let passed = report_passed(&artifact.join("test-results/results.json"))?;
    let mut sha256 = BTreeMap::new();
    files_in(&artifact, &artifact, &mut sha256)?;
    if passed && !sha256.keys().any(|path| path.ends_with(".png")) {
        return Err(invalid("Playwright produced no screenshot").into());
    }
    let evidence = Evidence {
        format: "ingredient-parser-e2e-evidence".into(),
        version: 1,
        git_head: git_head()?,
        playwright_passed: passed,
        replay: "cargo run -p food-app --example e2e_artifact -- verify ARTIFACT_DIR".into(),
        sha256,
    };
    fs::write(
        artifact.join("evidence.json"),
        serde_json::to_vec_pretty(&evidence)?,
    )?;
    verify(&artifact)?;
    println!("sealed {}", artifact.display());
    Ok(())
}

fn zip_bytes(zip: &mut ZipArchive<File>, name: &str) -> Result<Vec<u8>> {
    let mut data = Vec::new();
    zip.by_name(name)?.read_to_end(&mut data)?;
    Ok(data)
}

fn verify_bundle(root: &Path) -> Result {
    let epub = root.join("cookbook.epub");
    let run = root.join("run.json");
    let bundle = root.join("export.cookbook.zip");
    let mut zip = ZipArchive::new(File::open(&bundle)?)?;
    let manifest: BundleManifest = serde_json::from_slice(&zip_bytes(&mut zip, "manifest.json")?)?;
    if manifest.source_sha256 != file_digest(&epub)? {
        return Err(invalid("bundle source hash does not match retained EPUB").into());
    }
    let archived: serde_json::Value =
        serde_json::from_slice(&zip_bytes(&mut zip, &manifest.extraction)?)?;
    let saved: serde_json::Value = serde_json::from_slice(&fs::read(&run)?)?;
    if archived != saved {
        return Err(invalid("bundle extraction differs from retained run").into());
    }
    for image in &manifest.images {
        let bytes = zip_bytes(&mut zip, &image.path)?;
        if digest(&bytes) != image.sha256 || bytes.len() as u64 != image.bytes {
            return Err(invalid(format!("bundle image differs: {}", image.path)).into());
        }
    }
    zip_bytes(&mut zip, &manifest.preview)?;
    let temp = tempfile::tempdir()?;
    let replay = temp.path().join("replay.cookbook.zip");
    export(&run, &epub, Some(&replay))?;
    if fs::read(&bundle)? != fs::read(replay)? {
        return Err(invalid("re-export differs from retained bundle").into());
    }
    Ok(())
}

fn verify(root: &Path) -> Result {
    let evidence: Evidence = serde_json::from_slice(&fs::read(root.join("evidence.json"))?)?;
    if evidence.format != "ingredient-parser-e2e-evidence" || evidence.version != 1 {
        return Err(invalid("unsupported evidence format").into());
    }
    let mut actual = BTreeMap::new();
    files_in(root, root, &mut actual)?;
    if actual != evidence.sha256 {
        return Err(invalid("evidence file list or SHA-256 differs").into());
    }
    if evidence.playwright_passed && !actual.keys().any(|path| path.ends_with(".png")) {
        return Err(invalid("evidence contains no screenshot").into());
    }
    if report_passed(&root.join("test-results/results.json"))? != evidence.playwright_passed {
        return Err(invalid("Playwright status differs from evidence manifest").into());
    }
    verify_bundle(root)?;
    println!(
        "verified {} (Playwright passed: {})",
        root.display(),
        evidence.playwright_passed
    );
    Ok(())
}

fn main() -> Result {
    let mut args = std::env::args_os().skip(1);
    let mode = args
        .next()
        .ok_or_else(|| invalid("expected seal or verify"))?;
    let root = PathBuf::from(args.next().ok_or_else(|| invalid("expected directory"))?);
    match mode.to_str() {
        Some("seal") => {
            let results = PathBuf::from(
                args.next()
                    .ok_or_else(|| invalid("seal requires Playwright results directory"))?,
            );
            if args.next().is_some() {
                return Err(invalid("too many arguments").into());
            }
            seal(&root, &results)
        }
        Some("verify") if args.next().is_none() => verify(&root),
        _ => {
            Err(invalid("usage: e2e_artifact seal QA_DIR RESULTS_DIR | verify ARTIFACT_DIR").into())
        }
    }
}
