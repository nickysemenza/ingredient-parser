//! Offline compatibility and timing evidence for the shared source index.
//!
//! The baseline must be a previously captured `food-cli cookbook inspect`
//! JSON result. Full indexed source goes only to the explicit output path;
//! stdout contains hashes, counts, and timing, never cookbook prose.
use recipe_epub::chunk_epub_indexed;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{env, fs, io::Write, path::PathBuf, time::Instant};

fn hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = env::args_os().skip(1).map(PathBuf::from).collect();
    let [book, baseline, output] = args.as_slice() else {
        return Err("usage: benchmark_source_index BOOK.epub PREVIOUS_INSPECTION.json PRIVATE_INDEX_OUTPUT.json".into());
    };
    if output.exists() {
        return Err("index output already exists; choose a new evidence path".into());
    }
    let baseline_bytes = fs::read(baseline)?;
    let baseline: Value = serde_json::from_slice(&baseline_bytes)?;
    let expected = baseline["chunks"]
        .as_array()
        .ok_or("baseline lacks chunks")?;
    let started = Instant::now();
    let bytes = fs::read(book)?;
    let read_ms = started.elapsed().as_secs_f64() * 1000.0;
    let indexed_at = Instant::now();
    let indexed = chunk_epub_indexed(&bytes)?;
    let index_ms = indexed_at.elapsed().as_secs_f64() * 1000.0;
    if baseline["epub_sha256"].as_str() != Some(hash(&bytes).as_str()) {
        return Err("baseline EPUB hash differs from the measured book".into());
    }
    if indexed.len() != expected.len() {
        return Err("indexed chunk count differs from the frozen baseline".into());
    }
    let mut line_count = 0usize;
    let mut transformed = 0usize;
    let mut unlocated = 0usize;
    for (index, (chunk, previous)) in indexed.iter().zip(expected).enumerate() {
        if serde_json::to_value(&chunk.chunk)? != previous["source"] {
            return Err(format!("chunk {index} differs from the frozen baseline").into());
        }
        if chunk.lines.len() != chunk.chunk.text.lines().count() {
            return Err(format!("chunk {index} has incomplete source coordinates").into());
        }
        line_count += chunk.lines.len();
        transformed += chunk.lines.iter().filter(|line| line.transformed).count();
        unlocated += chunk
            .lines
            .iter()
            .filter(|line| line.contributors.is_empty())
            .count();
    }
    if unlocated > 0 {
        return Err(format!("{unlocated} cleaned lines lack DOM contributors").into());
    }
    let output_bytes = serde_json::to_vec(&indexed)?;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)?;
    file.write_all(&output_bytes)?;
    file.sync_all()?;
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "epub_sha256": hash(&bytes),
            "baseline_sha256": hash(&baseline_bytes),
            "index_sha256": hash(&output_bytes),
            "read_ms": read_ms,
            "source_index_ms": index_ms,
            "chunks": indexed.len(),
            "source_lines": line_count,
            "transformed_lines": transformed,
            "unlocated_lines": unlocated,
            "chunk_serialization_matches_frozen_baseline": true,
            "measurement": "single process, no model calls; filesystem cache uncontrolled; indexing only, not extraction or semantic quality"
        }))?
    );
    Ok(())
}
