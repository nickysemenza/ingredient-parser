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
}
