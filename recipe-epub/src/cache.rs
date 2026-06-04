//! On-disk cache of raw extractor results, keyed by a content hash of
//! (prompt version, model, chunk text). Makes re-running over a large library
//! incremental and free after the first pass.
#![cfg(feature = "native")]

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::{EpubError, ExtractedRecipe};

/// Bump when the system prompt or tool schema changes — old entries then miss
/// and are re-extracted rather than returning stale-shaped data.
pub(crate) const PROMPT_VERSION: &str = "2026-05-31-notes";

/// Default cache directory: `$XDG_CACHE_HOME/recipe-epub` or `$TMPDIR/recipe-epub`.
pub(crate) fn default_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("XDG_CACHE_HOME") {
        return PathBuf::from(dir).join("recipe-epub");
    }
    std::env::temp_dir().join("recipe-epub")
}

/// Stable hex cache key for a chunk under a given model + prompt version.
///
/// `title_hint` is included because it varies the prompt (continuation chunks
/// re-emit a spilled recipe), so chunks with identical text but different hints
/// must not share a cache entry.
pub(crate) fn key(model: &str, chunk_text: &str, title_hint: &str) -> String {
    let mut h = Sha256::new();
    h.update(PROMPT_VERSION.as_bytes());
    h.update([0]);
    h.update(model.as_bytes());
    h.update([0]);
    h.update(chunk_text.as_bytes());
    h.update([0]);
    h.update(title_hint.as_bytes());
    format!("{:x}", h.finalize())
}

/// Read a cached result, or `None` on miss / unreadable / stale-shaped entry.
pub(crate) fn read(dir: &Path, key: &str) -> Option<Vec<ExtractedRecipe>> {
    let bytes = std::fs::read(dir.join(format!("{key}.json"))).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// Write a result to the cache (creating the directory if needed).
pub(crate) fn write(dir: &Path, key: &str, recipes: &[ExtractedRecipe]) -> Result<(), EpubError> {
    std::fs::create_dir_all(dir).map_err(|e| EpubError::Cache(e.to_string()))?;
    let json = serde_json::to_vec(recipes)?;
    std::fs::write(dir.join(format!("{key}.json")), json)
        .map_err(|e| EpubError::Cache(e.to_string()))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::{RecipeMeta, RecipeSection};

    #[test]
    fn key_is_stable_and_sensitive() {
        assert_eq!(key("haiku", "abc", ""), key("haiku", "abc", ""));
        assert_ne!(key("haiku", "abc", ""), key("haiku", "abd", ""));
        assert_ne!(key("haiku", "abc", ""), key("sonnet", "abc", ""));
        assert_ne!(key("haiku", "abc", ""), key("haiku", "abc", "Hint"));
    }

    #[test]
    fn round_trips() {
        let dir = std::env::temp_dir().join("recipe-epub-cache-test");
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
        let k = key("m", "chunk text", "");
        assert!(read(&dir, &k).is_none());
        write(&dir, &k, &recipes).unwrap();
        assert_eq!(read(&dir, &k).unwrap(), recipes);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
