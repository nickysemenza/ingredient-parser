//! The table of contents: EPUB 3 `nav.xhtml` (`toc` and `page-list`) and the
//! EPUB 2 `toc.ncx` (`navMap` and `pageList`). Entries carry the document and
//! fragment they point at; mapping those to lines happens in `lines`.

/// Publisher NCX and OPF files carry DOCTYPE declarations, which roxmltree
/// rejects unless told otherwise.
const XML_OPTIONS: roxmltree::ParsingOptions = roxmltree::ParsingOptions {
    allow_dtd: true,
    nodes_limit: u32::MAX,
};

use scraper::{ElementRef, Html};
use serde::{Deserialize, Serialize};

use super::clean::normalize_ws;
use super::open::{Package, read_resource, resolve_href};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NavEntry {
    pub label: String,
    /// Archive-relative document path.
    pub doc_path: String,
    pub fragment: Option<String>,
    /// 1 = top level.
    pub depth: u8,
    /// Position in document order, from 0.
    pub order: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageEntry {
    pub page: String,
    pub doc_path: String,
    pub fragment: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Nav {
    pub entries: Vec<NavEntry>,
    pub page_list: Vec<PageEntry>,
}

impl Nav {
    /// Read the table of contents. `nav.xhtml` wins when both exist; the NCX
    /// fills whichever part (entries or page list) the nav document lacks.
    pub fn read(bytes: &[u8], package: &Package) -> Nav {
        let mut nav = Nav::default();
        if let Some(item) = &package.nav
            && let Some(raw) = read_resource(bytes, &item.path)
        {
            nav = parse_nav_xhtml(&String::from_utf8_lossy(&raw), &item.path);
        }
        if (nav.entries.is_empty() || nav.page_list.is_empty())
            && let Some(item) = &package.ncx
            && let Some(raw) = read_resource(bytes, &item.path)
        {
            let ncx = parse_ncx(&String::from_utf8_lossy(&raw), &item.path);
            if nav.entries.is_empty() {
                nav.entries = ncx.entries;
            }
            if nav.page_list.is_empty() {
                nav.page_list = ncx.page_list;
            }
        }
        nav
    }

    pub fn max_depth(&self) -> u8 {
        self.entries.iter().map(|e| e.depth).max().unwrap_or(0)
    }
}

fn split_target(base_dir: &str, href: &str) -> (String, Option<String>) {
    let (path, fragment) = match href.split_once('#') {
        Some((p, f)) => (p, Some(f.to_string()).filter(|f| !f.is_empty())),
        None => (href, None),
    };
    (resolve_href(base_dir, path), fragment)
}

fn dir_of(path: &str) -> &str {
    path.rfind('/').map(|i| &path[..i]).unwrap_or("")
}

pub(crate) fn parse_nav_xhtml(xhtml: &str, nav_path: &str) -> Nav {
    let dom = Html::parse_document(xhtml);
    let base = dir_of(nav_path);
    let mut nav = Nav::default();
    for element in dom.tree.nodes().filter_map(ElementRef::wrap) {
        if element.value().name() != "nav" {
            continue;
        }
        let kind = element
            .value()
            .attr("epub:type")
            .map(|t| t.split_whitespace().collect::<Vec<_>>())
            .unwrap_or_default();
        if kind.contains(&"toc") {
            for ol in element.children().filter_map(ElementRef::wrap) {
                if ol.value().name() == "ol" {
                    walk_ol(ol, 1, base, &mut nav.entries);
                }
            }
        } else if kind.contains(&"page-list") {
            for a in element.descendants().filter_map(ElementRef::wrap) {
                if a.value().name() == "a"
                    && let Some(href) = a.value().attr("href")
                {
                    let page = normalize_ws(&a.text().collect::<String>());
                    if page.is_empty() {
                        continue;
                    }
                    let (doc_path, fragment) = split_target(base, href);
                    nav.page_list.push(PageEntry {
                        page,
                        doc_path,
                        fragment,
                    });
                }
            }
        }
    }
    nav
}

fn walk_ol(ol: ElementRef<'_>, depth: u8, base: &str, out: &mut Vec<NavEntry>) {
    for li in ol.children().filter_map(ElementRef::wrap) {
        if li.value().name() != "li" {
            continue;
        }
        let mut nested: Option<ElementRef<'_>> = None;
        for child in li.children().filter_map(ElementRef::wrap) {
            match child.value().name() {
                "a" => {
                    let label = normalize_ws(&child.text().collect::<String>());
                    if let Some(href) = child.value().attr("href")
                        && !label.is_empty()
                    {
                        let (doc_path, fragment) = split_target(base, href);
                        out.push(NavEntry {
                            label,
                            doc_path,
                            fragment,
                            depth,
                            order: out.len(),
                        });
                    }
                }
                "ol" => nested = Some(child),
                _ => {}
            }
        }
        if let Some(ol) = nested {
            walk_ol(ol, depth + 1, base, out);
        }
    }
}

pub(crate) fn parse_ncx(xml: &str, ncx_path: &str) -> Nav {
    let mut nav = Nav::default();
    let doc = match roxmltree::Document::parse_with_options(xml, XML_OPTIONS) {
        Ok(doc) => doc,
        Err(e) => {
            tracing::warn!(path = ncx_path, error = %e, "unreadable toc.ncx");
            return nav;
        }
    };
    let base = dir_of(ncx_path);
    if let Some(map) = doc.descendants().find(|n| n.has_tag_name("navMap")) {
        walk_nav_points(map, 1, base, &mut nav.entries);
    }
    if let Some(list) = doc.descendants().find(|n| n.has_tag_name("pageList")) {
        for target in list.descendants().filter(|n| n.has_tag_name("pageTarget")) {
            let page = target
                .attribute("value")
                .map(str::to_string)
                .or_else(|| label_of(target));
            let src = target
                .children()
                .find(|n| n.has_tag_name("content"))
                .and_then(|c| c.attribute("src"));
            if let (Some(page), Some(src)) = (page, src) {
                let (doc_path, fragment) = split_target(base, src);
                nav.page_list.push(PageEntry {
                    page,
                    doc_path,
                    fragment,
                });
            }
        }
    }
    nav
}

fn label_of(node: roxmltree::Node<'_, '_>) -> Option<String> {
    let label = node
        .children()
        .find(|n| n.has_tag_name("navLabel"))?
        .descendants()
        .find(|n| n.has_tag_name("text"))?
        .text()?;
    let label = normalize_ws(label);
    (!label.is_empty()).then_some(label)
}

fn walk_nav_points(
    parent: roxmltree::Node<'_, '_>,
    depth: u8,
    base: &str,
    out: &mut Vec<NavEntry>,
) {
    for point in parent.children().filter(|n| n.has_tag_name("navPoint")) {
        let src = point
            .children()
            .find(|n| n.has_tag_name("content"))
            .and_then(|c| c.attribute("src"));
        if let (Some(label), Some(src)) = (label_of(point), src) {
            let (doc_path, fragment) = split_target(base, src);
            out.push(NavEntry {
                label,
                doc_path,
                fragment,
                depth,
                order: out.len(),
            });
        }
        walk_nav_points(point, depth + 1, base, out);
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn parses_nav_xhtml_toc_and_page_list() {
        let nav = parse_nav_xhtml(
            r#"<html xmlns:epub="http://www.idpf.org/2007/ops"><body>
            <nav epub:type="toc"><ol>
              <li><a href="xhtml/c02.xhtml">Pies and Tarts</a><ol>
                <li><a href="xhtml/c02.xhtml#page_79">Cranberry-Pomegranate Mousse Pie</a></li>
                <li><a href="xhtml/c02.xhtml#page_111">Sour Cherry Pie</a></li>
              </ol></li>
              <li><a href="xhtml/c07.xhtml">Foundational Recipes</a></li>
            </ol></nav>
            <nav epub:type="page-list" hidden=""><ol>
              <li><a href="xhtml/c02.xhtml#page_77">77</a></li>
              <li><a href="xhtml/c02.xhtml#page_79">79</a></li>
            </ol></nav></body></html>"#,
            "OEBPS/nav.xhtml",
        );
        let labels: Vec<(&str, u8)> = nav
            .entries
            .iter()
            .map(|e| (e.label.as_str(), e.depth))
            .collect();
        assert_eq!(
            labels,
            [
                ("Pies and Tarts", 1),
                ("Cranberry-Pomegranate Mousse Pie", 2),
                ("Sour Cherry Pie", 2),
                ("Foundational Recipes", 1)
            ]
        );
        assert_eq!(nav.entries[1].doc_path, "OEBPS/xhtml/c02.xhtml");
        assert_eq!(nav.entries[1].fragment.as_deref(), Some("page_79"));
        assert_eq!(nav.entries[0].fragment, None);
        assert_eq!(
            nav.entries.iter().map(|e| e.order).collect::<Vec<_>>(),
            [0, 1, 2, 3]
        );
        assert_eq!(nav.page_list.len(), 2);
        assert_eq!(nav.page_list[1].page, "79");
        assert_eq!(nav.max_depth(), 2);
    }

    #[test]
    fn parses_ncx_nav_map_and_page_list() {
        let nav = parse_ncx(
            r#"<?xml version="1.0"?><ncx xmlns="http://www.daisy.org/z3986/2005/ncx/"><navMap>
              <navPoint id="a" playOrder="1"><navLabel><text>Starchy Dishes</text></navLabel><content src="text/part0011.html"/>
                <navPoint id="b" playOrder="2"><navLabel><text>Polenta</text></navLabel><content src="text/part0011.html#page190"/></navPoint>
              </navPoint>
            </navMap><pageList><pageTarget type="normal" value="190"><navLabel><text>190</text></navLabel><content src="text/part0011.html#page190"/></pageTarget></pageList></ncx>"#,
            "toc.ncx",
        );
        assert_eq!(nav.entries.len(), 2);
        assert_eq!(nav.entries[0].depth, 1);
        assert_eq!(nav.entries[1].depth, 2);
        assert_eq!(nav.entries[1].doc_path, "text/part0011.html");
        assert_eq!(nav.entries[1].fragment.as_deref(), Some("page190"));
        assert_eq!(nav.page_list[0].page, "190");
    }
}
