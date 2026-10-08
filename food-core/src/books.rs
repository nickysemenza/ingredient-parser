//! Cookbooks the user opened: the offline outline and verdict, images, source
//! lines, and saved runs. Nothing here calls a model.
use std::sync::{Arc, Mutex, OnceLock};

use base64::Engine;
use cookbook::classify::{Classified, classify_structure};
use cookbook::{Book, BookOutline, Extraction, RunSummary};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::{AppResult, Host};

/// What `open_book` returns: the outline plus the offline cookbook verdict.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct OpenedBook {
    pub path: String,
    pub outline: BookOutline,
    pub classified: Classified,
    /// Runs of this exact file (by content hash), newest first.
    pub runs: Vec<RunSummary>,
    /// 0 when the book was already open.
    pub open_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct BookImage {
    pub path: String,
    pub data_url: String,
}

/// One line of a book's cleaned line stream.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct SourceLine {
    pub line: usize,
    pub text: String,
}

/// An opened run file.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct OpenedRun {
    pub summary: RunSummary,
    pub extraction: Extraction,
}

/// A book holds its whole archive, so only the two most recent stay resident.
const RESIDENT: usize = 2;
type Resident = Vec<(String, String, Arc<Book>)>;
static BOOKS: OnceLock<Mutex<Resident>> = OnceLock::new();

fn file_stem(path: &str) -> String {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    name.strip_suffix(".epub").unwrap_or(name).to_owned()
}

/// The opened book at `path`, reusing a resident handle while the file is
/// unchanged. Returns the open time in milliseconds (0 when reused).
pub fn book(host: &dyn Host, path: &str) -> AppResult<(Arc<Book>, u64)> {
    let stamp = host.stamp(path)?;
    let cache = BOOKS.get_or_init(|| Mutex::new(Vec::new()));
    {
        let books = cache.lock().map_err(|_| "Book cache is unavailable")?;
        if let Some((_, _, book)) = books.iter().find(|(p, s, _)| p == path && *s == stamp) {
            return Ok((book.clone(), 0));
        }
    }
    let started = web_time::Instant::now();
    let book = Arc::new(Book::open(host.read(path)?, file_stem(path)).map_err(|e| e.to_string())?);
    let open_ms = started.elapsed().as_millis() as u64;
    let mut books = cache.lock().map_err(|_| "Book cache is unavailable")?;
    books.retain(|(p, _, _)| p != path);
    if books.len() >= RESIDENT {
        books.remove(0);
    }
    books.push((path.to_owned(), stamp, book.clone()));
    Ok((book, open_ms))
}

pub fn open(host: &dyn Host, path: &str) -> AppResult<OpenedBook> {
    let (book, open_ms) = book(host, path)?;
    Ok(OpenedBook {
        runs: host.runs_for(&book.source().sha256)?,
        outline: book.outline(),
        classified: classify_structure(&book),
        path: path.to_owned(),
        open_ms,
    })
}

fn data_url(mime: &str, bytes: &[u8]) -> String {
    format!(
        "data:{mime};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    )
}

/// One image from the archive, as a data URL.
pub fn image(host: &dyn Host, path: &str, image: &str) -> AppResult<BookImage> {
    let (book, _) = book(host, path)?;
    let (bytes, mime) = book
        .read_image(image)
        .ok_or_else(|| format!("{image} is not in the book"))?;
    Ok(BookImage {
        data_url: data_url(&mime, &bytes),
        path: image.to_owned(),
    })
}

/// A library thumbnail, read without keeping the book resident.
pub fn cover(host: &dyn Host, path: &str) -> AppResult<Option<BookImage>> {
    let bytes = host.read(path)?;
    let package = cookbook::epub::open::Package::parse(&bytes).map_err(|e| e.to_string())?;
    let Some(cover) = package.cover_ref() else {
        return Ok(None);
    };
    Ok(
        cookbook::epub::open::read_image(&bytes, &cover.path).map(|(bytes, mime)| BookImage {
            path: path.to_owned(),
            data_url: data_url(&mime, &bytes),
        }),
    )
}

/// Lines `start..end` of the book's cleaned line stream, clamped to the book.
pub fn source(host: &dyn Host, path: &str, start: usize, end: usize) -> AppResult<Vec<SourceLine>> {
    let (book, _) = book(host, path)?;
    let lines = book.lines();
    if start >= lines.len() {
        return Err("Source line is outside this book".into());
    }
    Ok((start..end.min(lines.len()).max(start))
        .map(|line| SourceLine {
            line,
            text: lines.text(line).to_owned(),
        })
        .collect())
}

pub fn open_run(host: &dyn Host, path: &str) -> AppResult<OpenedRun> {
    let extraction: Extraction = serde_json::from_slice(&host.read(path)?)
        .map_err(|e| format!("{path} is not a saved extraction run: {e}"))?;
    Ok(OpenedRun {
        summary: RunSummary::of(path.to_owned(), &extraction),
        extraction,
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::test_host::MemoryHost;

    #[test]
    fn a_book_opens_offline_reuses_its_handle_and_serves_source_lines() {
        let mut host = MemoryHost::default();
        host.files.insert(
            "food-core-test-book.epub".into(),
            cookbook_fixtures::epub3_nav_pagebreaks().unwrap(),
        );
        let first = open(&host, "food-core-test-book.epub").unwrap();
        assert!(first.outline.chunks >= 1);
        assert!(first.outline.lines > 10);
        let second = open(&host, "food-core-test-book.epub").unwrap();
        assert_eq!(second.open_ms, 0, "second open should reuse the handle");
        let lines = source(&host, "food-core-test-book.epub", 2, 5).unwrap();
        assert_eq!(
            lines.iter().map(|l| l.line).collect::<Vec<_>>(),
            vec![2, 3, 4]
        );
        assert!(source(&host, "food-core-test-book.epub", usize::MAX, usize::MAX).is_err());
        let cover = cover(&host, "food-core-test-book.epub").unwrap();
        assert!(cover.is_none() || cover.unwrap().data_url.starts_with("data:image/"));
        assert!(open(&host, "missing.epub").is_err());
    }
}
