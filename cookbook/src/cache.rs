//! The chunk-level response cache: a model's raw answer to one exact request.
//!
//! The key covers the contract version, the model, the route, and the full
//! request body (prompt, schema, chunk text, any feedback), so a retry with
//! feedback and a second opinion by another model are distinct entries. The
//! value is the raw provider response: newer lowering or validation replays it
//! without a new charge. Non-2xx and truncated answers are never stored.
//!
//! Storage belongs to the host: native code keeps files, the browser keeps
//! nothing.

use std::collections::HashMap;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::cost::Usage;
use crate::transport::HttpResponse;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CachedCall {
    pub key: String,
    pub model: String,
    pub contract: String,
    pub response: HttpResponse,
    pub usage: Usage,
    /// RFC 3339.
    pub recorded_at: String,
}

/// Hex SHA-256 over the parts that determine an answer.
pub fn cache_key(contract: &str, model: &str, route: &str, body: &serde_json::Value) -> String {
    let mut hasher = Sha256::new();
    for part in [contract, model, route] {
        hasher.update(part.as_bytes());
        hasher.update(b"\0");
    }
    hasher.update(body.to_string().as_bytes());
    hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

pub trait ChunkCache {
    fn get(&self, key: &str) -> Option<CachedCall>;
    fn put(&self, key: &str, call: &CachedCall);
}

/// Remembers nothing.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoCache;

impl ChunkCache for NoCache {
    fn get(&self, _key: &str) -> Option<CachedCall> {
        None
    }

    fn put(&self, _key: &str, _call: &CachedCall) {}
}

/// In-process cache for tests and short-lived hosts.
#[derive(Debug, Default)]
pub struct MemoryCache(Mutex<HashMap<String, CachedCall>>);

impl MemoryCache {
    pub fn len(&self) -> usize {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl ChunkCache for MemoryCache {
    fn get(&self, key: &str) -> Option<CachedCall> {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(key)
            .cloned()
    }

    fn put(&self, key: &str, call: &CachedCall) {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(key.to_string(), call.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn key_covers_contract_model_route_and_body() {
        let base = cache_key("v1", "m", "r", &json!({"a": 1}));
        assert_eq!(base.len(), 64);
        assert_eq!(base, cache_key("v1", "m", "r", &json!({"a": 1})));
        assert_ne!(base, cache_key("v2", "m", "r", &json!({"a": 1})));
        assert_ne!(base, cache_key("v1", "n", "r", &json!({"a": 1})));
        assert_ne!(base, cache_key("v1", "m", "s", &json!({"a": 1})));
        assert_ne!(base, cache_key("v1", "m", "r", &json!({"a": 2})));
    }

    #[test]
    fn memory_cache_round_trips() {
        let cache = MemoryCache::default();
        let call = CachedCall {
            key: "k".into(),
            model: "m".into(),
            contract: "v1".into(),
            response: HttpResponse {
                status: 200,
                headers: vec![],
                body: "{}".into(),
            },
            usage: Usage::default(),
            recorded_at: "2026-09-10T00:00:00Z".into(),
        };
        assert!(cache.get("k").is_none());
        cache.put("k", &call);
        assert_eq!(cache.get("k"), Some(call));
        assert_eq!(cache.len(), 1);
        assert!(NoCache.get("k").is_none());
    }
}
