//! Open an EPUB and turn its content documents into cleaned text [`Chunk`]s.
//!
//! Phase 1: one chunk per spine document. Phase 3 replaces this with TOC-region
//! accumulation so a recipe split across multiple spine docs is reassembled.

use std::io::Cursor;

use ego_tree::iter::Edge;
use epub::doc::EpubDoc;
use scraper::{Html, Node};

use crate::{Chunk, EpubError, Link};

/// A cleaned text line plus any internal anchor links it contained.
struct CleanLine {
    text: String,
    links: Vec<Link>,
}

/// Whether an href is an EPUB *internal* link (points within the book) rather
/// than an external URL. Internal links are the cross-recipe signal.
fn is_internal_href(href: &str) -> bool {
    let h = href.trim();
    !h.is_empty()
        && !h.starts_with("http://")
        && !h.starts_with("https://")
        && !h.starts_with("mailto:")
        && (h.starts_with('#') || h.contains(".htm") || h.contains(".xhtml") || h.contains('#'))
}

/// Target chunk size in characters. Large enough that a single long recipe
/// (e.g. Claire Saffitz's detailed methods) stays whole within one chunk —
/// splitting a recipe across chunks loses its tail, since only the title-bearing
/// chunk yields a recipe. Still small enough that the model's structured output
/// for the few recipes in a chunk stays well under `max_tokens` (16k): ~12k
/// input chars ≈ a handful of recipes ≈ well under the cap. The windower prefers
/// to break at a title boundary (see [`looks_like_title`]), so most chunks end
/// cleanly between recipes rather than mid-recipe.
const CHUNK_BUDGET: usize = 12000;
/// Once over budget, accept up to this many extra chars looking for a clean
/// (title-like) boundary before hard-splitting.
const CHUNK_SLACK: usize = 6000;

/// Parse the EPUB `bytes` (a zip) and return text chunks in spine reading order.
///
/// Spine docs are cleaned to lines, concatenated in reading order, then windowed
/// into ~[`CHUNK_BUDGET`]-sized chunks broken at title-like lines. This caps each
/// model call's output (no truncation on dense chapters) while merging small
/// consecutive docs so a recipe split across files stays in one chunk.
pub fn chunk_epub(bytes: &[u8]) -> Result<Vec<Chunk>, EpubError> {
    // Borrow the bytes (`Cursor<&[u8]>` is Read+Seek) rather than copying them.
    // Image-heavy cookbooks can be hundreds of MB — an extra `.to_vec()` would
    // double that in (wasm) memory for nothing.
    let mut doc =
        EpubDoc::from_reader(Cursor::new(bytes)).map_err(|e| EpubError::Open(e.to_string()))?;

    let spine_len = doc.get_num_chapters();
    let mut tagged: Vec<(String, CleanLine)> = Vec::new();
    loop {
        // Read each spine doc as BYTES and decode lossily. The epub crate's
        // `get_current_str` runs a *strict* `String::from_utf8` and swallows the
        // failure into `None` — so a single stray byte or a leading UTF-8 BOM
        // (both common in Kobo EPUBs) silently drops the whole document, and a
        // book where every chapter has one yields zero text. Lossy decoding
        // recovers the text; clean UTF-8 is byte-for-byte unaffected.
        if let Some((raw, mime)) = doc.get_current() {
            if mime.contains("html") {
                let doc_path = doc
                    .get_current_path()
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let decoded = String::from_utf8_lossy(&raw);
                // Strip a leading BOM so the html parser sees a clean root.
                let content = decoded.strip_prefix('\u{feff}').unwrap_or(decoded.as_ref());
                for line in clean_xhtml_to_lines(content) {
                    tagged.push((doc_path.clone(), line));
                }
            }
        }
        if !doc.go_next() {
            break;
        }
    }

    // A non-empty spine that produced no text means every content read came back
    // empty (bad encoding or unresolved paths). Surface it rather than silently
    // returning an empty book — this is exactly how a whole cookbook once
    // vanished behind a misleading "0 chunks" success.
    if spine_len > 0 && tagged.is_empty() {
        tracing::warn!(
            "epub: {spine_len}-item spine but 0 readable text lines — every content read returned nothing (encoding or path issue)"
        );
    }

    Ok(window_chunks(tagged))
}

/// Greedily window `(doc_path, line)` pairs into chunks, preferring to break
/// before a title-like line once over budget.
///
/// When a chunk is cut mid-recipe (a *hard* split at `CHUNK_BUDGET+CHUNK_SLACK`
/// rather than a clean title boundary), the recipe's tail — its remaining steps
/// and "Do Ahead" / footnotes — would land in a title-less continuation chunk
/// and be dropped downstream. To avoid that, the continuation chunk inherits the
/// last-seen title as its [`Chunk::title_hint`], so the model re-emits the same
/// titled recipe and `assemble()` merges the two halves.
fn window_chunks(tagged: Vec<(String, CleanLine)>) -> Vec<Chunk> {
    let mut chunks = Vec::new();
    let mut lines: Vec<String> = Vec::new();
    let mut chunk_links: Vec<Link> = Vec::new();
    let mut len = 0usize;
    let mut doc: Option<String> = None;
    // The most recent title-like line, and the hint carried into the next chunk
    // after a mid-recipe hard split.
    let mut last_title: Option<String> = None;
    let mut next_hint: Option<String> = None;

    for (path, line) in tagged {
        let at_title = len >= CHUNK_BUDGET && looks_like_title(&line.text);
        let hard_split = len >= CHUNK_BUDGET + CHUNK_SLACK;
        let want_break = !lines.is_empty() && (at_title || hard_split);
        if want_break {
            chunks.push(Chunk {
                title_hint: next_hint.take(),
                text: lines.join("\n"),
                doc_path: doc.take().unwrap_or_default(),
                links: std::mem::take(&mut chunk_links),
            });
            lines = Vec::new();
            len = 0;
            // A clean break starts on a fresh title; a hard split severs a
            // recipe, so carry its title forward to the continuation chunk.
            if hard_split && !at_title {
                next_hint = last_title.clone();
            }
        }
        if doc.is_none() {
            doc = Some(path);
        }
        if looks_like_title(&line.text) {
            last_title = Some(line.text.clone());
        }
        len += line.text.len() + 1;
        lines.push(line.text);
        chunk_links.extend(line.links);
    }
    if !lines.is_empty() {
        chunks.push(Chunk {
            title_hint: next_hint,
            text: lines.join("\n"),
            doc_path: doc.unwrap_or_default(),
            links: chunk_links,
        });
    }
    chunks
}

/// Cheap, publisher-agnostic guess at whether a line starts a new recipe (a
/// short line that isn't an ingredient quantity or a prose sentence). Only used
/// to pick chunk boundaries — an imperfect guess just shifts where text is cut.
fn looks_like_title(line: &str) -> bool {
    let t = line.trim();
    if t.is_empty() || t.len() > 60 {
        return false;
    }
    // Leader/bullet chars that introduce an ingredient or list line (not titles).
    const LEADERS: &str = "•·*-–—";
    match t.chars().next() {
        // ingredient lines start with a quantity; bullets aren't titles
        Some(c)
            if c.is_ascii_digit() || ingredient::fraction::is_vulgar(c) || LEADERS.contains(c) =>
        {
            false
        }
        Some(_) => !t.ends_with('.'), // a trailing period suggests prose
        None => false,
    }
}

/// Tags that introduce a line break (block-level + `<br>`). Publishers split
/// ingredient lines across all of these — notably `<div>` (e.g. Pok Pok's
/// `<div class="IL_item">`) — so we treat every one as a boundary, not just an
/// allowlist of `<p>`/`<li>`.
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
            | "table"
            | "thead"
            | "tbody"
            | "tr"
            | "td"
            | "th"
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

/// Non-rendered subtrees whose text must be dropped (CSS, scripts, `<head>`).
fn is_skip(tag: &str) -> bool {
    matches!(tag, "head" | "script" | "style" | "noscript" | "title")
}

/// Strip XHTML to text and join into one string (one line per block element).
#[cfg(test)]
pub(crate) fn clean_xhtml_to_text(xhtml: &str) -> String {
    clean_xhtml_to_lines(xhtml)
        .into_iter()
        .map(|l| l.text)
        .collect::<Vec<_>>()
        .join("\n")
}

/// Convert self-closing `<script .../>` / `<style .../>` tags into explicit
/// empty pairs (`<script ...></script>`).
///
/// `scraper` parses with html5ever (an *HTML* parser), where `<script>` and
/// `<style>` are raw-text elements: a self-closing form — valid in XHTML and
/// common in Kobo EPUBs (`<script ... src="kobo.js"/>` in `<head>`) — is parsed
/// as an *unclosed* `<script>`, so html5ever swallows the entire rest of the
/// document as script text. The body then never leaves `<script>`, so the DOM
/// walk extracts zero text (this silently dropped a whole 22-chapter cookbook).
/// Returns a borrow when there's nothing to fix, so the common path doesn't allocate.
fn close_self_closing_rawtext(xhtml: &str) -> std::borrow::Cow<'_, str> {
    let lower = xhtml.to_ascii_lowercase();
    if !lower.contains("<script") && !lower.contains("<style") {
        return std::borrow::Cow::Borrowed(xhtml);
    }
    let mut out = String::with_capacity(xhtml.len() + 32);
    let mut i = 0;
    while i < xhtml.len() {
        let tag = if lower[i..].starts_with("<script") {
            Some("script")
        } else if lower[i..].starts_with("<style") {
            Some("style")
        } else {
            None
        };
        if let Some(tag) = tag {
            if let Some(rel_end) = xhtml[i..].find('>') {
                let end = i + rel_end; // index of '>'
                let open = &xhtml[i..end]; // tag without the closing '>'
                if open.trim_end().ends_with('/') {
                    out.push_str(open.trim_end().trim_end_matches('/').trim_end());
                    out.push('>');
                    out.push_str("</");
                    out.push_str(tag);
                    out.push('>');
                    i = end + 1;
                    continue;
                }
            }
        }
        let ch = xhtml[i..].chars().next().unwrap_or('\u{fffd}');
        out.push(ch);
        i += ch.len_utf8();
    }
    std::borrow::Cow::Owned(out)
}

/// Strip XHTML to text, one line per block-level element, by walking the DOM and
/// inserting a newline at every block boundary. Captures `<div>`/`<span>`-based
/// layouts that a tag allowlist would miss, and drops `<script>`/`<style>`/`<head>`.
/// Also captures internal `<a href>` links per line (the cross-recipe signal).
fn clean_xhtml_to_lines(xhtml: &str) -> Vec<CleanLine> {
    // NUL marks block boundaries — distinct from source newlines/indentation,
    // which appear inside text nodes and must NOT split a logical line.
    const SEP: char = '\u{0}';
    let xhtml = close_self_closing_rawtext(xhtml);
    let dom = Html::parse_document(&xhtml);
    let mut buf = String::new();
    let mut skip_depth = 0usize;
    // Internal anchors, recorded as (byte offset in `buf` where the link text
    // began, href, link text) so each can later be mapped to its split line.
    let mut links: Vec<(usize, String, String)> = Vec::new();
    // Open <a> with an internal href: (start offset in buf, href).
    let mut open_anchor: Option<(usize, String)> = None;

    for edge in dom.tree.root().traverse() {
        match edge {
            Edge::Open(node) => match node.value() {
                Node::Element(e) => {
                    let name = e.name();
                    if is_skip(name) {
                        skip_depth += 1;
                    } else if skip_depth == 0 {
                        if is_block(name) {
                            buf.push(SEP);
                        }
                        if name == "a" {
                            if let Some(href) = e.attr("href") {
                                if is_internal_href(href) {
                                    open_anchor = Some((buf.len(), href.to_string()));
                                }
                            }
                        }
                    }
                }
                Node::Text(t) if skip_depth == 0 => buf.push_str(t),
                _ => {}
            },
            Edge::Close(node) => {
                if let Node::Element(e) = node.value() {
                    let name = e.name();
                    if is_skip(name) {
                        skip_depth = skip_depth.saturating_sub(1);
                    } else if skip_depth == 0 {
                        if name == "a" {
                            if let Some((start, href)) = open_anchor.take() {
                                let text = buf[start..].to_string();
                                links.push((start, href, text));
                            }
                        }
                        if is_block(name) {
                            buf.push(SEP);
                        }
                    }
                }
            }
        }
    }

    // Map each captured anchor to the split-line it falls in (by byte offset),
    // then normalize. A line's links are those whose start offset lands between
    // that line's start and the next SEP.
    let mut out: Vec<CleanLine> = Vec::new();
    let mut line_start = 0usize;
    for segment in buf.split(SEP) {
        let line_end = line_start + segment.len();
        let text = normalize_ws(segment);
        if !text.is_empty() {
            let line_links: Vec<Link> = links
                .iter()
                .filter(|(off, _, _)| *off >= line_start && *off < line_end)
                .filter_map(|(_, href, t)| {
                    let lt = normalize_ws(t);
                    (!lt.is_empty()).then(|| Link {
                        text: lt,
                        href: href.clone(),
                    })
                })
                .collect();
            out.push(CleanLine {
                text,
                links: line_links,
            });
        }
        line_start = line_end + SEP.len_utf8();
    }
    out
}

/// Collapse all whitespace (incl. non-breaking spaces) to single spaces, trim,
/// and drop stray control characters (some EPUBs embed C0 control bytes around
/// custom-font glyphs). Private Use Area glyphs are left as-is — without the
/// embedded font's cmap we can't decode them to real characters.
fn normalize_ws(s: &str) -> String {
    let cleaned: String = s
        .replace('\u{a0}', " ")
        .chars()
        .filter(|c| c.is_whitespace() || !c.is_control())
        .collect();
    cleaned.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    /// A `(doc, CleanLine)` pair with no links, for window_chunks tests.
    fn tag(doc: &str, text: &str) -> (String, CleanLine) {
        (
            doc.to_string(),
            CleanLine {
                text: text.to_string(),
                links: Vec::new(),
            },
        )
    }

    #[rstest]
    // each <p>/<li> becomes its own line
    #[case::paragraphs("<p>1 cup flour</p><p>2 eggs</p>", "1 cup flour\n2 eggs")]
    #[case::list_items(
        "<ul><li>1 tsp salt</li><li>3 cloves garlic</li></ul>",
        "1 tsp salt\n3 cloves garlic"
    )]
    // div-based ingredient items (Pok Pok style) each become their own line
    #[case::divs(
        "<div class=\"IL_item\">1 oz chiles</div><div class=\"IL_item\">5 g salt</div>",
        "1 oz chiles\n5 g salt"
    )]
    // <br> forces a line break within a block
    #[case::line_break("<p>line one<br/>line two</p>", "line one\nline two")]
    // inline markup inside a block stays on one line
    #[case::inline("<p>1 cup <b>all-purpose</b> flour</p>", "1 cup all-purpose flour")]
    // nbsp + whitespace runs collapse
    #[case::nbsp("<p>1\u{a0}cup\n\n  flour</p>", "1 cup flour")]
    // nested block (p inside td) is not double-counted
    #[case::nested("<table><tr><td><p>200 g flour</p></td></tr></table>", "200 g flour")]
    // <style>/<head> contents are dropped
    #[case::drops_style(
        "<html><head><title>T</title><style>p{color:red}</style></head><body><p>real text</p></body></html>",
        "real text"
    )]
    // A self-closing <script/> in <head> (valid XHTML, common in Kobo EPUBs) must
    // NOT make html5ever swallow the whole body as raw script text. This is the
    // exact bug that silently dropped a 22-chapter cookbook to 0 lines.
    #[case::self_closing_script_head(
        "<html><head><script type=\"text/javascript\" src=\"kobo.js\"/></head><body><p>1 cup flour</p><p>2 eggs</p></body></html>",
        "1 cup flour\n2 eggs"
    )]
    // Self-closing <style/> likewise must not swallow following content.
    #[case::self_closing_style(
        "<html><head><style/></head><body><p>real text</p></body></html>",
        "real text"
    )]
    fn cleans_xhtml(#[case] html: &str, #[case] expected: &str) {
        assert_eq!(clean_xhtml_to_text(html), expected);
    }

    #[rstest]
    // Self-closing raw-text tags become explicit empty pairs; paired tags and
    // text without such tags are returned unchanged (a borrow).
    #[case::script("<script src=\"k.js\"/>", "<script src=\"k.js\"></script>")]
    #[case::style("<style/>", "<style></style>")]
    #[case::paired_untouched("<script>var x=1;</script>", "<script>var x=1;</script>")]
    #[case::no_rawtext("<p>plain</p>", "<p>plain</p>")]
    fn closes_self_closing_rawtext(#[case] input: &str, #[case] expected: &str) {
        assert_eq!(close_self_closing_rawtext(input).as_ref(), expected);
    }

    #[test]
    fn looks_like_title_distinguishes_titles_from_lines() {
        assert!(looks_like_title("Spiced Honey and Rye Cake"));
        assert!(looks_like_title("NAAM PHRIK LAAP"));
        assert!(!looks_like_title("1 cup flour")); // ingredient quantity
        assert!(!looks_like_title("½ teaspoon salt")); // fraction
        assert!(!looks_like_title("Whisk the eggs until pale and fluffy.")); // prose sentence
        assert!(!looks_like_title("")); // empty
    }

    #[test]
    fn window_splits_large_docs_and_merges_small_ones() {
        // Two "recipes" each ~one full budget's worth of body in one doc → the
        // windower should split, starting a fresh chunk at the second title.
        // Sized relative to CHUNK_BUDGET so it holds if the constant changes.
        let line = "x".repeat(140);
        let lines_per_recipe = CHUNK_BUDGET / line.len() + 1;
        let mut tagged = Vec::new();
        for title in ["Chocolate Cake", "Vanilla Cake"] {
            tagged.push(tag("big.html", title));
            for _ in 0..lines_per_recipe {
                tagged.push(tag("big.html", &line));
            }
        }
        let chunks = window_chunks(tagged);
        assert!(chunks.len() >= 2, "expected a split, got {}", chunks.len());
        // No chunk wildly exceeds the budget+slack guard.
        assert!(chunks
            .iter()
            .all(|c| c.text.len() <= CHUNK_BUDGET + CHUNK_SLACK + 200));
        // The split starts a fresh chunk at the second recipe's title.
        assert!(chunks.iter().any(|c| c.text.starts_with("Vanilla Cake")));

        // Two tiny docs merge into a single chunk (split-across-files case).
        let small = vec![tag("a.html", "Pancakes"), tag("b.html", "1 cup flour")];
        let merged = window_chunks(small);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].doc_path, "a.html");
    }

    #[test]
    fn captures_internal_anchor_links_and_strips_markup() {
        let xhtml = r#"<html><body>
            <p>1 recipe <a href="recipes.xhtml#piecrust">The Only Piecrust</a> (this page)</p>
            <p>2 cups flour</p>
            <p>See <a href="https://example.com">our site</a> for more</p>
        </body></html>"#;
        let lines = clean_xhtml_to_lines(xhtml);
        // Visible text is intact (link text inlined, no href leakage).
        assert_eq!(lines[0].text, "1 recipe The Only Piecrust (this page)");
        // The internal anchor is captured on its line.
        assert_eq!(lines[0].links.len(), 1);
        assert_eq!(lines[0].links[0].text, "The Only Piecrust");
        assert!(lines[0].links[0].href.contains("piecrust"));
        // A plain line has no links.
        assert!(lines[1].links.is_empty());
        // External (http) links are ignored.
        assert_eq!(lines[2].text, "See our site for more");
        assert!(lines[2].links.is_empty());
    }

    #[test]
    fn hard_split_carries_title_into_continuation_chunk() {
        // One recipe whose body has NO interior title and exceeds budget+slack,
        // forcing a mid-recipe hard split. The continuation chunk must inherit
        // the recipe's title as its hint so the model re-emits the same recipe
        // (and `assemble()` can merge the tail back in). Sized relative to the
        // constants so it survives changes to them.
        let line = "x".repeat(140);
        let lines = (CHUNK_BUDGET + CHUNK_SLACK) / line.len() + 5;
        let mut tagged = vec![tag("big.html", "Lone Long Recipe")];
        for _ in 0..lines {
            tagged.push(tag("big.html", &line));
        }
        let chunks = window_chunks(tagged);
        assert!(
            chunks.len() >= 2,
            "expected a hard split, got {}",
            chunks.len()
        );
        // First chunk leads the recipe; it has no inherited hint.
        assert!(chunks[0].text.starts_with("Lone Long Recipe"));
        assert_eq!(chunks[0].title_hint, None);
        // The continuation chunk inherits the title (no title line of its own).
        assert_eq!(chunks[1].title_hint.as_deref(), Some("Lone Long Recipe"));
        assert!(!chunks[1].text.starts_with("Lone Long Recipe"));
    }
}
