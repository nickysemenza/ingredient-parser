//! Source inspection and source-backed recipe associations. No transport or filesystem.
use crate::EpubError;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceBlock {
    pub id: String,
    pub element_index: usize,
    pub anchor: Option<String>,
    pub tag: String,
    pub classes: String,
    pub text: String,
    #[serde(default)]
    pub links: Vec<crate::Link>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceDocument {
    pub path: String,
    pub blocks: Vec<SourceBlock>,
    #[serde(default)]
    pub images: Vec<crate::ImageRef>,
    #[serde(default)]
    pub anchors: Vec<String>,
}

/// Structural evidence for one indexed source line. This deliberately carries
/// DOM facts only; it never assigns a recipe role or declares a block relevant
/// to a recipe. The source text stays in the caller's numbered line array.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SourceLineEvidence {
    pub line: usize,
    /// `indexed` comes from the same cleaning traversal as the chunk. The
    /// legacy fallback is an exact whitespace-only block-text match.
    pub provenance: String,
    /// `exact` for indexed lineage, otherwise `unique`, `ambiguous`, or
    /// `unmatched` for legacy whitespace-only block matching.
    pub match_state: String,
    pub document_line: Option<usize>,
    pub contributors: Vec<SourceEvidenceContributor>,
    pub anchors: Vec<usize>,
    #[serde(default)]
    pub link_targets: Vec<String>,
    /// Images attached by the same nearest-line rule as the legacy chunk.
    #[serde(default)]
    pub images: Vec<crate::ImageRef>,
    pub transformed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SourceEvidenceContributor {
    pub document: String,
    pub element_index: usize,
    /// `unique`, `ambiguous`, or `unmatched`. Ambiguity is retained rather
    /// than resolved by string matching.
    pub match_state: String,
    pub block_ids: Vec<String>,
}

/// One deduplicated DOM element referenced by [`SourceLineEvidence`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SourceEvidenceElement {
    pub document: String,
    pub element_index: usize,
    pub tag: String,
    pub classes: String,
    pub anchor: Option<String>,
    /// Ordered DOM element coordinates from the document root to the parent.
    pub ancestors: Vec<usize>,
}

/// A raw inspected block referenced by one or more source-line contributors.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SourceEvidenceBlock {
    pub id: String,
    pub document: String,
    pub element_index: usize,
    pub tag: String,
    pub classes: String,
    pub anchor: Option<String>,
    #[serde(default)]
    pub link_targets: Vec<String>,
}

/// Compact, lossless structural provenance for a chunk. `elements` and
/// `blocks` are tables so repeated ancestors and markup do not bloat every
/// numbered source line in a provider request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SourceChunkEvidence {
    pub document: String,
    pub lines: Vec<SourceLineEvidence>,
    pub elements: Vec<SourceEvidenceElement>,
    pub blocks: Vec<SourceEvidenceBlock>,
}

impl SourceChunkEvidence {
    /// Select numbered source lines without renumbering them or discarding
    /// their structural tables. Invalid coordinates leave the evidence intact.
    pub fn retain_lines(&mut self, selected: &BTreeSet<usize>) -> Result<(), String> {
        let available = self
            .lines
            .iter()
            .map(|line| line.line)
            .collect::<BTreeSet<_>>();
        if !selected.is_subset(&available) {
            return Err("selected provenance line is outside the source evidence".into());
        }
        self.lines.retain(|line| selected.contains(&line.line));
        Ok(())
    }

    /// Compact column-described tables for the hybrid protocol. Selected lines
    /// retain their contributors, anchors and full ancestor chains. Elements
    /// used only by omitted lines remain in the durable tables for expansion.
    /// Legacy ambiguous matches stay explicit.
    pub fn audit_projection(&self) -> Value {
        if !self
            .lines
            .iter()
            .all(|line| line.provenance == "indexed" && line.match_state == "exact")
        {
            return self.request_projection();
        }
        let referenced = self
            .lines
            .iter()
            .flat_map(|line| {
                line.contributors
                    .iter()
                    .map(|contributor| contributor.element_index)
                    .chain(line.anchors.iter().copied())
            })
            .collect::<BTreeSet<_>>();
        let mut required = referenced.clone();
        for element in &self.elements {
            if referenced.contains(&element.element_index) {
                required.extend(element.ancestors.iter().copied());
            }
        }
        json!({
            "document": self.document,
            "mode": "indexed_tables_v1",
            "element_columns": ["id", "tag", "classes", "anchor", "parent"],
            "elements": self.elements.iter().filter(|element| required.contains(&element.element_index)).map(|element| json!([
                element.element_index, element.tag, element.classes, element.anchor,
                element.ancestors.last(),
            ])).collect::<Vec<_>>(),
            "line_columns": ["line", "document_line", "elements", "anchors", "link_targets", "images", "transformed"],
            "lines": self.lines.iter().map(|line| json!([
                line.line, line.document_line,
                line.contributors.iter().map(|contributor| contributor.element_index).collect::<Vec<_>>(),
                line.anchors, line.link_targets, line.images, line.transformed,
            ])).collect::<Vec<_>>(),
        })
    }

    /// Provider-facing compact projection. The durable public structs above
    /// remain verbose and auditable; this representation removes repeated
    /// document names, exact flags, and ancestor chains from every line while
    /// retaining the same graph through the deduplicated element table.
    pub fn request_projection(&self) -> Value {
        if self
            .lines
            .iter()
            .all(|line| line.provenance == "indexed" && line.match_state == "exact")
        {
            let elements = self
                .elements
                .iter()
                .map(|element| {
                    let mut value = serde_json::Map::new();
                    value.insert("tag".into(), json!(element.tag));
                    if !element.classes.is_empty() {
                        value.insert("classes".into(), json!(element.classes));
                    }
                    if let Some(anchor) = &element.anchor {
                        value.insert("anchor".into(), json!(anchor));
                    }
                    if let Some(parent) = element.ancestors.last() {
                        value.insert("parent".into(), json!(parent));
                    }
                    (element.element_index.to_string(), Value::Object(value))
                })
                .collect::<serde_json::Map<_, _>>();
            let lines = self
                .lines
                .iter()
                .map(|line| {
                    let mut value = serde_json::Map::new();
                    value.insert("line".into(), json!(line.line));
                    value.insert("document_line".into(), json!(line.document_line));
                    value.insert(
                        "elements".into(),
                        json!(
                            line.contributors
                                .iter()
                                .map(|contributor| contributor.element_index)
                                .collect::<Vec<_>>()
                        ),
                    );
                    if !line.anchors.is_empty() {
                        value.insert("anchors".into(), json!(line.anchors));
                    }
                    if !line.link_targets.is_empty() {
                        value.insert("link_targets".into(), json!(line.link_targets));
                    }
                    if !line.images.is_empty() {
                        value.insert("images".into(), json!(line.images));
                    }
                    if line.transformed {
                        value.insert("transformed".into(), Value::Bool(true));
                    }
                    Value::Object(value)
                })
                .collect::<Vec<_>>();
            json!({"document":self.document,"mode":"indexed","elements":elements,"lines":lines})
        } else {
            // Legacy runs have no exact cleaning-line lineage. Preserve every
            // raw block-text match and its state explicitly; do not project a
            // fabricated element graph from the matching display text.
            let blocks = self
                .blocks
                .iter()
                .map(|block| {
                    let mut value = serde_json::Map::new();
                    value.insert("id".into(), json!(block.id));
                    value.insert("element_index".into(), json!(block.element_index));
                    value.insert("tag".into(), json!(block.tag));
                    if !block.classes.is_empty() {
                        value.insert("classes".into(), json!(block.classes));
                    }
                    if let Some(anchor) = &block.anchor {
                        value.insert("anchor".into(), json!(anchor));
                    }
                    if !block.link_targets.is_empty() {
                        value.insert("link_targets".into(), json!(block.link_targets));
                    }
                    Value::Object(value)
                })
                .collect::<Vec<_>>();
            let lines = self
                .lines
                .iter()
                .map(|line| {
                    json!({
                        "line":line.line,
                        "match_state":line.match_state,
                        "contributors":line.contributors.iter().map(|contributor| json!({
                            "element_index":contributor.element_index,
                            "match_state":contributor.match_state,
                            "block_ids":contributor.block_ids,
                        })).collect::<Vec<_>>(),
                    })
                })
                .collect::<Vec<_>>();
            json!({"document":self.document,"mode":"document_text_fallback","blocks":blocks,"lines":lines})
        }
    }
}

fn whitespace_normalized(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn link_targets(links: &[crate::Link]) -> Vec<String> {
    let mut targets = BTreeSet::new();
    for link in links {
        targets.insert(link.href.clone());
    }
    targets.into_iter().collect()
}

fn source_evidence_block(block: &SourceBlock, document: &str) -> SourceEvidenceBlock {
    SourceEvidenceBlock {
        id: block.id.clone(),
        document: document.into(),
        element_index: block.element_index,
        tag: block.tag.clone(),
        classes: block.classes.clone(),
        anchor: block.anchor.clone(),
        link_targets: link_targets(&block.links),
    }
}

/// Build raw DOM evidence for an already-cleaned chunk. Exact indexed lineage
/// is preferred when supplied. Older checkpoints that have no indexed lineage
/// fall back to matching only equivalent whitespace within the same document;
/// duplicate or absent matches remain explicit and are never selected.
pub fn chunk_source_evidence(
    chunk: &crate::Chunk,
    indexed_lines: Option<&[crate::SourceLine]>,
    documents: &[SourceDocument],
) -> Result<SourceChunkEvidence, String> {
    let document = documents
        .iter()
        .find(|document| document.path == chunk.doc_path);
    let mut blocks = BTreeMap::new();
    let mut elements = BTreeMap::new();
    let mut lines = Vec::new();

    if let Some(indexed_lines) = indexed_lines {
        let source_lines: Vec<_> = chunk.text.lines().collect();
        if indexed_lines.len() != source_lines.len() {
            return Err("indexed source provenance does not cover every chunk line".into());
        }
        for (line, indexed) in indexed_lines.iter().enumerate() {
            let mut contributors = Vec::new();
            for contributor in indexed.contributors.iter().chain(&indexed.anchors) {
                for (position, ancestor) in contributor.ancestors.iter().enumerate() {
                    elements.entry(ancestor.element_index).or_insert_with(|| {
                        SourceEvidenceElement {
                            document: chunk.doc_path.clone(),
                            element_index: ancestor.element_index,
                            tag: ancestor.tag.clone(),
                            classes: ancestor.classes.clone(),
                            anchor: ancestor.anchor.clone(),
                            ancestors: contributor.ancestors[..position]
                                .iter()
                                .map(|parent| parent.element_index)
                                .collect(),
                        }
                    });
                }
                elements
                    .entry(contributor.element_index)
                    .or_insert_with(|| SourceEvidenceElement {
                        document: chunk.doc_path.clone(),
                        element_index: contributor.element_index,
                        tag: contributor.tag.clone(),
                        classes: contributor.classes.clone(),
                        anchor: contributor.anchor.clone(),
                        ancestors: contributor
                            .ancestors
                            .iter()
                            .map(|ancestor| ancestor.element_index)
                            .collect(),
                    });
                if indexed
                    .anchors
                    .iter()
                    .any(|anchor| anchor.element_index == contributor.element_index)
                {
                    continue;
                }
                contributors.push(SourceEvidenceContributor {
                    document: chunk.doc_path.clone(),
                    element_index: contributor.element_index,
                    // Indexed contributors come from the exact DOM walk that
                    // produced this line. Do not degrade that lineage by
                    // guessing a selected SourceBlock from element text: an
                    // inline child commonly has a parent block, and direct
                    // text runs may share one element index.
                    match_state: "exact".into(),
                    block_ids: vec![],
                });
            }
            lines.push(SourceLineEvidence {
                line,
                provenance: "indexed".into(),
                // An indexed line may have several authored contributors (for
                // example a table row). That is exact lineage, not text-match
                // ambiguity; each contributor separately records whether its
                // raw SourceBlock lookup was unique, ambiguous, or absent.
                match_state: "exact".into(),
                document_line: Some(indexed.document_line),
                anchors: indexed
                    .anchors
                    .iter()
                    .map(|anchor| anchor.element_index)
                    .collect(),
                contributors,
                link_targets: link_targets(&indexed.links),
                images: indexed.images.clone(),
                transformed: indexed.transformed,
            });
        }
    } else {
        let document_blocks = document
            .map(|document| document.blocks.as_slice())
            .unwrap_or(&[]);
        for (line, text) in chunk.text.lines().enumerate() {
            let normalized = whitespace_normalized(text);
            let matches: Vec<_> = document_blocks
                .iter()
                .filter(|block| whitespace_normalized(&block.text) == normalized)
                .collect();
            for block in &matches {
                blocks
                    .entry(block.id.clone())
                    .or_insert_with(|| source_evidence_block(block, &chunk.doc_path));
            }
            let match_state = match matches.len() {
                0 => "unmatched",
                1 => "unique",
                _ => "ambiguous",
            };
            let contributors = if matches.is_empty() {
                vec![]
            } else {
                matches
                    .iter()
                    .map(|block| SourceEvidenceContributor {
                        document: chunk.doc_path.clone(),
                        element_index: block.element_index,
                        match_state: match_state.into(),
                        block_ids: vec![block.id.clone()],
                    })
                    .collect()
            };
            lines.push(SourceLineEvidence {
                line,
                provenance: "document_text_fallback".into(),
                match_state: match_state.into(),
                document_line: None,
                anchors: vec![],
                contributors,
                link_targets: vec![],
                images: vec![],
                transformed: false,
            });
        }
    }
    Ok(SourceChunkEvidence {
        document: chunk.doc_path.clone(),
        lines,
        elements: elements.into_values().collect(),
        blocks: blocks.into_values().collect(),
    })
}

fn has_block_descendant(element: scraper::ElementRef<'_>) -> bool {
    element
        .descendants()
        .skip(1)
        .filter_map(scraper::ElementRef::wrap)
        .any(|child| is_block_container(child.value().name()))
}

fn source_text(element: scraper::ElementRef<'_>) -> String {
    element
        .descendants()
        .filter(|node| {
            !node
                .ancestors()
                .filter_map(scraper::ElementRef::wrap)
                .any(crate::epub_text::is_footnote_marker)
        })
        .filter_map(|node| match node.value() {
            scraper::Node::Text(text) => Some::<&str>(text),
            scraper::Node::Element(element) if element.name() == "br" => Some(" "),
            _ => None,
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn source_link(path: &str, element: scraper::ElementRef<'_>) -> Option<crate::Link> {
    let href = element.value().attr("href")?;
    let (resource, fragment) = href.split_once('#').unwrap_or((href, ""));
    let target = if resource.is_empty() {
        path.to_owned()
    } else {
        crate::epub_text::resolve_relative(path, resource)?
    };
    Some(crate::Link {
        text: element.text().collect::<String>(),
        href: if fragment.is_empty() {
            target
        } else {
            format!("{target}#{fragment}")
        },
    })
}

fn source_links(path: &str, element: scraper::ElementRef<'_>) -> Vec<crate::Link> {
    element
        .descendants()
        .filter_map(scraper::ElementRef::wrap)
        .filter(|element| element.value().name() == "a")
        .filter_map(|element| source_link(path, element))
        .collect()
}

fn append_inline_text(
    node: ego_tree::NodeRef<'_, scraper::Node>,
    path: &str,
    text: &mut String,
    links: &mut Vec<crate::Link>,
) {
    match node.value() {
        scraper::Node::Text(value) => text.push_str(value),
        scraper::Node::Element(element) => {
            let Some(element_ref) = scraper::ElementRef::wrap(node) else {
                return;
            };
            if crate::epub_text::is_footnote_marker(element_ref) {
                return;
            }
            if element.name() == "br" {
                text.push(' ');
            }
            if element.name() == "a"
                && let Some(link) = source_link(path, element_ref)
            {
                links.push(link);
            }
            for child in node.children() {
                append_inline_text(child, path, text, links);
            }
        }
        _ => {}
    }
}

struct DirectTextRun {
    run_index: usize,
    start: Option<usize>,
    text: String,
    links: Vec<crate::Link>,
}

impl DirectTextRun {
    fn flush(
        &mut self,
        blocks: &mut Vec<(usize, SourceBlock)>,
        path: &str,
        element: scraper::ElementRef<'_>,
        element_index: usize,
    ) {
        let normalized = self.text.split_whitespace().collect::<Vec<_>>().join(" ");
        if !normalized.is_empty() {
            let run = self.run_index;
            self.run_index += 1;
            blocks.push((
                self.start.unwrap_or_default(),
                SourceBlock {
                    id: format!("{path}:element-{element_index}:text-run-{run}"),
                    element_index,
                    anchor: element.value().attr("id").map(str::to_owned),
                    tag: element.value().name().into(),
                    classes: element.value().attr("class").unwrap_or("").into(),
                    text: normalized,
                    links: std::mem::take(&mut self.links),
                },
            ));
        } else {
            self.links.clear();
        }
        self.text.clear();
        self.start = None;
    }
}

/// Preserve text that belongs directly to an omitted container, stopping at
/// nested block boundaries. The emitted ids retain the existing element id
/// prefix and add a run suffix so they cannot collide with child blocks.
fn direct_text_runs(
    path: &str,
    element: scraper::ElementRef<'_>,
    element_index: usize,
    node_order: &std::collections::HashMap<ego_tree::NodeId, usize>,
) -> Vec<(usize, SourceBlock)> {
    let mut blocks = vec![];
    let mut run = DirectTextRun {
        run_index: 0,
        start: None,
        text: String::new(),
        links: vec![],
    };
    for child in element.children() {
        let is_block_boundary = scraper::ElementRef::wrap(child).is_some_and(|child| {
            is_block_container(child.value().name()) || has_block_descendant(child)
        });
        if is_block_boundary {
            run.flush(&mut blocks, path, element, element_index);
            continue;
        }
        run.start
            .get_or_insert_with(|| node_order.get(&child.id()).copied().unwrap_or_default());
        append_inline_text(child, path, &mut run.text, &mut run.links);
    }
    run.flush(&mut blocks, path, element, element_index);
    blocks
}

/// Documents explicitly authored as navigation, for excluding navigation-only
/// incoming links from recipe audit context. Names and publisher classes are
/// never role evidence. Fragment-only guide references cannot classify a whole
/// document, and a semantic nav beside ordinary prose is retained.
pub fn inspect_navigation_documents(bytes: &[u8]) -> Result<BTreeSet<String>, EpubError> {
    inspect_source_with_navigation(bytes).map(|(_, navigation)| navigation)
}

fn guide_navigation_documents<R: std::io::Read + std::io::Seek>(
    doc: &mut epub::doc::EpubDoc<R>,
) -> BTreeSet<String> {
    let package_path = doc.root_file.to_string_lossy().replace('\\', "/");
    let mut result = BTreeSet::new();
    if let Some(package) = doc.get_resource_str_by_path(&package_path) {
        let html = scraper::Html::parse_document(&package);
        for reference in html.tree.nodes().filter_map(scraper::ElementRef::wrap) {
            if reference.value().name() != "reference"
                || !reference
                    .ancestors()
                    .filter_map(scraper::ElementRef::wrap)
                    .any(|element| element.value().name() == "guide")
                || !reference.value().attr("type").is_some_and(|kind| {
                    kind.split_whitespace()
                        .any(|value| matches!(value, "toc" | "index"))
                })
            {
                continue;
            }
            if let Some(href) = reference.value().attr("href")
                && !href.contains('#')
                && let Some(path) = crate::epub_text::resolve_relative(&package_path, href)
                && doc
                    .resources
                    .values()
                    .any(|resource| resource.path.to_string_lossy() == path)
            {
                result.insert(path);
            }
        }
    }
    result
}

fn navigation_only_document(html: &scraper::Html) -> bool {
    let mut navigation_text = false;
    for node in html.tree.nodes() {
        let scraper::Node::Text(text) = node.value() else {
            continue;
        };
        if text.trim().is_empty() {
            continue;
        }
        let ancestors: Vec<_> = node
            .ancestors()
            .filter_map(scraper::ElementRef::wrap)
            .collect();
        if ancestors
            .iter()
            .any(|element| matches!(element.value().name(), "head" | "script" | "style"))
        {
            continue;
        }
        let in_navigation = ancestors.iter().any(|element| {
            element.value().attr("epub:type").is_some_and(|kind| {
                kind.split_whitespace()
                    .any(|value| matches!(value, "toc" | "index" | "landmarks" | "page-list"))
            }) || element.value().attr("role").is_some_and(|kind| {
                kind.split_whitespace()
                    .any(|value| matches!(value, "doc-toc" | "doc-index" | "doc-pagelist"))
            })
        });
        if !in_navigation {
            return false;
        }
        navigation_text = true;
    }
    navigation_text
}

pub fn inspect_source(bytes: &[u8]) -> Result<Vec<SourceDocument>, EpubError> {
    inspect_source_with_navigation(bytes).map(|(documents, _)| documents)
}

/// Inspect source and authored navigation roles in the same XHTML traversal.
/// Existing source documents and their identities remain unchanged.
pub fn inspect_source_with_navigation(
    bytes: &[u8],
) -> Result<(Vec<SourceDocument>, BTreeSet<String>), EpubError> {
    let mut doc = epub::doc::EpubDoc::from_reader(std::io::Cursor::new(bytes))
        .map_err(|e| EpubError::Open(e.to_string()))?;
    let mut navigation = guide_navigation_documents(&mut doc);
    let mut result = Vec::new();
    loop {
        let path = doc
            .get_current_path()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        if let Some((raw, mime)) = doc.get_current()
            && mime.contains("html")
        {
            let decoded = String::from_utf8_lossy(&raw);
            let xhtml = crate::epub_text::close_self_closing_rawtext(&decoded);
            let html = scraper::Html::parse_document(&xhtml);
            if navigation_only_document(&html) {
                navigation.insert(path.clone());
            }
            let elements: Vec<_> = html
                .tree
                .nodes()
                .filter_map(scraper::ElementRef::wrap)
                .enumerate()
                .collect();
            let node_order: std::collections::HashMap<_, _> = html
                .tree
                .root()
                .traverse()
                .enumerate()
                .filter_map(|(order, edge)| match edge {
                    ego_tree::iter::Edge::Open(node) => Some((node.id(), order)),
                    ego_tree::iter::Edge::Close(_) => None,
                })
                .collect();
            let mut blocks: Vec<(usize, SourceBlock)> = vec![];
            for (element_index, element) in elements {
                let tag = element.value().name();
                let classes = element.value().attr("class").unwrap_or("");
                let styled_div = tag == "div"
                    && classes.split_whitespace().any(|class| {
                        is_ingredient_class(class)
                            || matches!(class, "method_step" | "headnote" | "headnote1")
                    });
                let has_block = has_block_descendant(element);
                // Preserve existing selected blocks and their ids. An omitted
                // container may still own direct prose around its child blocks;
                // direct_text_runs emits only those runs, never child content.
                let generic_leaf_div = tag == "div" && !has_block;
                let selected = styled_div
                    || generic_leaf_div
                    || matches!(tag, "p" | "li" | "h1" | "h2" | "h3" | "h4" | "figcaption");
                let order = node_order.get(&element.id()).copied().unwrap_or_default();
                if selected {
                    blocks.push((
                        order,
                        SourceBlock {
                            id: format!("{path}:element-{element_index}"),
                            element_index,
                            anchor: element.value().attr("id").map(str::to_owned),
                            tag: tag.into(),
                            classes: classes.into(),
                            links: source_links(&path, element),
                            text: source_text(element),
                        },
                    ));
                } else if tag == "div" && has_block {
                    blocks.extend(direct_text_runs(&path, element, element_index, &node_order));
                }
            }
            blocks.sort_by_key(|(order, _)| *order);
            let blocks = blocks.into_iter().map(|(_, block)| block).collect();
            let images = html
                .tree
                .nodes()
                .filter_map(scraper::ElementRef::wrap)
                .filter(|e| matches!(e.value().name(), "img" | "image"))
                .filter_map(|e| {
                    let resource = e
                        .value()
                        .attr("src")
                        .or_else(|| e.value().attr("href"))
                        // SVG xlink:href has a namespace; attr() only looks up
                        // unqualified attributes, while attrs() exposes local names.
                        .or_else(|| {
                            e.value()
                                .attrs()
                                .find(|(name, _)| *name == "href")
                                .map(|(_, value)| value)
                        })?;
                    let image_path = crate::epub_text::resolve_relative(&path, resource)?;
                    Some(crate::ImageRef {
                        mime: crate::epub_text::mime_from_ext(&image_path)?,
                        path: image_path,
                        alt: e.value().attr("alt").map(str::to_owned),
                    })
                })
                .collect();
            let anchors = html
                .tree
                .nodes()
                .filter_map(scraper::ElementRef::wrap)
                .filter_map(|e| e.value().attr("id").map(str::to_owned))
                .collect();
            result.push(SourceDocument {
                path,
                blocks,
                images,
                anchors,
            });
        }
        if !doc.go_next() {
            break;
        }
    }
    Ok((result, navigation))
}

/// Resolve literal ingredient hyperlinks when their target document contains one
/// recipe. Unqualified link text such as "here" is sufficient; ambiguous targets
/// and instruction-only links remain unassigned.
pub fn enrich_from_source(recipes: &mut [crate::CookbookRecipe], documents: &[SourceDocument]) {
    enrich_from_source_with_legacy_repairs(recipes, documents, true);
}

/// Source enrichment for a hybrid candidate whose section ownership was
/// already established from indexed source spans. It adds source-backed links
/// only; canonical title, field, and section ownership stay unchanged.
pub(crate) fn enrich_from_source_preserving_hybrid_ownership(
    recipes: &mut [crate::CookbookRecipe],
    documents: &[SourceDocument],
) {
    enrich_from_source_with_legacy_repairs(recipes, documents, false);
}

fn enrich_from_source_with_legacy_repairs(
    recipes: &mut [crate::CookbookRecipe],
    documents: &[SourceDocument],
    apply_legacy_repairs: bool,
) {
    // Older indexed outputs joined a repeated caption and heading. Repair only
    // an exact doubled source heading, including during offline replay.
    let repeated_titles: std::collections::HashMap<_, std::collections::HashMap<_, _>> = documents
        .iter()
        .map(|doc| {
            let titles = doc
                .blocks
                .iter()
                .filter(|block| matches!(block.tag.as_str(), "h1" | "h2"))
                .filter_map(|block| {
                    let title = block.text.split_whitespace().collect::<Vec<_>>().join(" ");
                    (!title.is_empty()).then(|| (format!("{title} {title}").to_lowercase(), title))
                })
                .collect();
            (doc.path.as_str(), titles)
        })
        .collect();
    if apply_legacy_repairs {
        for recipe in recipes.iter_mut() {
            let key = recipe
                .meta
                .title
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .to_lowercase();
            if let Some(title) = recipe
                .url
                .rsplit_once('#')
                .and_then(|(_, doc)| repeated_titles.get(doc))
                .and_then(|titles| titles.get(&key))
            {
                recipe.meta.title.clone_from(title);
            }
        }
    }
    let by_doc: std::collections::HashMap<String, Vec<usize>> =
        recipes
            .iter()
            .enumerate()
            .fold(std::collections::HashMap::new(), |mut map, (i, r)| {
                if let Some((_, doc)) = r.url.rsplit_once('#') {
                    map.entry(doc.to_owned()).or_default().push(i);
                }
                map
            });
    let titles: Vec<_> = recipes.iter().map(|r| r.meta.title.clone()).collect();
    let normalize = crate::extractor::normalize_source_whitespace;
    let normalized: std::collections::HashMap<_, _> = documents
        .iter()
        .map(|doc| {
            let mut blocks: std::collections::HashMap<String, Vec<usize>> =
                std::collections::HashMap::new();
            for (i, block) in doc.blocks.iter().enumerate() {
                blocks.entry(normalize(&block.text)).or_default().push(i);
            }
            (doc.path.as_str(), blocks)
        })
        .collect();
    for (index, recipe) in recipes.iter_mut().enumerate() {
        let Some((_, doc_path)) = recipe.url.rsplit_once('#') else {
            continue;
        };
        let Some(doc) = documents.iter().find(|d| d.path == doc_path) else {
            continue;
        };
        let positions = &normalized[doc_path];
        if apply_legacy_repairs {
            restore_preparation_ingredients(recipe, doc, positions);
            restore_empty_ingredient_sections(recipe, doc, positions);
            restore_shared_method(recipe, positions);
        }
        for line in recipe.sections.iter().flat_map(|s| &s.ingredients) {
            let Some(indices) = positions.get(&normalize(line)) else {
                continue;
            };
            // Repeated source wording with different links has no unique ownership.
            let [i] = indices.as_slice() else {
                continue;
            };
            let block = &doc.blocks[*i];
            for link in &block.links {
                let (target, anchor) = link.href.split_once('#').unwrap_or((&link.href, ""));
                let Some(target_doc) = documents.iter().find(|d| d.path == target) else {
                    continue;
                };
                if !anchor.is_empty() && !target_doc.anchors.iter().any(|a| a == anchor) {
                    continue;
                }
                let Some(indices) = by_doc.get(target) else {
                    continue;
                };
                let [other] = indices.as_slice() else {
                    continue;
                };
                if *other == index {
                    continue;
                }
                if let Some(existing) = recipe
                    .references
                    .iter_mut()
                    .find(|r| r.line == *line && r.title == titles[*other])
                {
                    existing.confidence = crate::RefConfidence::Linked;
                } else {
                    recipe.references.push(crate::RecipeRef {
                        title: titles[*other].clone(),
                        line: line.clone(),
                        confidence: crate::RefConfidence::Linked,
                    });
                }
            }
        }
    }
    // A standalone following photo is a candidate for source review, not enough
    // evidence by itself to attach a hero. Preserve it in SourceDocument.images.
}

/// Replay can restore source ordering even for older string-based model outputs.
pub(crate) fn is_ingredient_block(block: &SourceBlock) -> bool {
    block.classes.split_whitespace().any(is_ingredient_class)
}

fn is_block_container(tag: &str) -> bool {
    matches!(
        tag,
        "address"
            | "article"
            | "aside"
            | "blockquote"
            | "div"
            | "dl"
            | "fieldset"
            | "figcaption"
            | "figure"
            | "footer"
            | "form"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "header"
            | "hr"
            | "li"
            | "main"
            | "nav"
            | "ol"
            | "p"
            | "pre"
            | "section"
            | "table"
            | "tbody"
            | "td"
            | "tfoot"
            | "th"
            | "thead"
            | "tr"
            | "ul"
    )
}
fn is_ingredient_class(class: &str) -> bool {
    matches!(class, "ril" | "rilf") || class == "IL_item" || class.starts_with("IL_item_")
}

/// Restore leading ingredient lists classified as preparation notes. Require an
/// unambiguous authored recipe heading and source positions inside that recipe;
/// a matching ingredient elsewhere in the chapter is not sufficient evidence.
fn restore_preparation_ingredients(
    recipe: &mut crate::CookbookRecipe,
    doc: &SourceDocument,
    positions: &std::collections::HashMap<String, Vec<usize>>,
) {
    let compact = |s: &str| {
        s.chars()
            .filter(|c| !c.is_whitespace())
            .flat_map(char::to_lowercase)
            .collect::<String>()
    };
    let title = compact(&recipe.meta.title);
    let heading = |b: &SourceBlock| {
        matches!(b.tag.as_str(), "h1" | "h2") || b.classes.split_whitespace().any(|c| c == "rt")
    };
    let matches: Vec<_> = doc
        .blocks
        .iter()
        .enumerate()
        .filter(|(_, b)| heading(b) && compact(&b.text) == title)
        .map(|(i, _)| i)
        .collect();
    let [start] = matches.as_slice() else {
        return;
    };
    let end = doc
        .blocks
        .iter()
        .enumerate()
        .skip(start + 1)
        .find(|(_, b)| heading(b))
        .map_or(doc.blocks.len(), |(i, _)| i);
    let position = |text: &str| {
        let found: Vec<_> = positions
            .get(&crate::extractor::normalize_source_whitespace(text))?
            .iter()
            .copied()
            .filter(|&i| i > *start && i < end)
            .collect();
        if let [i] = found.as_slice() {
            Some(*i)
        } else {
            None
        }
    };
    let Some(first) = recipe
        .sections
        .iter()
        .flat_map(|s| &s.ingredients)
        .filter_map(|line| position(line))
        .min()
    else {
        return;
    };
    let mut restored: Vec<_> = recipe
        .meta
        .notes
        .iter()
        .enumerate()
        .filter_map(|(note, text)| {
            let i = position(text)?;
            (i < first
                && is_ingredient_block(&doc.blocks[i])
                && !recipe
                    .sections
                    .iter()
                    .flat_map(|s| &s.ingredients)
                    .any(|line| line == text))
            .then(|| (i, note, text.clone()))
        })
        .collect();
    restored.sort_by_key(|(i, _, _)| *i);
    if restored.is_empty() {
        return;
    }
    let mut moved: std::collections::HashSet<_> =
        restored.iter().map(|(_, note, _)| *note).collect();
    let section_name = restored
        .first()
        .and_then(|(i, _, _)| i.checked_sub(1))
        .and_then(|i| {
            recipe
                .meta
                .notes
                .iter()
                .enumerate()
                .find(|(_, text)| position(text) == Some(i))
                .filter(|(_, _)| !is_ingredient_block(&doc.blocks[i]))
                .map(|(note, text)| {
                    moved.insert(note);
                    text.clone()
                })
        });
    recipe.sections.insert(
        0,
        crate::RecipeSection {
            name: section_name,
            ..crate::RecipeSection::new(
                restored.into_iter().map(|(_, _, text)| text).collect(),
                vec![],
            )
        },
    );
    recipe.meta.notes = std::mem::take(&mut recipe.meta.notes)
        .into_iter()
        .enumerate()
        .filter_map(|(i, text)| (!moved.contains(&i)).then_some(text))
        .collect();
}

fn restore_empty_ingredient_sections(
    recipe: &mut crate::CookbookRecipe,
    doc: &SourceDocument,
    positions: &std::collections::HashMap<String, Vec<usize>>,
) {
    let mut i = 1;
    while i < recipe.sections.len() {
        let section = &recipe.sections[i];
        let source_ingredient = section
            .name
            .as_ref()
            .and_then(|name| positions.get(&crate::extractor::normalize_source_whitespace(name)))
            .is_some_and(|indices| {
                !indices.is_empty()
                    && indices
                        .iter()
                        .all(|&index| is_ingredient_block(&doc.blocks[index]))
            });
        let already_owned = section.name.as_ref().is_some_and(|name| {
            recipe
                .sections
                .iter()
                .flat_map(|s| &s.ingredients)
                .any(|line| {
                    crate::extractor::normalize_source_whitespace(line)
                        == crate::extractor::normalize_source_whitespace(name)
                })
        });
        if section.ingredients.is_empty()
            && section.instructions.is_empty()
            && source_ingredient
            && !already_owned
        {
            let removed = recipe.sections.remove(i);
            if let Some(name) = removed.name {
                recipe.sections[i - 1].ingredients.push(name);
            }
        } else {
            i += 1;
        }
    }
}

fn restore_shared_method(
    recipe: &mut crate::CookbookRecipe,
    positions: &std::collections::HashMap<String, Vec<usize>>,
) {
    let norm = crate::extractor::normalize_source_whitespace;
    let position = |text: &str| {
        let matches = positions.get(&norm(text))?;
        if let [i] = matches.as_slice() {
            Some(*i)
        } else {
            None
        }
    };
    let steps: Option<Vec<_>> = recipe
        .sections
        .iter()
        .flat_map(|s| &s.instructions)
        .map(|s| position(s).map(|i| (i, s.clone())))
        .collect();
    let Some(mut steps) = steps else {
        return;
    };
    let Some(first) = steps.iter().map(|(i, _)| *i).min() else {
        return;
    };
    if recipe
        .sections
        .iter()
        .flat_map(|s| &s.ingredients)
        .any(|s| {
            !positions
                .get(&norm(s))
                .is_some_and(|indices| indices.iter().any(|i| *i < first))
        })
        || recipe
            .sections
            .iter()
            .filter_map(|s| s.name.as_deref())
            .any(|s| position(s).is_none_or(|i| i >= first))
    {
        return;
    }
    steps.sort_by_key(|(i, _)| *i);
    for section in &mut recipe.sections {
        section.instructions.clear();
    }
    let instructions = steps.into_iter().map(|(_, s)| s).collect();
    if let Some(main) = recipe.sections.iter_mut().find(|s| s.name.is_none()) {
        main.instructions = instructions;
    } else {
        recipe
            .sections
            .insert(0, crate::RecipeSection::new(vec![], instructions));
    }
    recipe
        .sections
        .retain(|s| s.name.is_some() || !s.ingredients.is_empty() || !s.instructions.is_empty());
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    #[rstest::rstest]
    #[case(
        "<nav epub:type='toc'><h1>Contents</h1><a href='soup.xhtml'>Soup</a></nav>",
        true
    )]
    #[case("<section role='doc-index'><p>Soup, 12</p></section>", true)]
    #[case(
        "<nav epub:type='toc'><a>Soup</a></nav><p>Cook for ten minutes.</p>",
        false
    )]
    #[case("<div class='index'><p>Soup, 12</p></div>", false)]
    #[case("<nav><a>Soup</a></nav>", false)]
    #[case("<nav epub:type='toc'></nav>", false)]
    fn only_explicit_complete_navigation_is_excluded(#[case] body: &str, #[case] expected: bool) {
        assert_eq!(
            navigation_only_document(&scraper::Html::parse_document(&format!(
                "<html><head><title>Book</title></head><body>{body}</body></html>"
            ))),
            expected
        );
    }

    #[rstest::rstest]
    #[case("toc", "flatbread.xhtml", true)]
    #[case("index", "flatbread.xhtml", true)]
    #[case("toc", "flatbread.xhtml#contents", false)]
    #[case("text", "flatbread.xhtml", false)]
    fn navigation_guide_requires_whole_document_semantics(
        #[case] kind: &str,
        #[case] href: &str,
        #[case] expected: bool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        use std::io::{Cursor, Read, Write};
        let mut original =
            zip::ZipArchive::new(Cursor::new(recipe_epub_fixtures::cookbook_epub()?))?;
        let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for i in 0..original.len() {
            let mut file = original.by_index(i)?;
            if file.name() == "OEBPS/content.opf" {
                let mut package = String::new();
                file.read_to_string(&mut package)?;
                let package = package.replace(
                    "</package>",
                    &format!("<guide><reference type='{kind}' href='{href}'/></guide></package>"),
                );
                archive.start_file(file.name(), zip::write::SimpleFileOptions::default())?;
                archive.write_all(package.as_bytes())?;
            } else {
                archive.raw_copy_file(file)?;
            }
        }
        let navigation = inspect_navigation_documents(&archive.finish()?.into_inner())?;
        assert_eq!(navigation.contains("OEBPS/flatbread.xhtml"), expected);
        assert!(!navigation.contains("OEBPS/tomato-soup.xhtml"));
        Ok(())
    }
    #[test]
    fn source_inspection_includes_styled_divs_without_their_container()
    -> Result<(), Box<dyn std::error::Error>> {
        use std::io::{Cursor, Write};
        let mut original =
            zip::ZipArchive::new(Cursor::new(recipe_epub_fixtures::cookbook_epub()?))?;
        let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for i in 0..original.len() {
            let file = original.by_index(i)?;
            if file.name() == "OEBPS/tomato-soup.xhtml" {
                archive.start_file(file.name(), zip::write::SimpleFileOptions::default())?;
                archive.write_all(br#"<html xmlns="http://www.w3.org/1999/xhtml"><body><div class="recipe"><h1>Tacos<br/>with Sauce</h1><div class="headnote">A quick supper.</div><div class="IL_item"><a href="flatbread.xhtml">dipping sauce</a></div><div class="method_step">Serve with the sauce.</div></div></body></html>"#)?;
            } else {
                archive.raw_copy_file(file)?;
            }
        }
        let docs = inspect_source(&archive.finish()?.into_inner())?;
        let doc = docs
            .iter()
            .find(|doc| doc.path == "OEBPS/tomato-soup.xhtml")
            .ok_or("missing source document")?;
        assert_eq!(
            doc.blocks
                .iter()
                .map(|b| b.text.as_str())
                .collect::<Vec<_>>(),
            [
                "Tacos with Sauce",
                "A quick supper.",
                "dipping sauce",
                "Serve with the sauce."
            ]
        );
        assert_eq!(doc.blocks[2].links[0].text, "dipping sauce");
        Ok(())
    }

    #[test]
    fn source_inspection_keeps_component_headers_and_leaf_markup_without_parent_duplication()
    -> Result<(), Box<dyn std::error::Error>> {
        use std::io::{Cursor, Write};
        let synthetic = br#"<html xmlns="http://www.w3.org/1999/xhtml"><body><div class="recipe"><h1>Harvest Soup</h1><div class="recipe_subtitle"><span>Weeknight <em>variation</em></span></div><div class="IL_subheader_top"><span>For the sauce</span></div><div class="IL_subheader"><span>For the soup</span></div><div class="IL_item"><span>1 <a href="flatbread.xhtml#sauce"><em>tomato</em></a></span></div><div class="method_step"><span>Mix</span> gently.</div><div class="generalprose">Serving advice should stay out of recipe blocks.</div></div></body></html>"#;
        let mut original =
            zip::ZipArchive::new(Cursor::new(recipe_epub_fixtures::cookbook_epub()?))?;
        let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for i in 0..original.len() {
            let file = original.by_index(i)?;
            if file.name() == "OEBPS/tomato-soup.xhtml" {
                archive.start_file(file.name(), zip::write::SimpleFileOptions::default())?;
                archive.write_all(synthetic)?;
            } else {
                archive.raw_copy_file(file)?;
            }
        }
        let docs = inspect_source(&archive.finish()?.into_inner())?;
        let doc = docs
            .iter()
            .find(|doc| doc.path == "OEBPS/tomato-soup.xhtml")
            .ok_or("missing source document")?;
        let expected = [
            ("", "Harvest Soup"),
            ("recipe_subtitle", "Weeknight variation"),
            ("IL_subheader_top", "For the sauce"),
            ("IL_subheader", "For the soup"),
            ("IL_item", "1 tomato"),
            ("method_step", "Mix gently."),
            (
                "generalprose",
                "Serving advice should stay out of recipe blocks.",
            ),
        ];
        assert_eq!(
            doc.blocks
                .iter()
                .map(|block| (block.classes.as_str(), block.text.as_str()))
                .collect::<Vec<_>>(),
            expected
        );
        let dom = scraper::Html::parse_document(std::str::from_utf8(synthetic)?);
        let expected_indices: Vec<_> = dom
            .tree
            .nodes()
            .filter_map(scraper::ElementRef::wrap)
            .enumerate()
            .filter_map(|(index, element)| {
                let text = element
                    .text()
                    .collect::<String>()
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ");
                expected
                    .iter()
                    .any(|(classes, expected_text)| {
                        *classes == element.value().attr("class").unwrap_or("")
                            && *expected_text == text.as_str()
                    })
                    .then_some(index)
            })
            .collect();
        assert_eq!(
            doc.blocks
                .iter()
                .map(|block| block.element_index)
                .collect::<Vec<_>>(),
            expected_indices
        );
        assert_eq!(doc.blocks[4].links.len(), 1);
        assert_eq!(doc.blocks[4].links[0].text, "tomato");
        assert!(
            doc.blocks
                .iter()
                .any(|block| block.text == "Serving advice should stay out of recipe blocks.")
        );
        assert!(
            !doc.blocks
                .iter()
                .any(|block| block.text.contains("For the sauce For the soup"))
        );
        Ok(())
    }

    #[test]
    fn source_inspection_preserves_direct_wrapper_text_around_nested_blocks()
    -> Result<(), Box<dyn std::error::Error>> {
        use std::io::{Cursor, Write};
        let synthetic = br#"<html xmlns="http://www.w3.org/1999/xhtml"><body><div id="mixed" class="wrapper">Opening <em>intro</em>.<p id="nested">Nested block <a href="flatbread.xhtml#sauce">sauce</a>.</p>Closing <a href="flatbread.xhtml#note">note</a>.</div></body></html>"#;
        let mut original =
            zip::ZipArchive::new(Cursor::new(recipe_epub_fixtures::cookbook_epub()?))?;
        let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for i in 0..original.len() {
            let file = original.by_index(i)?;
            if file.name() == "OEBPS/tomato-soup.xhtml" {
                archive.start_file(file.name(), zip::write::SimpleFileOptions::default())?;
                archive.write_all(synthetic)?;
            } else {
                archive.raw_copy_file(file)?;
            }
        }
        let docs = inspect_source(&archive.finish()?.into_inner())?;
        let doc = docs
            .iter()
            .find(|doc| doc.path == "OEBPS/tomato-soup.xhtml")
            .ok_or("missing source document")?;
        let dom = scraper::Html::parse_document(std::str::from_utf8(synthetic)?);
        let element_index = |id| {
            dom.tree
                .nodes()
                .filter_map(scraper::ElementRef::wrap)
                .enumerate()
                .find_map(|(index, element)| {
                    (element.value().attr("id") == Some(id)).then_some(index)
                })
                .ok_or("missing element")
        };
        let mixed = element_index("mixed")?;
        let nested = element_index("nested")?;
        assert_eq!(
            doc.blocks
                .iter()
                .map(|block| block.text.as_str())
                .collect::<Vec<_>>(),
            ["Opening intro.", "Nested block sauce.", "Closing note."]
        );
        assert_eq!(
            doc.blocks[0].id,
            format!("{}:element-{mixed}:text-run-0", doc.path)
        );
        assert_eq!(doc.blocks[0].element_index, mixed);
        assert_eq!(doc.blocks[0].anchor.as_deref(), Some("mixed"));
        assert_eq!(doc.blocks[1].id, format!("{}:element-{nested}", doc.path));
        assert_eq!(doc.blocks[1].element_index, nested);
        assert_eq!(doc.blocks[1].anchor.as_deref(), Some("nested"));
        assert_eq!(doc.blocks[1].links[0].text, "sauce");
        assert_eq!(
            doc.blocks[2].id,
            format!("{}:element-{mixed}:text-run-1", doc.path)
        );
        assert_eq!(doc.blocks[2].element_index, mixed);
        assert_eq!(doc.blocks[2].anchor.as_deref(), Some("mixed"));
        assert_eq!(doc.blocks[2].links[0].text, "note");
        assert_eq!(
            doc.blocks
                .iter()
                .map(|block| block.id.as_str())
                .collect::<std::collections::HashSet<_>>()
                .len(),
            doc.blocks.len()
        );
        assert_eq!(
            doc.blocks
                .iter()
                .filter(|block| block.text.contains("Nested block"))
                .count(),
            1
        );
        Ok(())
    }

    #[rstest::rstest]
    #[case("Tacos", "IL_item", true)]
    #[case("Other recipe", "IL_item", false)]
    #[case("Tacos", "note", false)]
    fn preparation_notes_restore_only_source_proven_ingredients(
        #[case] heading: &str,
        #[case] class: &str,
        #[case] repair: bool,
    ) {
        let mut recipe = crate::CookbookRecipe {
            meta: crate::RecipeMeta {
                title: "Tacos".into(),
                notes: vec![
                    "ADVANCE PREPARATION".into(),
                    "Salsa Roja, for serving".into(),
                ],
                ..Default::default()
            },
            sections: vec![crate::RecipeSection::new(vec!["1 tomato".into()], vec![])],
            source: "book".into(),
            url: "book#chapter".into(),
            references: vec![],
            image: None,
        };
        let doc = SourceDocument {
            path: "chapter".into(),
            images: vec![],
            anchors: vec![],
            blocks: [
                ("h2", "", heading),
                ("p", "heading", "ADVANCE PREPARATION"),
                ("p", class, "Salsa Roja, for serving"),
                ("p", "IL_item", "1 tomato"),
            ]
            .into_iter()
            .enumerate()
            .map(|(i, (tag, classes, text))| SourceBlock {
                id: i.to_string(),
                element_index: i,
                anchor: None,
                tag: tag.into(),
                classes: classes.into(),
                text: text.into(),
                links: vec![],
            })
            .collect(),
        };
        enrich_from_source(
            std::slice::from_mut(&mut recipe),
            std::slice::from_ref(&doc),
        );
        assert_eq!(recipe.sections.len(), if repair { 2 } else { 1 });
        assert_eq!(recipe.meta.notes.len(), if repair { 0 } else { 2 });
        if repair {
            assert_eq!(
                recipe.sections[0].name.as_deref(),
                Some("ADVANCE PREPARATION")
            );
            assert_eq!(recipe.sections[0].ingredients, ["Salsa Roja, for serving"]);
        }
        let once = serde_json::to_value(&recipe).unwrap();
        enrich_from_source(std::slice::from_mut(&mut recipe), &[doc]);
        assert_eq!(serde_json::to_value(recipe).unwrap(), once);
    }
    #[rstest::rstest]
    #[case("IL_item", true)]
    #[case("IL_item_space", true)]
    #[case("rilh", false)]
    #[case("", false)]
    fn empty_section_is_an_ingredient_only_with_source_markup(
        #[case] class: &str,
        #[case] repair: bool,
    ) {
        let mut recipe = crate::CookbookRecipe {
            meta: Default::default(),
            sections: vec![
                crate::RecipeSection::new(vec!["1 tomato".into()], vec![]),
                crate::RecipeSection {
                    name: Some("dipping sauce".into()),
                    ..crate::RecipeSection::new(vec![], vec![])
                },
            ],
            source: "book".into(),
            url: "book#main.xhtml".into(),
            references: vec![],
            image: None,
        };
        let docs = vec![SourceDocument {
            path: "main.xhtml".into(),
            anchors: vec![],
            images: vec![],
            blocks: vec![SourceBlock {
                id: "ingredient".into(),
                element_index: 0,
                anchor: None,
                tag: "p".into(),
                classes: class.into(),
                text: "dipping sauce".into(),
                links: vec![],
            }],
        }];
        enrich_from_source(std::slice::from_mut(&mut recipe), &docs);
        assert_eq!(recipe.sections.len(), if repair { 1 } else { 2 });
        assert_eq!(
            recipe.sections[0].ingredients.len(),
            if repair { 2 } else { 1 }
        );
    }
    #[rstest::rstest]
    #[case("Lemon Cake LEMON CAKE", "Lemon Cake")]
    #[case("Lemon Lemon Cake", "Lemon Lemon Cake")]
    #[case("Other Cake Other Cake", "Other Cake Other Cake")]
    fn replay_repairs_only_exact_doubled_source_headings(
        #[case] title: &str,
        #[case] expected: &str,
    ) {
        let mut recipes = vec![crate::CookbookRecipe {
            meta: crate::RecipeMeta {
                title: title.into(),
                ..Default::default()
            },
            sections: vec![],
            source: "book".into(),
            url: "book#main.xhtml".into(),
            references: vec![],
            image: None,
        }];
        let docs = vec![SourceDocument {
            path: "main.xhtml".into(),
            anchors: vec![],
            images: vec![],
            blocks: vec![SourceBlock {
                id: "title".into(),
                element_index: 0,
                anchor: None,
                tag: "h2".into(),
                classes: String::new(),
                text: "Lemon Cake".into(),
                links: vec![],
            }],
        }];
        enrich_from_source(&mut recipes, &docs);
        assert_eq!(recipes[0].meta.title, expected);
    }

    #[test]
    fn hybrid_preserving_enrichment_does_not_repair_doubled_title() {
        let mut recipe = crate::CookbookRecipe {
            meta: crate::RecipeMeta {
                title: "Lemon Cake LEMON CAKE".into(),
                ..Default::default()
            },
            sections: vec![],
            source: "book".into(),
            url: "book#main.xhtml".into(),
            references: vec![],
            image: None,
        };
        let document = SourceDocument {
            path: "main.xhtml".into(),
            anchors: vec![],
            images: vec![],
            blocks: vec![SourceBlock {
                id: "title".into(),
                element_index: 0,
                anchor: None,
                tag: "h2".into(),
                classes: String::new(),
                text: "Lemon Cake".into(),
                links: vec![],
            }],
        };

        enrich_from_source_preserving_hybrid_ownership(
            std::slice::from_mut(&mut recipe),
            std::slice::from_ref(&document),
        );
        assert_eq!(recipe.meta.title, "Lemon Cake LEMON CAKE");
    }

    #[test]
    fn literal_here_link_resolves_only_a_unique_recipe_target() {
        let recipe = |title: &str, doc: &str, ingredients: Vec<String>| crate::CookbookRecipe {
            meta: crate::RecipeMeta {
                title: title.into(),
                ..Default::default()
            },
            sections: vec![crate::RecipeSection::new(ingredients, vec![])],
            source: "book".into(),
            url: format!("book#{doc}"),
            references: vec![],
            image: None,
        };
        let line = "2 tablespoons sauce (see here)";
        let docs = vec![
            SourceDocument {
                path: "main.xhtml".into(),
                anchors: vec![],
                images: vec![],
                blocks: vec![SourceBlock {
                    id: "line".into(),
                    element_index: 0,
                    anchor: None,
                    tag: "p".into(),
                    classes: String::new(),
                    text: line.into(),
                    links: vec![crate::Link {
                        text: "here".into(),
                        href: "sauce.xhtml#sauce".into(),
                    }],
                }],
            },
            SourceDocument {
                path: "sauce.xhtml".into(),
                anchors: vec!["sauce".into()],
                images: vec![],
                blocks: vec![],
            },
        ];
        let main = recipe("Main dish", "main.xhtml", vec![line.into()]);
        let sauce = recipe(
            "Sauce – full bilingual title",
            "sauce.xhtml",
            vec!["1 tomato".into()],
        );
        let mut recipes = vec![main.clone(), sauce.clone()];
        enrich_from_source(&mut recipes, &docs);
        assert_eq!(recipes[0].references[0].title, sauce.meta.title);
        assert_eq!(
            recipes[0].references[0].confidence,
            crate::RefConfidence::Linked
        );
        let mut ambiguous = vec![main, sauce.clone(), sauce];
        enrich_from_source(&mut ambiguous, &docs);
        assert!(ambiguous[0].references.is_empty());
    }

    #[test]
    fn hybrid_preserving_enrichment_keeps_named_component_methods_after_ingredients() {
        let mut recipe = crate::CookbookRecipe {
            meta: crate::RecipeMeta {
                title: "Component dish".into(),
                ..Default::default()
            },
            sections: vec![
                crate::RecipeSection {
                    name: Some("Salad".into()),
                    ingredients: vec!["1 cup salad base".into()],
                    instructions: vec!["Prepare the salad.".into()],
                },
                crate::RecipeSection::new(vec![], vec!["Serve the components together.".into()]),
                crate::RecipeSection {
                    name: Some("Sauce".into()),
                    ingredients: vec!["1 cup sauce base".into()],
                    instructions: vec!["Make the sauce.".into()],
                },
            ],
            source: "book".into(),
            url: "book#components.xhtml".into(),
            references: vec![],
            image: None,
        };
        let document = SourceDocument {
            path: "components.xhtml".into(),
            anchors: vec![],
            images: vec![],
            blocks: [
                "Component dish",
                "Sauce",
                "1 cup sauce base",
                "Salad",
                "1 cup salad base",
                "Make the sauce.",
                "Prepare the salad.",
                "Serve the components together.",
            ]
            .into_iter()
            .enumerate()
            .map(|(index, text)| SourceBlock {
                id: index.to_string(),
                element_index: index,
                anchor: None,
                tag: "p".into(),
                classes: String::new(),
                text: text.into(),
                links: vec![],
            })
            .collect(),
        };

        enrich_from_source_preserving_hybrid_ownership(
            std::slice::from_mut(&mut recipe),
            std::slice::from_ref(&document),
        );

        assert_eq!(recipe.sections[0].instructions, ["Prepare the salad."]);
        assert_eq!(
            recipe.sections[1].instructions,
            ["Serve the components together."]
        );
        assert_eq!(recipe.sections[2].instructions, ["Make the sauce."]);
    }

    #[test]
    fn indexed_provenance_keeps_caption_and_heading_coordinates_without_text_matching() {
        let chunk = crate::Chunk {
            title_hint: None,
            text: "Spring Soup".into(),
            doc_path: "chapter.xhtml".into(),
            links: vec![],
            images: vec![],
        };
        let heading = crate::SourceElement {
            element_index: 7,
            tag: "h1".into(),
            classes: "recipe_title".into(),
            anchor: Some("soup".into()),
            ancestors: vec![crate::SourceElementCoordinate {
                element_index: 1,
                tag: "body".into(),
                classes: String::new(),
                anchor: None,
            }],
        };
        let indexed = vec![crate::SourceLine {
            anchors: Vec::new(),
            document_line: 12,
            contributors: vec![heading],
            links: vec![],
            images: vec![],
            transformed: false,
        }];
        let documents = vec![SourceDocument {
            path: "chapter.xhtml".into(),
            anchors: vec![],
            images: vec![],
            blocks: vec![
                SourceBlock {
                    id: "caption".into(),
                    element_index: 3,
                    anchor: None,
                    tag: "p".into(),
                    classes: "caption0".into(),
                    text: "Spring Soup".into(),
                    links: vec![crate::Link {
                        text: "Spring Soup".into(),
                        href: "chapter.xhtml#photo".into(),
                    }],
                },
                SourceBlock {
                    id: "heading".into(),
                    element_index: 7,
                    anchor: Some("soup".into()),
                    tag: "h1".into(),
                    classes: "recipe_title".into(),
                    text: "Spring Soup".into(),
                    links: vec![],
                },
            ],
        }];

        let exact = chunk_source_evidence(&chunk, Some(&indexed), &documents).unwrap();
        assert_eq!(exact.lines[0].provenance, "indexed");
        assert_eq!(exact.lines[0].match_state, "exact");
        assert_eq!(exact.lines[0].contributors[0].element_index, 7);
        assert_eq!(exact.elements.len(), 2);
        assert!(exact.blocks.is_empty());

        let fallback = chunk_source_evidence(&chunk, None, &documents).unwrap();
        assert_eq!(fallback.lines[0].provenance, "document_text_fallback");
        assert_eq!(fallback.lines[0].match_state, "ambiguous");
        assert_eq!(fallback.blocks.len(), 2);
        let compact_fallback = fallback.request_projection();
        assert_eq!(compact_fallback["mode"], "document_text_fallback");
        assert_eq!(compact_fallback["lines"][0]["match_state"], "ambiguous");
        assert_eq!(compact_fallback["blocks"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn provenance_retains_inline_ancestors_and_multiple_exact_contributors() {
        let chunk = crate::Chunk {
            title_hint: None,
            text: "Use the herb paste".into(),
            doc_path: "chapter.xhtml".into(),
            links: vec![],
            images: vec![],
        };
        let ancestor = crate::SourceElementCoordinate {
            element_index: 1,
            tag: "body".into(),
            classes: String::new(),
            anchor: None,
        };
        let paragraph = crate::SourceElementCoordinate {
            element_index: 2,
            tag: "p".into(),
            classes: "method_step".into(),
            anchor: Some("step".into()),
        };
        let line = crate::SourceLine {
            anchors: Vec::new(),
            document_line: 4,
            contributors: vec![
                crate::SourceElement {
                    element_index: 3,
                    tag: "em".into(),
                    classes: String::new(),
                    anchor: None,
                    ancestors: vec![ancestor.clone(), paragraph.clone()],
                },
                crate::SourceElement {
                    element_index: 4,
                    tag: "a".into(),
                    classes: "hlink".into(),
                    anchor: None,
                    ancestors: vec![ancestor, paragraph],
                },
            ],
            links: vec![crate::Link {
                text: "herb paste".into(),
                href: "sauce.xhtml#paste".into(),
            }],
            images: vec![crate::ImageRef {
                path: "images/paste.jpg".into(),
                mime: "image/jpeg".into(),
                alt: Some("paste".into()),
            }],
            transformed: true,
        };
        let evidence = chunk_source_evidence(&chunk, Some(&[line]), &[]).unwrap();
        assert_eq!(evidence.lines[0].match_state, "exact");
        assert_eq!(evidence.lines[0].contributors.len(), 2);
        assert!(evidence.lines[0].transformed);
        assert_eq!(evidence.lines[0].link_targets, vec!["sauce.xhtml#paste"]);
        assert_eq!(evidence.lines[0].images[0].path, "images/paste.jpg");
        assert_eq!(evidence.elements.len(), 4);
        let paragraph = evidence
            .elements
            .iter()
            .find(|element| element.element_index == 2)
            .unwrap();
        assert_eq!(paragraph.tag, "p");
        assert_eq!(paragraph.ancestors, vec![1]);

        let compact = evidence.request_projection();
        assert_eq!(compact["document"], "chapter.xhtml");
        assert_eq!(compact["mode"], "indexed");
        let line = &compact["lines"][0];
        assert_eq!(line["line"], 0);
        assert_eq!(line["document_line"], 4);
        assert_eq!(line["elements"], json!([3, 4]));
        assert_eq!(line["link_targets"], json!(["sauce.xhtml#paste"]));
        assert_eq!(line["images"][0]["path"], "images/paste.jpg");
        assert_eq!(line["transformed"], true);
        // The compact table keeps the complete chain without repeating it on
        // every line: em -> p -> body.
        assert_eq!(compact["elements"]["3"]["parent"], 2);
        assert_eq!(compact["elements"]["2"]["parent"], 1);
        assert_eq!(compact["elements"]["1"]["tag"], "body");
        assert!(line.get("provenance").is_none());
        let audit = evidence.audit_projection();
        assert_eq!(
            audit["line_columns"],
            json!([
                "line",
                "document_line",
                "elements",
                "anchors",
                "link_targets",
                "images",
                "transformed"
            ])
        );
        assert_eq!(
            audit["lines"][0],
            json!([
                0,
                4,
                [3, 4],
                [],
                ["sauce.xhtml#paste"],
                line["images"],
                true
            ])
        );
        assert_eq!(
            audit["element_columns"],
            json!(["id", "tag", "classes", "anchor", "parent"])
        );
        for element in audit["elements"].as_array().unwrap() {
            let id = element[0].to_string();
            assert_eq!(element[1], compact["elements"][&id]["tag"]);
            assert_eq!(
                element[2].as_str().unwrap(),
                compact["elements"][&id]["classes"].as_str().unwrap_or("")
            );
            assert_eq!(element[3], compact["elements"][&id]["anchor"]);
            assert_eq!(element[4], compact["elements"][&id]["parent"]);
        }
    }

    #[rstest::rstest]
    #[case(vec![0], vec![1, 2, 3, 4])]
    #[case(vec![1], vec![1, 5, 6, 7])]
    #[case(vec![0, 1], vec![1, 2, 3, 4, 5, 6, 7])]
    #[case(vec![], vec![])]
    fn selected_audit_evidence_retains_only_referenced_ancestor_chains(
        #[case] selected: Vec<usize>,
        #[case] expected_elements: Vec<usize>,
    ) {
        let chunk = crate::Chunk {
            title_hint: None,
            text: "Sauce\nFilling".into(),
            doc_path: "recipe.xhtml".into(),
            links: vec![],
            images: vec![],
        };
        let indexed = [2, 5].map(|parent| {
            let ancestors = vec![
                crate::SourceElementCoordinate {
                    element_index: 1,
                    tag: "body".into(),
                    classes: String::new(),
                    anchor: None,
                },
                crate::SourceElementCoordinate {
                    element_index: parent,
                    tag: "div".into(),
                    classes: "component".into(),
                    anchor: None,
                },
            ];
            crate::SourceLine {
                document_line: parent + 10,
                contributors: vec![crate::SourceElement {
                    element_index: parent + 1,
                    tag: "h2".into(),
                    classes: String::new(),
                    anchor: None,
                    ancestors: ancestors.clone(),
                }],
                anchors: vec![crate::SourceElement {
                    element_index: parent + 2,
                    tag: "a".into(),
                    classes: String::new(),
                    anchor: Some(format!("component-{parent}")),
                    ancestors,
                }],
                links: vec![],
                images: vec![],
                transformed: false,
            }
        });
        let mut evidence = chunk_source_evidence(&chunk, Some(&indexed), &[]).unwrap();
        let original = evidence.clone();
        let full_projection = evidence.audit_projection();
        evidence
            .retain_lines(&selected.iter().copied().collect())
            .unwrap();
        let projected = evidence.audit_projection();
        assert_eq!(
            projected["elements"]
                .as_array()
                .unwrap()
                .iter()
                .map(|row| row[0].as_u64().unwrap() as usize)
                .collect::<Vec<_>>(),
            expected_elements,
        );
        assert_eq!(
            projected["lines"],
            json!(
                selected
                    .iter()
                    .map(|line| &full_projection["lines"][*line])
                    .collect::<Vec<_>>()
            )
        );
        // Projection cannot alter the durable tables or the legacy projection.
        assert_eq!(evidence.elements, original.elements);
        assert_eq!(
            evidence.request_projection()["elements"],
            original.request_projection()["elements"]
        );
        for row in projected["elements"].as_array().unwrap() {
            assert!(
                full_projection["elements"]
                    .as_array()
                    .unwrap()
                    .contains(row)
            );
        }
    }

    #[test]
    fn selecting_evidence_preserves_coordinates_and_tables_and_rejects_missing_lines() {
        let chunk = crate::Chunk {
            title_hint: None,
            text: "Soup\nSimmer gently.\nServe hot.".into(),
            doc_path: "recipe.xhtml".into(),
            links: vec![],
            images: vec![],
        };
        // Legacy evidence must preserve its ambiguity and table representation
        // just as indexed evidence preserves DOM coordinates.
        let mut evidence = chunk_source_evidence(&chunk, None, &[]).unwrap();
        let original = evidence.clone();
        assert!(evidence.retain_lines(&BTreeSet::from([3])).is_err());
        assert_eq!(evidence, original);
        evidence.retain_lines(&BTreeSet::from([1, 2])).unwrap();
        assert_eq!(evidence.lines, original.lines[1..]);
        assert_eq!(evidence.elements, original.elements);
        assert_eq!(evidence.blocks, original.blocks);
        assert_eq!(evidence.document, original.document);
        evidence.retain_lines(&BTreeSet::new()).unwrap();
        assert!(evidence.lines.is_empty());
    }

    #[test]
    fn document_text_fallback_only_normalizes_whitespace() {
        let chunk = crate::Chunk {
            title_hint: None,
            text: "Tea  Cake\nTea Cake!".into(),
            doc_path: "chapter.xhtml".into(),
            links: vec![],
            images: vec![],
        };
        let docs = vec![SourceDocument {
            path: "chapter.xhtml".into(),
            anchors: vec![],
            images: vec![],
            blocks: vec![SourceBlock {
                id: "title".into(),
                element_index: 1,
                anchor: None,
                tag: "h1".into(),
                classes: String::new(),
                text: "Tea\tCake".into(),
                links: vec![],
            }],
        }];
        let evidence = chunk_source_evidence(&chunk, None, &docs).unwrap();
        assert_eq!(evidence.lines[0].match_state, "unique");
        assert_eq!(evidence.lines[1].match_state, "unmatched");
        assert!(evidence.lines[1].contributors.is_empty());
    }
}
