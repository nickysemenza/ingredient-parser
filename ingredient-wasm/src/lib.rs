use std::{collections::HashSet, str::FromStr};

use ingredient::{
    decompose as decompose_str, from_str as parse_ingredient_str,
    ingredient::Ingredient,
    rich_text::{Chunk, RichParser},
    unit::{convert_measure_with_graph, is_valid, make_graph, print_graph, Measure, MeasureKind},
    unit_mapping::{parse_unit_mapping as parse_unit_mapping_internal, ParsedUnitMapping},
    util::truncate_3_decimals,
    Decomposition, Field,
};
use recipe_scraper::{RecipeSection, RecipeTimes, ScrapedRecipe};
use serde::{Deserialize, Serialize};
use tsify_next::Tsify;
use wasm_bindgen::prelude::*;

#[wasm_bindgen(start)]
pub fn init() {
    #[cfg(feature = "console_error_panic_hook")]
    console_error_panic_hook::set_once();

    let mut config = wasm_tracing::WasmLayerConfig::new();
    config.set_max_level(tracing::Level::INFO);
    let _ = wasm_tracing::set_as_global_default_with_config(config);
}

// A pair of measures usable for unit conversion.
type UnitMappingPairs = Vec<(Measure, Measure)>;

// Boundary types: `#[derive(Tsify)]` generates the `.d.ts` from these structs
// (no hand-written `typescript_custom_section`), and the `From<upstream>` impls
// are the compile-time drift check against ingredient-parser / recipe-scraper —
// rename or drop a field upstream and these stop compiling. The one type that
// can't be derived (`AmountKind`, a template-literal union) stays hand-authored.

/// A measurement value + unit (mirrors `Measure`).
#[derive(Tsify, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct WAmount {
    pub unit: String,
    pub value: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upper_value: Option<f64>,
}

impl WAmount {
    fn to_measure(&self) -> Measure {
        match self.upper_value {
            Some(upper) => Measure::with_range(&self.unit, self.value, upper),
            None => Measure::new(&self.unit, self.value),
        }
    }
}

impl From<&Measure> for WAmount {
    fn from(m: &Measure) -> Self {
        Self {
            // `unit().to_str()` (canonical/singular, matching serde) — NOT
            // `unit_as_string()`, which pluralizes for display.
            unit: m.unit().to_str().into_owned(),
            value: m.value(),
            upper_value: m.upper_value(),
        }
    }
}

impl From<Measure> for WAmount {
    fn from(m: Measure) -> Self {
        Self::from(&m)
    }
}

/// Parse-fidelity notes (mirrors `ParseNotes`). Into-only. Always present on a
/// `WIngredient`; review-queue consumers should key off the discrete
/// `fell_back` / `unparsed_digit` booleans rather than a `confidence` threshold.
#[derive(Tsify, Serialize)]
pub struct WParseNotes {
    /// Convenience rollup of the booleans below.
    #[tsify(type = "\"high\" | \"medium\" | \"low\"")]
    pub confidence: ingredient::Confidence,
    /// The parse fell back to a name-only ingredient (no recognizer/core parse).
    /// Always emitted (no `skip_serializing_if`) so the TS `boolean` type matches
    /// runtime exactly — a review-queue consumer reads concrete booleans, never
    /// `undefined`.
    pub fell_back: bool,
    /// A digit was present but no amount was parsed — a likely missed quantity.
    pub unparsed_digit: bool,
}

impl From<&ingredient::ParseNotes> for WParseNotes {
    fn from(n: &ingredient::ParseNotes) -> Self {
        Self {
            confidence: n.confidence,
            fell_back: n.fell_back,
            unparsed_digit: n.unparsed_digit,
        }
    }
}

/// A parsed ingredient (mirrors `Ingredient`). Into-only: never deserialized
/// back from JS, so no `Deserialize`/`default` ceremony.
#[derive(Tsify, Serialize)]
#[tsify(into_wasm_abi)]
pub struct WIngredient {
    pub name: String,
    pub amounts: Vec<WAmount>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modifier: Option<String>,
    /// Whether this ingredient is optional (e.g., wrapped in parentheses).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub optional: bool,
    /// The role the line declares (snake_case `IngredientUsage`, e.g.
    /// "frying_medium" for "oil, for frying").
    #[tsify(
        type = "\"normal\" | \"frying_medium\" | \"pan_grease\" | \"seasoning\" | \"dredging\" | \"garnish\" | \"marinade\""
    )]
    pub usage: ingredient::IngredientUsage,
    /// Non-failing metadata about how this line parsed (confidence + flags).
    pub parse_notes: WParseNotes,
}

impl From<Ingredient> for WIngredient {
    fn from(i: Ingredient) -> Self {
        Self {
            name: i.name,
            amounts: i.amounts.iter().map(WAmount::from).collect(),
            modifier: i.modifier,
            optional: i.optional,
            usage: i.usage,
            parse_notes: WParseNotes::from(&i.parse_notes),
        }
    }
}

/// A unit-conversion pair (mirrors `ParsedUnitMapping`).
#[derive(Tsify, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct WUnitMapping {
    pub a: WAmount,
    pub b: WAmount,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[tsify(optional, type = "string | null")]
    pub source: Option<String>,
}

impl WUnitMapping {
    fn to_pair(&self) -> (Measure, Measure) {
        (self.a.to_measure(), self.b.to_measure())
    }
}

impl From<ParsedUnitMapping> for WUnitMapping {
    fn from(p: ParsedUnitMapping) -> Self {
        Self {
            a: WAmount::from(&p.a),
            b: WAmount::from(&p.b),
            source: p.source,
        }
    }
}

/// `WUnitMapping[]` as a single wasm arg (wasm-bindgen can't take a bare
/// `Vec<TsifyStruct>` parameter); `transparent` → `type WUnitMappings = WUnitMapping[]`.
#[derive(Tsify, Serialize, Deserialize)]
#[tsify(from_wasm_abi)]
#[serde(transparent)]
pub struct WUnitMappings(pub Vec<WUnitMapping>);

impl WUnitMappings {
    fn to_pairs(&self) -> UnitMappingPairs {
        self.0.iter().map(WUnitMapping::to_pair).collect()
    }
}

/// Prep/cook/total times (mirrors `RecipeTimes`). Into-only.
#[derive(Tsify, Serialize)]
pub struct WRecipeTimes {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prep: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cook: Option<String>,
}

impl From<RecipeTimes> for WRecipeTimes {
    fn from(t: RecipeTimes) -> Self {
        Self {
            active: t.active,
            total: t.total,
            prep: t.prep,
            cook: t.cook,
        }
    }
}

/// A recipe component with raw ingredient/instruction lines (mirrors
/// `RecipeSection`). Into-only.
#[derive(Tsify, Serialize)]
pub struct WRecipeSection {
    /// Component label (e.g., "For the sauce"); absent for the main/only section.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub ingredients: Vec<String>,
    pub instructions: Vec<String>,
}

impl From<RecipeSection> for WRecipeSection {
    fn from(s: RecipeSection) -> Self {
        Self {
            name: s.name,
            ingredients: s.ingredients,
            instructions: s.instructions,
        }
    }
}

/// A scraped recipe (mirrors `ScrapedRecipe`). `recipe_yield`/`servings` from the
/// upstream struct are intentionally omitted — the demo never consumes them.
#[derive(Tsify, Serialize)]
#[tsify(into_wasm_abi)]
pub struct WScrapedRecipe {
    /// Recipe components; most recipes have a single unnamed section.
    pub sections: Vec<WRecipeSection>,
    pub name: String,
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub times: Option<WRecipeTimes>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub equipment: Vec<String>,
}

impl From<ScrapedRecipe> for WScrapedRecipe {
    fn from(r: ScrapedRecipe) -> Self {
        Self {
            sections: r.sections.into_iter().map(WRecipeSection::from).collect(),
            name: r.name,
            url: r.url,
            image: r.image,
            description: r.description,
            times: r.times.map(WRecipeTimes::from),
            category: r.category,
            notes: r.notes,
            equipment: r.equipment,
        }
    }
}

/// One span of measurement-aware instruction text (mirrors `Chunk`). Into-only.
#[derive(Tsify, Serialize)]
#[serde(tag = "kind", content = "value")]
pub enum RichItem {
    Text(String),
    Ing(String),
    Measure(Vec<WAmount>),
}

impl From<Chunk> for RichItem {
    fn from(c: Chunk) -> Self {
        match c {
            Chunk::Text(t) => RichItem::Text(t),
            Chunk::Ing(i) => RichItem::Ing(i),
            Chunk::Measure(ms) => RichItem::Measure(ms.iter().map(WAmount::from).collect()),
        }
    }
}

/// `RichItem[]` (`transparent` → `type RichItems = RichItem[]`). Into-only.
#[derive(Tsify, Serialize)]
#[tsify(into_wasm_abi)]
#[serde(transparent)]
pub struct RichItems(pub Vec<RichItem>);

/// Which output field a decomposition segment became (mirrors `Field`). Renders
/// as the TS string union `"amount" | "name" | "modifier"`.
#[derive(Tsify, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum WField {
    Amount,
    Name,
    Modifier,
}

impl From<Field> for WField {
    fn from(f: Field) -> Self {
        match f {
            Field::Amount => WField::Amount,
            Field::Name => WField::Name,
            Field::Modifier => WField::Modifier,
        }
    }
}

/// One contiguous chunk of the decomposed line: either a labeled field span or
/// unlabeled gap text. The segments concatenate back to the whole source, so JS
/// renders them in order with no byte-offset math (the Rust spans are UTF-8 byte
/// ranges, which don't map to JS UTF-16 indices). Into-only.
#[derive(Tsify, Serialize)]
pub struct WSegment {
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field: Option<WField>,
}

/// How the grammar carved a line into fields (mirrors `Decomposition`), as an
/// ordered list of segments covering the whole source. `segments` carries no
/// labeled entries when a recognizer or name-only fallback produced the result.
/// Into-only.
#[derive(Tsify, Serialize)]
#[tsify(into_wasm_abi)]
pub struct WDecomposition {
    pub source: String,
    pub segments: Vec<WSegment>,
}

impl From<Decomposition> for WDecomposition {
    fn from(d: Decomposition) -> Self {
        // Walk the sorted, non-overlapping spans, emitting any gap text before
        // each labeled span, then the span itself, then the trailing gap.
        let mut segments = Vec::new();
        let mut prev_end = 0usize;
        for span in &d.spans {
            if span.range.start > prev_end {
                segments.push(WSegment {
                    text: d.source[prev_end..span.range.start].to_string(),
                    field: None,
                });
            }
            segments.push(WSegment {
                text: span.text.clone(),
                field: Some(span.field.into()),
            });
            prev_end = span.range.end;
        }
        if prev_end < d.source.len() {
            segments.push(WSegment {
                text: d.source[prev_end..].to_string(),
                field: None,
            });
        }
        WDecomposition {
            source: d.source,
            segments,
        }
    }
}

// Hand-authored boundary types that can't be derived: `AmountKind` (a
// template-literal union) and the nutrient-conversion result record.
#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(typescript_type = "AmountKind")]
    pub type WAmountKind;

    #[wasm_bindgen(typescript_type = "Record<string, WAmount | null>")]
    pub type WNutrientAmounts;
}

// Keep in lockstep with `MeasureKind::to_str()`: the parameterized variants
// render as `other:<unit>` / `nutrient:<unit>` (a bare "other" is never
// produced — `Unit::Whole` is `other:whole`). Pinned by `amount_kind_strings`.
#[wasm_bindgen(typescript_custom_section)]
const HAND_AUTHORED_TS: &str = r#"
type AmountKind = "weight" | "volume" | "money" | "calories" | "time" | "temperature" | "length" | `other:${string}` | `nutrient:${string}`;
"#;

fn from_js<T: for<'de> Deserialize<'de>>(v: impl Into<JsValue>, ctx: &str) -> Result<T, String> {
    serde_wasm_bindgen::from_value(v.into()).map_err(|e| format!("Failed to parse {ctx}: {e}"))
}

fn to_js<T: Serialize>(v: &T, ctx: &str) -> Result<JsValue, String> {
    serde_wasm_bindgen::to_value(v).map_err(|e| format!("Failed to serialize {ctx}: {e}"))
}

// Public API

#[wasm_bindgen]
pub fn parse_ingredient(input: &str) -> WIngredient {
    parse_ingredient_str(input).into()
}

/// Decompose a line into ordered `{text, field?}` segments showing how the
/// grammar carved it into amount / name / modifier spans (for the demo's
/// diagnostic-style annotation).
#[wasm_bindgen]
pub fn decompose_ingredient(input: &str) -> WDecomposition {
    decompose_str(input).into()
}

#[wasm_bindgen]
pub fn format_amount(amount: WAmount) -> String {
    amount.to_measure().to_string()
}

#[wasm_bindgen]
pub fn format_amount_value(amount: WAmount) -> f64 {
    truncate_3_decimals(amount.value)
}

#[wasm_bindgen]
pub fn amount_kind(amount: WAmount) -> Result<WAmountKind, String> {
    let kind = amount.to_measure().kind();
    to_js(&kind.to_str(), "amount kind").map(Into::into)
}

#[wasm_bindgen]
pub fn is_valid_unit(unit: &str, extra_units: Vec<String>) -> bool {
    is_valid(&HashSet::from_iter(extra_units), unit)
}

#[wasm_bindgen]
pub fn conv_amount_to_kind(
    mappings: WUnitMappings,
    target_kind: WAmountKind,
    amount: WAmount,
) -> Result<WAmount, String> {
    let pairs = mappings.to_pairs();
    let measure = amount.to_measure();
    let kind_str: String = from_js(target_kind, "amount kind")?;
    // `MeasureKind::from_str` is infallible (unknown strings map to Other);
    // the fallback arm is unreachable.
    let kind = MeasureKind::from_str(&kind_str).unwrap_or(MeasureKind::Other(kind_str));

    measure
        .convert_measure_via_mappings(kind.clone(), &pairs)
        .ok_or_else(|| format!("Failed to convert '{measure}' to '{kind}'"))
        .map(WAmount::from)
}

#[wasm_bindgen]
pub fn conv_amount_to_unit(
    mappings: WUnitMappings,
    target_unit: String,
    amount: WAmount,
) -> Result<WAmount, String> {
    let pairs = mappings.to_pairs();
    let measure = amount.to_measure();
    let kind = MeasureKind::Nutrient(target_unit.clone());

    measure
        .convert_measure_via_mappings(kind, &pairs)
        .ok_or_else(|| format!("Failed to convert to '{target_unit}'"))
        .map(WAmount::from)
}

/// Convert an amount to multiple nutrient targets in a single call (graph built
/// once). Returns an object keyed by target — the converted amount, or null when
/// no conversion path exists.
#[wasm_bindgen]
pub fn conv_amount_to_nutrients(
    mappings: WUnitMappings,
    nutrient_targets: Vec<String>,
    amount: WAmount,
) -> Result<WNutrientAmounts, String> {
    use wasm_bindgen::JsCast;

    let measure = amount.to_measure();
    let graph = make_graph(&mappings.to_pairs());

    let result = js_sys::Object::new();
    for target in nutrient_targets {
        let kind = MeasureKind::Nutrient(target.clone());
        let converted = convert_measure_with_graph(&measure, kind, &graph);

        let js_value = match converted {
            Some(m) => to_js(&WAmount::from(&m), "amount")?,
            None => JsValue::NULL,
        };

        js_sys::Reflect::set(&result, &JsValue::from_str(&target), &js_value)
            .map_err(|_| "Failed to set property on result object")?;
    }

    Ok(result.unchecked_into())
}

#[wasm_bindgen]
pub fn graph_unit_mappings(mappings: WUnitMappings) -> String {
    print_graph(make_graph(&mappings.to_pairs()))
}

#[wasm_bindgen]
pub fn parse_unit_mapping(input: String) -> Result<WUnitMapping, String> {
    Ok(parse_unit_mapping_internal(&input)?.into())
}

#[wasm_bindgen]
pub fn scrape(body: String, url: String) -> Result<WScrapedRecipe, String> {
    recipe_scraper::scrape(body.as_str(), &url)
        .map_err(|e| format!("Failed to scrape recipe: {e}"))
        .map(WScrapedRecipe::from)
}

#[wasm_bindgen]
pub fn parse_rich_text(text: String, ingredient_names: Vec<String>) -> Result<RichItems, String> {
    RichParser::new(ingredient_names)
        .parse(&text)
        .map_err(|e| e.to_string())
        .map(|chunks| RichItems(chunks.into_iter().map(RichItem::from).collect()))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    /// The hand-authored `AmountKind` TS union must stay in lockstep with
    /// `MeasureKind::to_str()`. The parameterized kinds render as
    /// `other:<unit>` / `nutrient:<unit>` — a bare "other" is never produced
    /// (even `whole` is `other:whole`), so the union must carry the
    /// `other:${string}` arm, not a bare "other".
    #[test]
    fn amount_kind_strings_match_ts_union() {
        let kind_str = |unit: &str| {
            WAmount {
                unit: unit.to_string(),
                value: 1.0,
                upper_value: None,
            }
            .to_measure()
            .kind()
            .to_str()
            .into_owned()
        };
        assert_eq!(kind_str("g"), "weight");
        assert_eq!(kind_str("cup"), "volume");
        assert_eq!(kind_str("whole"), "other:whole");
        assert_eq!(kind_str("clove"), "other:clove");
        assert_eq!(kind_str("g protein"), "nutrient:g protein");
        // Every static union member must be producible too.
        for (unit, want) in [
            ("$", "money"),
            ("kcal", "calories"),
            ("minute", "time"),
            ("fahrenheit", "temperature"),
            ("inch", "length"),
        ] {
            assert_eq!(kind_str(unit), want, "unit: {unit}");
        }
    }

    /// The decomposition segments must concatenate back to the source exactly
    /// (no dropped/duplicated text) and carry the right field labels — this is
    /// what lets the JS side render without any byte-offset math.
    #[test]
    fn decomposition_segments_cover_source() {
        let w: WDecomposition = decompose_str("2¼ cups all-purpose flour, sifted").into();
        let joined: String = w.segments.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(joined, w.source, "segments must reconstruct the source");

        // Labeled segments, in order, are the grammar fields.
        let labeled: Vec<(&str, &str)> = w
            .segments
            .iter()
            .filter_map(|s| match &s.field {
                Some(WField::Amount) => Some(("amount", s.text.as_str())),
                Some(WField::Name) => Some(("name", s.text.as_str())),
                Some(WField::Modifier) => Some(("modifier", s.text.as_str())),
                None => None,
            })
            .collect();
        assert_eq!(
            labeled,
            vec![
                ("amount", "2¼ cups"),
                ("name", "all-purpose flour"),
                ("modifier", "sifted"),
            ]
        );
    }

    /// A recognizer-handled line has no grammar carve → all segments unlabeled,
    /// still reconstructing the source.
    #[test]
    fn recognizer_line_has_no_labeled_segments() {
        let w: WDecomposition = decompose_str("Juice of 1 lemon").into();
        let joined: String = w.segments.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(joined, w.source);
        assert!(w.segments.iter().all(|s| s.field.is_none()));
    }
}
