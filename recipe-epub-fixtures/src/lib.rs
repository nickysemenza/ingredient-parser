//! Real EPUB bytes for offline tests and UI previews.
//!
//! The fixture contains invented recipes, not extraction output. It needs no
//! filesystem, clock, randomness, network, or model calls. ZIP entries have fixed
//! ordering, timestamps, and permissions so repeated generation is byte-identical.
//!
//! ```
//! let bytes = recipe_epub_fixtures::cookbook_epub()?;
//! assert!(bytes.starts_with(b"PK"));
//! # Ok::<(), zip::result::ZipError>(())
//! ```

use std::io::{Cursor, Write};
use zip::{CompressionMethod, DateTime, ZipWriter, result::ZipResult, write::SimpleFileOptions};

/// Generate the synthetic three-recipe cookbook as an EPUB 2 archive.
///
/// Chapters, in spine order: roasted tomato soup with white beans and basil,
/// flatbread, and lemon dressing. The first recipe contains a long title, twelve
/// ingredient lines, Unicode quantities, optional seasoning, and multiple steps.
/// Source XHTML and navigation are embedded in the crate; no fixture files need
/// to be copied into the consuming test project.
///
/// This fixture intentionally has no images, DRM, or extraction responses.
/// Returns ZIP/I/O errors if archive construction fails.
pub fn cookbook_epub() -> ZipResult<Vec<u8>> {
    let mut archive = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Stored)
        .last_modified_time(DateTime::default())
        .unix_permissions(0o644);
    // EPUB requires this to be the first entry, uncompressed, without extras.
    for (path, content) in [
        ("mimetype", "application/epub+zip"),
        (
            "META-INF/container.xml",
            include_str!("../assets/container.xml"),
        ),
        ("OEBPS/content.opf", include_str!("../assets/content.opf")),
        ("OEBPS/toc.ncx", include_str!("../assets/toc.ncx")),
        (
            "OEBPS/tomato-soup.xhtml",
            include_str!("../assets/tomato-soup.xhtml"),
        ),
        (
            "OEBPS/flatbread.xhtml",
            include_str!("../assets/flatbread.xhtml"),
        ),
        (
            "OEBPS/lemon-dressing.xhtml",
            include_str!("../assets/lemon-dressing.xhtml"),
        ),
    ] {
        archive.start_file(path, options)?;
        archive.write_all(content.as_bytes())?;
    }
    Ok(archive.finish()?.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    #[test]
    fn deterministic_epub_container() -> Result<(), Box<dyn std::error::Error>> {
        let bytes = cookbook_epub()?;
        assert_eq!(bytes, cookbook_epub()?);
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes))?;
        assert_eq!(archive.len(), 7);
        let mut mime = archive.by_index(0)?;
        assert_eq!(mime.name(), "mimetype");
        assert_eq!(mime.compression(), CompressionMethod::Stored);
        assert_eq!(mime.extra_data(), Some([].as_slice()));
        let mut content = String::new();
        mime.read_to_string(&mut content)?;
        assert_eq!(content, "application/epub+zip");
        Ok(())
    }
}
