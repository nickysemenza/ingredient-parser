//! A small, deterministic EPUB writer.
//!
//! The builder owns every part of the archive that is boilerplate — the
//! `mimetype` entry, `container.xml`, the OPF package document, `toc.ncx`, and
//! the EPUB 3 `nav.xhtml` — so a fixture only has to supply the content
//! documents that make its layout distinctive.

use std::io::{Cursor, Write};
use zip::{CompressionMethod, DateTime, ZipWriter, result::ZipResult, write::SimpleFileOptions};

/// The content directory every generated path is relative to.
const OEBPS: &str = "OEBPS";

/// Fixed modification timestamp for `dcterms:modified`.
///
/// Matches `DateTime::default()` (the ZIP epoch), so nothing in the archive
/// varies with the wall clock.
const MODIFIED: &str = "1980-01-01T00:00:00Z";

/// Which EPUB generation the package document declares.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EpubVersion {
    /// EPUB 2.0.1: NCX navigation, `<meta name="cover">`.
    #[default]
    V2,
    /// EPUB 3.0: `nav.xhtml`, `properties="cover-image"`.
    V3,
}

impl EpubVersion {
    const fn package_version(self) -> &'static str {
        match self {
            Self::V2 => "2.0",
            Self::V3 => "3.0",
        }
    }
}

struct Doc {
    path: String,
    xhtml: String,
}

struct Image {
    path: String,
    mime: String,
    bytes: Vec<u8>,
}

struct NavEntry {
    label: String,
    href: String,
    depth: u8,
}

struct Page {
    number: u32,
    href: String,
}

/// Fluent builder for a byte-identical EPUB archive.
///
/// Every path passed to [`EpubBuilder::doc`] and [`EpubBuilder::image`] is
/// relative to `OEBPS/`, and so is every `href` in [`EpubBuilder::nav_entry`]
/// and [`EpubBuilder::page`] — `toc.ncx` and `nav.xhtml` are both written at
/// `OEBPS/`, so fixture hrefs resolve the same way from either.
///
/// ```
/// use cookbook_fixtures::{EpubBuilder, EpubVersion};
/// let bytes = EpubBuilder::new("Tiny book")
///     .version(EpubVersion::V3)
///     .doc("c01.xhtml", "<html xmlns=\"http://www.w3.org/1999/xhtml\"><body/></html>")
///     .nav_entry("Chapter one", "c01.xhtml", 1)
///     .build()?;
/// assert!(bytes.starts_with(b"PK"));
/// # Ok::<(), zip::result::ZipError>(())
/// ```
pub struct EpubBuilder {
    title: String,
    version: EpubVersion,
    author: Option<String>,
    identifier: Option<String>,
    subject: Option<String>,
    docs: Vec<Doc>,
    images: Vec<Image>,
    cover: Option<String>,
    ncx: bool,
    nav: Option<bool>,
    page_list: bool,
    nav_entries: Vec<NavEntry>,
    pages: Vec<Page>,
}

impl EpubBuilder {
    /// Start an EPUB 2 archive titled `title`.
    #[must_use]
    pub fn new(title: &str) -> Self {
        Self {
            title: title.to_owned(),
            version: EpubVersion::V2,
            author: None,
            identifier: None,
            subject: None,
            docs: Vec::new(),
            images: Vec::new(),
            cover: None,
            ncx: true,
            nav: None,
            page_list: false,
            nav_entries: Vec::new(),
            pages: Vec::new(),
        }
    }

    /// Select the EPUB generation. Defaults to [`EpubVersion::V2`].
    #[must_use]
    pub const fn version(mut self, version: EpubVersion) -> Self {
        self.version = version;
        self
    }

    /// Set `<dc:creator>`.
    #[must_use]
    pub fn author(mut self, author: &str) -> Self {
        self.author = Some(author.to_owned());
        self
    }

    /// Set `<dc:identifier>`. Defaults to `urn:cookbook-fixtures:<slug>`.
    #[must_use]
    pub fn identifier(mut self, identifier: &str) -> Self {
        self.identifier = Some(identifier.to_owned());
        self
    }

    /// Set `<dc:subject>`.
    #[must_use]
    pub fn subject(mut self, subject: &str) -> Self {
        self.subject = Some(subject.to_owned());
        self
    }

    /// Append a content document to the manifest and the spine, in call order.
    #[must_use]
    pub fn doc(mut self, path: &str, xhtml: &str) -> Self {
        self.docs.push(Doc {
            path: path.to_owned(),
            xhtml: xhtml.to_owned(),
        });
        self
    }

    /// Add a manifest-only resource. Images never enter the spine.
    #[must_use]
    pub fn image(mut self, path: &str, mime: &str, bytes: &[u8]) -> Self {
        self.images.push(Image {
            path: path.to_owned(),
            mime: mime.to_owned(),
            bytes: bytes.to_vec(),
        });
        self
    }

    /// Mark an already-added image as the cover.
    ///
    /// EPUB 2 records this as `<meta name="cover" content="…">`; EPUB 3 adds
    /// `properties="cover-image"` to the manifest item.
    #[must_use]
    pub fn cover_image(mut self, path: &str) -> Self {
        self.cover = Some(path.to_owned());
        self
    }

    /// Write `toc.ncx` from the [`nav_entry`](Self::nav_entry) calls. Default `true`.
    #[must_use]
    pub const fn ncx(mut self, ncx: bool) -> Self {
        self.ncx = ncx;
        self
    }

    /// Write `nav.xhtml`. Defaults to `true` for [`EpubVersion::V3`], `false` otherwise.
    #[must_use]
    pub const fn nav(mut self, nav: bool) -> Self {
        self.nav = Some(nav);
        self
    }

    /// Add an `epub:type="page-list"` nav built from the [`page`](Self::page) calls.
    #[must_use]
    pub const fn page_list(mut self, page_list: bool) -> Self {
        self.page_list = page_list;
        self
    }

    /// Append a navigation entry. `depth` 1 is a chapter, 2 a recipe inside it.
    #[must_use]
    pub fn nav_entry(mut self, label: &str, href: &str, depth: u8) -> Self {
        self.nav_entries.push(NavEntry {
            label: label.to_owned(),
            href: href.to_owned(),
            depth,
        });
        self
    }

    /// Append a printed-page anchor for the page-list nav.
    #[must_use]
    pub fn page(mut self, number: u32, href: &str) -> Self {
        self.pages.push(Page {
            number,
            href: href.to_owned(),
        });
        self
    }

    fn wants_nav(&self) -> bool {
        self.nav.unwrap_or(self.version == EpubVersion::V3)
    }

    fn identifier_value(&self) -> String {
        self.identifier.clone().unwrap_or_else(|| {
            let slug = slug(&self.title);
            format!("urn:cookbook-fixtures:{slug}")
        })
    }

    /// Serialize the archive.
    ///
    /// Entry order, timestamps, and permissions are fixed, so two calls with
    /// the same inputs produce identical bytes. Returns a ZIP/I/O error only if
    /// archive construction fails.
    pub fn build(self) -> ZipResult<Vec<u8>> {
        let mut entries: Vec<(String, Vec<u8>)> = Vec::new();
        entries.push(("mimetype".to_owned(), b"application/epub+zip".to_vec()));
        entries.push((
            "META-INF/container.xml".to_owned(),
            container().into_bytes(),
        ));
        entries.push((format!("{OEBPS}/content.opf"), self.opf().into_bytes()));
        if self.ncx {
            entries.push((format!("{OEBPS}/toc.ncx"), self.ncx_doc().into_bytes()));
        }
        if self.wants_nav() {
            entries.push((format!("{OEBPS}/nav.xhtml"), self.nav_doc().into_bytes()));
        }
        for doc in &self.docs {
            let path = &doc.path;
            entries.push((format!("{OEBPS}/{path}"), doc.xhtml.as_bytes().to_vec()));
        }
        for image in &self.images {
            let path = &image.path;
            entries.push((format!("{OEBPS}/{path}"), image.bytes.clone()));
        }

        let mut archive = ZipWriter::new(Cursor::new(Vec::new()));
        let options = SimpleFileOptions::default()
            .compression_method(CompressionMethod::Stored)
            .last_modified_time(DateTime::default())
            .unix_permissions(0o644);
        // EPUB requires `mimetype` first, uncompressed, without extra fields —
        // which is why every entry uses `Stored` rather than switching methods.
        for (path, bytes) in &entries {
            archive.start_file(path.as_str(), options)?;
            archive.write_all(bytes)?;
        }
        Ok(archive.finish()?.into_inner())
    }

    fn doc_id(index: usize) -> String {
        format!("doc{index}")
    }

    fn image_id(index: usize) -> String {
        format!("img{index}")
    }

    fn opf(&self) -> String {
        let version = self.version;
        let identifier = self.identifier_value();
        let mut out = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
        out.push_str(&format!(
            "<package xmlns=\"http://www.idpf.org/2007/opf\" unique-identifier=\"bookid\" version=\"{}\">\n",
            version.package_version()
        ));
        out.push_str("<metadata xmlns:dc=\"http://purl.org/dc/elements/1.1/\" xmlns:opf=\"http://www.idpf.org/2007/opf\">\n");
        out.push_str(&format!("<dc:title>{}</dc:title>\n", esc(&self.title)));
        if let Some(author) = &self.author {
            out.push_str(&format!("<dc:creator>{}</dc:creator>\n", esc(author)));
        }
        out.push_str(&format!(
            "<dc:identifier id=\"bookid\">{}</dc:identifier>\n",
            esc(&identifier)
        ));
        out.push_str("<dc:language>en</dc:language>\n");
        if let Some(subject) = &self.subject {
            out.push_str(&format!("<dc:subject>{}</dc:subject>\n", esc(subject)));
        }
        if version == EpubVersion::V3 {
            out.push_str(&format!(
                "<meta property=\"dcterms:modified\">{MODIFIED}</meta>\n"
            ));
        }
        if version == EpubVersion::V2
            && let Some(id) = self.cover_id()
        {
            out.push_str(&format!("<meta name=\"cover\" content=\"{id}\"/>\n"));
        }
        out.push_str("</metadata>\n<manifest>\n");
        if self.ncx {
            out.push_str(
                "<item id=\"ncx\" href=\"toc.ncx\" media-type=\"application/x-dtbncx+xml\"/>\n",
            );
        }
        if self.wants_nav() {
            out.push_str("<item id=\"nav\" href=\"nav.xhtml\" media-type=\"application/xhtml+xml\" properties=\"nav\"/>\n");
        }
        for (index, doc) in self.docs.iter().enumerate() {
            let id = Self::doc_id(index);
            out.push_str(&format!(
                "<item id=\"{id}\" href=\"{}\" media-type=\"application/xhtml+xml\"/>\n",
                esc(&doc.path)
            ));
        }
        for (index, image) in self.images.iter().enumerate() {
            let id = Self::image_id(index);
            let cover = if version == EpubVersion::V3 && self.cover.as_deref() == Some(&image.path)
            {
                " properties=\"cover-image\""
            } else {
                ""
            };
            out.push_str(&format!(
                "<item id=\"{id}\" href=\"{}\" media-type=\"{}\"{cover}/>\n",
                esc(&image.path),
                esc(&image.mime)
            ));
        }
        out.push_str("</manifest>\n");
        out.push_str(if self.ncx {
            "<spine toc=\"ncx\">\n"
        } else {
            "<spine>\n"
        });
        for index in 0..self.docs.len() {
            let id = Self::doc_id(index);
            out.push_str(&format!("<itemref idref=\"{id}\"/>\n"));
        }
        out.push_str("</spine>\n</package>\n");
        out
    }

    fn cover_id(&self) -> Option<String> {
        let cover = self.cover.as_deref()?;
        self.images
            .iter()
            .position(|image| image.path == cover)
            .map(Self::image_id)
    }

    fn ncx_doc(&self) -> String {
        let identifier = self.identifier_value();
        let depth = self
            .nav_entries
            .iter()
            .map(|entry| entry.depth)
            .max()
            .unwrap_or(1);
        let pages = self.pages.len();
        let max_page = self.pages.iter().map(|page| page.number).max().unwrap_or(0);
        let mut out = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
        out.push_str("<ncx xmlns=\"http://www.daisy.org/z3986/2005/ncx/\" version=\"2005-1\">\n");
        out.push_str(&format!(
            "<head><meta name=\"dtb:uid\" content=\"{}\"/><meta name=\"dtb:depth\" content=\"{depth}\"/><meta name=\"dtb:totalPageCount\" content=\"{pages}\"/><meta name=\"dtb:maxPageNumber\" content=\"{max_page}\"/></head>\n",
            esc(&identifier)
        ));
        out.push_str(&format!(
            "<docTitle><text>{}</text></docTitle>\n<navMap>\n",
            esc(&self.title)
        ));
        let mut open: Vec<u8> = Vec::new();
        for (index, entry) in self.nav_entries.iter().enumerate() {
            while open.last().is_some_and(|depth| *depth >= entry.depth) {
                open.pop();
                out.push_str("</navPoint>\n");
            }
            let order = index + 1;
            out.push_str(&format!(
                "<navPoint id=\"navpoint-{order}\" playOrder=\"{order}\"><navLabel><text>{}</text></navLabel><content src=\"{}\"/>\n",
                esc(&entry.label),
                esc(&entry.href)
            ));
            open.push(entry.depth);
        }
        while open.pop().is_some() {
            out.push_str("</navPoint>\n");
        }
        out.push_str("</navMap>\n</ncx>\n");
        out
    }

    fn nav_doc(&self) -> String {
        let mut out = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
        out.push_str("<html xmlns=\"http://www.w3.org/1999/xhtml\" xmlns:epub=\"http://www.idpf.org/2007/ops\" xml:lang=\"en\">\n");
        out.push_str(&format!(
            "<head><title>{}</title></head>\n<body>\n",
            esc(&self.title)
        ));
        out.push_str("<nav epub:type=\"toc\" id=\"toc\"><h1>Contents</h1>\n");
        out.push_str(&self.toc_list());
        out.push_str("</nav>\n");
        if self.page_list {
            out.push_str("<nav epub:type=\"page-list\" id=\"page-list\" hidden=\"hidden\"><h1>List of Pages</h1>\n<ol>\n");
            for page in &self.pages {
                let number = page.number;
                out.push_str(&format!(
                    "<li><a href=\"{}\">{number}</a></li>\n",
                    esc(&page.href)
                ));
            }
            out.push_str("</ol>\n</nav>\n");
        }
        out.push_str("</body>\n</html>\n");
        out
    }

    fn toc_list(&self) -> String {
        struct Open {
            depth: u8,
            child_list: bool,
        }
        let mut out = String::from("<ol>\n");
        let mut open: Vec<Open> = Vec::new();
        for entry in &self.nav_entries {
            while open.last().is_some_and(|item| item.depth >= entry.depth) {
                if let Some(item) = open.pop() {
                    if item.child_list {
                        out.push_str("</ol>\n");
                    }
                    out.push_str("</li>\n");
                }
            }
            if let Some(parent) = open.last_mut()
                && !parent.child_list
            {
                parent.child_list = true;
                out.push_str("<ol>\n");
            }
            out.push_str(&format!(
                "<li><a href=\"{}\">{}</a>",
                esc(&entry.href),
                esc(&entry.label)
            ));
            open.push(Open {
                depth: entry.depth,
                child_list: false,
            });
        }
        while let Some(item) = open.pop() {
            if item.child_list {
                out.push_str("</ol>\n");
            }
            out.push_str("</li>\n");
        }
        out.push_str("</ol>\n");
        out
    }
}

fn container() -> String {
    let mut out = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    out.push_str("<container version=\"1.0\" xmlns=\"urn:oasis:names:tc:opendocument:xmlns:container\"><rootfiles>");
    out.push_str(&format!(
        "<rootfile full-path=\"{OEBPS}/content.opf\" media-type=\"application/oebps-package+xml\"/>"
    ));
    out.push_str("</rootfiles></container>\n");
    out
}

/// Escape the five XML metacharacters so metadata text is safe in attributes
/// and element content alike.
fn esc(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(ch),
        }
    }
    out
}

/// Lowercase ASCII slug used for the default identifier.
fn slug(title: &str) -> String {
    let mut out = String::with_capacity(title.len());
    let mut pending_dash = false;
    for ch in title.chars() {
        if ch.is_ascii_alphanumeric() {
            if pending_dash && !out.is_empty() {
                out.push('-');
            }
            pending_dash = false;
            out.push(ch.to_ascii_lowercase());
        } else {
            pending_dash = true;
        }
    }
    out
}
