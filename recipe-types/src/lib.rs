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

/// Deserialize a `Vec<T>` tolerantly: an explicit JSON `null` becomes an empty
/// vec, exactly like a missing key. `#[serde(default)]` alone does NOT cover
/// this — `default` fills a *missing* key, but a present-but-`null` value is
/// still handed to the `Vec` deserializer, which rejects it with "invalid type:
/// null, expected a sequence". Pair this with `#[serde(default)]` so missing,
/// null, and a real array all yield a vec.
///
/// This hardens the recipe shape against malformed LLM tool output: the EPUB
/// extractor's model occasionally emits `"instructions": null` (or omits a
/// required array field), which would otherwise sink the whole chunk. Kept
/// dependency-free (a serde `Visitor`, no `serde_json`) so this crate stays the
/// minimal shared contract.
pub fn null_as_empty_vec<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    use std::marker::PhantomData;

    use serde::de::{self, SeqAccess, Visitor};

    struct LenientVec<T>(PhantomData<T>);

    impl<'de, T: Deserialize<'de>> Visitor<'de> for LenientVec<T> {
        type Value = Vec<T>;

        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("an array or null")
        }

        // An explicit JSON `null` (serde_json calls `visit_unit`) → empty.
        fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
            Ok(Vec::new())
        }

        // `Option`-style null paths, for completeness across data formats.
        fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
            Ok(Vec::new())
        }

        fn visit_some<D2: serde::Deserializer<'de>>(self, d: D2) -> Result<Self::Value, D2::Error> {
            null_as_empty_vec(d)
        }

        fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
            let mut out = Vec::new();
            while let Some(item) = seq.next_element()? {
                out.push(item);
            }
            Ok(out)
        }
    }

    deserializer.deserialize_any(LenientVec(PhantomData))
}

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
    // `default` + `null_as_empty_vec`: the LLM extractor sometimes omits
    // `ingredients` entirely or sends it as `null` (e.g. an instructions-only
    // block it mis-shaped as a section). Both now yield an empty list instead of
    // failing the whole chunk's deserialize.
    #[serde(default, deserialize_with = "null_as_empty_vec")]
    pub ingredients: Vec<String>,
    #[serde(default, deserialize_with = "null_as_empty_vec")]
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
    #[serde(
        default,
        deserialize_with = "null_as_empty_vec",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub equipment: Vec<String>,
    /// Do-ahead/make-ahead notes, tips, "serve with" suggestions.
    #[serde(
        default,
        deserialize_with = "null_as_empty_vec",
        skip_serializing_if = "Vec::is_empty"
    )]
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

/// A reference to an image embedded in an EPUB — the archive-relative path of the
/// resource plus its MIME type, *not* the bytes. The bytes are materialized lazily
/// from the still-available EPUB (the file is hundreds of MB; embedding every
/// hero/cover would bloat the JSON cache and CLI output). A book cover and each
/// recipe's hero photo are both expressed as one of these.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImageRef {
    /// Path of the image resource within the EPUB archive (resolved relative to
    /// the referencing content document).
    pub path: String,
    /// MIME type, e.g. `image/jpeg`.
    pub mime: String,
    /// The `<img alt>` text, when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alt: Option<String>,
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
    /// The recipe's hero photo, if one was found near its title in the EPUB.
    /// Derived after assembly (not extracted by the LLM, not part of the cache
    /// payload), so it defaults to `None` when deserializing older cached JSON.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<ImageRef>,
}
