//! Lightweight recipe data types shared across the workspace and with external
//! consumers.
//!
//! These are plain serde data structures — no parser or extraction dependencies —
//! so the recipe *shape* can be depended on without pulling in any of the heavy
//! crates. The parser-aware "parsed" variants and the methods that run the
//! `ingredient` parser live in the crates that produce recipes, which re-export
//! these types so existing call sites are unchanged.

use serde::{Deserialize, Serialize};

/// Deserialize a `Vec<T>` tolerantly: an explicit JSON `null` becomes an empty
/// vec, exactly like a missing key. `#[serde(default)]` alone does NOT cover
/// this — `default` fills a *missing* key, but a present-but-`null` value is
/// still handed to the `Vec` deserializer, which rejects it with "invalid type:
/// null, expected a sequence". Pair this with `#[serde(default)]` so missing,
/// null, and a real array all yield a vec.
///
/// This hardens the recipe shape against malformed producer output: an LLM-backed
/// extractor occasionally emits `"instructions": null` (or omits a required array
/// field), which would otherwise sink the whole record. Kept
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
/// fills it from JSON-LD ISO-8601 durations, cookbook extraction from the model's
/// output. `active` has no JSON-LD source, so it stays `None` for scraped recipes.
///
/// Each time is carried twice: the `*_minutes` field is the same duration as a
/// number, so consumers can sort and filter without re-parsing the prose. The
/// strings stay the display form (they preserve how the source wrote it); the
/// minutes are derived and may be `None` where the prose couldn't be parsed
/// confidently, so a present string does NOT imply a present count.
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "wasm", derive(tsify_next::Tsify))]
pub struct RecipeTimes {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prep: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cook: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_minutes: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_minutes: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prep_minutes: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cook_minutes: Option<u32>,
}

impl RecipeTimes {
    /// `true` when every field is absent (so callers can collapse to `None`).
    /// The minute counts count: a row carrying only numbers is still a time.
    pub fn is_empty(&self) -> bool {
        self.active.is_none()
            && self.total.is_none()
            && self.prep.is_none()
            && self.cook.is_none()
            && self.active_minutes.is_none()
            && self.total_minutes.is_none()
            && self.prep_minutes.is_none()
            && self.cook_minutes.is_none()
    }
}

/// One component of a recipe (e.g. "For the sauce"). A recipe is fundamentally
/// metadata + sections; the common case is a single unnamed section. Ingredient
/// and instruction lines are raw strings — the core `ingredient` parser
/// structures them downstream.
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
/// public output types so they all serialize as one flat object.
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
