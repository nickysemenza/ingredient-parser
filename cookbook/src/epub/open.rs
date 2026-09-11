//! Open an EPUB archive and read its package: metadata, manifest, spine, cover,
//! and the content documents in reading order.
//!
//! Implemented directly over `zip` + `roxmltree` (no `epub` crate). The parser is
//! lenient where real books are sloppy: a missing `mimetype` entry, an
//! `.htm` document, a percent-encoded href, or a lossy UTF-8 document all still
//! open. It fails only when there is no container, no package, or no spine.

use std::io::{Cursor, Read, Seek};

use sha2::{Digest, Sha256};
use zip::ZipArchive;

/// Publisher NCX and OPF files carry DOCTYPE declarations, which roxmltree
/// rejects unless told otherwise.
const XML_OPTIONS: roxmltree::ParsingOptions = roxmltree::ParsingOptions {
    allow_dtd: true,
    nodes_limit: u32::MAX,
};

use crate::error::{Error, Result};
use crate::model::ImageRef;

/// One content document from the spine, decoded.
#[derive(Debug, Clone)]
pub struct SpineDoc {
    /// Position in the spine, from 0.
    pub index: usize,
    /// Archive-relative path, e.g. `OEBPS/xhtml/c02.xhtml`.
    pub path: String,
    pub xhtml: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestItem {
    pub id: String,
    /// Archive-relative path (href resolved against the OPF directory,
    /// percent-decoded).
    pub path: String,
    pub media_type: String,
    /// Space-separated `properties` tokens (EPUB 3), e.g. `nav`, `cover-image`.
    pub properties: Vec<String>,
}

/// The parsed package document. Holds no archive bytes.
#[derive(Debug, Clone)]
pub struct Package {
    pub version: String,
    pub title: String,
    pub authors: Vec<String>,
    pub identifiers: Vec<String>,
    pub subjects: Vec<String>,
    pub manifest: Vec<ManifestItem>,
    /// Manifest items in spine order.
    pub spine: Vec<ManifestItem>,
    pub cover: Option<ManifestItem>,
    /// `toc.ncx`, when the book has one.
    pub ncx: Option<ManifestItem>,
    /// EPUB 3 `nav.xhtml`, when the book has one.
    pub nav: Option<ManifestItem>,
}

impl Package {
    /// Parse the package from EPUB bytes.
    pub fn parse(bytes: &[u8]) -> Result<Package> {
        Self::parse_reader(Cursor::new(bytes))
    }

    /// Parse the package straight from a file, reading only the container and
    /// package entries. Library scans use this so a 600 MB book costs two
    /// small reads instead of loading every image.
    pub fn parse_file(path: &std::path::Path) -> Result<Package> {
        Self::parse_reader(std::io::BufReader::new(std::fs::File::open(path)?))
    }

    pub fn parse_reader<R: Read + Seek>(reader: R) -> Result<Package> {
        let mut archive = ZipArchive::new(reader)?;
        let container = read_entry(&mut archive, "META-INF/container.xml")
            .ok_or_else(|| Error::NotAnEpub("missing META-INF/container.xml".into()))?;
        let opf_path = rootfile_path(&container)?;
        let opf = read_entry(&mut archive, &opf_path)
            .ok_or_else(|| Error::NotAnEpub(format!("missing package document {opf_path}")))?;
        parse_opf(&opf_path, &String::from_utf8_lossy(&opf))
    }

    /// Every spine document, decoded lossily (strict UTF-8 would silently drop
    /// Kobo chapters) with any BOM stripped. A document missing from the archive
    /// is skipped with a warning rather than failing the book.
    pub fn spine_docs(&self, bytes: &[u8]) -> Result<Vec<SpineDoc>> {
        let mut archive = ZipArchive::new(Cursor::new(bytes))?;
        let mut docs = Vec::with_capacity(self.spine.len());
        for item in &self.spine {
            let Some(raw) = read_entry(&mut archive, &item.path) else {
                tracing::warn!(path = %item.path, "spine document missing from archive");
                continue;
            };
            let text = String::from_utf8_lossy(&raw);
            let xhtml = text.strip_prefix('\u{feff}').unwrap_or(&text).to_string();
            docs.push(SpineDoc {
                index: docs.len(),
                path: item.path.clone(),
                xhtml,
            });
        }
        if docs.is_empty() {
            return Err(Error::NotAnEpub("no readable spine documents".into()));
        }
        Ok(docs)
    }

    pub fn cover_ref(&self) -> Option<ImageRef> {
        let item = self.cover.as_ref()?;
        Some(ImageRef {
            path: item.path.clone(),
            mime: if item.media_type.is_empty() {
                image_mime(&item.path)?
            } else {
                item.media_type.clone()
            },
            alt: None,
            caption: None,
            line: None,
        })
    }
}

/// Read one archive entry by archive-relative path. Tries the exact name, then
/// a percent-decoded form, then a case-insensitive match.
pub fn read_resource(bytes: &[u8], path: &str) -> Option<Vec<u8>> {
    let mut archive = ZipArchive::new(Cursor::new(bytes)).ok()?;
    read_entry(&mut archive, path)
}

/// Bytes plus mime for an image path, or `None` when it is not in the archive.
pub fn read_image(bytes: &[u8], path: &str) -> Option<(Vec<u8>, String)> {
    let data = read_resource(bytes, path)?;
    let mime = image_mime(path).unwrap_or_else(|| "application/octet-stream".to_string());
    Some((data, mime))
}

/// Hex SHA-256 of the whole archive.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

/// `image/*` mime from the path's extension, else `None`.
pub fn image_mime(path: &str) -> Option<String> {
    let mime = mime_guess::from_path(path).first()?;
    (mime.type_() == mime_guess::mime::IMAGE).then(|| mime.essence_str().to_string())
}

fn read_entry<R: Read + Seek>(archive: &mut ZipArchive<R>, path: &str) -> Option<Vec<u8>> {
    let path = path.trim_start_matches('/');
    let name = if archive.by_name(path).is_ok() {
        path.to_string()
    } else {
        let decoded = percent_decode(path);
        if archive.by_name(&decoded).is_ok() {
            decoded
        } else {
            let lower = decoded.to_ascii_lowercase();
            archive
                .file_names()
                .find(|n| n.to_ascii_lowercase() == lower)?
                .to_string()
        }
    };
    let mut entry = archive.by_name(&name).ok()?;
    let mut out = Vec::with_capacity(entry.size() as usize);
    entry.read_to_end(&mut out).ok()?;
    Some(out)
}

fn rootfile_path(container: &[u8]) -> Result<String> {
    let text = String::from_utf8_lossy(container);
    let doc =
        roxmltree::Document::parse_with_options(&text, XML_OPTIONS).map_err(|e| Error::Xml {
            path: "META-INF/container.xml".into(),
            message: e.to_string(),
        })?;
    doc.descendants()
        .find(|n| n.has_tag_name("rootfile"))
        .and_then(|n| n.attribute("full-path"))
        .map(|p| p.trim_start_matches('/').to_string())
        .ok_or_else(|| Error::NotAnEpub("container.xml has no rootfile".into()))
}

fn parse_opf(opf_path: &str, xml: &str) -> Result<Package> {
    let doc =
        roxmltree::Document::parse_with_options(xml, XML_OPTIONS).map_err(|e| Error::Xml {
            path: opf_path.to_string(),
            message: e.to_string(),
        })?;
    let root = doc.root_element();
    let version = root.attribute("version").unwrap_or("2.0").to_string();
    let opf_dir = opf_path.rfind('/').map(|i| &opf_path[..i]).unwrap_or("");

    let mut title = String::new();
    let mut authors = Vec::new();
    let mut identifiers = Vec::new();
    let mut subjects = Vec::new();
    let mut cover_meta_id: Option<String> = None;
    if let Some(metadata) = root.children().find(|n| n.has_tag_name("metadata")) {
        for node in metadata.children().filter(|n| n.is_element()) {
            let value = node.text().map(str::trim).unwrap_or("").to_string();
            match node.tag_name().name() {
                "title" if title.is_empty() && !value.is_empty() => title = value,
                "creator" if !value.is_empty() => authors.push(value),
                "identifier" if !value.is_empty() => identifiers.push(value),
                "subject" if !value.is_empty() => subjects.push(value),
                "meta" if node.attribute("name") == Some("cover") => {
                    cover_meta_id = node.attribute("content").map(str::to_string);
                }
                _ => {}
            }
        }
    }

    let manifest: Vec<ManifestItem> = root
        .children()
        .find(|n| n.has_tag_name("manifest"))
        .map(|m| {
            m.children()
                .filter(|n| n.has_tag_name("item"))
                .filter_map(|n| {
                    Some(ManifestItem {
                        id: n.attribute("id")?.to_string(),
                        path: resolve_href(opf_dir, n.attribute("href")?),
                        media_type: n.attribute("media-type").unwrap_or("").to_string(),
                        properties: n
                            .attribute("properties")
                            .map(|p| p.split_whitespace().map(str::to_string).collect())
                            .unwrap_or_default(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    let by_id = |id: &str| manifest.iter().find(|i| i.id == id).cloned();

    let spine_node = root
        .children()
        .find(|n| n.has_tag_name("spine"))
        .ok_or_else(|| Error::NotAnEpub("package has no spine".into()))?;
    let spine: Vec<ManifestItem> = spine_node
        .children()
        .filter(|n| n.has_tag_name("itemref"))
        .filter(|n| n.attribute("linear") != Some("no"))
        .filter_map(|n| by_id(n.attribute("idref")?))
        .filter(is_content_doc)
        .collect();
    if spine.is_empty() {
        return Err(Error::NotAnEpub("spine lists no content documents".into()));
    }

    let ncx = spine_node.attribute("toc").and_then(by_id).or_else(|| {
        manifest
            .iter()
            .find(|i| i.media_type == "application/x-dtbncx+xml")
            .cloned()
    });
    let nav = manifest
        .iter()
        .find(|i| i.properties.iter().any(|p| p == "nav"))
        .cloned();
    let cover = manifest
        .iter()
        .find(|i| i.properties.iter().any(|p| p == "cover-image"))
        .cloned()
        .or_else(|| cover_meta_id.as_deref().and_then(by_id))
        .filter(|item| item.media_type.starts_with("image/") || image_mime(&item.path).is_some());

    Ok(Package {
        version,
        title,
        authors,
        identifiers,
        subjects,
        manifest,
        spine,
        cover,
        ncx,
        nav,
    })
}

fn is_content_doc(item: &ManifestItem) -> bool {
    item.media_type == "application/xhtml+xml"
        || item.media_type == "text/html"
        || item.path.ends_with(".xhtml")
        || item.path.ends_with(".html")
        || item.path.ends_with(".htm")
}

/// Resolve an href relative to a directory inside the archive, normalizing
/// `.`/`..`, a leading `/`, and percent-encoding. Drops any fragment or query.
pub fn resolve_href(base_dir: &str, href: &str) -> String {
    let href = href.split(['#', '?']).next().unwrap_or(href).trim();
    let href = percent_decode(href);
    let mut stack: Vec<&str> = Vec::new();
    if !href.starts_with('/') {
        for seg in base_dir.split('/') {
            push_segment(&mut stack, seg);
        }
    }
    for seg in href.trim_start_matches('/').split('/') {
        push_segment(&mut stack, seg);
    }
    stack.join("/")
}

fn push_segment<'a>(stack: &mut Vec<&'a str>, seg: &'a str) {
    match seg {
        "" | "." => {}
        ".." => {
            stack.pop();
        }
        s => stack.push(s),
    }
}

fn percent_decode(s: &str) -> String {
    if !s.contains('%') {
        return s.to_string();
    }
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let (Some(h), Some(l)) = (hex(bytes[i + 1]), hex(bytes[i + 2]))
        {
            out.push(h << 4 | l);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use rstest::rstest;

    #[rstest]
    #[case::sibling("OEBPS", "ch1.xhtml", "OEBPS/ch1.xhtml")]
    #[case::parent("OEBPS/text", "../images/a.jpg", "OEBPS/images/a.jpg")]
    #[case::root("OEBPS/text", "/images/a.jpg", "images/a.jpg")]
    #[case::fragment("OEBPS", "ch1.xhtml#page_12", "OEBPS/ch1.xhtml")]
    #[case::encoded("OEBPS", "Nothing%20Fancy.jpg", "OEBPS/Nothing Fancy.jpg")]
    #[case::no_base("", "content.opf", "content.opf")]
    fn resolves_hrefs(#[case] base: &str, #[case] href: &str, #[case] expected: &str) {
        assert_eq!(resolve_href(base, href), expected);
    }

    fn mini_epub(opf: &str, docs: &[(&str, &str)]) -> Vec<u8> {
        let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let opts = zip::write::SimpleFileOptions::default();
        let mut put = |name: &str, body: &str| {
            archive.start_file(name, opts).unwrap();
            std::io::Write::write_all(&mut archive, body.as_bytes()).unwrap();
        };
        put("mimetype", "application/epub+zip");
        put(
            "META-INF/container.xml",
            "<?xml version=\"1.0\"?><container xmlns=\"urn:oasis:names:tc:opendocument:xmlns:container\"><rootfiles><rootfile full-path=\"OEBPS/content.opf\" media-type=\"application/oebps-package+xml\"/></rootfiles></container>",
        );
        put("OEBPS/content.opf", opf);
        for (name, body) in docs {
            put(name, body);
        }
        archive.finish().unwrap().into_inner()
    }

    const OPF3: &str = "<?xml version=\"1.0\"?><package xmlns=\"http://www.idpf.org/2007/opf\" version=\"3.0\" unique-identifier=\"id\">\
<metadata xmlns:dc=\"http://purl.org/dc/elements/1.1/\"><dc:title>Test Book</dc:title><dc:creator>A. Cook</dc:creator>\
<dc:identifier id=\"id\">urn:isbn:123</dc:identifier><dc:subject>Cooking</dc:subject><meta name=\"cover\" content=\"cov\"/></metadata>\
<manifest><item id=\"nav\" href=\"nav.xhtml\" media-type=\"application/xhtml+xml\" properties=\"nav\"/>\
<item id=\"ncx\" href=\"toc.ncx\" media-type=\"application/x-dtbncx+xml\"/>\
<item id=\"cov\" href=\"images/cover%20art.jpg\" media-type=\"image/jpeg\"/>\
<item id=\"c1\" href=\"text/c01.xhtml\" media-type=\"application/xhtml+xml\"/>\
<item id=\"c2\" href=\"text/c02.xhtml\" media-type=\"application/xhtml+xml\"/>\
<item id=\"css\" href=\"style.css\" media-type=\"text/css\"/></manifest>\
<spine toc=\"ncx\"><itemref idref=\"c1\"/><itemref idref=\"css\"/><itemref idref=\"c2\" linear=\"no\"/></spine></package>";

    #[test]
    fn parses_package_metadata_manifest_and_spine() {
        let bytes = mini_epub(
            OPF3,
            &[
                (
                    "OEBPS/text/c01.xhtml",
                    "\u{feff}<html><body><p>Soup</p></body></html>",
                ),
                (
                    "OEBPS/text/c02.xhtml",
                    "<html><body><p>Skipped</p></body></html>",
                ),
                ("OEBPS/images/cover art.jpg", "jpg"),
            ],
        );
        let package = Package::parse(&bytes).unwrap();
        assert_eq!(package.version, "3.0");
        assert_eq!(package.title, "Test Book");
        assert_eq!(package.authors, ["A. Cook"]);
        assert_eq!(package.identifiers, ["urn:isbn:123"]);
        assert_eq!(package.subjects, ["Cooking"]);
        // Non-linear and non-document itemrefs are not spine documents.
        assert_eq!(package.spine.len(), 1);
        assert_eq!(package.spine[0].path, "OEBPS/text/c01.xhtml");
        assert_eq!(package.ncx.as_ref().unwrap().path, "OEBPS/toc.ncx");
        assert_eq!(package.nav.as_ref().unwrap().path, "OEBPS/nav.xhtml");
        let cover = package.cover_ref().unwrap();
        assert_eq!(cover.path, "OEBPS/images/cover art.jpg");
        assert_eq!(cover.mime, "image/jpeg");
        assert_eq!(read_image(&bytes, &cover.path).unwrap().0, b"jpg");
        let docs = package.spine_docs(&bytes).unwrap();
        assert_eq!(docs.len(), 1);
        assert_eq!(docs[0].index, 0);
        assert!(docs[0].xhtml.starts_with("<html>"), "BOM stripped");
        assert_eq!(sha256_hex(&bytes).len(), 64);
    }

    #[test]
    fn rejects_non_epubs() {
        assert!(matches!(Package::parse(b"not a zip"), Err(Error::Zip(_))));
        let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
        archive
            .start_file("hello.txt", zip::write::SimpleFileOptions::default())
            .unwrap();
        std::io::Write::write_all(&mut archive, b"hi").unwrap();
        let bytes = archive.finish().unwrap().into_inner();
        assert!(matches!(Package::parse(&bytes), Err(Error::NotAnEpub(_))));
    }
}
