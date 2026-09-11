//! Ground truth for the shape fixtures.
//!
//! Every fixture ships the answer key next to the bytes so a consuming test can
//! assert against [`Expected`] instead of restating counts it would have to keep
//! in sync by hand. The crate's own tests re-derive the ingredient and step
//! counts from the asset text, so an edit to an asset that is not mirrored here
//! fails in this crate rather than downstream.

/// A reference from one recipe to another place in the book.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CrossRef {
    /// Visible reference text, e.g. `"this page"` or `"page 190"`.
    pub text: &'static str,
    /// The `href` when [`linked`](Self::linked), otherwise the bare printed
    /// page reference the prose gives instead.
    pub target: &'static str,
    /// `false` for a plain-text page reference with no `<a>` element.
    pub linked: bool,
}

/// What a reader should find for one recipe-shaped block of content.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExpectedRecipe {
    /// Human-readable title. Some fixtures shred the title across `<small>`
    /// runs, so this is the cleaned form, not the raw DOM text.
    pub title: &'static str,
    /// Spine documents the recipe's content spans, in order. More than one
    /// means the reader has to stitch across a page-split boundary.
    pub docs: &'static [&'static str],
    /// Number of ingredient paragraphs.
    pub ingredient_lines: usize,
    /// Number of procedure paragraphs. Excludes `DO AHEAD`/`NOTE` trailers.
    pub steps: usize,
    /// True when the block is a variation on the preceding recipe rather than
    /// a standalone one.
    pub is_variation: bool,
    /// True when the block is prose with no ingredient list: an essay,
    /// sidebar, or back-matter section a reader must not emit as a recipe.
    pub is_essay: bool,
    /// References out of this recipe, in document order.
    pub cross_references: &'static [CrossRef],
}

/// Ground truth for a whole fixture book.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Expected {
    /// `<dc:title>` of the generated package.
    pub book_title: &'static str,
    /// `<dc:identifier>` of the generated package.
    pub identifier: &'static str,
    /// Spine document paths, relative to `OEBPS/`, in spine order.
    pub spine: &'static [&'static str],
    /// Recipe-shaped blocks in reading order, essays and variations included.
    pub recipes: &'static [ExpectedRecipe],
}

impl Expected {
    /// Blocks that are neither an essay nor a variation.
    pub fn standalone_recipes(self) -> impl Iterator<Item = &'static ExpectedRecipe> {
        self.recipes
            .iter()
            .filter(|recipe| !recipe.is_essay && !recipe.is_variation)
    }
}
