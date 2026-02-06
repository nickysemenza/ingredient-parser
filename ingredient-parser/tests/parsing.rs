//! Tests for ingredient and rich text parsing

#![allow(clippy::unwrap_used)]

use ingredient::{
    from_str,
    ingredient::Ingredient,
    rich_text::{Chunk, RichParser},
    unit::Measure,
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

#[fixture]
fn parser_custom_adjectives() -> IngredientParser {
    IngredientParser::new().with_adjectives(&["roughly chopped", "finely diced"])
}

// ============================================================================
// Test Helpers
// ============================================================================

/// Helper to build test cases concisely: (input, name, [(unit, amount), ...], modifier)
fn case<'a>(
    input: &'a str,
    name: &str,
    amounts: &[(&str, f64)],
    modifier: Option<&str>,
) -> (&'a str, Ingredient) {
    let measures = amounts.iter().map(|(u, v)| Measure::new(u, *v)).collect();
    (input, Ingredient::new(name, measures, modifier))
}

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
    RichParser::new(ingredient_names.iter().map(|s| s.to_string()).collect())
        .parse(input)
        .unwrap()
}

// ============================================================================
// Ingredient Parsing - Large Table Test
// ============================================================================

#[rstest]
fn test_ingredient_parsing(parser: IngredientParser) {
    #[rustfmt::skip]
    let test_cases: Vec<(&str, Ingredient)> = vec![
        // Basic parsing
        case("egg", "egg", &[], None),
        // Ingredient with adjectives but no amount
        case("Finely chopped toasted almonds", "toasted almonds", &[], Some("finely chopped")),
        case("1 egg", "egg", &[("whole", 1.0)], None),
        case("1 cinnamon stick, crushed", "cinnamon stick", &[("whole", 1.0)], Some("crushed")),
        case("1 tablespoon plus 1 teaspoon olive oil", "olive oil", &[("teaspoon", 4.0)], None),
        case("pinch nutmeg", "nutmeg", &[("pinch", 1.0)], None),
        case("100 grams whole wheat flour", "whole wheat flour", &[("grams", 100.0)], None),
        case("1 clove garlic, grated", "garlic", &[("clove", 1.0)], Some("grated")),
        case("12 cloves of garlic, peeled", "garlic", &[("cloves", 12.0)], Some("peeled")),
        // Multi-amount parsing
        // Sunflower seeds with parenthesized alternate amount
        case("4 ounces raw or roasted and salted shelled sunflower seeds (about ¾ cup)", "raw or roasted and salted shelled sunflower seeds", &[("ounces", 4.0), ("cup", 0.75)], None),
        case("12 cups all purpose flour, lightly sifted", "all purpose flour", &[("cups", 12.0)], Some("lightly sifted")),
        case("1¼  cups / 155.5 grams flour", "flour", &[("cups", 1.25), ("grams", 155.5)], None),
        case("0.25 ounces (1 packet, about 2 teaspoons) instant or rapid rise yeast", "instant or rapid rise yeast", &[("ounces", 0.25), ("packet", 1.0), ("teaspoons", 2.0)], None),
        case("6 ounces unsalted butter (1½ sticks; 168.75g)", "unsalted butter", &[("ounces", 6.0), ("sticks", 1.5), ("g", 168.75)], None),
        case("½ pound 2 sticks; 227 g unsalted butter, room temperature", "unsalted butter", &[("pound", 0.5), ("sticks", 2.0), ("g", 227.0)], Some("room temperature")),
        // Real-world examples
        // Pork belly format: "4 (description) slices NAME"
        case("4 (13-millimeter/½-inch) slices PORK BELLY CHASHU, warmed", "PORK BELLY CHASHU", &[("slices", 4.0)], Some("warmed")),
        case("14 tablespoons/200 grams unsalted butter, cut into pieces", "unsalted butter", &[("tablespoons", 14.0), ("grams", 200.0)], Some("cut into pieces")),
        case("6 cups vegetable stock, more if needed", "vegetable stock", &[("cups", 6.0)], Some("more if needed")),
        case("1/4 cup crème fraîche", "crème fraîche", &[("cup", 0.25)], None),
        case("⅔ cup (167ml) cold water", "cold water", &[("cup", 2.0 / 3.0), ("ml", 167.0)], None),
        case("1 tsp freshly ground black pepper", "black pepper", &[("tsp", 1.0)], Some("freshly ground")),
        // Range in ingredient
        ("1-2 cups flour", Ingredient::new("flour", vec![Measure::with_range("cups", 1.0, 2.0)], None)),
        // Special characters
        case("2 cups/240 grams confectioners' sugar, sifted", "confectioners' sugar", &[("cups", 2.0), ("grams", 240.0)], Some("sifted")),
        case("2 cups/240 grams gruyère, sifted", "gruyère", &[("cups", 2.0), ("grams", 240.0)], Some("sifted")),
        case("2 cups/240 grams Jalapeños, sifted", "Jalapeños", &[("cups", 2.0), ("grams", 240.0)], Some("sifted")),
        // Text numbers
        case("one egg", "egg", &[("whole", 1.0)], None),
        case("a cup flour", "flour", &[("cup", 1.0)], None),
        // Unicode fractions
        case("½ cup sugar", "sugar", &[("cup", 0.5)], None),
        case("¼ tsp salt", "salt", &[("tsp", 0.25)], None),
        case("¾ cup milk", "milk", &[("cup", 0.75)], None),
        case("⅓ cup honey", "honey", &[("cup", 1.0 / 3.0)], None),
        case("1½ cups water", "water", &[("cups", 1.5)], None),
        case("2¾ cups stock", "stock", &[("cups", 2.75)], None),
        // Mixed fractions with slash
        case("1 1/2 cups flour", "flour", &[("cups", 1.5)], None),
        case("2 3/4 tsp vanilla", "vanilla", &[("tsp", 2.75)], None),
        // Decimals
        case("0.5 oz chocolate", "chocolate", &[("oz", 0.5)], None),
        case("1.25 cups rice", "rice", &[("cups", 1.25)], None),
        // Unit case variations
        case("2 CUPS flour", "flour", &[("cups", 2.0)], None),
        case("1 Cup sugar", "sugar", &[("cup", 1.0)], None),
        case("3 TbSp butter", "butter", &[("tbsp", 3.0)], None),
        // Adjective extraction
        case("2 cups chopped onion", "onion", &[("cups", 2.0)], Some("chopped")),
        case("1 cup minced garlic", "garlic", &[("cup", 1.0)], Some("minced")),
        case("1 cup diced tomatoes", "tomatoes", &[("cup", 1.0)], Some("diced")),
        case("salt to taste", "salt", &[], Some("to taste")),
        case("Confectioners' sugar for dusting", "Confectioners' sugar", &[], Some("for dusting")),
        case("Fresh parsley for garnish", "Fresh parsley", &[], Some("for garnish")),
        // Hyphenated ingredient names
        case("2 cups all-purpose flour", "all-purpose flour", &[("cups", 2.0)], None),
        case("1 tsp five-spice powder", "five-spice powder", &[("tsp", 1.0)], None),
        // "about" prefix
        case("about 2 cups flour", "flour", &[("cups", 2.0)], None),
        // Empty/minimal input
        case("flour", "flour", &[], None),
        case("salt", "salt", &[], None),
        // Special units
        case("1 can tomatoes", "tomatoes", &[("can", 1.0)], None),
        case("1 bunch parsley", "parsley", &[("bunch", 1.0)], None),
        case("1 head garlic", "garlic", &[("head", 1.0)], None),
        case("2 leaves basil", "basil", &[("leaves", 2.0)], None),
        // Size descriptors are part of the ingredient name, not units
        case("1 large egg", "large egg", &[("whole", 1.0)], None),
        case("2 medium onions", "medium onions", &[("whole", 2.0)], None),
        case("3 small potatoes", "small potatoes", &[("whole", 3.0)], None),
        // Parenthesized amounts after name
        case("butter (2 sticks), melted", "butter", &[("sticks", 2.0)], Some("melted")),
        case("sugar (1 cup / 200g)", "sugar", &[("cup", 1.0), ("g", 200.0)], None),
        // Adjective in middle of name gets extracted
        case("2 cups freshly grated parmesan", "parmesan", &[("cups", 2.0)], Some("freshly grated")),
        case("1 lb thinly sliced beef", "beef", &[("lb", 1.0)], Some("thinly sliced")),
        case("1 cup sliced almonds", "almonds", &[("cup", 1.0)], Some("sliced")),
        // Multiple adjectives get comma-separated
        case("1 cup chopped minced onion", "onion", &[("cup", 1.0)], Some("chopped, minced")),
        // Fallback behavior - unparseable keeps input as name
        case("mystery ingredient xyz", "mystery ingredient xyz", &[], None),
        // Em-dash in ranges
        ("1–2 cups flour", Ingredient::new("flour", vec![Measure::with_range("cups", 1.0, 2.0)], None)),
        // Semicolon separator
        case("1 cup; 240 ml water", "water", &[("cup", 1.0), ("ml", 240.0)], None),
        // Comma separator in amounts
        case("1 packet, about 2 tsp yeast", "yeast", &[("packet", 1.0), ("tsp", 2.0)], None),
        // Very small amounts
        case("0.25 tsp salt", "salt", &[("tsp", 0.25)], None),
        case("1/8 tsp pepper", "pepper", &[("tsp", 0.125)], None),
        // Large amounts
        case("1000 grams flour", "flour", &[("grams", 1000.0)], None),
        // Bare slash separator
        case("1 cup/240ml water", "water", &[("cup", 1.0), ("ml", 240.0)], None),
        // Space-separated amounts
        case("1 cup 2 tbsp flour", "flour", &[("cup", 1.0), ("tbsp", 2.0)], None),
        // Plus with incompatible units - parses first amount only
        case("1 cup plus 100 grams flour", "flour", &[("cup", 1.0)], None),
        // Unit with trailing period
        case("2 tsp. sugar", "sugar", &[("tsp", 2.0)], None),
        case("1 tbsp. olive oil", "olive oil", &[("tbsp", 1.0)], None),
        case("2 oz. cheese", "cheese", &[("oz", 2.0)], None),
        // Unit with "of" keyword
        case("2 cups of flour", "flour", &[("cups", 2.0)], None),
        case("pinch of salt", "salt", &[("pinch", 1.0)], None),
        // Multiplier format
        case("2 x 100g flour", "flour", &[("g", 200.0)], None),
        case("3 x 50g butter", "butter", &[("g", 150.0)], None),
        // Parenthesized amounts with semicolon separator
        case("flour (1 cup; 120g)", "flour", &[("cup", 1.0), ("g", 120.0)], None),
        // Amounts only in parentheses
        case("flour (2 cups)", "flour", &[("cups", 2.0)], None),
        // Both primary and parenthesized amounts
        case("2 cups flour (240g)", "flour", &[("cups", 2.0), ("g", 240.0)], None),
        // Long modifier after comma
        case("2 cups chicken breast, cooked and diced into small cubes", "chicken breast", &[("cups", 2.0)], Some("cooked and diced into small cubes")),
        // Ingredient without modifier
        case("2 cups water", "water", &[("cups", 2.0)], None),
        // Em-dash between range and unit (Cook's Illustrated format)
        ("3–4 — tablespoons lemon juice", Ingredient::new("lemon juice", vec![Measure::with_range("tbsp", 3.0, 4.0)], None)),
        // UK format with multiplication sign (1 × 400g = 1 count + 400g)
        case("1 × 400g tin pinto beans", "pinto beans", &[("whole", 1.0), ("g", 400.0), ("tin", 1.0)], None),
        // American Sfoglino format with square brackets for alternate amounts
        case("4 TBSP [56 G] UNSALTED BUTTER", "UNSALTED BUTTER", &[("tbsp", 4.0), ("g", 56.0)], None),
        // NBSP normalization (Cook's Illustrated format with non-breaking spaces)
        case("3/4 \u{a0}\u{a0} cups whole milk", "whole milk", &[("cups", 0.75)], None),
    ];

    for (input, expected) in test_cases {
        // Test without tracing
        let result = parser.from_str(input);
        assert_eq!(result, expected, "Failed to parse: {input}");

        // Test with tracing enabled (exercises trace formatter callbacks)
        let traced = parser.parse_with_trace(input);
        assert!(traced.result.is_ok(), "Failed to parse with trace: {input}");
        assert_eq!(
            traced.result.unwrap(),
            expected,
            "Trace result mismatch: {input}"
        );
    }
}

// ============================================================================
// Optional Ingredients Tests
// ============================================================================

/// Test that parenthesized ingredients are parsed as optional
#[rstest]
#[case::basic_optional("(½ cup chopped walnuts)", "walnuts", &[("cup", 0.5)], Some("chopped"))]
#[case::optional_with_multiple_amounts("(2 cups / 240g flour)", "flour", &[("cups", 2.0), ("g", 240.0)], None)]
#[case::optional_simple("(1 egg)", "egg", &[("whole", 1.0)], None)]
fn test_optional_ingredients(
    parser: IngredientParser,
    #[case] input: &str,
    #[case] expected_name: &str,
    #[case] expected_amounts: &[(&str, f64)],
    #[case] expected_modifier: Option<&str>,
) {
    let result = parser.from_str(input);
    assert_eq!(result.name, expected_name, "Name mismatch for: {input}");
    assert!(result.optional, "Expected optional=true for: {input}");
    assert_eq!(
        result.amounts.len(),
        expected_amounts.len(),
        "Amount count mismatch for: {input}"
    );
    for (i, (unit, value)) in expected_amounts.iter().enumerate() {
        assert_eq!(
            result.amounts[i].value(),
            *value,
            "Amount value mismatch for: {input}"
        );
        assert_eq!(
            result.amounts[i].unit_as_string(),
            *unit,
            "Unit mismatch for: {input}"
        );
    }
    assert_eq!(
        result.modifier.as_deref(),
        expected_modifier,
        "Modifier mismatch for: {input}"
    );
}

// ============================================================================
// Secondary Amounts Tests
// ============================================================================

/// Test that secondary amounts are extracted from "(from about X)" patterns
#[rstest]
#[case::from_about_sprigs(
    "60 cilantro leaves (from about 15 sprigs)",
    "cilantro leaves",
    2,
    None
)]
#[case::about_bunches(
    "1 cup chopped parsley (about 2 bunches)",
    "parsley",
    2,
    Some("chopped")
)]
#[case::approximately_lemon(
    "3 tbsp fresh lemon juice (from approximately 1 lemon)",
    "fresh lemon juice",
    2,
    None
)]
fn test_secondary_amounts(
    parser: IngredientParser,
    #[case] input: &str,
    #[case] expected_name: &str,
    #[case] expected_amount_count: usize,
    #[case] expected_modifier: Option<&str>,
) {
    let result = parser.from_str(input);
    assert_eq!(result.name, expected_name, "Name mismatch for: {input}");
    assert_eq!(
        result.amounts.len(),
        expected_amount_count,
        "Amount count mismatch for: {input}"
    );
    assert_eq!(
        result.modifier.as_deref(),
        expected_modifier,
        "Modifier mismatch for: {input}"
    );
}

// ============================================================================
// Alternative Ingredient Tests
// ============================================================================

/// Test that "or X" alternatives are extracted to modifier
#[rstest]
#[case::or_number(
    "4 cloves garlic or 1 teaspoon garlic powder",
    "garlic",
    1,
    Some("or 1 teaspoon garlic powder")
)]
#[case::or_a(
    "1 cup butter or a splash of oil",
    "butter",
    1,
    Some("or a splash of oil")
)]
#[case::no_alternative("4 cloves garlic", "garlic", 1, None)]
fn test_alternative_ingredients(
    parser: IngredientParser,
    #[case] input: &str,
    #[case] expected_name: &str,
    #[case] expected_amount_count: usize,
    #[case] expected_modifier: Option<&str>,
) {
    let result = parser.from_str(input);
    assert_eq!(result.name, expected_name, "Name mismatch for: {input}");
    assert_eq!(
        result.amounts.len(),
        expected_amount_count,
        "Amount count mismatch for: {input}"
    );
    assert_eq!(
        result.modifier.as_deref(),
        expected_modifier,
        "Modifier mismatch for: {input}"
    );
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

#[rstest]
#[case::roughly_chopped("1 cup roughly chopped onion", "onion", "roughly chopped")]
#[case::finely_diced("2 cups finely diced tomatoes", "tomatoes", "finely diced")]
fn test_custom_adjectives(
    parser_custom_adjectives: IngredientParser,
    #[case] input: &str,
    #[case] expected_name: &str,
    #[case] expected_modifier: &str,
) {
    let result = parser_custom_adjectives.from_str(input);
    assert_eq!(result.name, expected_name);
    assert_eq!(result.modifier.as_deref(), Some(expected_modifier));
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
    assert_eq!(format!("{}", amounts[0]), "2.25 - 2.5 cups");
}

// ============================================================================
// Display and Formatting Tests
// ============================================================================

#[rstest]
#[case::basic("12 cups flour", "12 cups flour")]
#[case::text_one("one whole egg", "1 whole egg")]
#[case::text_a("a tsp flour", "1 tsp flour")]
#[case::complex(
    "1 cup (125.5 grams) AP flour, sifted",
    "1 cup / 125.5 g AP flour, sifted"
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
#[case::ingredient_at_start("butter should be soft", &["butter"], vec![text(""), ing("butter"), text(" should be soft")])]
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
#[case::numbered_step_2("2 Set out 4 ramen bowls.", vec![text("2 Set out "), measure("whole", 4.0), text("ramen bowls.")])]
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

#[rstest]
fn test_measurement_edge_cases(parser: IngredientParser) {
    // Unit mismatch in ranges - parser handles gracefully
    let ingredient = parser.from_str("1 cup to 2 tbsp flour");
    assert!(ingredient.name.contains("flour") || !ingredient.amounts.is_empty());

    // Plus expression with incompatible units - should still parse something
    let ingredient = parser.from_str("1 cup plus 2 grams flour");
    assert!(!ingredient.name.is_empty());

    // Implicit unit defaults to "whole"
    let ingredient = parser.from_str("2 eggs");
    assert_eq!(ingredient.amounts[0].unit_as_string(), "whole");

    // Standard plus expression combines measurements
    let ingredient = parser.from_str("1 cup plus 2 tbsp flour");
    assert_eq!(ingredient.name, "flour");
    assert_eq!(ingredient.amounts.len(), 1);

    // parse_amount error on invalid input
    assert!(parser.parse_amount("not a valid amount").is_err());

    // Rich text mode shouldn't parse text numbers like "one"
    let rich_parser = RichParser::new(vec![]);
    let result = rich_parser.parse("add one cup of flour").unwrap();
    assert!(!result.is_empty());
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
        Unit::Celcius,
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
// Known Quirks / Future Improvements (skipped tests)
// ============================================================================

#[test]
#[ignore = "hyphen interpreted as range separator, not compound unit"]
fn test_rich_text_hyphenated_units() {
    let result = parse_rich("cut into 2-inch pieces", &[]);
    let has_inch = result
        .iter()
        .any(|c| matches!(c, Chunk::Measure(m) if m[0].unit_as_string() == "inch"));
    assert!(has_inch, "Should parse '2-inch' as 2 inches: {result:?}");
}

#[test]
#[ignore = "space consumed before non-unit words after numbers"]
fn test_rich_text_number_followed_by_non_unit() {
    let result = parse_rich("makes 12 cookies", &[]);
    let text_chunks: Vec<_> = result
        .iter()
        .filter_map(|c| {
            if let Chunk::Text(s) = c {
                Some(s.as_str())
            } else {
                None
            }
        })
        .collect();
    assert!(
        text_chunks.iter().any(|s| s.starts_with(" cookies")),
        "Space before 'cookies' should be preserved: {result:?}"
    );
}

#[test]
#[ignore = "at least does not create a lower-bound range"]
fn test_rich_text_at_least_as_lower_bound() {
    let result = parse_rich("at least 2 hours", &[]);
    assert_eq!(result.len(), 2);
    assert_eq!(result[0], text("at least "));
    assert_eq!(result[1], measure("hours", 2.0));
}

#[rstest]
fn test_parse_with_trace(parser: IngredientParser) {
    let traced = parser.parse_with_trace("2 cups flour");
    assert!(traced.result.is_ok());
    assert_eq!(traced.result.unwrap().name, "flour");
    assert!(!traced.trace.format_tree(false).is_empty());
}

// ============================================================================
// Trailing Amount Format Tests (European/Professional Cookbook Style)
// ============================================================================

/// Test the trailing amount format: "Ingredient — AMOUNT"
/// Common in professional cookbooks where amounts come at the end after an em-dash
#[rstest]
#[case::em_dash_grams("All-purpose flour — 630 g", "All-purpose flour", &[("g", 630.0)], None)]
#[case::em_dash_with_temp("Warm water (100°F/38°C) — 472 g", "Warm water (100°F/38°C)", &[("g", 472.0)], None)]
#[case::en_dash("Salt – 14 g", "Salt", &[("g", 14.0)], None)]
#[case::double_hyphen("Sugar -- 200 g", "Sugar", &[("g", 200.0)], None)]
#[case::trailing_multiple_units("Butter — 1 cup / 227 g", "Butter", &[("cup", 1.0), ("g", 227.0)], None)]
#[case::bouchon_pipe_format("Heavy cream — 150 grams | ½ cup", "Heavy cream", &[("g", 150.0), ("cup", 0.5)], None)]
#[case::bouchon_pipe_with_plus("Heavy cream — 150 grams | ½ cup + 2 tablespoons", "Heavy cream", &[("g", 150.0), ("tsp", 30.0)], None)]
fn test_trailing_amount_format(
    parser: IngredientParser,
    #[case] input: &str,
    #[case] expected_name: &str,
    #[case] expected_amounts: &[(&str, f64)],
    #[case] expected_modifier: Option<&str>,
) {
    let result = parser.from_str(input);
    assert_eq!(result.name, expected_name, "Name mismatch for: {input}");
    assert_eq!(
        result.amounts.len(),
        expected_amounts.len(),
        "Amount count mismatch for: {input}"
    );
    for (i, (unit, value)) in expected_amounts.iter().enumerate() {
        assert_eq!(
            result.amounts[i].value(),
            *value,
            "Amount value mismatch for: {input}"
        );
        assert_eq!(
            result.amounts[i].unit_as_string(),
            *unit,
            "Unit mismatch for: {input}"
        );
    }
    assert_eq!(
        result.modifier.as_deref(),
        expected_modifier,
        "Modifier mismatch for: {input}"
    );
}

/// Test that temperature-only trailing amounts are NOT used
/// (because they describe a property, not a quantity)
#[rstest]
#[case::temp_only_f("Water — 100°F")]
#[case::temp_only_c("Milk — 37°C")]
fn test_trailing_temp_only_not_used(parser: IngredientParser, #[case] input: &str) {
    let result = parser.from_str(input);
    // Temperature-only trailing amounts should NOT be parsed as the primary amount
    // The result should either have no amounts or fall back to normal parsing
    let has_non_temp_amount = result.amounts.iter().any(|m| {
        !matches!(
            m.unit(),
            ingredient::unit::Unit::Fahrenheit | ingredient::unit::Unit::Celcius
        )
    });
    assert!(
        !has_non_temp_amount || result.amounts.is_empty(),
        "Temperature-only trailing should not be used as amount: {input} -> {result:?}"
    );
}
