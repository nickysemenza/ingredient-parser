//! Portable extraction cache format. Storage and credentials belong to callers.
use crate::{ExtractedRecipe, Usage};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CacheIdentity {
    pub provider: String,
    pub model: String,
    pub configuration: String,
    pub contract: String,
    pub prompt_schema: String,
    /// Exact serialized request, including source and title hints.
    pub request: String,
}
impl CacheIdentity {
    pub fn key(&self) -> Result<String, serde_json::Error> {
        Ok(Sha256::digest(serde_json::to_vec(self)?)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheEntry {
    pub version: u32,
    pub identity: CacheIdentity,
    pub outputs: Vec<ExtractedRecipe>,
    pub usage: Option<Usage>,
}
impl CacheEntry {
    pub fn reusable_for(&self, identity: &CacheIdentity) -> bool {
        self.version == 1 && &self.identity == identity
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cubby_legacy_request_and_indexed_request_remain_separate()
    -> Result<(), Box<dyn std::error::Error>> {
        let chunk = crate::Chunk {
            doc_path: "recipe.xhtml".into(),
            text: "Soup\n1 cup water\nBoil.".into(),
            title_hint: Some("Soup".into()),
            images: vec![],
            links: vec![],
        };
        // This is the public request builder used by Cubby's recipebridge.
        let legacy = crate::build_chunk_request(&chunk);
        assert!(legacy.user.starts_with("Section title: Soup"));
        let identity = CacheIdentity {
            provider: "google-ai-studio".into(),
            model: "gemini-2.5-flash".into(),
            configuration: "forced-tool".into(),
            contract: "legacy-recipe-text-v1".into(),
            prompt_schema: legacy.system.clone(),
            request: serde_json::to_string(&legacy)?,
        };
        let wire = serde_json::to_string(&identity)?;
        let decoded: CacheIdentity = serde_json::from_str(&wire)?;
        assert_eq!(identity.key()?, decoded.key()?);
        let mut indexed = identity.clone();
        indexed.contract = "indexed-source-v1".into();
        indexed.request =
            serde_json::to_string(&crate::indexed::build_indexed_chunk_request(&chunk))?;
        assert_ne!(identity.key()?, indexed.key()?);
        let entry = CacheEntry {
            version: 1,
            identity,
            outputs: vec![],
            usage: None,
        };
        let decoded: CacheEntry = serde_json::from_str(&serde_json::to_string(&entry)?)?;
        assert!(decoded.reusable_for(&entry.identity));
        assert!(!decoded.reusable_for(&indexed));
        Ok(())
    }
    #[test]
    fn serialized_identity_preserves_contract_isolation() -> Result<(), Box<dyn std::error::Error>>
    {
        let a = CacheIdentity {
            provider: "google".into(),
            model: "flash".into(),
            configuration: "{}".into(),
            contract: "indexed-source".into(),
            prompt_schema: "v5".into(),
            request: "exact request".into(),
        };
        let copy: CacheIdentity = serde_json::from_slice(&serde_json::to_vec(&a)?)?;
        assert_eq!(a.key()?, copy.key()?);
        let mut b = a.clone();
        b.contract = "legacy".into();
        assert_ne!(a.key()?, b.key()?);
        let entry = CacheEntry {
            version: 1,
            identity: a.clone(),
            outputs: vec![],
            usage: None,
        };
        assert!(entry.reusable_for(&a));
        assert!(!entry.reusable_for(&b));
        Ok(())
    }
}
