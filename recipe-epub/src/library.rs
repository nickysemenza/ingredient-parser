//! EPUB library scanning: find epubs under a directory, read book-level
//! metadata (title / authors / subject tags) cheaply, and guess whether a book
//! is a cookbook from its tags. The ambiguous (untagged) books can optionally be
//! settled by the AI fallback [`classify_cookbooks_ai`].

use std::path::{Path, PathBuf};

use epub::doc::EpubDoc;
use serde::{Deserialize, Serialize};

use crate::extractor::Backend;
use crate::{EpubError, Options};

/// Recursively collect `.epub` files under `dir` (depth-first; unreadable
/// directories are silently skipped). Shared by the CLI's `scan-cookbooks` and
/// the app's library browser.
pub fn find_epubs(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else {
            continue;
        };
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                stack.push(p);
            } else if p
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("epub"))
            {
                out.push(p);
            }
        }
    }
    out
}

/// Book-level metadata read from an EPUB's OPF — enough to list a library and
/// guess whether a book is a cookbook, without extracting any recipes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BookMeta {
    pub path: PathBuf,
    pub title: String,
    pub authors: Vec<String>,
    /// `<dc:subject>` tags (Calibre genres/tags). Often empty.
    pub subjects: Vec<String>,
}

/// Read a book's title, authors, and subject tags from its OPF.
///
/// Uses [`EpubDoc::new`] (file-backed and lazy): it parses the container + OPF
/// but does not decompress the book's content documents, so scanning a large
/// library stays cheap. The title falls back to the file stem when the OPF has
/// none.
pub fn book_metadata(path: &Path) -> Result<BookMeta, EpubError> {
    let doc = EpubDoc::new(path).map_err(|e| EpubError::Open(e.to_string()))?;
    let collect = |prop: &str| -> Vec<String> {
        doc.metadata
            .iter()
            .filter(|m| m.property == prop)
            .map(|m| m.value.trim().to_string())
            .filter(|v| !v.is_empty())
            .collect()
    };
    let title = doc
        .get_title()
        .filter(|t| !t.trim().is_empty())
        .unwrap_or_else(|| {
            path.file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default()
        });
    Ok(BookMeta {
        path: path.to_path_buf(),
        title,
        authors: collect("creator"),
        subjects: collect("subject"),
    })
}

/// Whether a book looks like a cookbook, judged only from its tags/title.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CookbookGuess {
    /// A subject tag (or the title) matched a cookbook keyword.
    Yes,
    /// Has subject tags, but none matched — probably not a cookbook.
    No,
    /// No subject tags to judge from — a candidate for the AI fallback.
    Unknown,
}

/// Lowercased substrings that mark a subject tag or title as cookbook-ish.
/// Substring match so "Cooking", "Cookbooks", "Quick & Easy Cooking" all hit.
pub(crate) const COOKBOOK_KEYWORDS: &[&str] = &[
    "cook",
    "recipe",
    "baking",
    "food",
    "cuisine",
    "kitchen",
    "gastronom",
    "culinary",
    "dessert",
    "pastry",
    "beverage",
    "cocktail",
    "vegetarian",
    "vegan",
];

/// Guess whether `meta` is a cookbook from its subject tags (primary signal) and
/// title (weak secondary). See [`CookbookGuess`].
pub fn classify_by_tags(meta: &BookMeta) -> CookbookGuess {
    let hits = |s: &str| {
        let s = s.to_lowercase();
        COOKBOOK_KEYWORDS.iter().any(|kw| s.contains(kw))
    };
    if meta.subjects.iter().any(|s| hits(s)) || hits(&meta.title) {
        CookbookGuess::Yes
    } else if meta.subjects.is_empty() {
        CookbookGuess::Unknown
    } else {
        CookbookGuess::No
    }
}

/// AI fallback: ask the model which of `books` are cookbooks, returning one bool
/// per input book (same order). Intended for the `Unknown` (untagged) subset the
/// tag heuristic can't settle. One batched API call; uses the same backend/auth
/// as extraction (selected from `opts.model`). Returns all-`false` for an empty
/// input without making a call.
pub async fn classify_cookbooks_ai(
    books: &[BookMeta],
    opts: &Options,
) -> Result<Vec<bool>, EpubError> {
    if books.is_empty() {
        return Ok(Vec::new());
    }
    let backend = Backend::from_env(opts)?;
    backend.classify_cookbooks(books).await
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn meta(title: &str, subjects: &[&str]) -> BookMeta {
        BookMeta {
            path: PathBuf::from("/x.epub"),
            title: title.to_string(),
            authors: vec![],
            subjects: subjects.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn tags_classify_cookbooks() {
        assert_eq!(
            classify_by_tags(&meta("Whatever", &["Cooking"])),
            CookbookGuess::Yes
        );
        assert_eq!(
            classify_by_tags(&meta("Whatever", &["Cookbooks", "Italian"])),
            CookbookGuess::Yes
        );
        // Title is a weak secondary signal even with no useful subjects.
        assert_eq!(
            classify_by_tags(&meta("The Food Lab", &["Reference"])),
            CookbookGuess::Yes
        );
    }

    #[test]
    fn tags_classify_non_cookbooks() {
        assert_eq!(
            classify_by_tags(&meta("A Novel", &["Fiction", "Thriller"])),
            CookbookGuess::No
        );
    }

    #[test]
    fn untagged_is_unknown() {
        assert_eq!(
            classify_by_tags(&meta("Mystery Title", &[])),
            CookbookGuess::Unknown
        );
    }

    #[test]
    fn find_epubs_walks_recursively() {
        let dir = std::env::temp_dir().join(format!("recipe-epub-find-{}", std::process::id()));
        let nested = dir.join("Author").join("Title (1)");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(nested.join("book.epub"), b"x").unwrap();
        std::fs::write(nested.join("cover.jpg"), b"x").unwrap();
        std::fs::write(dir.join("top.EPUB"), b"x").unwrap(); // case-insensitive ext

        let mut found = find_epubs(&dir);
        found.sort();
        let names: Vec<_> = found
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        std::fs::remove_dir_all(&dir).ok();
        assert!(names.contains(&"book.epub".to_string()));
        assert!(names.contains(&"top.EPUB".to_string()));
        assert_eq!(found.len(), 2, "only .epub files, recursively");
    }
}
