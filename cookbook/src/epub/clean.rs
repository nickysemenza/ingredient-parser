//! XHTML → cleaned text lines, one per block element, with the provenance the
//! rest of the pipeline needs: the block's tag and classes, element ids
//! (anchors), internal links, images, heading level, figure context, and page
//! break markers.
//!
//! Every block-level tag is a line boundary — publishers split ingredient lines
//! across `<p>`, `<li>`, and `<div>` alike — and a `<table>` renders one line
//! per row so measure tables (`name | baker's % | weight`) stay parseable.
//! `<head>`, `<script>`, `<style>`, and numeric footnote markers are dropped.

use ego_tree::iter::Edge;
use scraper::{ElementRef, Html, Node};

use super::open::{image_mime, resolve_href};
use crate::model::ImageRef;

/// An internal `<a href>` found on a line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    pub text: String,
    /// Verbatim href, e.g. `c07.xhtml#page_327` or `#note1`.
    pub href: String,
}

/// One cleaned line from one document.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CleanLine {
    pub text: String,
    /// The innermost line-boundary element that opened this line (`p`, `div`,
    /// `li`, `h2`, `tr`, …).
    pub block_tag: String,
    /// Class tokens of that element.
    pub classes: Vec<String>,
    /// Element ids at this line: on the block itself, on inline children, and
    /// on empty elements immediately before it (an anchor on an empty block
    /// attaches to the next line).
    pub anchors: Vec<String>,
    pub links: Vec<Link>,
    /// Images on this line, or in empty blocks nearest to it.
    pub images: Vec<ImageRef>,
    /// `Some(1..=6)` for `<h1>`–`<h6>`.
    pub heading: Option<u8>,
    /// Inside `<figure>`/`<figcaption>` or an element whose class marks a
    /// caption. Such a line is never a recipe title.
    pub in_figure: bool,
    /// A printed page number, from an `epub:type="pagebreak"` /
    /// `role="doc-pagebreak"` marker at or just before this line.
    pub pagebreak: Option<String>,
    /// A rendered table row rather than authored text.
    pub transformed: bool,
}

/// Clean one content document. `doc_path` resolves image sources.
pub fn clean_document(xhtml: &str, doc_path: &str) -> Vec<CleanLine> {
    // NUL marks block boundaries — distinct from source newlines, which appear
    // inside text nodes and must not split a logical line.
    const SEP: char = '\u{0}';
    let xhtml = expand_self_closing(xhtml);
    let dom = Html::parse_document(&xhtml);

    let mut buf = String::new();
    // (offset, block tag, classes, heading level): the boundary element that
    // opened at that offset.
    let mut blocks: Vec<(usize, String, Vec<String>, Option<u8>)> = Vec::new();
    let mut anchors: Vec<(usize, String)> = Vec::new();
    let mut pagebreaks: Vec<(usize, String)> = Vec::new();
    let mut links: Vec<(usize, String, String)> = Vec::new();
    let mut images: Vec<(usize, ImageRef)> = Vec::new();
    // Byte ranges of `buf` written while inside a figure/caption element.
    let mut figure_ranges: Vec<(usize, usize)> = Vec::new();
    let mut figure_depth = 0usize;
    let mut figure_start = 0usize;
    let mut skip_depth = 0usize;
    let mut open_anchor: Option<(usize, String)> = None;
    let mut table_depth = 0usize;
    let mut row_cells: Vec<String> = Vec::new();
    let mut in_cell = false;
    let mut in_caption = false;
    let mut transformed_ranges: Vec<(usize, usize)> = Vec::new();

    for edge in dom.tree.root().traverse() {
        match edge {
            Edge::Open(node) => match node.value() {
                Node::Element(e) => {
                    let name = e.name();
                    let element = ElementRef::wrap(node);
                    if skip_depth == 0
                        && !is_skip(name)
                        && let Some(id) = e.attr("id").map(str::trim).filter(|id| !id.is_empty())
                    {
                        // Recorded before any skip: a page-break marker's id is
                        // what `href="…#page_79"` links point at.
                        anchors.push((buf.len(), id.to_string()));
                    }
                    if let Some(page) = element.and_then(pagebreak_value) {
                        if skip_depth == 0 {
                            pagebreaks.push((buf.len(), page));
                        }
                        // The marker's own text (often the number) is not prose.
                        skip_depth += 1;
                        continue;
                    }
                    if is_skip(name) || element.is_some_and(is_footnote_marker) {
                        open_anchor = None;
                        skip_depth += 1;
                        continue;
                    }
                    if skip_depth > 0 {
                        continue;
                    }
                    if is_figure(e) {
                        if figure_depth == 0 {
                            figure_start = buf.len();
                        }
                        figure_depth += 1;
                    }
                    match name {
                        "table" => {
                            buf.push(SEP);
                            table_depth += 1;
                        }
                        "tr" if table_depth > 0 => {
                            row_cells.clear();
                            in_cell = false;
                            blocks.push((buf.len(), "tr".into(), classes_of(e), None));
                        }
                        "td" | "th" if table_depth > 0 => {
                            row_cells.push(String::new());
                            in_cell = true;
                        }
                        "caption" if table_depth > 0 => {
                            buf.push(SEP);
                            in_caption = true;
                        }
                        _ if table_depth == 0 => {
                            if is_line_boundary(e) {
                                buf.push(SEP);
                                blocks.push((
                                    buf.len(),
                                    name.to_string(),
                                    classes_of(e),
                                    heading_level(name),
                                ));
                            }
                            if name == "a"
                                && let Some(href) = e.attr("href")
                                && is_internal_href(href)
                            {
                                open_anchor = Some((buf.len(), href.trim().to_string()));
                            }
                            // <img> is void (no Close edge): capture it here.
                            if name == "img"
                                && let Some(src) = e.attr("src")
                                && let Some(path) = resolve_image_src(doc_path, src)
                                && let Some(mime) = image_mime(&path)
                            {
                                let alt = e.attr("alt").map(str::trim).map(str::to_string);
                                images.push((
                                    buf.len(),
                                    ImageRef {
                                        path,
                                        mime,
                                        alt: alt.filter(|a| !a.is_empty()),
                                        caption: None,
                                        line: None,
                                    },
                                ));
                            }
                        }
                        _ => {}
                    }
                }
                Node::Text(t) if skip_depth == 0 => {
                    if table_depth > 0 {
                        if in_cell {
                            if let Some(cell) = row_cells.last_mut() {
                                cell.push_str(t);
                            }
                        } else if in_caption {
                            buf.push_str(t);
                        }
                        // Text between cells/rows (source indentation) is dropped.
                    } else {
                        buf.push_str(t);
                    }
                }
                _ => {}
            },
            Edge::Close(node) => {
                let Node::Element(e) = node.value() else {
                    continue;
                };
                let name = e.name();
                let element = ElementRef::wrap(node);
                if element.and_then(pagebreak_value).is_some()
                    || is_skip(name)
                    || element.is_some_and(is_footnote_marker)
                {
                    skip_depth = skip_depth.saturating_sub(1);
                    continue;
                }
                if skip_depth > 0 {
                    continue;
                }
                if is_figure(e) {
                    figure_depth = figure_depth.saturating_sub(1);
                    if figure_depth == 0 {
                        figure_ranges.push((figure_start, buf.len()));
                    }
                }
                match name {
                    "table" if table_depth > 0 => {
                        table_depth -= 1;
                        buf.push(SEP);
                    }
                    "td" | "th" if table_depth > 0 => in_cell = false,
                    "tr" if table_depth > 0 => {
                        let rendered = render_table_row(&row_cells);
                        if !rendered.is_empty() {
                            buf.push(SEP);
                            let start = buf.len();
                            buf.push_str(&rendered);
                            transformed_ranges.push((start, buf.len()));
                            buf.push(SEP);
                        }
                        row_cells.clear();
                        in_cell = false;
                    }
                    "caption" if table_depth > 0 => {
                        buf.push(SEP);
                        in_caption = false;
                    }
                    _ if table_depth == 0 => {
                        if name == "a"
                            && let Some((start, href)) = open_anchor.take()
                        {
                            links.push((start, href, buf[start..].to_string()));
                        }
                        if is_line_boundary(e) {
                            buf.push(SEP);
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    // Split at boundaries, keep non-empty lines, and map every captured
    // offset onto its line.
    let mut out: Vec<CleanLine> = Vec::new();
    let mut ranges: Vec<(usize, usize)> = Vec::new();
    let mut line_start = 0usize;
    for segment in buf.split(SEP) {
        let line_end = line_start + segment.len();
        let text = normalize_ws(segment);
        if !text.is_empty() {
            let block = blocks
                .iter()
                .rev()
                .find(|(off, ..)| *off <= line_start)
                .or_else(|| blocks.iter().find(|(off, ..)| *off < line_end));
            let (block_tag, classes, heading) = match block {
                Some((_, tag, classes, heading)) => (tag.clone(), classes.clone(), *heading),
                None => (String::new(), Vec::new(), None),
            };
            let line_links = links
                .iter()
                .filter(|(off, _, _)| *off >= line_start && *off < line_end)
                .filter_map(|(_, href, t)| {
                    let text = normalize_ws(t);
                    (!text.is_empty()).then(|| Link {
                        text,
                        href: href.clone(),
                    })
                })
                .collect();
            let in_figure = figure_ranges
                .iter()
                .any(|&(s, e)| s <= line_start && line_end <= e.max(s));
            let transformed = transformed_ranges
                .iter()
                .any(|&(s, e)| s < line_end && line_start < e);
            ranges.push((line_start, line_end));
            out.push(CleanLine {
                text,
                block_tag,
                classes,
                anchors: Vec::new(),
                links: line_links,
                images: Vec::new(),
                heading,
                in_figure,
                pagebreak: None,
                transformed,
            });
        }
        line_start = line_end + SEP.len_utf8();
    }

    // Anchors and page breaks attach to the line containing them, else the
    // next line (an id on an empty block names what follows), else the last.
    for (offset, id) in anchors {
        if let Some(idx) = containing_or_next(&ranges, offset) {
            out[idx].anchors.push(id);
        }
    }
    for (offset, page) in pagebreaks {
        if let Some(idx) = containing_or_next(&ranges, offset)
            && out[idx].pagebreak.is_none()
        {
            out[idx].pagebreak = Some(page);
        }
    }
    // Images attach to the nearest line; ties prefer the line after (a figure
    // introduces the heading that follows it).
    for (offset, img) in images {
        if let Some(idx) = nearest_line(&ranges, offset) {
            out[idx].images.push(img);
        }
    }
    out
}

/// All lines of a document joined, for tests and `inspect`.
pub fn clean_to_text(xhtml: &str) -> String {
    clean_document(xhtml, "")
        .into_iter()
        .map(|l| l.text)
        .collect::<Vec<_>>()
        .join("\n")
}

fn classes_of(e: &scraper::node::Element) -> Vec<String> {
    e.attr("class")
        .map(|c| c.split_whitespace().map(str::to_string).collect())
        .unwrap_or_default()
}

fn heading_level(tag: &str) -> Option<u8> {
    match tag {
        "h1" => Some(1),
        "h2" => Some(2),
        "h3" => Some(3),
        "h4" => Some(4),
        "h5" => Some(5),
        "h6" => Some(6),
        _ => None,
    }
}

/// `<figure>`, `<figcaption>`, or any element whose class marks a caption
/// (`cap`, `caption`, `caption_img`, `figcaption`, …). Publisher class names
/// vary but the `cap` stem is near-universal.
fn is_figure(e: &scraper::node::Element) -> bool {
    matches!(e.name(), "figure" | "figcaption")
        || e.attr("class").is_some_and(|classes| {
            classes.split_whitespace().any(|c| {
                let c = c.to_ascii_lowercase();
                c == "cap"
                    || c.starts_with("caption")
                    || c.starts_with("figcap")
                    || c.starts_with("fig_cap")
            })
        })
}

/// The page number carried by a page break marker, when `element` is one.
fn pagebreak_value(element: ElementRef<'_>) -> Option<String> {
    let e = element.value();
    let is_marker = e
        .attr("epub:type")
        .is_some_and(|t| t.split_whitespace().any(|t| t == "pagebreak"))
        || e.attr("role") == Some("doc-pagebreak");
    if !is_marker {
        return None;
    }
    let value = e
        .attr("title")
        .or_else(|| e.attr("aria-label"))
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_string)
        .or_else(|| {
            let text = normalize_ws(&element.text().collect::<String>());
            (!text.is_empty()).then_some(text)
        })
        .or_else(|| e.attr("id").and_then(page_from_id));
    Some(value.unwrap_or_default())
}

/// `page_79`, `pg79`, `p79`, `page79` → `79`.
pub fn page_from_id(id: &str) -> Option<String> {
    let lower = id.to_ascii_lowercase();
    for prefix in ["page_", "page-", "page", "pg_", "pg", "p"] {
        if let Some(rest) = lower.strip_prefix(prefix)
            && !rest.is_empty()
            && rest.chars().all(|c| c.is_ascii_digit())
        {
            return Some(rest.to_string());
        }
    }
    None
}

fn containing_or_next(ranges: &[(usize, usize)], offset: usize) -> Option<usize> {
    ranges
        .iter()
        .position(|&(start, end)| start <= offset && offset < end)
        .or_else(|| ranges.iter().position(|&(start, _)| start >= offset))
        .or_else(|| ranges.len().checked_sub(1))
}

/// Index of the line nearest a buffer `offset`. Distance is 0 inside a line's
/// range, else the gap to the nearest edge; ties prefer the line after.
fn nearest_line(ranges: &[(usize, usize)], offset: usize) -> Option<usize> {
    let mut best: Option<(usize, usize, bool)> = None;
    for (i, &(start, end)) in ranges.iter().enumerate() {
        let (dist, after) = if offset < start {
            (start - offset, true)
        } else if offset >= end {
            (offset - end + 1, false)
        } else {
            (0, false)
        };
        let better = match best {
            None => true,
            Some((bd, _, b_after)) => dist < bd || (dist == bd && after && !b_after),
        };
        if better {
            best = Some((dist, i, after));
        }
    }
    best.map(|(_, i, _)| i)
}

/// Resolve an `<img src>` against its document. External and inline sources
/// have no archive entry and are dropped.
fn resolve_image_src(doc_path: &str, src: &str) -> Option<String> {
    let src = src.trim();
    let lower = src.to_ascii_lowercase();
    if src.is_empty()
        || lower.starts_with("http://")
        || lower.starts_with("https://")
        || lower.starts_with("data:")
    {
        return None;
    }
    let dir = doc_path.rfind('/').map(|i| &doc_path[..i]).unwrap_or("");
    let path = resolve_href(dir, src);
    (!path.is_empty()).then_some(path)
}

/// Whether an href points within the book rather than to the web.
pub fn is_internal_href(href: &str) -> bool {
    let h = href.trim();
    !h.is_empty()
        && !h.starts_with("http://")
        && !h.starts_with("https://")
        && !h.starts_with("mailto:")
        && (h.starts_with('#') || h.contains(".htm") || h.contains(".xhtml") || h.contains('#'))
}

/// Numeric superscript internal links are editorial footnote markers, not
/// food text.
fn is_footnote_marker(element: ElementRef<'_>) -> bool {
    element.value().name() == "a"
        && element.value().attr("href").is_some_and(is_internal_href)
        && element
            .descendants()
            .filter_map(ElementRef::wrap)
            .any(|e| e.value().name() == "sup")
        && {
            let text = element.text().collect::<String>();
            !text.trim().is_empty() && text.trim().chars().all(|c| c.is_ascii_digit())
        }
}

/// Tags that introduce a line break: every block-level element plus `<br>`.
fn is_block(tag: &str) -> bool {
    matches!(
        tag,
        "p" | "div"
            | "li"
            | "ul"
            | "ol"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "dl"
            | "dt"
            | "dd"
            | "blockquote"
            | "section"
            | "article"
            | "header"
            | "footer"
            | "aside"
            | "main"
            | "nav"
            | "figure"
            | "figcaption"
            | "caption"
            | "pre"
            | "hr"
            | "br"
    )
}

/// Publisher yield spans are standalone metadata even when nested in a
/// headnote. Other inline spans stay joined (fractions, emphasis, names).
fn is_line_boundary(e: &scraper::node::Element) -> bool {
    is_block(e.name())
        || e.attr("class").is_some_and(|classes| {
            classes
                .split_whitespace()
                .any(|class| matches!(class, "yield" | "bullseye"))
        })
}

/// Non-rendered subtrees whose text must be dropped.
fn is_skip(tag: &str) -> bool {
    matches!(tag, "head" | "script" | "style" | "noscript" | "title")
}

/// Expand every self-closing non-void tag (`<span …/>`, `<a id="x"/>`,
/// `<script src="k.js"/>`) into an explicit open/close pair.
///
/// EPUB documents are XHTML, where `<span/>` is empty. html5ever is an HTML
/// parser, where `<span/>` *opens* a span that swallows everything up to the
/// enclosing block. A self-closing page-break marker would then own the recipe
/// title after it, and a self-closing `<script/>` in `<head>` would own the
/// whole body (this once dropped a 22-chapter cookbook to zero lines).
/// Comments, CDATA, and processing instructions are copied through.
fn expand_self_closing(xhtml: &str) -> std::borrow::Cow<'_, str> {
    if !xhtml.contains("/>") {
        return std::borrow::Cow::Borrowed(xhtml);
    }
    let bytes = xhtml.as_bytes();
    let mut out = String::with_capacity(xhtml.len() + 64);
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'<' {
            let ch = xhtml[i..].chars().next().unwrap_or('\u{fffd}');
            out.push(ch);
            i += ch.len_utf8();
            continue;
        }
        // Copy comments, CDATA, doctype, and processing instructions verbatim.
        let rest = &xhtml[i..];
        let literal_end = if rest.starts_with("<!--") {
            rest.find("-->").map(|e| e + 3)
        } else if rest.starts_with("<![CDATA[") {
            rest.find("]]>").map(|e| e + 3)
        } else if rest.starts_with("<!") || rest.starts_with("<?") {
            rest.find('>').map(|e| e + 1)
        } else {
            None
        };
        if let Some(len) = literal_end {
            out.push_str(&rest[..len]);
            i += len;
            continue;
        }
        // A tag: read the name, then attributes honoring quotes, up to '>'.
        let name_len = rest[1..]
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == ':' || c == '_'))
            .unwrap_or(rest.len() - 1);
        if name_len == 0 {
            out.push('<');
            i += 1;
            continue;
        }
        let name = &rest[1..1 + name_len];
        let mut j = 1 + name_len;
        let mut quote: Option<u8> = None;
        let rb = rest.as_bytes();
        while j < rb.len() {
            match (quote, rb[j]) {
                (Some(q), c) if c == q => quote = None,
                (Some(_), _) => {}
                (None, b'"') | (None, b'\'') => quote = Some(rb[j]),
                (None, b'>') => break,
                _ => {}
            }
            j += 1;
        }
        let Some(tag) = rest.get(..j + 1) else {
            out.push_str(rest);
            break;
        };
        let inner = &tag[..tag.len() - 1];
        if inner.trim_end().ends_with('/') && !is_void(&name.to_ascii_lowercase()) {
            out.push_str(inner.trim_end().trim_end_matches('/').trim_end());
            out.push_str("></");
            out.push_str(name);
            out.push('>');
        } else {
            out.push_str(tag);
        }
        i += tag.len();
    }
    std::borrow::Cow::Owned(out)
}

fn is_void(tag: &str) -> bool {
    matches!(
        tag,
        "area"
            | "base"
            | "br"
            | "col"
            | "embed"
            | "hr"
            | "img"
            | "input"
            | "link"
            | "meta"
            | "param"
            | "source"
            | "track"
            | "wbr"
    )
}

/// Render one table row into a single line. Multi-column measure tables (name /
/// baker's % / weight) render as `"{name} ({weight})"`, dropping the bare ratio
/// column so the parser recovers the weight. Other rows space-join.
fn render_table_row(cells: &[String]) -> String {
    let cells: Vec<String> = cells
        .iter()
        .map(|c| normalize_ws(c))
        .filter(|c| !c.is_empty())
        .collect();
    match cells.as_slice() {
        [] => String::new(),
        [only] => only.clone(),
        _ => {
            let kinds: Vec<CellKind> = cells.iter().map(|c| classify_cell(c)).collect();
            let measures: Vec<usize> = kinds
                .iter()
                .enumerate()
                .filter(|(_, k)| matches!(k, CellKind::Measure))
                .map(|(i, _)| i)
                .collect();
            if let [w] = measures.as_slice()
                && matches!(kinds[0], CellKind::Text)
            {
                format!("{} ({})", cells[0], cells[*w])
            } else {
                cells.join(" ")
            }
        }
    }
}

enum CellKind {
    /// A number with a real unit (`400 g`): the weight column.
    Measure,
    /// A bare number (`40`): a ratio cell.
    Number,
    Text,
}

fn classify_cell(cell: &str) -> CellKind {
    let parsed = ingredient::from_str(cell);
    if parsed
        .amounts
        .iter()
        .any(|m| !matches!(m.unit(), ingredient::unit::Unit::Whole))
    {
        CellKind::Measure
    } else if !parsed.amounts.is_empty() && parsed.name.trim().is_empty() {
        CellKind::Number
    } else {
        CellKind::Text
    }
}

/// Collapse whitespace (including NBSP) to single spaces, trim, and drop stray
/// control characters. Private Use Area glyphs are kept as-is.
pub fn normalize_ws(s: &str) -> String {
    let cleaned: String = s
        .chars()
        .filter(|c| c.is_whitespace() || !c.is_control())
        .collect();
    cleaned.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use rstest::rstest;

    #[rstest]
    #[case::paragraphs("<p>1 cup flour</p><p>2 eggs</p>", "1 cup flour\n2 eggs")]
    #[case::list_items(
        "<ul><li>1 tsp salt</li><li>3 cloves garlic</li></ul>",
        "1 tsp salt\n3 cloves garlic"
    )]
    #[case::divs(
        "<div class=\"IL_item\">1 oz chiles</div><div class=\"IL_item\">5 g salt</div>",
        "1 oz chiles\n5 g salt"
    )]
    #[case::line_break("<p>line one<br/>line two</p>", "line one\nline two")]
    #[case::footnote(
        "<p>1 teaspoon seeds, roasted<a href='#note1'><sup>1</sup></a> and crushed</p><p id='note1'>1 Roast in a dry pan.</p>",
        "1 teaspoon seeds, roasted and crushed\n1 Roast in a dry pan."
    )]
    #[case::superscript_not_reference("<p>2<sup>3</sup> cups</p>", "23 cups")]
    #[case::inline("<p>1 cup <b>all-purpose</b> flour</p>", "1 cup all-purpose flour")]
    #[case::small_runs(
        "<p class=\"sub-head-fp\">P<small>OLENTA</small> <i>with</i> F<small>RESH</small> C<small>ORN</small></p>",
        "POLENTA with FRESH CORN"
    )]
    #[case::nbsp("<p>1\u{a0}cup\n\n  flour</p>", "1 cup flour")]
    #[case::nested("<table><tr><td><p>200 g flour</p></td></tr></table>", "200 g flour")]
    #[case::bakers_table_row(
        "<table><tr>\
         <td><p class=\"table\">High-extraction wheat flour</p></td>\
         <td><p class=\"table\">40</p></td>\
         <td><p class=\"table\">400 g</p></td></tr></table>",
        "High-extraction wheat flour (400 g)"
    )]
    #[case::bakers_table_full(
        "<table>\
         <tr><td><p>FLOUR</p></td><td><p>BAKER\u{2019}S %</p></td><td><p>WEIGHT</p></td></tr>\
         <tr><td><p>WATER</p></td><td><p>85</p></td><td><p>850 g</p></td></tr>\
         <tr><td colspan=\"3\"><p>+ RYE FLAKES FOR COATING (OPTIONAL)</p></td></tr>\
         </table>",
        "FLOUR BAKER\u{2019}S % WEIGHT\nWATER (850 g)\n+ RYE FLAKES FOR COATING (OPTIONAL)"
    )]
    #[case::drops_style(
        "<html><head><title>T</title><style>p{color:red}</style></head><body><p>real text</p></body></html>",
        "real text"
    )]
    #[case::self_closing_script_head(
        "<html><head><script type=\"text/javascript\" src=\"kobo.js\"/></head><body><p>1 cup flour</p><p>2 eggs</p></body></html>",
        "1 cup flour\n2 eggs"
    )]
    #[case::self_closing_style(
        "<html><head><style/></head><body><p>real text</p></body></html>",
        "real text"
    )]
    // A page-break marker's own text is the page number, not prose.
    #[case::pagebreak_text_dropped(
        "<p><span epub:type=\"pagebreak\" id=\"pg79\">79</span>Cranberry Pie</p>",
        "Cranberry Pie"
    )]
    fn cleans_xhtml(#[case] html: &str, #[case] expected: &str) {
        assert_eq!(clean_to_text(html), expected);
    }

    #[rstest]
    #[case::span("<p><span a=\"1\"/>Title</p>", "<p><span a=\"1\"></span>Title</p>")]
    #[case::anchor("<a id=\"x\" /><p>T</p>", "<a id=\"x\"></a><p>T</p>")]
    #[case::void_untouched("<p><br/><img src=\"a.jpg\"/></p>", "<p><br/><img src=\"a.jpg\"/></p>")]
    #[case::quoted_slash_gt("<a title=\"x/>y\">t</a>", "<a title=\"x/>y\">t</a>")]
    #[case::comment("<!-- <span/> --><p>T</p>", "<!-- <span/> --><p>T</p>")]
    #[case::script(
        "<script src=\"k.js\"/><p>T</p>",
        "<script src=\"k.js\"></script><p>T</p>"
    )]
    #[case::pi("<?xml version=\"1.0\"?><p/>", "<?xml version=\"1.0\"?><p></p>")]
    fn expands_self_closing(#[case] input: &str, #[case] expected: &str) {
        assert_eq!(expand_self_closing(input), expected);
    }

    #[test]
    fn records_block_tag_classes_and_heading() {
        let lines = clean_document(
            "<h2 class=\"rt bold\">Title</h2><p class=\"ril\">1 cup flour</p>",
            "c.xhtml",
        );
        assert_eq!(lines[0].block_tag, "h2");
        assert_eq!(lines[0].classes, ["rt", "bold"]);
        assert_eq!(lines[0].heading, Some(2));
        assert_eq!(lines[1].block_tag, "p");
        assert_eq!(lines[1].classes, ["ril"]);
        assert_eq!(lines[1].heading, None);
    }

    #[rstest]
    #[case::on_block("<p id='t'>Title</p>", 0)]
    #[case::inline("<p>Before<a id='t'></a>after</p>", 0)]
    #[case::empty_block_before("<p>Before</p><a id='t'></a><p>After</p>", 1)]
    #[case::image_container("<p>Before</p><div id='t'><img src='x.jpg'/></div><p>After</p>", 1)]
    #[case::trailing("<p>Title</p><a id='t'></a>", 0)]
    fn anchors_attach_to_their_line_or_the_next(#[case] html: &str, #[case] expected: usize) {
        let lines = clean_document(html, "c.xhtml");
        let found = lines
            .iter()
            .position(|l| l.anchors.iter().any(|a| a == "t"))
            .unwrap();
        assert_eq!(found, expected);
    }

    #[test]
    fn pagebreaks_map_to_lines() {
        let lines = clean_document(
            "<p class=\"rt\"><span epub:type=\"pagebreak\" id=\"page_79\" title=\"79\"/>Mousse Pie</p>\
             <div><span role=\"doc-pagebreak\" aria-label=\"80\"/></div><p>1 cup cream</p>\
             <p><span epub:type=\"pagebreak\" id=\"page_81\"/>Steps</p>",
            "c.xhtml",
        );
        assert_eq!(lines[0].pagebreak.as_deref(), Some("79"));
        assert_eq!(lines[1].pagebreak.as_deref(), Some("80"));
        assert_eq!(lines[2].pagebreak.as_deref(), Some("81"));
        assert!(lines[0].anchors.contains(&"page_79".to_string()));
    }

    #[test]
    fn figure_and_caption_lines_are_marked() {
        let lines = clean_document(
            "<div class=\"caption_img\"><img src=\"../images/a.jpg\"/><p class=\"cap\">Sour Cherry Pie, <a href=\"c02.xhtml#page_111\">this page</a></p></div>\
             <p class=\"rt\">Cranberry Pie</p>\
             <figure><img src=\"b.png\"/><figcaption>A caption</figcaption></figure>",
            "OEBPS/xhtml/c02.xhtml",
        );
        assert_eq!(lines.len(), 3);
        assert!(lines[0].in_figure);
        assert_eq!(
            lines[0].links,
            [Link {
                text: "this page".into(),
                href: "c02.xhtml#page_111".into()
            }]
        );
        assert_eq!(lines[0].images[0].path, "OEBPS/images/a.jpg");
        assert_eq!(lines[0].images[0].mime, "image/jpeg");
        assert!(!lines[1].in_figure);
        assert!(lines[2].in_figure);
        assert_eq!(lines[2].images[0].path, "OEBPS/xhtml/b.png");
    }

    #[test]
    fn images_in_empty_blocks_attach_to_the_following_line() {
        let lines = clean_document(
            "<p>Intro</p><div class=\"img\"><img src=\"hero.jpg\" alt=\"Pie\"/></div><h1>Pie</h1>",
            "c.xhtml",
        );
        assert_eq!(lines[1].text, "Pie");
        assert_eq!(lines[1].images[0].alt.as_deref(), Some("Pie"));
        assert!(lines[0].images.is_empty());
    }

    #[test]
    fn external_links_and_images_are_ignored() {
        let lines = clean_document(
            "<p><a href=\"https://example.com\">web</a> <a href=\"#here\">local</a> <img src=\"http://x/y.jpg\"/></p>",
            "c.xhtml",
        );
        assert_eq!(lines[0].links.len(), 1);
        assert_eq!(lines[0].links[0].href, "#here");
        assert!(lines[0].images.is_empty());
    }

    #[test]
    fn table_rows_are_transformed() {
        let lines = clean_document(
            "<p>Plain</p><table><tr><td>Flour</td><td>500 g</td></tr></table>",
            "c.xhtml",
        );
        assert!(!lines[0].transformed);
        assert!(lines[1].transformed);
        assert_eq!(lines[1].block_tag, "tr");
    }

    #[rstest]
    #[case("page_79", Some("79"))]
    #[case("pg12", Some("12"))]
    #[case("p7", Some("7"))]
    #[case("page-3", Some("3"))]
    #[case("filepos107429", None)]
    #[case("pie", None)]
    fn page_ids(#[case] id: &str, #[case] expected: Option<&str>) {
        assert_eq!(page_from_id(id).as_deref(), expected);
    }
}
