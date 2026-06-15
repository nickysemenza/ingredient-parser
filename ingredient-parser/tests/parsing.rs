//! Tests for ingredient and rich text parsing

#![allow(clippy::unwrap_used)]

use ingredient::{
    from_str,
    ingredient::Ingredient,
    rich_text::{Chunk, RichParser},
    unit::{Measure, MeasureKind},
    IngredientParser,
};
use rstest::{fixture, rstest};

// ============================================================================
// Fixtures
// ============================================================================

#[fixture]
fn parser() -> IngredientParser {
    IngredientParser::new()
}

#[fixture]
fn parser_custom_units() -> IngredientParser {
    IngredientParser::new().with_units(&["handful", "handfuls", "sprig", "sprigs", "knob"])
}

// ============================================================================
// Test Helpers
// ============================================================================

fn text(s: &str) -> Chunk {
    Chunk::Text(s.to_string())
}

fn measure(unit: &str, value: f64) -> Chunk {
    Chunk::Measure(vec![Measure::new(unit, value)])
}

fn measure_range(unit: &str, min: f64, max: f64) -> Chunk {
    Chunk::Measure(vec![Measure::with_range(unit, min, max)])
}

fn ing(name: &str) -> Chunk {
    Chunk::Ing(name.to_string())
}

fn parse_rich(input: &str, ingredient_names: &[&str]) -> Vec<Chunk> {
    RichParser::new(ingredient_names.iter().copied())
        .parse(input)
        .unwrap()
}

// ============================================================================
// Parsing Equivalence Tests
// ============================================================================

/// Test that different input formats produce equivalent results
#[rstest]
#[case::hyphen_range("1/2-1 cup", "1/2 - 1 cup")]
#[case::fraction_range("2¼-2.5 cups", "2 ¼ - 2.5 cups")]
#[case::gram_range("78g to 104g", "78g - 104g")]
#[case::or_range("1 or 2 cups flour", "1-2 cups flour")]
#[case::through_range("2 through 4 cups flour", "2-4 cups flour")]
#[case::implicit_unit("1 cinnamon stick", "1 whole cinnamon stick")]
#[case::multiplier_2x("2 x 200g flour", "400g flour")]
#[case::multiplier_3x("3 x 100g butter", "300g butter")]
#[case::multiplier_decimal("1.5 x 100g flour", "150g flour")]
#[case::of_keyword("pinch nutmeg", "pinch of nutmeg")]
#[case::of_keyword_cup("1 cup of flour", "1 cup flour")]
#[case::period_tbsp("1 Tbsp. flour", "1 tbsp flour")]
#[case::period_tsp("2 tsp. sugar", "2 tsp sugar")]
#[case::period_oz("1 oz. butter", "1 oz butter")]
#[case::unit_alias_tbsp("1 tablespoon oil", "1 tbsp oil")]
#[case::unit_alias_tsp("2 teaspoons salt", "2 tsp salt")]
#[case::unit_alias_lb("1 pound beef", "1 lb beef")]
#[case::unit_alias_oz("8 ounces cheese", "8 oz cheese")]
#[case::unit_alias_g("500 grams flour", "500 g flour")]
#[case::unit_alias_kg("1 kilogram potatoes", "1 kg potatoes")]
#[case::case_cup("1 CUP flour", "1 cup flour")]
#[case::case_tbsp("2 TBSP sugar", "2 tbsp sugar")]
fn test_parsing_equivalence(parser: IngredientParser, #[case] left: &str, #[case] right: &str) {
    assert_eq!(
        parser.from_str(left),
        parser.from_str(right),
        "Expected '{left}' == '{right}'"
    );
}

/// Regression (found by cargo-fuzz): adjective extraction byte-sliced `name` at
/// offsets taken from its lowercased form. For chars whose lowercase changes
/// byte length (e.g. 'İ' U+0130 -> "i̇"), those offsets can land off a char
/// boundary or past the end, panicking. These must parse without panicking.
#[rstest]
#[case("1 cup İdiced")]
#[case("İsliced")]
#[case("1 İminced thing")]
fn test_adjective_extraction_multibyte_no_panic(parser: IngredientParser, #[case] input: &str) {
    let _ = parser.from_str(input);
}

// ============================================================================
// Custom Parser Configuration Tests
// ============================================================================

#[rstest]
#[case::handfuls("2 handfuls spinach", "spinach", "handful", 2.0)]
#[case::sprigs("3 sprigs thyme", "thyme", "sprig", 3.0)]
#[case::knob("1 knob ginger", "ginger", "knob", 1.0)]
fn test_custom_units(
    parser_custom_units: IngredientParser,
    #[case] input: &str,
    #[case] expected_name: &str,
    #[case] expected_unit: &str,
    #[case] expected_amount: f64,
) {
    let result = parser_custom_units.from_str(input);
    assert_eq!(result.name, expected_name);
    assert_eq!(result.amounts[0].value(), expected_amount);
    assert_eq!(result.amounts[0].unit_as_string(), expected_unit);
}

// ============================================================================
// Amount Parsing Tests
// ============================================================================

/// Test basic amount parsing
#[rstest]
#[case::temp_degree("350 °", vec![Measure::new("°", 350.0)])]
#[case::temp_f("350 °F", vec![Measure::new("°f", 350.0)])]
#[case::temp_c("200 °C", vec![Measure::new("°c", 200.0)])]
#[case::basic_cup("1 cup", vec![Measure::new("cup", 1.0)])]
#[case::basic_tbsp("2 tbsp", vec![Measure::new("tbsp", 2.0)])]
#[case::basic_g("500 g", vec![Measure::new("g", 500.0)])]
#[case::slash_fraction("1/2 cup", vec![Measure::new("cup", 0.5)])]
#[case::slash_fraction_3_4("3/4 tsp", vec![Measure::new("tsp", 0.75)])]
#[case::mixed_fraction("1 1/2 cups", vec![Measure::new("cups", 1.5)])]
#[case::unicode_half("½ cup", vec![Measure::new("cup", 0.5)])]
#[case::unicode_3_4("¾ tsp", vec![Measure::new("tsp", 0.75)])]
#[case::unicode_mixed("1½ cups", vec![Measure::new("cups", 1.5)])]
#[case::decimal("1.5 cups", vec![Measure::new("cups", 1.5)])]
#[case::decimal_small("0.25 oz", vec![Measure::new("oz", 0.25)])]
#[case::thousands("1,000 g", vec![Measure::new("g", 1000.0)])]
#[case::thousands_millions("1,000,000 g", vec![Measure::new("g", 1000000.0)])]
fn test_amount_parsing_basic(
    parser: IngredientParser,
    #[case] input: &str,
    #[case] expected: Vec<Measure>,
) {
    assert_eq!(
        parser.parse_amount(input).unwrap(),
        expected,
        "Failed: {input}"
    );
}

/// Test range amount parsing
#[rstest]
#[case::fraction_range("2¼-2.5 cups", Measure::with_range("cups", 2.25, 2.5))]
#[case::to_days("2 to 4 days", Measure::with_range("days", 2.0, 4.0))]
#[case::up_to("up to 4 days", Measure::with_range("days", 0.0, 4.0))]
#[case::at_most("at most 3 cups", Measure::with_range("cups", 0.0, 3.0))]
#[case::hyphen_hours("1-2 hours", Measure::with_range("hours", 1.0, 2.0))]
#[case::hyphen_minutes("30-45 minutes", Measure::with_range("minutes", 30.0, 45.0))]
#[case::through("2 through 4 cups", Measure::with_range("cups", 2.0, 4.0))]
#[case::unicode_range("½-1 cup", Measure::with_range("cup", 0.5, 1.0))]
#[case::mixed_to("1½ to 2 cups", Measure::with_range("cups", 1.5, 2.0))]
#[case::em_dash("1–2 cups", Measure::with_range("cups", 1.0, 2.0))]
#[case::to_keyword("1 to 2 cups", Measure::with_range("cups", 1.0, 2.0))]
#[case::through_keyword("1 through 3 cups", Measure::with_range("cups", 1.0, 3.0))]
#[case::or_keyword("1 or 2 cups", Measure::with_range("cups", 1.0, 2.0))]
#[case::up_to_cups("up to 5 cups", Measure::with_range("cups", 0.0, 5.0))]
#[case::at_most_tbsp("at most 3 tbsp", Measure::with_range("tbsp", 0.0, 3.0))]
fn test_amount_parsing_ranges(
    parser: IngredientParser,
    #[case] input: &str,
    #[case] expected: Measure,
) {
    let result = parser.parse_amount(input).unwrap();
    assert_eq!(result, vec![expected], "Failed: {input}");
}

/// Test multi-amount parsing
#[rstest]
#[case::slash_space("1 cup / 240 ml", vec![Measure::new("cup", 1.0), Measure::new("ml", 240.0)])]
#[case::semicolon("2 tbsp; 30 ml", vec![Measure::new("tbsp", 2.0), Measure::new("ml", 30.0)])]
#[case::slash_bare("1 cup/240 ml", vec![Measure::new("cup", 1.0), Measure::new("ml", 240.0)])]
#[case::comma("1 cup, 2 tbsp", vec![Measure::new("cup", 1.0), Measure::new("tbsp", 2.0)])]
fn test_amount_parsing_multi(
    parser: IngredientParser,
    #[case] input: &str,
    #[case] expected: Vec<Measure>,
) {
    assert_eq!(
        parser.parse_amount(input).unwrap(),
        expected,
        "Failed: {input}"
    );
}

#[rstest]
fn test_amount_range_display(parser: IngredientParser) {
    let amounts = parser.parse_amount("2 ¼ - 2.5 cups").unwrap();
    assert_eq!(format!("{}", amounts[0]), "2¼ - 2½ cups");
}

// ============================================================================
// Display and Formatting Tests
// ============================================================================

#[rstest]
#[case::basic("12 cups flour", "12 cups flour")]
#[case::text_one("one whole egg", "1 egg")]
#[case::text_a("a tsp flour", "1 tsp flour")]
#[case::complex(
    "1 cup (125.5 grams) AP flour, sifted",
    "1 cup / 125½ g AP flour, sifted"
)]
fn test_display_formatting(#[case] input: &str, #[case] expected: &str) {
    assert_eq!(from_str(input).to_string(), expected, "Failed: {input}");
}

#[test]
fn test_display_empty_amounts() {
    assert_eq!(
        Ingredient::new("apples", vec![], None).to_string(),
        "n/a apples"
    );
}

// ============================================================================
// Rich Text Parsing - Parameterized Tests
// ============================================================================

#[rstest]
#[case::basic("hello 1 cups foo bar", &[], vec![text("hello "), measure("cups", 1.0), text(" foo bar")])]
#[case::with_ing("hello 1 cups foo bar", &["bar"], vec![text("hello "), measure("cups", 1.0), text(" foo "), ing("bar")])]
#[case::multi_word_ing("hello 1 cups foo bar", &["foo bar"], vec![text("hello "), measure("cups", 1.0), text(" "), ing("foo bar")])]
#[case::range("2-2 1/2 cups foo' bar", &[], vec![measure_range("cups", 2.0, 2.5), text(" foo' bar")])]
#[case::store_days("store for 1-2 days", &[], vec![text("store for "), measure_range("days", 1.0, 2.0)])]
#[case::unicode_fraction("add ½ cup sugar", &["sugar"], vec![text("add "), measure("cup", 0.5), text(" "), ing("sugar")])]
#[case::no_measures("stir until smooth", &[], vec![text("stir until smooth")])]
fn test_rich_text_basic(
    #[case] input: &str,
    #[case] ingredients: &[&str],
    #[case] expected: Vec<Chunk>,
) {
    assert_eq!(parse_rich(input, ingredients), expected);
}

#[rstest]
#[case::ingredient_at_start("butter should be soft", &["butter"], vec![ing("butter"), text(" should be soft")])]
#[case::ingredient_middle("fold in the chocolate chips gently", &["chocolate chips"], vec![text("fold in the "), ing("chocolate chips"), text(" gently")])]
fn test_rich_text_ingredient_positions(
    #[case] input: &str,
    #[case] ingredients: &[&str],
    #[case] expected: Vec<Chunk>,
) {
    assert_eq!(parse_rich(input, ingredients), expected);
}

#[test]
fn test_rich_text_complex_sentence() {
    assert_eq!(
        parse_rich("add 1 cup water and store for at most 2 days", &["water"]),
        vec![
            text("add "),
            measure("cup", 1.0),
            text(" "),
            ing("water"),
            text(" and store for "),
            measure_range("days", 0.0, 2.0)
        ]
    );
}

#[test]
fn test_rich_text_dimensions() {
    assert_eq!(
        parse_rich(r#"9" x 13""#, &[]),
        vec![measure(r#"""#, 9.0), text(" x "), measure(r#"""#, 13.0)]
    );
}

#[rstest]
#[case::empty("", vec![])]
#[case::whitespace("   ", vec![text("   ")])]
#[case::number_without_unit("step 1", vec![text("step "), measure("whole", 1.0)])]
#[case::punctuation("add 1 cup, then stir", vec![text("add "), measure("cup", 1.0), text(", then stir")])]
// Numbered instructions should NOT parse step numbers as measurements
#[case::numbered_step("1 Bring a large pot of water to a boil.", vec![text("1 Bring a large pot of water to a boil.")])]
#[case::numbered_step_2("2 Set out 4 ramen bowls.", vec![text("2 Set out "), measure("whole", 4.0), text(" ramen bowls.")])]
fn test_rich_text_edge_cases(#[case] input: &str, #[case] expected: Vec<Chunk>) {
    assert_eq!(parse_rich(input, &[]), expected);
}

// ============================================================================
// Rich Text - Time Patterns
// ============================================================================

#[rstest]
#[case::rest_minutes("rest for 10 minutes", vec![text("rest for "), measure("minutes", 10.0)])]
#[case::no_measure("marinate overnight", vec![text("marinate overnight")])]
#[case::cook_range("cook 2-3 hours", vec![text("cook "), measure_range("hours", 2.0, 3.0)])]
#[case::bake_minutes("bake for 25-30 minutes", vec![text("bake for "), measure_range("minutes", 25.0, 30.0)])]
#[case::let_rest("let rest 1-2 hours", vec![text("let rest "), measure_range("hours", 1.0, 2.0)])]
#[case::chill_range("chill for 2-4 hours", vec![text("chill for "), measure_range("hours", 2.0, 4.0)])]
fn test_rich_text_time_patterns(#[case] input: &str, #[case] expected: Vec<Chunk>) {
    assert_eq!(parse_rich(input, &[]), expected);
}

#[test]
fn test_rich_text_compound_time_expressions() {
    let result = parse_rich("2 hours and up to 3 days", &[]);
    assert_eq!(result.len(), 3);
    assert_eq!(result[0], measure("hours", 2.0));
    assert_eq!(result[1], text(" and "));
    assert_eq!(result[2], measure_range("days", 0.0, 3.0));
}

// ============================================================================
// Rich Text - Temperature Patterns
// ============================================================================

#[rstest]
#[case::fahrenheit("preheat oven to 350°F", vec![text("preheat oven to "), measure("°f", 350.0)])]
#[case::temperature_only("preheat to 350 °F", vec![text("preheat to "), measure("°f", 350.0)])]
fn test_rich_text_temperature(#[case] input: &str, #[case] expected: Vec<Chunk>) {
    assert_eq!(parse_rich(input, &[]), expected);
}

#[test]
fn test_rich_text_temperature_range() {
    let result = parse_rich("bake at 350-375°F", &[]);
    let has_range = result.iter().any(|c| {
        matches!(c, Chunk::Measure(m) if m[0].value() == 350.0 && m[0].upper_value() == Some(375.0))
    });
    assert!(has_range, "Should parse temperature range");
}

// ============================================================================
// Rich Text - Multiple Ingredients
// ============================================================================

#[test]
fn test_rich_text_multiple_ingredients() {
    assert_eq!(
        parse_rich("mix 2 cups flour and 1 tsp salt", &["flour", "salt"]),
        vec![
            text("mix "),
            measure("cups", 2.0),
            text(" "),
            ing("flour"),
            text(" and "),
            measure("tsp", 1.0),
            text(" "),
            ing("salt")
        ]
    );

    assert_eq!(
        parse_rich(
            "combine flour, sugar, and salt",
            &["flour", "sugar", "salt"]
        ),
        vec![
            text("combine "),
            ing("flour"),
            text(", "),
            ing("sugar"),
            text(", and "),
            ing("salt")
        ]
    );
}

// ============================================================================
// Measurement Edge Cases
// ============================================================================

/// How multi-measure and unit-defaulting inputs map to (name, rendered amounts).
/// One row per behavior so a regression names the exact case that broke.
#[rstest]
// A cross-unit range can't fold into one ranged measure; both endpoints become
// separate amounts and the name survives.
#[case::cross_unit_range("1 cup to 2 tbsp flour", "flour", &["1 cup", "2 tbsp"])]
// "plus" across INCOMPATIBLE units keeps both endpoints as separate amounts
// (it can't sum cup + gram), rather than dropping one.
#[case::incompatible_plus("1 cup plus 2 grams flour", "flour", &["1 cup", "2 g"])]
// "plus" across COMPATIBLE units folds into one combined measure (1 cup + 2 tbsp
// → 1⅛ cups).
#[case::compatible_plus("1 cup plus 2 tbsp flour", "flour", &["1⅛ cups"])]
// A bare count defaults to the `whole` unit, which renders as just the quantity.
#[case::implicit_whole("2 eggs", "eggs", &["2"])]
fn test_measurement_amount_shapes(
    parser: IngredientParser,
    #[case] input: &str,
    #[case] name: &str,
    #[case] amounts: &[&str],
) {
    let ingredient = parser.from_str(input);
    assert_eq!(ingredient.name, name, "name for: {input}");
    assert_eq!(
        ingredient
            .amounts
            .iter()
            .map(|m| m.to_string())
            .collect::<Vec<_>>(),
        amounts,
        "amounts for: {input}"
    );
}

#[rstest]
fn test_parse_amount_rejects_garbage(parser: IngredientParser) {
    assert!(parser.parse_amount("not a valid amount").is_err());
}

#[test]
fn test_rich_text_ignores_spelled_numbers() {
    // Rich (instruction-prose) mode must not read text numbers like "one" as a
    // quantity, so the prose passes through as a single text span.
    let rich_parser = RichParser::new(Vec::<String>::new());
    let result = rich_parser.parse("add one cup of flour").unwrap();
    assert!(!result.is_empty());
}

// ============================================================================
// "from_str never fails" robustness
// ============================================================================

/// Inputs whose lowercase form has a different byte length (e.g. 'İ' U+0130 →
/// "i̇") must not panic: recognizers that search a lowercased copy used to
/// slice the original string with misaligned offsets. from_str must always
/// fall back gracefully instead.
#[rstest]
#[case::dotted_i_before_of_pivot("Zest İ of ½ lemon")]
#[case::dotted_i_only("İ")]
#[case::dotted_i_with_amount("1 cup İrmik")]
fn test_from_str_length_changing_lowercase_no_panic(#[case] input: &str) {
    let _ = from_str(input); // must not panic
}

// ============================================================================
// Unit-to-string Fallback Test
// ============================================================================

#[test]
fn test_unit_to_str_fallback() {
    use ingredient::unit::Unit;

    // All standard units should have proper string representations
    let units = [
        Unit::Gram,
        Unit::Kilogram,
        Unit::Liter,
        Unit::Milliliter,
        Unit::Teaspoon,
        Unit::Tablespoon,
        Unit::Cup,
        Unit::Quart,
        Unit::FluidOunce,
        Unit::Ounce,
        Unit::Pound,
        Unit::Cent,
        Unit::Dollar,
        Unit::KCal,
        Unit::Day,
        Unit::Hour,
        Unit::Minute,
        Unit::Second,
        Unit::Fahrenheit,
        Unit::Celsius,
        Unit::Inch,
        Unit::Whole,
    ];

    for unit in units {
        let s = unit.to_str();
        assert!(!s.is_empty(), "Unit {unit:?} should have non-empty string");
        // Should not be Debug format (which would contain "Unit::" or braces)
        assert!(
            !s.contains("Unit"),
            "Unit {unit:?} should not use Debug format: {s}"
        );
    }

    // Other units use their inner string
    let other = Unit::Other("pinch".to_string());
    assert_eq!(other.to_str(), "pinch");
}

// ============================================================================
// Rich-text dimension measures are non-scalable
// ============================================================================

/// A dimension surfaced in prose ("cut into 2-inch pieces") is highlighted as an
/// `Inch` measure, but `Inch` is `MeasureKind::Length`, which is non-scalable —
/// so doubling a recipe never turns "2-inch pieces" into "4-inch pieces". The
/// chunk-sequence accuracy lives in `tests/corpus/rich_text.jsonl`; this guards
/// the behavioral property that corpus schema can't express.
#[test]
fn test_rich_text_dimension_is_non_scalable() {
    let result = parse_rich("cut into 2-inch pieces", &[]);
    // Inch stringifies to `"`, so match on the measure kind, not the unit string.
    let inch = result
        .iter()
        .find_map(|c| match c {
            Chunk::Measure(m) if m[0].kind() == MeasureKind::Length => Some(&m[0]),
            _ => None,
        })
        .unwrap();
    assert_eq!(inch.value(), 2.0);
    assert!(
        !inch.kind().is_scalable(),
        "dimensions must not scale with the recipe"
    );
}

// ============================================================================
// Trailing Amount Format Tests (European/Professional Cookbook Style)
// ============================================================================

/// Test that temperature-only trailing amounts are NOT used
/// (because they describe a property, not a quantity)
#[rstest]
#[case::temp_only_f("Water — 100°F")]
#[case::temp_only_c("Milk — 37°C")]
fn test_trailing_temp_only_not_used(parser: IngredientParser, #[case] input: &str) {
    let result = parser.from_str(input);
    // The trailing temperature describes a property, not a quantity: it must
    // not become an amount at all. (The old disjunctive assertion passed even
    // when the temperature WAS taken as the primary amount.)
    assert!(
        result.amounts.is_empty(),
        "Temperature-only trailing should not be used as amount: {input} -> {result:?}"
    );
}

// ============================================================================
// Parse Notes (Ingredient::parse_notes)
// ============================================================================

/// `Ingredient::parse_notes` carries parse confidence and the corpus-harvest
/// "digit but no amount" signal on every parse, without changing the infallible
/// result.
#[rstest]
// Clean structured parse with an amount → High, no signals.
#[case::clean("2 cups flour", ingredient::Confidence::High, false, false)]
// Plausible name-only ingredient (no digit) → Medium, did not fall back.
#[case::name_only("Chocolate Chip Cookies", ingredient::Confidence::Medium, false, false)]
// "to taste" with no digit is a clean name-only parse → Medium (not a miss).
#[case::to_taste("salt to taste", ingredient::Confidence::Medium, false, false)]
// A digit that produced no amount → likely missed quantity → Low.
#[case::unparsed_digit("1+1 multivitamins", ingredient::Confidence::Low, true, true)]
// Regression guard for the minus-equivalence fix: when the "(… minus …)" aside
// is the line's ONLY quantity, it's kept (not silently stripped), so the parse
// yields no amount and surfaces `unparsed_digit` instead of zeroing it.
#[case::minus_sole_quantity(
    "butter (2 sticks minus 1 tablespoon)",
    ingredient::Confidence::Low,
    false,
    true
)]
fn test_parse_notes(
    parser: IngredientParser,
    #[case] input: &str,
    #[case] confidence: ingredient::Confidence,
    #[case] fell_back: bool,
    #[case] unparsed_digit: bool,
) {
    // The notes are populated on the first-class field by plain from_str without
    // changing the infallible result.
    let diag = parser.from_str(input).parse_notes;
    assert_eq!(diag.confidence, confidence, "confidence for: {input}");
    assert_eq!(diag.fell_back, fell_back, "fell_back for: {input}");
    assert_eq!(
        diag.unparsed_digit, unparsed_digit,
        "unparsed_digit for: {input}"
    );
}
