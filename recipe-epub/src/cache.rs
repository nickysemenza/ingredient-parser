//! On-disk cache of raw extractor results, keyed by a content hash of
//! (prompt version, model, chunk text). Makes re-running over a large library
//! incremental and free after the first pass.
#![cfg(feature = "native")]

use std::path::{Path, PathBuf};

use crate::{EpubError, ExtractedRecipe};

/// Bump when the system prompt or tool schema changes — old entries then miss
/// and are re-extracted rather than returning stale-shaped data.
pub(crate) const PROMPT_VERSION: &str = "2026-09-09-indexed-source-v11";

/// Per-user cache directory; independent of the durable run store.
pub(crate) fn default_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("XDG_CACHE_HOME") {
        return PathBuf::from(dir).join("recipe-epub");
    }
    if let Some(home) = std::env::var_os("HOME") {
        let base = PathBuf::from(home).join(if cfg!(target_os = "macos") {
            "Library/Caches"
        } else {
            ".cache"
        });
        return base.join("ingredient-parser/recipe-epub");
    }
    std::env::temp_dir().join("recipe-epub")
}

/// Fingerprint the actual prompt and schema, independently of source content.
pub(crate) fn prompt_fingerprint() -> String {
    let request = crate::indexed::build_indexed_chunk_request(&crate::Chunk {
        doc_path: String::new(),
        text: String::new(),
        title_hint: None,
        images: vec![],
        links: vec![],
    });
    crate::review::hash(
        format!(
            "{}:{}:{}",
            PROMPT_VERSION, request.system, request.tool_schema
        )
        .as_bytes(),
    )
}
/// Full portable identity for the source-indexed native extractor.
pub(crate) fn identity(
    model: &str,
    chunk: &crate::Chunk,
) -> Result<crate::cache_contract::CacheIdentity, EpubError> {
    Ok(crate::cache_contract::CacheIdentity {
        provider: crate::models::provider(model).unwrap_or("custom").into(),
        model: model.into(),
        configuration: format!(
            "transport={};max_output_tokens=16000;forced-tool;retry-v1",
            crate::models::catalog()
                .iter()
                .find(|m| m.id == model)
                .map(|m| m.transport)
                .unwrap_or("legacy-native")
        ),
        contract: "indexed-source-v1".into(),
        prompt_schema: prompt_fingerprint(),
        request: serde_json::to_string(&crate::indexed::build_indexed_chunk_request(chunk))?,
    })
}
pub(crate) fn read_entry(
    dir: &Path,
    identity: &crate::cache_contract::CacheIdentity,
) -> Option<Vec<ExtractedRecipe>> {
    let bytes = std::fs::read(dir.join(format!("{}.entry.json", identity.key().ok()?))).ok()?;
    let entry: crate::cache_contract::CacheEntry = serde_json::from_slice(&bytes).ok()?;
    entry.reusable_for(identity).then_some(entry.outputs)
}
pub(crate) fn write_entry(
    dir: &Path,
    identity: &crate::cache_contract::CacheIdentity,
    recipes: &[ExtractedRecipe],
    usage: Option<crate::Usage>,
) -> Result<(), EpubError> {
    std::fs::create_dir_all(dir).map_err(|e| EpubError::Cache(e.to_string()))?;
    let target = dir.join(format!("{}.entry.json", identity.key()?));
    let entry = crate::cache_contract::CacheEntry {
        version: 1,
        identity: identity.clone(),
        outputs: recipes.to_vec(),
        usage,
    };
    let temporary = dir.join(format!("{}.tmp", crate::review::store::new_id()));
    std::fs::write(&temporary, serde_json::to_vec(&entry)?)
        .map_err(|e| EpubError::Cache(e.to_string()))?;
    let result = std::fs::rename(&temporary, target).map_err(|e| EpubError::Cache(e.to_string()));
    if result.is_err() {
        let _ = std::fs::remove_file(temporary);
    }
    result
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::{RecipeMeta, RecipeSection};

    fn request(
        model: &str,
        text: &str,
        hint: Option<&str>,
    ) -> crate::cache_contract::CacheIdentity {
        identity(
            model,
            &crate::Chunk {
                doc_path: "test.xhtml".into(),
                text: text.into(),
                title_hint: hint.map(str::to_owned),
                images: vec![],
                links: vec![],
            },
        )
        .unwrap()
    }
    #[test]
    fn exact_native_requests_invalidate_by_model_source_and_hint() {
        let a = request("gemini-2.5-flash", "abc", None);
        assert_eq!(
            a.key().unwrap(),
            request("gemini-2.5-flash", "abc", None).key().unwrap()
        );
        for b in [
            request("gemini-2.5-flash", "abd", None),
            request("claude-haiku-4-5", "abc", None),
            request("gemini-2.5-flash", "abc", Some("Hint")),
        ] {
            assert_ne!(a.key().unwrap(), b.key().unwrap());
        }
    }

    #[test]
    fn round_trips() {
        let dir = std::env::temp_dir().join(format!(
            "recipe-epub-cache-test-{}",
            crate::review::store::new_id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let recipes = vec![ExtractedRecipe {
            meta: RecipeMeta {
                title: "Pancakes".to_string(),
                ..Default::default()
            },
            sections: vec![RecipeSection {
                name: None,
                ingredients: vec!["1 cup flour".to_string()],
                instructions: vec![],
            }],
        }];
        let k = request("gemini-2.5-flash", "chunk text", None);
        assert!(read_entry(&dir, &k).is_none());
        write_entry(&dir, &k, &recipes, None).unwrap();
        assert_eq!(read_entry(&dir, &k).unwrap(), recipes);
        let path = dir.join(format!("{}.entry.json", k.key().unwrap()));
        let mut entry: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        entry["version"] = serde_json::json!(999);
        std::fs::write(&path, serde_json::to_vec(&entry).unwrap()).unwrap();
        assert!(read_entry(&dir, &k).is_none());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
