//! Native recovery-cache envelope. The action key already hashes the exact
//! request/candidate identity; explicit metadata makes accidental raw or stale
//! cache reuse a miss rather than an un-auditable response.
use crate::recovery::{Action, VERIFICATION_CONTRACT};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;

const VERSION: u8 = 1;

#[derive(Debug, Serialize, Deserialize)]
struct Envelope {
    version: u8,
    key: String,
    contract: String,
    model: String,
    output_limit: u32,
    payload: Value,
}

pub(super) fn read(directory: &Path, action: &Action) -> Option<Value> {
    let bytes = std::fs::read(directory.join(format!("{}.json", action.key))).ok()?;
    let envelope: Envelope = serde_json::from_slice(&bytes).ok()?;
    (envelope.version == VERSION
        && envelope.key == action.key
        && envelope.contract == VERIFICATION_CONTRACT
        && envelope.model == action.model
        && envelope.output_limit == action.output_limit)
        .then_some(envelope.payload)
}

pub(super) fn write(directory: &Path, action: &Action, payload: &Value) -> Result<(), String> {
    let target = directory.join(format!("{}.json", action.key));
    let temp = directory.join(format!("{}.tmp", super::store::new_id()));
    let envelope = Envelope {
        version: VERSION,
        key: action.key.clone(),
        contract: VERIFICATION_CONTRACT.into(),
        model: action.model.clone(),
        output_limit: action.output_limit,
        payload: payload.clone(),
    };
    let result = (|| {
        std::fs::write(
            &temp,
            serde_json::to_vec(&envelope).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        std::fs::rename(&temp, &target).map_err(|e| e.to_string())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(temp);
    }
    result
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::{Chunk, recovery::State};

    #[test]
    fn raw_or_mismatched_metadata_cache_is_a_miss() {
        let state = State::new(
            vec![Chunk {
                text: "Copyright".into(),
                doc_path: "source.xhtml".into(),
                title_hint: None,
                links: vec![],
                images: vec![],
            }],
            "gemini-2.5-flash",
            1.0,
        )
        .unwrap();
        let action = state.planned_actions().unwrap().pop().unwrap();
        let directory = std::env::temp_dir().join(format!(
            "recovery-envelope-{}",
            super::super::store::new_id()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join(format!("{}.json", action.key));
        std::fs::write(&path, br#"{"recipes":[]}"#).unwrap();
        assert!(read(&directory, &action).is_none());
        write(&directory, &action, &serde_json::json!({"recipes":[]})).unwrap();
        assert!(read(&directory, &action).is_some());
        let mut envelope: Envelope =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        envelope.output_limit += 1;
        std::fs::write(&path, serde_json::to_vec(&envelope).unwrap()).unwrap();
        assert!(read(&directory, &action).is_none());
        let _ = std::fs::remove_dir_all(directory);
    }
}
