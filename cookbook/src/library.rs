//! Finding EPUBs in a library directory.

/// Every `.epub` under `dir`, sorted by path. Hidden directories are skipped.
#[cfg(feature = "native")]
pub fn find_epubs(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut out: Vec<std::path::PathBuf> = walkdir::WalkDir::new(dir)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| e.depth() == 0 || !e.file_name().to_string_lossy().starts_with('.'))
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .filter(|e| {
            e.path()
                .extension()
                .is_some_and(|x| x.eq_ignore_ascii_case("epub"))
        })
        .map(|e| e.into_path())
        .collect();
    out.sort();
    out
}

/// sha256 per library file, remembered by size and modification time so a
/// rescan reads only files that changed. Persisted as JSON.
#[cfg(feature = "native")]
#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct ShaCache {
    entries: std::collections::BTreeMap<String, ShaEntry>,
    #[serde(skip)]
    dirty: bool,
}

#[cfg(feature = "native")]
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct ShaEntry {
    len: u64,
    modified_ms: u128,
    sha256: String,
}

#[cfg(feature = "native")]
impl ShaCache {
    /// `<data dir>/ingredient-parser/cookbook/library/sha-cache.json`.
    pub fn default_path() -> Option<std::path::PathBuf> {
        directories::BaseDirs::new().map(|b| {
            b.data_dir()
                .join("ingredient-parser")
                .join("cookbook")
                .join("library")
                .join("sha-cache.json")
        })
    }

    /// The cache at `path`, empty when it is missing or unreadable.
    pub fn load(path: Option<&std::path::Path>) -> ShaCache {
        path.and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    /// The cache at its default location.
    pub fn open_default() -> ShaCache {
        Self::load(Self::default_path().as_deref())
    }

    /// Write the cache to `path` if anything was hashed since it was loaded.
    pub fn save(&self, path: Option<&std::path::Path>) -> std::io::Result<()> {
        let Some(path) = path else {
            return Ok(());
        };
        if !self.dirty {
            return Ok(());
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(
            path,
            serde_json::to_string(self).map_err(std::io::Error::other)?,
        )
    }

    pub fn save_default(&self) -> std::io::Result<()> {
        self.save(Self::default_path().as_deref())
    }

    /// The file's sha256, hashing it only when its size or mtime changed.
    pub fn sha_for(&mut self, path: &std::path::Path) -> std::io::Result<String> {
        let meta = std::fs::metadata(path)?;
        let modified_ms = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_millis())
            .unwrap_or(0);
        let key = path.to_string_lossy().into_owned();
        if let Some(entry) = self.entries.get(&key)
            && entry.len == meta.len()
            && entry.modified_ms == modified_ms
        {
            return Ok(entry.sha256.clone());
        }
        let sha256 = crate::epub::open::sha256_hex(&std::fs::read(path)?);
        self.entries.insert(
            key,
            ShaEntry {
                len: meta.len(),
                modified_ms,
                sha256: sha256.clone(),
            },
        );
        self.dirty = true;
        Ok(sha256)
    }
}

/// One distinct EPUB in a library.
#[cfg(feature = "native")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScannedBook {
    pub path: std::path::PathBuf,
    pub sha256: String,
}

/// A library directory hashed and deduplicated.
#[cfg(feature = "native")]
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct LibraryScan {
    /// Distinct books, sorted by path; the first path of each sha wins.
    pub books: Vec<ScannedBook>,
    /// `(duplicate, kept)` pairs: byte-identical copies of a book kept above.
    pub duplicates: Vec<(std::path::PathBuf, std::path::PathBuf)>,
    /// Files that could not be read, with the error.
    pub unreadable: Vec<(std::path::PathBuf, String)>,
}

/// Find, hash and deduplicate every EPUB under `dir`.
#[cfg(feature = "native")]
pub fn scan(dir: &std::path::Path, shas: &mut ShaCache) -> LibraryScan {
    let mut out = LibraryScan::default();
    let mut seen: std::collections::HashMap<String, std::path::PathBuf> =
        std::collections::HashMap::new();
    for path in find_epubs(dir) {
        match shas.sha_for(&path) {
            Ok(sha256) => {
                if let Some(kept) = seen.get(&sha256) {
                    out.duplicates.push((path, kept.clone()));
                } else {
                    seen.insert(sha256.clone(), path.clone());
                    out.books.push(ScannedBook { path, sha256 });
                }
            }
            Err(e) => out.unreadable.push((path, e.to_string())),
        }
    }
    out
}

#[cfg(all(test, feature = "native"))]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn finds_epubs_recursively_and_sorted() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("b/Book")).unwrap();
        std::fs::create_dir_all(dir.path().join(".hidden")).unwrap();
        std::fs::write(dir.path().join("b/Book/z.EPUB"), b"").unwrap();
        std::fs::write(dir.path().join("a.epub"), b"").unwrap();
        std::fs::write(dir.path().join("a.pdf"), b"").unwrap();
        std::fs::write(dir.path().join(".hidden/x.epub"), b"").unwrap();
        let found = find_epubs(dir.path());
        assert_eq!(
            found,
            [dir.path().join("a.epub"), dir.path().join("b/Book/z.EPUB")]
        );
    }

    /// Byte-identical copies collapse to the first path; the sha cache
    /// rehashes only when a file changes.
    #[test]
    fn scan_deduplicates_by_sha_and_remembers_hashes() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("copy")).unwrap();
        std::fs::write(dir.path().join("a.epub"), b"same bytes").unwrap();
        std::fs::write(dir.path().join("copy/a.epub"), b"same bytes").unwrap();
        std::fs::write(dir.path().join("b.epub"), b"other bytes").unwrap();
        let cache_path = dir.path().join("sha-cache.json");
        let mut shas = ShaCache::load(Some(&cache_path));
        let scan1 = scan(dir.path(), &mut shas);
        assert_eq!(scan1.books.len(), 2);
        assert_eq!(
            scan1.duplicates,
            vec![(dir.path().join("copy/a.epub"), dir.path().join("a.epub"))]
        );
        assert!(scan1.unreadable.is_empty());
        shas.save(Some(&cache_path)).unwrap();
        let mut reloaded = ShaCache::load(Some(&cache_path));
        assert_eq!(reloaded.entries.len(), 3);
        let scan2 = scan(dir.path(), &mut reloaded);
        assert_eq!(scan1, scan2);
        assert!(!reloaded.dirty, "nothing changed, nothing rehashed");
        std::fs::write(dir.path().join("b.epub"), b"other bytes, longer").unwrap();
        let scan3 = scan(dir.path(), &mut reloaded);
        assert!(reloaded.dirty);
        assert_ne!(scan3.books[1].sha256, scan2.books[1].sha256);
    }
}
