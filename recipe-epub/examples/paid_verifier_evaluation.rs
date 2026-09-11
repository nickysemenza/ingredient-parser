//! Run one source-indexed candidate through the opt-in verifier trial stages.
//!
//! This example is intentionally not part of normal extraction. It uses a
//! separately maintained evaluation ledger and writes its checkpoint outside
//! the repository. The ledger reservation occurs in the call closure, directly
//! before the native provider dispatch; the portable state is checkpointed
//! before dispatch by `recovery::run`.

use recipe_epub::{
    ExtractedRecipe, Options, RecoveryBackend, Usage,
    experiment::Ledger,
    recovery::{Action, Adapter, Candidate, Reply, State, run},
    review::ReviewRun,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, HashSet},
    env, fs,
    path::{Path, PathBuf},
    rc::Rc,
};

#[derive(Clone)]
struct Args {
    run: PathBuf,
    /// Original saved-run indices, in source order. They remain distinct from
    /// the compact source indices used by `recovery::State`.
    chunks: Vec<usize>,
    ledger: PathBuf,
    checkpoint: PathBuf,
    candidate_model: Option<String>,
    candidate_output: Option<PathBuf>,
    candidate_output_chunk: Option<usize>,
    candidate_output_source_index: Option<usize>,
    /// An external source-derived split/linked group. It has exact source
    /// fragments and candidate outputs; it is never generated from a model.
    source_group: Option<PathBuf>,
    source_inventory: PathBuf,
    verifier_mode: String,
}

fn next_value(values: &mut impl Iterator<Item = String>, name: &str) -> Result<String, String> {
    values.next().ok_or_else(|| format!("missing {name}"))
}

fn args() -> Result<Args, String> {
    let mut values = env::args().skip(1);
    let mut run = None;
    let mut chunks = vec![];
    let mut ledger = None;
    let mut checkpoint = None;
    let mut candidate_model = None;
    let mut candidate_output = None;
    let mut candidate_output_chunk = None;
    let mut candidate_output_source_index = None;
    let mut source_group = None;
    let mut source_inventory = None;
    let mut verifier_mode = "both".to_owned();
    while let Some(flag) = values.next() {
        match flag.as_str() {
            "--run" => run = Some(next_value(&mut values, "--run value")?.into()),
            "--chunk" => chunks.push(next_value(&mut values, "--chunk value")?.parse().map_err(|_| "--chunk must be a zero-based integer")?),
            "--ledger" => ledger = Some(next_value(&mut values, "--ledger value")?.into()),
            "--checkpoint" => checkpoint = Some(next_value(&mut values, "--checkpoint value")?.into()),
            "--candidate-model" => candidate_model = Some(next_value(&mut values, "--candidate-model value")?),
            "--candidate-output" => candidate_output = Some(next_value(&mut values, "--candidate-output value")?.into()),
            "--candidate-output-chunk" => candidate_output_chunk = Some(next_value(&mut values, "--candidate-output-chunk value")?.parse().map_err(|_| "--candidate-output-chunk must be a zero-based integer")?),
            "--candidate-output-source-index" => candidate_output_source_index = Some(next_value(&mut values, "--candidate-output-source-index value")?.parse().map_err(|_| "--candidate-output-source-index must be a zero-based integer")?),
            "--source-group" => source_group = Some(next_value(&mut values, "--source-group value")?.into()),
            "--source-inventory" => source_inventory = Some(next_value(&mut values, "--source-inventory value")?.into()),
            "--verifier-mode" => verifier_mode = next_value(&mut values, "--verifier-mode value")?,
            "--ledger-tool" => { let _ = next_value(&mut values, "--ledger-tool value")?; },
            "--help" => return Err("usage: paid_verifier_evaluation --run RUN (--chunk INDEX [--chunk INDEX ...] | --source-group GROUP.json) --ledger LEDGER --checkpoint OUT --source-inventory INVENTORY [--verifier-mode baseline|cheap|both] [--candidate-model MODEL] [--candidate-output JSON (--candidate-output-chunk INDEX | --candidate-output-source-index INDEX)] [--ledger-tool PYTHON_TOOL]".into()),
            other => return Err(format!("unknown option {other}")),
        }
    }
    Ok(Args {
        run: run.ok_or("--run is required")?,
        chunks,
        ledger: ledger.ok_or("--ledger is required")?,
        checkpoint: checkpoint.ok_or("--checkpoint is required")?,
        candidate_model,
        candidate_output,
        candidate_output_chunk,
        candidate_output_source_index,
        source_group,
        source_inventory: source_inventory.ok_or("--source-inventory is required")?,
        verifier_mode,
    })
}

#[derive(Serialize)]
struct EvaluationMapping<'a> {
    /// State source index to immutable original saved-run index. This lets
    /// reviewers recover the exact EPUB chunk even when a linked group is
    /// hydrated compactly into State.
    original_chunk_indices: &'a [usize],
    /// Deliberately an array: a simulated source split can map several compact
    /// State chunks to the same original saved-run chunk.
    source_chunks: Vec<SourceChunkMapping<'a>>,
    ledger_entries: &'a HashMap<String, String>,
}

#[derive(Serialize)]
struct SourceChunkMapping<'a> {
    source_index: usize,
    original_chunk_index: usize,
    original_chunk_id: &'a str,
}

/// External artifact for a source-faithful simulated hard boundary. Every
/// fragment must retain its original saved-run identity; no provider response
/// is accepted as source input.
#[derive(Deserialize)]
struct SourceGroup {
    source_run_sha256: String,
    fragments: Vec<SourceFragment>,
}

#[derive(Deserialize)]
struct SourceFragment {
    original_chunk_index: usize,
    original_chunk_id: String,
    source: recipe_epub::Chunk,
    source_sha256: String,
    candidate_output: Option<Vec<ExtractedRecipe>>,
}

fn sha256(text: &[u8]) -> String {
    Sha256::digest(text)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn ledger(args: &Args, action: &str, entry: &str, amount: f64) -> Result<(), String> {
    let ledger = Ledger::open(&args.ledger);
    match action {
        "reserve" => ledger.reserve(
            "verifier",
            entry,
            amount,
            "source-indexed verifier admission",
        ),
        "settle" => ledger.settle("verifier", entry, amount),
        "unknown" => ledger.mark_unknown("verifier", entry, amount),
        _ => return Err("unknown ledger operation".into()),
    }
    .map_err(|error| error.to_string())
}

fn checkpoint(path: &Path, state: &State) -> Result<(), String> {
    let temporary = path.with_extension(format!("{}.tmp", std::process::id()));
    fs::write(
        &temporary,
        serde_json::to_vec_pretty(state).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    fs::rename(temporary, path).map_err(|e| e.to_string())
}

fn nth_entry(key: &str, attempts: &[recipe_epub::recovery::Attempt], index: usize) -> String {
    let ordinal = attempts[..=index]
        .iter()
        .filter(|attempt| attempt.key == key)
        .count();
    format!("{key}#{ordinal}")
}

/// Candidate-owned claims for every source line.
///
/// These are deliberately derived from the candidate payload alone. The
/// frozen publisher inventory is an offline adjudication oracle; putting its
/// labels here would leak the expected answer into the verifier request.
fn candidate_only_roles(source: &recipe_epub::Chunk, candidate: &[ExtractedRecipe]) -> Vec<String> {
    let mut titles = HashSet::new();
    let mut ingredients = HashSet::new();
    let mut methods = HashSet::new();
    let mut metadata = HashSet::new();
    let mut variations = HashSet::new();
    let mut title_line_indices = HashSet::new();
    for recipe in candidate {
        titles.extend(candidate_lines(&recipe.meta.title));
        title_line_indices.extend(title_constituent_lines(source, &recipe.meta.title));
        let meta = serde_json::to_value(&recipe.meta).unwrap_or(Value::Null);
        for text in json_strings(&meta) {
            metadata.extend(candidate_lines(text));
        }
        for section in &recipe.sections {
            if let Some(name) = &section.name {
                metadata.extend(candidate_lines(name));
            }
            ingredients.extend(
                section
                    .ingredients
                    .iter()
                    .flat_map(|line| candidate_lines(line)),
            );
            methods.extend(
                section
                    .instructions
                    .iter()
                    .flat_map(|line| candidate_lines(line)),
            );
        }
    }
    for text in titles
        .iter()
        .chain(ingredients.iter())
        .chain(methods.iter())
        .chain(metadata.iter())
        .filter(|text| text.to_ascii_lowercase().contains("variation"))
    {
        variations.insert(text.clone());
    }
    source
        .text
        .lines()
        .enumerate()
        .map(|(index, line)| {
            let line = normalize(line);
            if title_line_indices.contains(&index) || titles.contains(&line) {
                "title"
            } else if ingredients.contains(&line) {
                "ingredient"
            } else if methods.contains(&line) {
                "method"
            } else if variations.contains(&line) {
                "variation"
            } else if metadata.contains(&line) {
                "metadata"
            } else if candidate.is_empty() {
                "non_recipe"
            } else {
                "ambiguous"
            }
        })
        .map(str::to_owned)
        .collect()
}

fn normalize(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Candidate fields sometimes retain multi-paragraph newlines. Splitting them
/// before matching preserves their actual claims at the source-line granularity
/// without consulting the publisher inventory.
fn candidate_lines(text: &str) -> Vec<String> {
    text.lines()
        .map(normalize)
        .filter(|line| !line.is_empty())
        .collect()
}

/// Return a title's source-line constituents only when exactly one ordered
/// sequence of two or more *whole* source lines reconstructs the candidate
/// title. This supports publishers that put bilingual title fragments on
/// separate lines (with headnote lines between them) without guessing from
/// title words or using an external source inventory.
fn title_constituent_lines(source: &recipe_epub::Chunk, title: &str) -> Vec<usize> {
    let title = normalize(title);
    if title.is_empty() {
        return vec![];
    }
    let source_lines: Vec<_> = source
        .text
        .lines()
        .enumerate()
        .filter_map(|(index, line)| {
            let line = normalize(line);
            (!line.is_empty()).then_some((index, line))
        })
        .collect();
    let mut matches = vec![];
    title_constituent_search(&title, &source_lines, 0, 0, &mut vec![], &mut matches);
    if matches.len() == 1 {
        matches.pop().unwrap_or_default()
    } else {
        vec![]
    }
}

fn title_constituent_search(
    title: &str,
    source_lines: &[(usize, String)],
    next_source_index: usize,
    offset: usize,
    path: &mut Vec<usize>,
    matches: &mut Vec<Vec<usize>>,
) {
    // Ambiguous decompositions are intentionally discarded by the caller.
    if matches.len() > 1 {
        return;
    }
    if offset == title.len() {
        if path.len() >= 2 {
            matches.push(path.clone());
        }
        return;
    }
    for (source_index, line) in source_lines {
        if *source_index < next_source_index || !title[offset..].starts_with(line) {
            continue;
        }
        let end = offset + line.len();
        if end < title.len() && title.as_bytes()[end] != b' ' {
            continue;
        }
        path.push(*source_index);
        title_constituent_search(
            title,
            source_lines,
            source_index.saturating_add(1),
            if end == title.len() { end } else { end + 1 },
            path,
            matches,
        );
        path.pop();
    }
}

fn json_strings(value: &Value) -> Vec<&str> {
    match value {
        Value::String(text) => vec![text],
        Value::Array(items) => items.iter().flat_map(json_strings).collect(),
        Value::Object(entries) => entries.values().flat_map(json_strings).collect(),
        _ => vec![],
    }
}

#[cfg(test)]
mod candidate_role_tests {
    use super::candidate_only_roles;
    use recipe_epub::{Chunk, ExtractedRecipe, RecipeMeta, RecipeSection};

    #[test]
    fn missing_candidate_method_is_ambiguous_not_gold_labeled_method() {
        let source = Chunk {
            text: "Example soup\n1 cup water\nStir the soup.\nCover the pot.".into(),
            doc_path: "chapter.xhtml".into(),
            title_hint: None,
            links: vec![],
            images: vec![],
        };
        let candidate = ExtractedRecipe {
            meta: RecipeMeta {
                title: "Example soup".into(),
                ..Default::default()
            },
            sections: vec![RecipeSection::new(
                vec!["1 cup water".into()],
                vec!["Stir the soup.".into()],
            )],
        };

        assert_eq!(
            candidate_only_roles(&source, &[candidate]),
            vec!["title", "ingredient", "method", "ambiguous"]
        );
    }

    #[test]
    fn combined_bilingual_title_maps_only_its_ordered_whole_line_constituents() {
        let source = Chunk {
            text: "Sopa de ejemplo\nA short headnote.\nExample Soup\n1 cup water\nStir the soup.\nCover the pot."
                .into(),
            doc_path: "chapter.xhtml".into(),
            title_hint: None,
            links: vec![],
            images: vec![],
        };
        let candidate = ExtractedRecipe {
            meta: RecipeMeta {
                title: "Sopa de ejemplo Example Soup".into(),
                ..Default::default()
            },
            sections: vec![RecipeSection::new(
                vec!["1 cup water".into()],
                vec!["Stir the soup.".into()],
            )],
        };

        assert_eq!(
            candidate_only_roles(&source, &[candidate]),
            vec![
                "title",
                "ambiguous",
                "title",
                "ingredient",
                "method",
                "ambiguous"
            ]
        );
    }

    #[test]
    fn title_reconstruction_rejects_duplicate_paths_and_partial_words() {
        for text in ["Example\nExample\nSoup", "Exam\nSoup", "Soup\nExample"] {
            let source = Chunk {
                text: text.into(),
                doc_path: "chapter.xhtml".into(),
                title_hint: None,
                links: vec![],
                images: vec![],
            };
            assert!(super::title_constituent_lines(&source, "Example Soup").is_empty());
        }
    }

    #[test]
    fn empty_candidate_claims_non_recipe_without_source_inventory() {
        let source = Chunk {
            text: "Kitchen notes\nUse fresh herbs.".into(),
            doc_path: "intro.xhtml".into(),
            title_hint: None,
            links: vec![],
            images: vec![],
        };
        assert_eq!(
            candidate_only_roles(&source, &[]),
            vec!["non_recipe", "non_recipe"]
        );
    }
}

fn save_mapping(
    path: &Path,
    original_chunk_indices: &[usize],
    source_chunk_ids: &[String],
    mapping: &HashMap<String, String>,
) -> Result<(), String> {
    let temporary = path.with_extension(format!("{}.tmp", std::process::id()));
    let source_chunks = original_chunk_indices
        .iter()
        .enumerate()
        .map(|(source_index, original_chunk_index)| SourceChunkMapping {
            source_index,
            original_chunk_index: *original_chunk_index,
            original_chunk_id: source_chunk_ids[source_index].as_str(),
        })
        .collect();
    let audit = EvaluationMapping {
        original_chunk_indices,
        source_chunks,
        ledger_entries: mapping,
    };
    fs::write(
        &temporary,
        serde_json::to_vec_pretty(&audit).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    fs::rename(temporary, path).map_err(|e| e.to_string())
}

fn no_cache(_: &Action) -> Option<Value> {
    None
}
fn no_cache_write(_: &Action, _: &Value) -> Result<(), String> {
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = args().map_err(std::io::Error::other)?;
    if !matches!(args.verifier_mode.as_str(), "baseline" | "cheap" | "both") {
        return Err("--verifier-mode must be baseline, cheap, or both".into());
    }
    if args.chunks.is_empty() && args.source_group.is_none() {
        return Err("at least one --chunk is required".into());
    }
    if !args.chunks.is_empty() && args.source_group.is_some() {
        return Err("--chunk and --source-group are mutually exclusive".into());
    }
    if args.chunks.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err("--chunk values must be unique and in original source order".into());
    }
    if args.candidate_output_chunk.is_some() && args.candidate_output_source_index.is_some() {
        return Err(
            "choose either --candidate-output-chunk or --candidate-output-source-index".into(),
        );
    }
    let saved_run = ReviewRun::read(&args.run)?;
    // The frozen inventory is retained as an external offline adjudication
    // artifact. It must not influence provider-facing candidate line roles.
    let _offline_inventory = fs::read(&args.source_inventory)?;
    let override_output = match &args.candidate_output {
        Some(path) => Some(serde_json::from_slice(&fs::read(path)?)?),
        None => None,
    };
    let (source, outputs, original_chunk_indices, source_chunk_ids, candidate_original_chunk) =
        if let Some(path) = &args.source_group {
            if args.candidate_output_chunk.is_some() {
                return Err("--candidate-output-chunk cannot target a split source group; use --candidate-output-source-index".into());
            }
            let group: SourceGroup = serde_json::from_slice(&fs::read(path)?)?;
            if group.source_run_sha256 != sha256(&fs::read(&args.run)?) {
                return Err("source group was not derived from this exact saved run".into());
            }
            if group.fragments.is_empty() {
                return Err("source group has no fragments".into());
            }
            for fragment in &group.fragments {
                let original = saved_run
                    .chunks
                    .get(fragment.original_chunk_index)
                    .ok_or("source group original chunk is out of range")?;
                if original.id != fragment.original_chunk_id
                    || original.source.doc_path != fragment.source.doc_path
                    || !original.source.text.contains(&fragment.source.text)
                    || sha256(fragment.source.text.as_bytes()) != fragment.source_sha256
                    || fragment.source.title_hint.as_ref().is_some_and(|title| {
                        !original.source.text.lines().any(|line| line == title)
                    })
                {
                    return Err("source group fragment is not an exact source-derived slice".into());
                }
            }
            let target = match (&override_output, args.candidate_output_source_index) {
                (Some(_), Some(index)) if index < group.fragments.len() => index,
                (Some(_), Some(_)) => {
                    return Err("--candidate-output-source-index is outside source group".into());
                }
                (Some(_), None) if group.fragments.len() == 1 => 0,
                (Some(_), None) => return Err(
                    "--candidate-output-source-index is required for a multi-fragment source group"
                        .into(),
                ),
                (None, Some(_)) => {
                    return Err(
                        "--candidate-output-source-index requires --candidate-output".into(),
                    );
                }
                (None, None) => usize::MAX,
            };
            let original_chunk_indices: Vec<_> = group
                .fragments
                .iter()
                .map(|fragment| fragment.original_chunk_index)
                .collect();
            let source_chunk_ids: Vec<_> = group
                .fragments
                .iter()
                .map(|fragment| fragment.original_chunk_id.clone())
                .collect();
            let source: Vec<_> = group
                .fragments
                .iter()
                .map(|fragment| fragment.source.clone())
                .collect();
            let outputs = group
                .fragments
                .iter()
                .enumerate()
                .map(|(index, fragment)| {
                    if index == target {
                        override_output
                            .clone()
                            .or_else(|| fragment.candidate_output.clone())
                    } else {
                        fragment.candidate_output.clone()
                    }
                    .ok_or("source group fragment has no candidate output")
                })
                .collect::<Result<Vec<_>, _>>()?;
            (
                source,
                outputs,
                original_chunk_indices,
                source_chunk_ids,
                group.fragments[0].original_chunk_index,
            )
        } else {
            let selected: Vec<_> = args
                .chunks
                .iter()
                .map(|&index| saved_run.chunks.get(index).ok_or("chunk is out of range"))
                .collect::<Result<_, _>>()?;
            if args.candidate_output_source_index.is_some() {
                return Err("--candidate-output-source-index requires --source-group".into());
            }
            let target = match (&override_output, args.candidate_output_chunk) {
                (Some(_), Some(index)) if args.chunks.contains(&index) => index,
                (Some(_), Some(_)) => {
                    return Err(
                        "--candidate-output-chunk must be one of the selected --chunk values"
                            .into(),
                    );
                }
                (Some(_), None) if args.chunks.len() == 1 => args.chunks[0],
                (Some(_), None) => {
                    return Err(
                        "--candidate-output-chunk is required with multiple --chunk values".into(),
                    );
                }
                (None, Some(_)) => {
                    return Err("--candidate-output-chunk requires --candidate-output".into());
                }
                (None, None) => args.chunks[0],
            };
            let source: Vec<_> = selected.iter().map(|saved| saved.source.clone()).collect();
            let outputs = selected
                .iter()
                .zip(&args.chunks)
                .map(|(saved, &original_index)| {
                    if original_index == target {
                        override_output.clone().or_else(|| saved.output.clone())
                    } else {
                        saved.output.clone()
                    }
                    .ok_or("selected chunk has no candidate output")
                })
                .collect::<Result<Vec<_>, _>>()?;
            let source_chunk_ids: Vec<_> = selected.iter().map(|saved| saved.id.clone()).collect();
            (
                source,
                outputs,
                args.chunks.clone(),
                source_chunk_ids,
                target,
            )
        };
    let candidate_saved = saved_run
        .chunks
        .get(candidate_original_chunk)
        .ok_or("candidate source chunk is out of range")?;
    let model = args
        .candidate_model
        .clone()
        .or_else(|| candidate_saved.model.clone())
        .unwrap_or(saved_run.model.clone());

    // $20 internal policy capacity gives its 20% verification slice the same
    // $4 ceiling as the external, non-transferable verifier ledger.
    let mut state = State::new(source, &model, 20.0)?;
    state.bind_documents(&saved_run.documents)?;
    let exact_provenance: Option<Vec<_>> = original_chunk_indices
        .iter()
        .zip(&state.source)
        .map(|(original, source)| {
            saved_run.chunks.get(*original).and_then(|saved| {
                (saved.source.text == source.text
                    && saved.source.doc_path == source.doc_path
                    && saved.source.title_hint == source.title_hint
                    && saved.source.links == source.links
                    && saved.source.images == source.images)
                    .then(|| saved_run.source_line_provenance.get(*original).cloned())
                    .flatten()
            })
        })
        .collect();
    if let Some(provenance) =
        exact_provenance.filter(|_| !saved_run.source_line_provenance.is_empty())
    {
        state.bind_source_line_provenance(&provenance)?;
    }
    if state.groups.len() != 1
        || state.groups[0].chunks != (0..state.source.len()).collect::<Vec<_>>()
    {
        return Err("selected chunks do not form one source-derived linked group".into());
    }
    let roles = state
        .source
        .iter()
        .zip(&outputs)
        .map(|(source, candidate)| candidate_only_roles(source, candidate))
        .collect();
    let mut candidate = Candidate::from_outputs(model, outputs);
    candidate.cached_chunks.fill(true);
    candidate.source_roles = roles;
    state.groups[0].candidates.push(candidate);
    state.set_trial_verifiers(args.verifier_mode != "baseline");

    let mapping_path = args.checkpoint.with_extension("ledger-map.json");
    let mapping = Rc::new(RefCell::new(HashMap::<String, String>::new()));
    let settled = Rc::new(RefCell::new(HashSet::new()));
    let ordinals = Rc::new(RefCell::new(HashMap::<String, usize>::new()));
    let admission_stopped = Rc::new(Cell::new(false));
    let source_label = saved_run.source.clone();
    let call_args = args.clone();
    let save_args = args;
    let final_args = save_args.clone();
    // Ledger entry identity includes the durable trial checkpoint. The engine
    // action key is intentionally stable across cold reruns, while the ledger
    // must never merge charges from separate evaluation trials.
    let trial_id = sha256(final_args.checkpoint.to_string_lossy().as_bytes());
    let call_mapping = mapping.clone();
    let call_ordinals = ordinals.clone();
    let call_stopped = admission_stopped.clone();
    let call_trial_id = trial_id.clone();
    let cheap_only = save_args.verifier_mode == "cheap";
    let save_mapping_state = mapping.clone();
    let save_settled = settled.clone();
    let mut adapter = Adapter {
        concurrency: 1,
        allow_network: true,
        refresh: true,
        now: || Some(recipe_epub::review::store::now()),
        cancelled: move || admission_stopped.get(),
        wait: |seconds| async move { tokio::time::sleep(std::time::Duration::from_secs(seconds)).await },
        load: no_cache,
        store: no_cache_write,
        call: move |action: recipe_epub::recovery::Action| {
            let invalid = action.chunk.is_some();
            let reserved = if invalid {
                call_stopped.set(true);
                Err("evaluation harness refuses extraction actions".into())
            } else {
                let ordinal = call_ordinals
                    .borrow_mut()
                    .entry(action.key.clone())
                    .and_modify(|n| *n += 1)
                    .or_insert(1)
                    .to_owned();
                let attempt_key = format!("{}#{ordinal}", action.key);
                let entry = format!("{call_trial_id}:{attempt_key}");
                let reserved = ledger(&call_args, "reserve", &entry, action.reservation_usd);
                if reserved.is_ok() {
                    call_mapping.borrow_mut().insert(attempt_key, entry);
                } else {
                    call_stopped.set(true);
                }
                reserved
            };
            if cheap_only && reserved.is_ok() {
                call_stopped.set(true);
            }
            let source_label = source_label.clone();
            async move {
                if reserved.is_err() {
                    return Reply {
                        error: Some(
                            if invalid {
                                "evaluation harness refuses extraction actions"
                            } else {
                                "evaluation ledger rejected dispatch"
                            }
                            .into(),
                        ),
                        ..Default::default()
                    };
                }
                let backend = match RecoveryBackend::from_env(
                    &Options {
                        model: Some(action.model.clone()),
                        ..Default::default()
                    },
                    &source_label,
                ) {
                    Ok(backend) => backend,
                    Err(error) => {
                        return Reply {
                            error: Some(error.to_string()),
                            ..Default::default()
                        };
                    }
                };
                match backend.recovery_call(&action).await {
                    Ok((payload, usage, reason)) => Reply {
                        payload,
                        usage: (usage != Usage::default()).then_some(usage),
                        raw_usage: backend.recovery_usage(&action.key),
                        error: matches!(reason.as_deref(), Some("length" | "max_tokens"))
                            .then(|| "truncated response".into()),
                        failure: None,
                    },
                    Err(failure) => Reply {
                        payload: None,
                        usage: (failure.usage != Usage::default()).then_some(failure.usage),
                        raw_usage: backend.recovery_usage(&action.key),
                        error: Some(failure.error.to_string()),
                        failure: match failure.error {
                            recipe_epub::EpubError::Request(details) => Some(*details),
                            _ => None,
                        },
                    },
                }
            }
        },
        save: move |state: &State| {
            for (index, attempt) in state
                .attempts
                .iter()
                .enumerate()
                .filter(|(_, attempt)| !attempt.pending)
            {
                let attempt_key = nth_entry(&attempt.key, &state.attempts, index);
                let entry = save_mapping_state.borrow().get(&attempt_key).cloned();
                if let Some(entry) =
                    entry.filter(|entry| save_settled.borrow_mut().insert(entry.clone()))
                {
                    match attempt.estimated_usd {
                        Some(known) => ledger(&save_args, "settle", &entry, known)?,
                        None => ledger(&save_args, "unknown", &entry, attempt.reservation_usd)?,
                    }
                }
            }
            checkpoint(&save_args.checkpoint, state)?;
            save_mapping(
                &mapping_path,
                &original_chunk_indices,
                &source_chunk_ids,
                &save_mapping_state.borrow(),
            )
        },
    };
    run(&mut state, &mut adapter)
        .await
        .map_err(std::io::Error::other)?;
    checkpoint(&final_args.checkpoint, &state).map_err(std::io::Error::other)?;
    println!(
        "{}",
        serde_json::json!({"complete": state.complete(), "attempts": state.attempts.len(), "checkpoint": final_args.checkpoint})
    );
    Ok(())
}
