//! Lightweight recipe data types shared across the workspace and with external
//! consumers.
//!
//! These are plain serde data structures — no parser, scraper, or EPUB/LLM
//! dependencies — so the recipe *shape* (the JSON contract emitted by the web
//! scraper and the EPUB extractor) can be depended on without pulling in any of
//! the heavy crates. The parser-aware "parsed" variants and the methods that run
//! the `ingredient` parser live in `recipe-scraper` / `recipe-epub`, which
//! re-export these types so existing call sites are unchanged.

use serde::{Deserialize, Serialize};

/// Structured yield from a recipe (e.g., "12 pancakes").
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct RecipeYield {
    pub value: f64,
    pub unit: String,
}

/// Printed times. Any field may be absent. Shared workspace-wide: the web scraper
/// fills it from JSON-LD ISO-8601 durations, `recipe-epub` from the model's output.
/// `active` has no JSON-LD source, so it stays `None` for scraped recipes.
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
pub struct RecipeTimes {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prep: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cook: Option<String>,
}

impl RecipeTimes {
    /// `true` when every field is absent (so callers can collapse to `None`).
    pub fn is_empty(&self) -> bool {
        self.active.is_none() && self.total.is_none() && self.prep.is_none() && self.cook.is_none()
    }
}

/// One component of a recipe (e.g. "For the sauce"). A recipe is fundamentally
/// metadata + sections; the common case is a single unnamed section. Ingredient
/// and instruction lines are raw strings — the core `ingredient` parser
/// (in `recipe-scraper`/`recipe-epub`) structures them downstream.
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Default)]
pub struct RecipeSection {
    /// Component label; `None` for the main/only section.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub ingredients: Vec<String>,
    #[serde(default)]
    pub instructions: Vec<String>,
}

impl RecipeSection {
    /// An unnamed section — the common single-section case.
    pub fn new(ingredients: Vec<String>, instructions: Vec<String>) -> Self {
        Self {
            name: None,
            ingredients,
            instructions,
        }
    }
}

/// Recipe metadata (everything except the component sections). Flattened into the
/// extractor's recipe shape and the public output types so they all serialize as
/// one flat object.
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
pub struct RecipeMeta {
    pub title: String,
    /// Headnote / intro blurb.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Yield/servings line, e.g. "Makes 1 loaf".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recipe_yield: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub times: Option<RecipeTimes>,
    /// Special-equipment lines.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub equipment: Vec<String>,
    /// Do-ahead/make-ahead notes, tips, "serve with" suggestions.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
    /// Chapter/category within the book.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    /// Page number, if printed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page: Option<String>,
}

/// How confident we are that an ingredient line references another recipe.
//
// NOTE: deliberately NOT `#[non_exhaustive]`. `recipe-epub` constructs
// `RefConfidence::Linked`/`TitleMatch` externally (recipe-epub/src/lib.rs:458,460,675);
// `#[non_exhaustive]` forbids external variant construction (E0639), which would
// break that crate. A small closed enum that downstreams build by value, so the
// non-exhaustive hardening doesn't apply here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RefConfidence {
    /// Backed by an EPUB internal hyperlink (`<a href="#…">`) — the author's
    /// literal pointer to the target recipe (Layer 2).
    Linked,
    /// The target recipe's title appears in the ingredient line (Layer 1).
    TitleMatch,
}

/// A detected reference from one recipe to another in the same cookbook, e.g.
/// the ingredient line "1 recipe The Only Piecrust (this page)" referencing the
/// "The Only Piecrust" recipe.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecipeRef {
    /// The referenced recipe's title (as that recipe reports it).
    pub title: String,
    /// The verbatim ingredient line the reference was found in.
    pub line: String,
    /// How the reference was detected.
    pub confidence: RefConfidence,
}

/// A fully assembled recipe (raw verbatim strings) with provenance. The EPUB
/// extractor's output type; `recipe-epub`'s `CookbookRecipeExt::parse` structures
/// the lines with the core parser.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CookbookRecipe {
    pub meta: RecipeMeta,
    pub sections: Vec<RecipeSection>,
    /// The book this came from (the caller's `source` label).
    pub source: String,
    /// `source#doc_path` for traceability.
    pub url: String,
    /// Other recipes in the same book this one references as ingredients.
    /// Derived after assembly (not extracted by the LLM, not part of the cache
    /// payload), so it defaults to empty when deserializing older cached JSON.
    #[serde(default)]
    pub references: Vec<RecipeRef>,
}
