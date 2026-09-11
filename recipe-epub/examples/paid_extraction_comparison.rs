//! Frozen Haiku-primary comparison; no retry, fallback, verification, or cache.
//! Preparation is the default. Each dispatch requires durable ledger admission.
use futures::{StreamExt, stream};
use recipe_epub::{
    Chunk, ChunkRequest, ExtractionStats, IndexedChunk, Options, RecoveryBackend, Usage,
    experiment::Ledger,
    recovery::{Action, RequestTelemetry},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    env, fs,
    future::Future,
    io::Write,
    path::{Path, PathBuf},
    time::Instant,
};

const MODEL: &str = "claude-haiku-4-5";
fn hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
fn durable(path: &Path, value: &Value) -> Result<(), String> {
    let temporary = path.with_extension("pending");
    let mut file = fs::File::create(&temporary).map_err(|e| e.to_string())?;
    file.write_all(&serde_json::to_vec(value).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    file.sync_all().map_err(|e| e.to_string())?;
    fs::rename(&temporary, path).map_err(|e| e.to_string())?;
    fs::File::open(path.parent().ok_or("output has no parent")?)
        .and_then(|f| f.sync_all())
        .map_err(|e| e.to_string())
}
fn ledger(
    _tool: &Path,
    path: &Path,
    operation: &str,
    entry: &str,
    amount: f64,
) -> Result<(), String> {
    let ledger = Ledger::open(path);
    match operation {
        "reserve" => ledger.reserve(
            "throughput",
            entry,
            amount,
            "frozen Haiku-primary extraction-contract comparison",
        ),
        "settle" => ledger.settle("throughput", entry, amount),
        "unknown" => ledger.mark_unknown("throughput", entry, amount),
        _ => Err(recipe_epub::experiment::ExperimentError::Rejected(
            "unknown ledger operation".into(),
        )),
    }
    .map_err(|error| error.to_string())
}
fn decode(contract: &str, chunk: &Chunk, payload: Value, truncated: bool) -> Result<Value, String> {
    if truncated {
        return Err("truncated response".into());
    }
    let output = match contract {
        "legacy" => recipe_epub::parse_recipes_payload_for_chunk(chunk, payload),
        "indexed" => recipe_epub::indexed::parse_indexed_recipes(chunk, payload),
        _ => return Err("unknown extraction contract".into()),
    }
    .map_err(|e| e.to_string())?;
    serde_json::to_value(output).map_err(|e| e.to_string())
}

/// Persist raw evidence before settlement or decoding. A failed reservation
/// never invokes transport; a failed write after reserve leaves the charge held.
async fn execute_one<C, CF, L, W>(
    action: &Action,
    entry: &str,
    mut call: C,
    mut account: L,
    mut persist: W,
) -> Result<Value, String>
where
    C: FnMut() -> CF,
    CF: Future<Output = Value>,
    L: FnMut(&str, &str, f64) -> Result<(), String>,
    W: FnMut(&Value) -> Result<(), String>,
{
    let mut evidence = json!({"action_key":action.key,"chunk":action.chunk,"model":action.model,
        "reservation_usd":action.reservation_usd,"ledger_entry":entry,"bucket":"throughput","status":"prepared_for_reservation"});
    persist(&evidence)?;
    if let Err(error) = account("reserve", entry, action.reservation_usd) {
        evidence["status"] = json!("reservation_rejected");
        evidence["error"] = json!(error);
        persist(&evidence)?;
        return Ok(evidence);
    }
    evidence["status"] = json!("reserved");
    persist(&evidence)?;
    evidence["response"] = call().await;
    evidence["status"] = json!("response_received");
    persist(&evidence)?;
    let usage: Option<Usage> = serde_json::from_value(evidence["response"]["usage"].clone()).ok();
    let cost = usage
        .filter(|usage| *usage != Usage::default())
        .and_then(|usage| {
            ExtractionStats {
                model: action.model.clone(),
                usage,
                ..Default::default()
            }
            .cost_usd()
        });
    let settled = if let Some(cost) = cost {
        if account("settle", entry, cost).is_ok() {
            evidence["known_usd"] = json!(cost);
            true
        } else {
            account("unknown", entry, action.reservation_usd)?;
            false
        }
    } else {
        account("unknown", entry, action.reservation_usd)?;
        false
    };
    evidence["status"] = json!(if settled {
        "settled"
    } else {
        "unknown_charge_held"
    });
    persist(&evidence)?;
    Ok(evidence)
}

fn prepare(
    manifest: &Value,
    directory: &Path,
    indexed: &[IndexedChunk],
    contract: &str,
) -> Result<Vec<Action>, String> {
    if manifest["kind"] != "extraction-contract-comparison-prepare-v2"
        || !matches!(contract, "legacy" | "indexed")
    {
        return Err("unsupported preparation contract".into());
    }
    if !recipe_epub::models::catalog()
        .iter()
        .any(|m| m.id == MODEL && m.enabled && m.max_output_tokens >= 16000)
    {
        return Err("primary model is not admitted".into());
    }
    let source: Vec<_> = indexed.iter().map(|entry| &entry.chunk).collect();
    if manifest["source_sha256"] != hash(&serde_json::to_vec(&source).map_err(|e| e.to_string())?) {
        return Err("full source hash changed".into());
    }
    let mut identity_state = recipe_epub::recovery::State::new(
        indexed.iter().map(|entry| entry.chunk.clone()).collect(),
        MODEL,
        10.0,
    )?;
    identity_state.bind_source_line_provenance(
        &indexed
            .iter()
            .map(|entry| entry.lines.clone())
            .collect::<Vec<_>>(),
    )?;
    if manifest["source_line_provenance_sha256"]
        != json!(identity_state.source_line_provenance_sha256)
    {
        return Err("source provenance hash changed".into());
    }
    let entries = manifest["requests"].as_array().ok_or("missing requests")?;
    let selected = |kind: &str| {
        entries
            .iter()
            .filter(|entry| entry["contract"] == kind)
            .map(|entry| entry["chunk"].clone())
            .collect::<Vec<_>>()
    };
    if selected("legacy") != selected("indexed") {
        return Err("comparison arms select different source chunks".into());
    }

    let mut actions = Vec::new();
    for entry in entries.iter().filter(|entry| entry["contract"] == contract) {
        let index = entry["chunk"]
            .as_u64()
            .and_then(|n| usize::try_from(n).ok())
            .ok_or("invalid source index")?;
        let source = indexed.get(index).ok_or("source index outside full book")?;
        let filename = format!("{contract}-{index}.json");
        if entry["file"] != filename {
            return Err("unexpected request filename".into());
        }
        let bytes = fs::read(directory.join(filename)).map_err(|e| e.to_string())?;
        if entry["sha256"] != hash(&bytes) {
            return Err("request hash changed".into());
        }
        let request: ChunkRequest = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
        // Rebuild from authoritative source as well as checking the frozen hash.
        let expected = if contract == "legacy" {
            recipe_epub::build_chunk_request(&source.chunk)
        } else {
            let evidence = recipe_epub::source::chunk_source_evidence(
                &source.chunk,
                Some(&source.lines),
                &[],
            )?;
            recipe_epub::indexed::build_indexed_chunk_request_with_source_evidence(
                &source.chunk,
                &evidence,
            )
            .map_err(|e| e.to_string())?
        };
        if serde_json::to_value(&request).map_err(|e| e.to_string())?
            != serde_json::to_value(&expected).map_err(|e| e.to_string())?
        {
            return Err("request differs from current source contract".into());
        }
        let identity = json!({"kind":"extraction-contract-comparison-action-v1","model":MODEL,"contract":contract,"chunk":index,"source_line_provenance_sha256":manifest["source_line_provenance_sha256"],"output_limit":16000,"request":request}).to_string().into_bytes();
        let usage = Usage {
            input_tokens: identity.len().saturating_mul(2) as u64,
            output_tokens: 16000,
            ..Default::default()
        };
        let reservation = ExtractionStats {
            model: MODEL.into(),
            usage,
            ..Default::default()
        }
        .cost_usd()
        .ok_or("unpriced model")?;
        let planned = entry["proposed_reservations"]
            .as_array()
            .ok_or("missing reservations")?
            .iter()
            .find(|r| r["model"] == MODEL)
            .ok_or("missing primary reservation")?;
        if planned["reservation_usd"].as_f64() != Some(reservation) || planned["enabled"] != true {
            return Err("reservation differs from frozen plan".into());
        }
        let schema_bytes = serde_json::to_vec(&request.tool_schema)
            .map_err(|e| e.to_string())?
            .len();
        actions.push(Action {
            group: actions.len(),
            candidate: 0,
            chunk: Some(index),
            verification_chunk: None,
            verification_stage: None,
            model: MODEL.into(),
            key: hash(&identity),
            reservation_usd: reservation,
            priced: true,
            output_limit: 16000,
            telemetry: RequestTelemetry {
                operation: "extraction".into(),
                model: MODEL.into(),
                contract: format!("comparison-{contract}-v1"),
                output_limit: 16000,
                source_bytes: source.chunk.text.len(),
                context_bytes: request.user.len().saturating_sub(source.chunk.text.len()),
                schema_bytes,
                reservation_usd: reservation,
                ..Default::default()
            },
            request,
        });
    }
    if actions.len() < 8 || actions.windows(2).any(|a| a[0].chunk >= a[1].chunk) {
        return Err("need at least eight unique ordered chunks".into());
    }
    Ok(actions)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = env::args().skip(1).collect();
    if ![6, 8].contains(&args.len()) {
        return Err("usage: paid_extraction_comparison MANIFEST INDEX legacy|indexed NEW_OUTPUT LEDGER EXPECTATIONS [--dispatch LEDGER_TOOL]".into());
    }
    let manifest_path = PathBuf::from(&args[0]);
    let manifest: Value = serde_json::from_slice(&fs::read(&manifest_path)?)?;
    let indexed: Vec<IndexedChunk> = serde_json::from_slice(&fs::read(&args[1])?)?;
    let contract = &args[2];
    let output = PathBuf::from(&args[3]);
    let ledger_path = PathBuf::from(&args[4]);
    let expectations = fs::read(&args[5])?;
    let _: Value = serde_json::from_slice(&expectations)?;
    let dispatch = args.len() == 8 && args[6] == "--dispatch";
    if args.len() != 6 && !dispatch {
        return Err("invalid dispatch arguments".into());
    }
    let actions = prepare(
        &manifest,
        manifest_path.parent().ok_or("manifest parent missing")?,
        &indexed,
        contract,
    )?;
    fs::create_dir(&output)?;
    let trial_id = hash(output.to_string_lossy().as_bytes());
    let summary = json!({"kind":"haiku-primary-extraction-comparison-v1","contract":contract,"manifest_sha256":hash(&fs::read(&manifest_path)?),"expectations_sha256":hash(&expectations),"dispatch_requested":dispatch,"concurrency":4,"retry":false,"fallback":false,"local_cache":false,"provider_cache":"unchanged transport defaults; inspect reported cache usage","verification":false,"actions":actions.iter().map(|a| json!({"key":a.key,"chunk":a.chunk,"reservation_usd":a.reservation_usd})).collect::<Vec<_>>()});
    durable(&output.join("preparation.json"), &summary)?;
    if !dispatch {
        println!("{summary}");
        return Ok(());
    }
    let backend = RecoveryBackend::from_env(
        &Options {
            model: Some(MODEL.into()),
            use_cache: false,
            concurrency: 4,
            ..Default::default()
        },
        "haiku-primary-extraction-comparison",
    )?;
    let ledger_tool = PathBuf::from(&args[7]);
    let started = Instant::now();
    let outcomes = stream::iter(actions.iter().map(|action| {
        let backend = &backend; let output = &output; let ledger_tool = &ledger_tool; let ledger_path = &ledger_path; let trial_id = &trial_id; let indexed = &indexed;
        async move {
            let index = action.chunk.ok_or("extraction chunk missing")?;
            let path = output.join(format!("chunk-{index}.json"));
            let entry = format!("{trial_id}:{}", action.key);
            let mut evidence = execute_one(action, &entry, || async {
                let began = Instant::now();
                let response = backend.recovery_call(action).await;
                let raw_usage = backend.recovery_usage(&action.key);
                match response {
                    Ok((payload, usage, finish)) => json!({"payload":payload,"usage":usage,"raw_usage":raw_usage,"finish_reason":finish,"provider_ms":began.elapsed().as_millis()}),
                    Err(failure) => json!({"payload":null,"usage":failure.usage,"raw_usage":raw_usage,"error":"provider request failed","truncated":failure.truncated,"provider_ms":began.elapsed().as_millis()}),
                }
            }, |operation, id, amount| ledger(ledger_tool, ledger_path, operation, id, amount), |value| durable(&path, value)).await?;
            if !evidence["response"]["payload"].is_null() {
                let truncated = evidence["response"]["truncated"] == true || matches!(evidence["response"]["finish_reason"].as_str(), Some("length" | "max_tokens"));
                match decode(contract, &indexed[index].chunk, evidence["response"]["payload"].clone(), truncated) {
                    Ok(recipes) => { evidence["decoded"] = recipes; evidence["decode_status"] = json!("complete_unverified"); },
                    Err(error) => { evidence["decode_status"] = json!("rejected"); evidence["decode_error"] = json!(error); },
                }
                durable(&path, &evidence)?;
            }
            Ok::<_, String>(json!({"chunk":index,"status":evidence["status"],"decode_status":evidence["decode_status"],"known_usd":evidence["known_usd"]}))
        }
    })).buffer_unordered(4).collect::<Vec<_>>().await;
    let result = json!({"contract":contract,"wall_ms":started.elapsed().as_millis(),"outcomes":outcomes.iter().map(|r| match r { Ok(v)=>v.clone(),Err(e)=>json!({"error":e}) }).collect::<Vec<_>>()});
    durable(&output.join("result.json"), &result)?;
    println!("{result}");
    if outcomes.iter().any(Result::is_err) {
        return Err(
            "comparison has interrupted or failed accounting; inspect private evidence".into(),
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};
    fn action() -> Action {
        Action {
            group: 0,
            candidate: 0,
            chunk: Some(0),
            verification_chunk: None,
            verification_stage: None,
            model: MODEL.into(),
            request: ChunkRequest {
                system: String::new(),
                user: String::new(),
                tool_name: String::new(),
                tool_schema: json!({}),
            },
            key: "fixture".into(),
            reservation_usd: 1.0,
            priced: true,
            output_limit: 16000,
            telemetry: RequestTelemetry::default(),
        }
    }
    #[tokio::test]
    async fn response_is_durable_before_settlement() {
        let events = RefCell::new(Vec::new());
        let result = execute_one(&action(), "entry", || async {
            events.borrow_mut().push("call".to_string());
            json!({"payload":{"recipes":[]},"usage":Usage {input_tokens:10,output_tokens:10,..Default::default()}})
        }, |operation, _, _| { events.borrow_mut().push(operation.to_string()); Ok(()) }, |value| {events.borrow_mut().push(value["status"].as_str().unwrap().to_string());Ok(())}).await.unwrap();
        assert_eq!(
            *events.borrow(),
            [
                "prepared_for_reservation",
                "reserve",
                "reserved",
                "call",
                "response_received",
                "settle",
                "settled"
            ]
        );
        assert_eq!(result["response"]["payload"], json!({"recipes":[]}));
    }
    #[tokio::test]
    async fn rejected_admission_never_calls_transport() {
        let calls = Cell::new(0);
        let result = execute_one(
            &action(),
            "entry",
            || async {
                calls.set(calls.get() + 1);
                json!({})
            },
            |operation, _, _| {
                assert_eq!(operation, "reserve");
                Err("cap exceeded".into())
            },
            |_| Ok(()),
        )
        .await
        .unwrap();
        assert_eq!(calls.get(), 0);
        assert_eq!(result["status"], "reservation_rejected");
    }
    #[tokio::test]
    async fn interrupted_reserved_checkpoint_keeps_reservation_without_dispatch() {
        let operations = RefCell::new(Vec::new());
        let calls = Cell::new(0);
        let result = execute_one(
            &action(),
            "entry",
            || async {
                calls.set(calls.get() + 1);
                json!({})
            },
            |operation, _, _| {
                operations.borrow_mut().push(operation.to_string());
                Ok(())
            },
            |value| {
                if value["status"] == "reserved" {
                    Err("disk failure".into())
                } else {
                    Ok(())
                }
            },
        )
        .await;
        assert!(result.is_err());
        assert_eq!(calls.get(), 0);
        assert_eq!(*operations.borrow(), ["reserve"]);
    }
    #[rstest::rstest]
    #[case(false)]
    #[case(true)]
    #[tokio::test]
    async fn unknown_usage_or_failed_settlement_keeps_full_reservation(#[case] known_usage: bool) {
        let operations = RefCell::new(Vec::new());
        let result = execute_one(&action(), "entry", || async {json!({"payload":null,"usage":if known_usage {json!(Usage {input_tokens:10,..Default::default()})} else {Value::Null}})}, |operation,_,amount| {
            operations.borrow_mut().push(operation.to_string());
            if operation=="settle" {Err("settlement failed".into())} else {assert_eq!(amount,1.0);Ok(())}
        }, |_|Ok(())).await.unwrap();
        assert_eq!(result["status"], "unknown_charge_held");
        assert_eq!(operations.borrow().last().unwrap(), "unknown");
    }
    fn chunk() -> Chunk {
        Chunk {
            text: "Soup\n1 cup water\nCook gently.".into(),
            doc_path: "chapter.xhtml".into(),
            title_hint: None,
            links: vec![],
            images: vec![],
        }
    }
    #[test]
    fn decoders_preserve_valid_output_and_reject_invention_missing_lines_and_truncation() {
        let legacy = json!({"recipes":[{"title":"Soup","sections":[{"ingredients":["1 cup water"],"instructions":["Cook gently."]}]}]});
        let indexed = json!({"recipes":[{"title":[0],"description":[],"notes":[],"equipment":[],"sections":[{"name":[],"ingredients":[1],"instructions":[2]}]}],"ignored":[]});
        let a = decode("legacy", &chunk(), legacy.clone(), false).unwrap();
        let b = decode("indexed", &chunk(), indexed.clone(), false).unwrap();
        assert_eq!(a[0]["title"], b[0]["title"]);
        assert_eq!(a[0]["sections"], b[0]["sections"]);
        let mut invented = legacy.clone();
        invented["recipes"][0]["title"] = json!("Invented title");
        assert!(decode("legacy", &chunk(), invented, false).is_err());
        let mut missing = indexed.clone();
        missing["recipes"][0]["sections"][0]["instructions"] = json!([]);
        assert!(decode("indexed", &chunk(), missing, false).is_err());
        for (contract, payload) in [("legacy", legacy), ("indexed", indexed)] {
            assert!(decode(contract, &chunk(), payload, true).is_err());
        }
    }
}
