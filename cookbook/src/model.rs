//! The public book tree: what an extraction produces and what every consumer
//! (CLI, desktop app, cubby) reads. Field names are snake_case in JSON; the
//! TypeScript declarations are generated from these structs.

use ingredient::{Confidence, Ingredient};
use recipe_types::RecipeTimes;
use serde::{Deserialize, Serialize};

/// A whole extracted book.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "wasm", derive(tsify_next::Tsify))]
#[cfg_attr(feature = "wasm", tsify(into_wasm_abi, from_wasm_abi))]
pub struct Cookbook {
    /// The model contract version that produced this tree (`CONTRACT_VERSION`).
    pub contract: String,
    pub source: BookSource,
    pub cover: Option<ImageRef>,
    /// In reading order. A leading chapter with `title: None` holds anything
    /// that precedes the first table-of-contents entry.
    pub chapters: Vec<Chapter>,
    /// Every cross-reference between items, deduplicated. Dependencies are the
    /// edges whose `kind` is `ingredient` or `variation`.
    pub edges: Vec<Edge>,
}

impl Cookbook {
    /// Every item in reading order.
    pub fn items(&self) -> impl Iterator<Item = &Item> {
        self.chapters.iter().flat_map(|c| c.items.iter())
    }

    /// Every recipe in reading order.
    pub fn recipes(&self) -> impl Iterator<Item = &Recipe> {
        self.items().filter_map(|item| match item {
            Item::Recipe(r) => Some(r.as_ref()),
            _ => None,
        })
    }

    pub fn item(&self, id: &str) -> Option<&Item> {
        self.items().find(|item| item.id() == id)
    }
}

/// Where the book came from, plus enough to recognize the same file again.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "wasm", derive(tsify_next::Tsify))]
pub struct BookSource {
    /// The caller's label (a file name, a display title).
    pub label: String,
    /// SHA-256 of the EPUB bytes, hex.
    pub sha256: String,
    /// `<dc:title>`.
    pub title: String,
    pub authors: Vec<String>,
    /// `<dc:identifier>` values (ISBNs, URNs), verbatim.
    pub identifiers: Vec<String>,
    pub subjects: Vec<String>,
    pub spine_docs: usize,
    /// Cleaned text lines in the whole book.
    pub lines: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "wasm", derive(tsify_next::Tsify))]
pub struct Chapter {
    /// `"ch00"`, `"ch01"`, … in reading order.
    pub id: String,
    /// The table-of-contents label; `None` for the leading pre-contents chapter.
    pub title: Option<String>,
    pub span: Span,
    /// Prose lines between the chapter heading and its first item.
    pub intro: Vec<String>,
    pub items: Vec<Item>,
}

/// One titled unit of the book. Only recipes carry ingredient lines (a recipe
/// requires at least one); techniques have steps but no ingredients; essays are
/// prose. Everything titled is kept so the whole book is on record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "wasm", derive(tsify_next::Tsify))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Item {
    /// Boxed: a recipe is several times larger than the other variants.
    Recipe(Box<Recipe>),
    Technique(Technique),
    Essay(Essay),
}

impl Item {
    pub fn id(&self) -> &str {
        match self {
            Item::Recipe(r) => &r.id,
            Item::Technique(t) => &t.id,
            Item::Essay(e) => &e.id,
        }
    }

    pub fn title(&self) -> &str {
        match self {
            Item::Recipe(r) => &r.title,
            Item::Technique(t) => &t.title,
            Item::Essay(e) => &e.title,
        }
    }

    pub fn name(&self) -> &str {
        match self {
            Item::Recipe(r) => &r.name,
            Item::Technique(t) => &t.name,
            Item::Essay(e) => &e.name,
        }
    }

    pub fn span(&self) -> &Span {
        match self {
            Item::Recipe(r) => &r.span,
            Item::Technique(t) => &t.span,
            Item::Essay(e) => &e.span,
        }
    }

    pub fn photos(&self) -> &[ImageRef] {
        match self {
            Item::Recipe(r) => &r.photos,
            Item::Technique(t) => &t.photos,
            Item::Essay(e) => &e.photos,
        }
    }

    pub fn photos_mut(&mut self) -> &mut Vec<ImageRef> {
        match self {
            Item::Recipe(r) => &mut r.photos,
            Item::Technique(t) => &mut t.photos,
            Item::Essay(e) => &mut e.photos,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "wasm", derive(tsify_next::Tsify))]
pub struct Recipe {
    /// Deterministic for a given EPUB: `"{spine_index:03}.{doc_line:04}"` of
    /// the title line, so re-extraction yields the same ids.
    pub id: String,
    /// Verbatim from the source.
    pub title: String,
    /// Unique within the book: the title, disambiguated when the book repeats
    /// it (variation qualifier, then chapter, then a counter).
    pub name: String,
    pub meta: RecipeMeta,
    pub sections: Vec<Section>,
    /// Hero photo first.
    pub photos: Vec<ImageRef>,
    pub notes: Vec<Note>,
    /// The parent recipe's id when this is a titled variation with its own
    /// ingredient list.
    pub variant_of: Option<String>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "wasm", derive(tsify_next::Tsify))]
pub struct Technique {
    pub id: String,
    pub title: String,
    pub name: String,
    pub description: Vec<String>,
    pub steps: Vec<Step>,
    pub photos: Vec<ImageRef>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "wasm", derive(tsify_next::Tsify))]
pub struct Essay {
    pub id: String,
    pub title: String,
    pub name: String,
    pub text: Vec<String>,
    pub photos: Vec<ImageRef>,
    pub span: Span,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "wasm", derive(tsify_next::Tsify))]
pub struct RecipeMeta {
    /// Headnote paragraphs, verbatim.
    pub description: Vec<String>,
    pub recipe_yield: Option<String>,
    pub times: Option<RecipeTimes>,
    pub equipment: Vec<String>,
    /// A category printed with the recipe (not the chapter, which is
    /// structural).
    pub category: Option<String>,
    /// The printed page, when the source carries one.
    pub page: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "wasm", derive(tsify_next::Tsify))]
pub struct Section {
    /// `None` for the main or only section.
    pub name: Option<String>,
    pub ingredients: Vec<IngredientLine>,
    pub steps: Vec<Step>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "wasm", derive(tsify_next::Tsify))]
pub struct IngredientLine {
    /// The source line, verbatim.
    pub raw: String,
    /// Global line index in the book's cleaned line stream.
    pub line: usize,
    /// The `ingredient` parser's structured reading of `raw`.
    pub parsed: Ingredient,
    pub confidence: Confidence,
    /// Set when the line names another item in this book.
    #[serde(rename = "ref")]
    pub reference: Option<RecipeRef>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "wasm", derive(tsify_next::Tsify))]
pub struct Step {
    pub text: String,
    pub line: usize,
    pub refs: Vec<RecipeRef>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "wasm", derive(tsify_next::Tsify))]
pub struct Note {
    /// A printed label such as `DO AHEAD` or a variation's title.
    pub label: Option<String>,
    pub text: String,
    pub line: usize,
    pub refs: Vec<RecipeRef>,
}

/// A resolved reference from one item's text to another item in the book.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "wasm", derive(tsify_next::Tsify))]
pub struct RecipeRef {
    pub target_id: String,
    /// The text that carried the reference (link text or matched title).
    pub text: String,
    pub kind: RefKind,
    pub method: RefMethod,
}

/// Where in the referencing item the reference appeared.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "wasm", derive(tsify_next::Tsify))]
#[serde(rename_all = "snake_case")]
pub enum RefKind {
    /// An ingredient line: the true cooking dependency.
    Ingredient,
    Step,
    Note,
    /// `variant_of`.
    Variation,
    /// A photo caption naming another recipe.
    Caption,
}

/// How the reference was resolved, strongest first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "wasm", derive(tsify_next::Tsify))]
#[serde(rename_all = "snake_case")]
pub enum RefMethod {
    /// An internal `<a href>` whose target lies inside the referenced item.
    Anchor,
    /// A printed page number mapped through the book's page markers.
    Page,
    /// The referenced item's title appears in the text.
    Title,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "wasm", derive(tsify_next::Tsify))]
pub struct Edge {
    pub from: String,
    pub to: String,
    pub kind: RefKind,
    pub method: RefMethod,
}

/// An image inside the EPUB archive: a path and mime type, never the bytes.
/// Bytes are read on demand from the still-open book.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "wasm", derive(tsify_next::Tsify))]
#[cfg_attr(feature = "wasm", tsify(into_wasm_abi, from_wasm_abi))]
pub struct ImageRef {
    /// Archive-relative path (resolved against the referencing document).
    pub path: String,
    pub mime: String,
    pub alt: Option<String>,
    /// Caption text printed with the image, when any.
    pub caption: Option<String>,
    /// Global line index the image sits at; `None` for the cover.
    pub line: Option<usize>,
}

/// A half-open range of global line indices, with the spine document and
/// printed page where it starts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "wasm", derive(tsify_next::Tsify))]
pub struct Span {
    pub start: usize,
    /// Exclusive.
    pub end: usize,
    pub doc_path: String,
    pub page: Option<String>,
}

impl Span {
    pub fn contains(&self, line: usize) -> bool {
        self.start <= line && line < self.end
    }
}
