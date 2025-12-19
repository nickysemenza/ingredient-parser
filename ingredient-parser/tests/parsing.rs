//! Tests for ingredient and rich text parsing

#![allow(clippy::unwrap_used)]

use ingredient::{
    from_str,
    ingredient::Ingredient,
    rich_text::{Chunk, RichParser},
    unit::Measure,
    IngredientParser,
};

// ============================================================================
// Ingredient Parsing Tests
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

#[test]
fn test_ingredient_parsing() {
    #[rustfmt::skip]
    let test_cases: Vec<(&str, Ingredient)> = vec![
        // Basic parsing
        case("egg", "egg", &[], None),
        case("1 egg", "egg", &[("whole", 1.0)], None),
        case("1 cinnamon stick, crushed", "cinnamon stick", &[("whole", 1.0)], Some("crushed")),
        case("1 tablespoon plus 1 teaspoon olive oil", "olive oil", &[("teaspoon", 4.0)], None),
        case("pinch nutmeg", "nutmeg", &[("pinch", 1.0)], None),
        case("100 grams whole wheat flour", "whole wheat flour", &[("grams", 100.0)], None),
        case("1 clove garlic, grated", "garlic", &[("clove", 1.0)], Some("grated")),
        case("12 cloves of garlic, peeled", "garlic", &[("cloves", 12.0)], Some("peeled")),
        // Multi-amount parsing
        case("12 cups all purpose flour, lightly sifted", "all purpose flour", &[("cups", 12.0)], Some("lightly sifted")),
        case("1¼  cups / 155.5 grams flour", "flour", &[("cups", 1.25), ("grams", 155.5)], None),
        case("0.25 ounces (1 packet, about 2 teaspoons) instant or rapid rise yeast", "instant or rapid rise yeast", &[("ounces", 0.25), ("packet", 1.0), ("teaspoons", 2.0)], None),
        case("6 ounces unsalted butter (1½ sticks; 168.75g)", "unsalted butter", &[("ounces", 6.0), ("sticks", 1.5), ("g", 168.75)], None),
        case("½ pound 2 sticks; 227 g unsalted butter, room temperature", "unsalted butter", &[("pound", 0.5), ("sticks", 2.0), ("g", 227.0)], Some("room temperature")),
        // Real-world examples
        case("14 tablespoons/200 grams unsalted butter, cut into pieces", "unsalted butter", &[("tablespoons", 14.0), ("grams", 200.0)], Some("cut into pieces")),
        case("6 cups vegetable stock, more if needed", "vegetable stock", &[("cups", 6.0)], Some("more if needed")),
        case("1/4 cup crème fraîche", "crème fraîche", &[("cup", 0.25)], None),
        case("⅔ cup (167ml) cold water", "cold water", &[("cup", 2.0 / 3.0), ("ml", 167.0)], None),
        case("1 tsp freshly ground black pepper", "black pepper", &[("tsp", 1.0)], Some("freshly ground")),
        // Range in ingredient (uses Ingredient::new directly since case() doesn't support ranges)
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
        case("1 large egg", "egg", &[("large", 1.0)], None),
        case("2 medium onions", "onions", &[("medium", 2.0)], None),
        case("3 small potatoes", "potatoes", &[("small", 3.0)], None),
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
        // Bare slash separator: "1 cup/240ml"
        case("1 cup/240ml water", "water", &[("cup", 1.0), ("ml", 240.0)], None),
        // Space-separated amounts
        case("1 cup 2 tbsp flour", "flour", &[("cup", 1.0), ("tbsp", 2.0)], None),
        // Plus with incompatible units - parses first amount only (can't add cup + grams)
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
        // Ingredient without modifier - modifier should be None
        case("2 cups water", "water", &[("cups", 2.0)], None),
    ];

    let parser = IngredientParser::new();
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

#[test]
fn test_parsing_equivalence() {
    #[rustfmt::skip]
    let equivalent_pairs: Vec<(&str, &str)> = vec![
        // Range formats
        ("1/2-1 cup", "1/2 - 1 cup"),
        ("2¼-2.5 cups", "2 ¼ - 2.5 cups"),
        ("78g to 104g", "78g - 104g"),
        ("1 or 2 cups flour", "1-2 cups flour"),
        ("2 through 4 cups flour", "2-4 cups flour"),
        // Implicit unit
        ("1 cinnamon stick", "1 whole cinnamon stick"),
        // Multiplier
        ("2 x 200g flour", "400g flour"),
        ("3 x 100g butter", "300g butter"),
        ("1.5 x 100g flour", "150g flour"),
        // "of" keyword
        ("pinch nutmeg", "pinch of nutmeg"),
        ("1 cup of flour", "1 cup flour"),
        // Period after unit
        ("1 Tbsp. flour", "1 tbsp flour"),
        ("2 tsp. sugar", "2 tsp sugar"),
        ("1 oz. butter", "1 oz butter"),
        // Unit aliases
        ("1 tablespoon oil", "1 tbsp oil"),
        ("2 teaspoons salt", "2 tsp salt"),
        ("1 pound beef", "1 lb beef"),
        ("8 ounces cheese", "8 oz cheese"),
        ("500 grams flour", "500 g flour"),
        ("1 kilogram potatoes", "1 kg potatoes"),
        // Case insensitivity
        ("1 CUP flour", "1 cup flour"),
        ("2 TBSP sugar", "2 tbsp sugar"),
    ];

    let parser = IngredientParser::new();
    for (left, right) in equivalent_pairs {
        assert_eq!(
            parser.from_str(left),
            parser.from_str(right),
            "Expected '{left}' == '{right}'"
        );
    }
}

#[test]
fn test_custom_units() {
    // Must add both singular and plural forms
    let parser =
        IngredientParser::new().with_units(&["handful", "handfuls", "sprig", "sprigs", "knob"]);

    // Custom units should be recognized
    assert_eq!(
        parser.from_str("2 handfuls spinach"),
        Ingredient::new("spinach", vec![Measure::new("handfuls", 2.0)], None)
    );
    assert_eq!(
        parser.from_str("3 sprigs thyme"),
        Ingredient::new("thyme", vec![Measure::new("sprigs", 3.0)], None)
    );
    assert_eq!(
        parser.from_str("1 knob ginger"),
        Ingredient::new("ginger", vec![Measure::new("knob", 1.0)], None)
    );
}

#[test]
fn test_custom_adjectives() {
    let parser = IngredientParser::new().with_adjectives(&["roughly chopped", "finely diced"]);

    assert_eq!(
        parser.from_str("1 cup roughly chopped onion"),
        Ingredient::new(
            "onion",
            vec![Measure::new("cup", 1.0)],
            Some("roughly chopped")
        )
    );
    assert_eq!(
        parser.from_str("2 cups finely diced tomatoes"),
        Ingredient::new(
            "tomatoes",
            vec![Measure::new("cups", 2.0)],
            Some("finely diced")
        )
    );
}

// ============================================================================
// Amount Parsing Tests
// ============================================================================

#[test]
fn test_amount_parsing() {
    let parser = IngredientParser::new();

    #[rustfmt::skip]
    let test_cases: Vec<(&str, Vec<Measure>)> = vec![
        // Temperature units
        ("350 °", vec![Measure::new("°", 350.0)]),
        ("350 °F", vec![Measure::new("°f", 350.0)]),
        ("200 °C", vec![Measure::new("°c", 200.0)]),
        // Ranges
        ("2¼-2.5 cups", vec![Measure::with_range("cups", 2.25, 2.5)]),
        ("2 to 4 days", vec![Measure::with_range("days", 2.0, 4.0)]),
        ("up to 4 days", vec![Measure::with_range("days", 0.0, 4.0)]),
        ("at most 3 cups", vec![Measure::with_range("cups", 0.0, 3.0)]),
        ("1-2 hours", vec![Measure::with_range("hours", 1.0, 2.0)]),
        ("30-45 minutes", vec![Measure::with_range("minutes", 30.0, 45.0)]),
        ("2 through 4 cups", vec![Measure::with_range("cups", 2.0, 4.0)]),
        ("½-1 cup", vec![Measure::with_range("cup", 0.5, 1.0)]),
        ("1½ to 2 cups", vec![Measure::with_range("cups", 1.5, 2.0)]),
        // Basic amounts
        ("1 cup", vec![Measure::new("cup", 1.0)]),
        ("2 tbsp", vec![Measure::new("tbsp", 2.0)]),
        ("500 g", vec![Measure::new("g", 500.0)]),
        // Fractions
        ("1/2 cup", vec![Measure::new("cup", 0.5)]),
        ("3/4 tsp", vec![Measure::new("tsp", 0.75)]),
        ("1 1/2 cups", vec![Measure::new("cups", 1.5)]),
        // Unicode fractions
        ("½ cup", vec![Measure::new("cup", 0.5)]),
        ("¾ tsp", vec![Measure::new("tsp", 0.75)]),
        ("1½ cups", vec![Measure::new("cups", 1.5)]),
        // Decimals
        ("1.5 cups", vec![Measure::new("cups", 1.5)]),
        ("0.25 oz", vec![Measure::new("oz", 0.25)]),
        // Multiple amounts with various separators
        ("1 cup / 240 ml", vec![Measure::new("cup", 1.0), Measure::new("ml", 240.0)]),
        ("2 tbsp; 30 ml", vec![Measure::new("tbsp", 2.0), Measure::new("ml", 30.0)]),
        ("1 cup/240 ml", vec![Measure::new("cup", 1.0), Measure::new("ml", 240.0)]),
        ("1 cup, 2 tbsp", vec![Measure::new("cup", 1.0), Measure::new("tbsp", 2.0)]),
        // Em-dash range
        ("1–2 cups", vec![Measure::with_range("cups", 1.0, 2.0)]),
        // Range keywords
        ("1 to 2 cups", vec![Measure::with_range("cups", 1.0, 2.0)]),
        ("1 through 3 cups", vec![Measure::with_range("cups", 1.0, 3.0)]),
        ("1 or 2 cups", vec![Measure::with_range("cups", 1.0, 2.0)]),
        // Upper bound formats
        ("up to 5 cups", vec![Measure::with_range("cups", 0.0, 5.0)]),
        ("at most 3 tbsp", vec![Measure::with_range("tbsp", 0.0, 3.0)]),
    ];

    for (input, expected) in test_cases {
        assert_eq!(
            parser.parse_amount(input).unwrap(),
            expected,
            "Failed: {input}"
        );
    }

    // Range display format
    let amounts = parser.parse_amount("2 ¼ - 2.5 cups").unwrap();
    assert_eq!(format!("{}", amounts[0]), "2.25 - 2.5 cups");
}

// ============================================================================
// Display and Formatting Tests
// ============================================================================

#[test]
fn test_display_formatting() {
    let test_cases: Vec<(&str, &str)> = vec![
        ("12 cups flour", "12 cups flour"),
        ("one whole egg", "1 whole egg"),
        ("a tsp flour", "1 tsp flour"),
        (
            "1 cup (125.5 grams) AP flour, sifted",
            "1 cup / 125.5 g AP flour, sifted",
        ),
    ];

    for (input, expected) in test_cases {
        assert_eq!(from_str(input).to_string(), expected, "Failed: {input}");
    }

    // Empty amounts display as "n/a"
    assert_eq!(
        Ingredient::new("apples", vec![], None).to_string(),
        "n/a apples"
    );
}

// ============================================================================
// Rich Text Parsing Tests
// ============================================================================

fn parse_rich(input: &str, ingredient_names: &[&str]) -> Vec<Chunk> {
    RichParser::new(ingredient_names.iter().map(|s| s.to_string()).collect())
        .parse(input)
        .unwrap()
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

#[test]
fn test_rich_text_parsing() {
    // Basic rich text with measure extraction
    assert_eq!(
        parse_rich("hello 1 cups foo bar", &[]),
        vec![text("hello "), measure("cups", 1.0), text(" foo bar")]
    );

    // With ingredient name extraction
    assert_eq!(
        parse_rich("hello 1 cups foo bar", &["bar"]),
        vec![
            text("hello "),
            measure("cups", 1.0),
            text(" foo "),
            ing("bar")
        ]
    );

    // Multi-word ingredient names
    assert_eq!(
        parse_rich("hello 1 cups foo bar", &["foo bar"]),
        vec![
            text("hello "),
            measure("cups", 1.0),
            text(" "),
            ing("foo bar")
        ]
    );

    // Ranges in rich text
    assert_eq!(
        parse_rich("2-2 1/2 cups foo' bar", &[]),
        vec![measure_range("cups", 2.0, 2.5), text(" foo' bar")]
    );

    // "store for X days" pattern
    assert_eq!(
        parse_rich("store for 1-2 days", &[]),
        vec![text("store for "), measure_range("days", 1.0, 2.0)]
    );

    // Complex sentence with ingredient and time
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

    // Dimensions (inches)
    assert_eq!(
        parse_rich(r#"9" x 13""#, &[]),
        vec![measure(r#"""#, 9.0), text(" x "), measure(r#"""#, 13.0)]
    );

    // Temperature
    assert_eq!(
        parse_rich("preheat to 350 °F", &[]),
        vec![text("preheat to "), measure("°f", 350.0)]
    );

    // Multiple ingredients
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

    // Unicode fractions
    assert_eq!(
        parse_rich("add ½ cup sugar", &["sugar"]),
        vec![text("add "), measure("cup", 0.5), text(" "), ing("sugar")]
    );

    // No measures, just text
    assert_eq!(
        parse_rich("stir until smooth", &[]),
        vec![text("stir until smooth")]
    );

    // Ingredient at start (note: empty text prefix is expected)
    assert_eq!(
        parse_rich("butter should be soft", &["butter"]),
        vec![text(""), ing("butter"), text(" should be soft")]
    );

    // Time durations
    assert_eq!(
        parse_rich("bake for 25-30 minutes", &[]),
        vec![text("bake for "), measure_range("minutes", 25.0, 30.0)]
    );
    assert_eq!(
        parse_rich("let rest 1-2 hours", &[]),
        vec![text("let rest "), measure_range("hours", 1.0, 2.0)]
    );
}

// Additional Rich Text Edge Cases
// ============================================================================

#[test]
fn test_rich_text_compound_time_expressions() {
    // Compound time expressions with "and up to"
    let result = parse_rich("2 hours and up to 3 days", &[]);
    assert_eq!(result.len(), 3);
    assert_eq!(result[0], measure("hours", 2.0));
    assert_eq!(result[1], text(" and ")); // Space is preserved
    assert_eq!(result[2], measure_range("days", 0.0, 3.0));

    // Full sentence with "at least X and up to Y"
    let result = parse_rich(
        "Cover and chill for at least 2 hours and up to 3 days before baking.",
        &[],
    );
    // Check that both time values are captured
    let measures: Vec<_> = result
        .iter()
        .filter(|c| matches!(c, Chunk::Measure(_)))
        .collect();
    assert_eq!(measures.len(), 2, "Should find two measurements");

    // "at least X" should parse X as a single value (lower bound of open-ended range)
    let result = parse_rich("at least 2 hours", &[]);
    assert_eq!(result[0], text("at least "));
    assert_eq!(result[1], measure("hours", 2.0));

    // "up to X" parses as 0-X range
    let result = parse_rich("up to 3 days", &[]);
    assert_eq!(result[0], measure_range("days", 0.0, 3.0));

    // "at most X" should also work like "up to X"
    let result = parse_rich("at most 5 minutes", &[]);
    assert_eq!(result[0], measure_range("minutes", 0.0, 5.0));
}

#[test]
fn test_rich_text_time_patterns() {
    // "rest for X minutes"
    assert_eq!(
        parse_rich("rest for 10 minutes", &[]),
        vec![text("rest for "), measure("minutes", 10.0)]
    );

    // "marinate overnight" - no parsable measure
    assert_eq!(
        parse_rich("marinate overnight", &[]),
        vec![text("marinate overnight")]
    );

    // "cook 2-3 hours" without "for"
    assert_eq!(
        parse_rich("cook 2-3 hours", &[]),
        vec![text("cook "), measure_range("hours", 2.0, 3.0)]
    );

    // "simmer for about 20 minutes"
    let result = parse_rich("simmer for about 20 minutes", &[]);
    let has_20_min = result
        .iter()
        .any(|c| matches!(c, Chunk::Measure(m) if m[0].values().0 == 20.0));
    assert!(has_20_min, "Should parse '20 minutes'");

    // Multiple time references
    let result = parse_rich("cook for 5 minutes, then bake for 30 minutes", &[]);
    let measures: Vec<_> = result
        .iter()
        .filter(|c| matches!(c, Chunk::Measure(_)))
        .collect();
    assert_eq!(measures.len(), 2, "Should find two time measurements");
}

#[test]
fn test_rich_text_temperature_patterns() {
    // Fahrenheit with degree symbol
    assert_eq!(
        parse_rich("preheat oven to 350°F", &[]),
        vec![text("preheat oven to "), measure("°f", 350.0)]
    );

    // Fahrenheit with space before symbol
    let result = parse_rich("bake at 400 °F", &[]);
    let has_temp = result
        .iter()
        .any(|c| matches!(c, Chunk::Measure(m) if m[0].values().0 == 400.0));
    assert!(has_temp, "Should parse '400 °F'");

    // Celsius
    let result = parse_rich("heat to 180°C", &[]);
    let has_temp = result
        .iter()
        .any(|c| matches!(c, Chunk::Measure(m) if m[0].values().0 == 180.0));
    assert!(has_temp, "Should parse '180°C'");

    // Temperature range
    let result = parse_rich("bake at 350-375°F", &[]);
    let has_range = result.iter().any(|c| {
        matches!(c, Chunk::Measure(m) if m[0].values().0 == 350.0 && m[0].values().1 == Some(375.0))
    });
    assert!(has_range, "Should parse temperature range");
}

#[test]
fn test_rich_text_dimension_patterns() {
    // Pan dimensions with x
    assert_eq!(
        parse_rich(r#"use a 9" x 13" pan"#, &[]),
        vec![
            text("use a "),
            measure(r#"""#, 9.0),
            text(" x "),
            measure(r#"""#, 13.0),
            text(" pan")
        ]
    );

    // Single dimension
    assert_eq!(
        parse_rich(r#"roll to 1/4" thick"#, &[]),
        vec![text("roll to "), measure(r#"""#, 0.25), text(" thick")]
    );

    // "2 inch" with space - actually parses correctly as inches!
    let result = parse_rich("cut into 2 inch squares", &[]);
    assert_eq!(
        result,
        vec![text("cut into "), measure("inch", 2.0), text(" squares")]
    );

    // "2-inch" hyphenated - hyphen is interpreted as potential range separator
    let result = parse_rich("cut into 2-inch squares", &[]);
    // This currently parses strangely because "-inch" looks like a range continuation
    assert!(!result.is_empty());
}

#[test]
fn test_rich_text_quantity_patterns() {
    // "a few" - doesn't parse as number
    assert_eq!(
        parse_rich("add a few drops", &[]),
        vec![text("add a few drops")]
    );

    // Plural units
    assert_eq!(
        parse_rich("add 3 cups water", &["water"]),
        vec![text("add "), measure("cups", 3.0), text(" "), ing("water")]
    );

    // Fractions in text
    assert_eq!(
        parse_rich("add 1/2 cup milk", &["milk"]),
        vec![text("add "), measure("cup", 0.5), text(" "), ing("milk")]
    );

    // Mixed number
    assert_eq!(
        parse_rich("use 1 1/2 cups flour", &["flour"]),
        vec![text("use "), measure("cups", 1.5), text(" "), ing("flour")]
    );

    // Unicode fraction
    assert_eq!(
        parse_rich("add ¼ teaspoon salt", &["salt"]),
        vec![
            text("add "),
            measure("teaspoon", 0.25),
            text(" "),
            ing("salt")
        ]
    );
}

#[test]
fn test_rich_text_ingredient_extraction() {
    // Ingredient in middle of text
    assert_eq!(
        parse_rich("fold in the chocolate chips gently", &["chocolate chips"]),
        vec![
            text("fold in the "),
            ing("chocolate chips"),
            text(" gently")
        ]
    );

    // Multiple ingredients
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

    // Ingredient with measurement
    assert_eq!(
        parse_rich("whisk in 2 tbsp butter", &["butter"]),
        vec![
            text("whisk in "),
            measure("tbsp", 2.0),
            text(" "),
            ing("butter")
        ]
    );

    // Ingredient name that's a substring - should match exact word
    let result = parse_rich("add the cream cheese", &["cream"]);
    assert!(result
        .iter()
        .any(|c| matches!(c, Chunk::Ing(s) if s == "cream")));
}

#[test]
fn test_rich_text_edge_cases() {
    // Empty string
    assert_eq!(parse_rich("", &[]), vec![]);

    // Just whitespace
    assert_eq!(parse_rich("   ", &[]), vec![text("   ")]);

    // Numbers without units
    assert_eq!(
        parse_rich("step 1", &[]),
        vec![text("step "), measure("whole", 1.0)]
    );

    // Punctuation handling
    assert_eq!(
        parse_rich("add 1 cup, then stir", &[]),
        vec![text("add "), measure("cup", 1.0), text(", then stir")]
    );

    // Parenthetical amounts - note: parentheses don't prevent parsing
    let result = parse_rich("butter (about 2 tablespoons)", &["butter"]);
    // Result is: [Text(""), Ing("butter"), Text(" "), Measure([Tablespoon, 2.0])]
    // Note: "(about" and ")" are stripped - parentheses are consumed as separators
    let has_tbsp = result.iter().any(|c| matches!(c, Chunk::Measure(_)));
    assert!(
        has_tbsp,
        "Should parse '2 tablespoons' in parentheses: {result:?}"
    );

    // Hyphenated time (should be range)
    assert_eq!(
        parse_rich("chill for 2-4 hours", &[]),
        vec![text("chill for "), measure_range("hours", 2.0, 4.0)]
    );

    // "to" as range indicator
    let result = parse_rich("bake for 45 to 50 minutes", &[]);
    let has_range = result.iter().any(|c| {
        matches!(c, Chunk::Measure(m) if m[0].values().0 == 45.0 && m[0].values().1 == Some(50.0))
    });
    assert!(has_range, "Should parse '45 to 50 minutes' as a range");
}

#[test]
fn test_rich_text_parentheses() {
    // Test various parenthetical expressions
    // Note: parentheses are treated as separators and stripped from text chunks
    let result = parse_rich("(about 2 tablespoons)", &[]);
    assert_eq!(result.len(), 1); // Just the measure, parentheses and "about" absorbed
    assert!(matches!(&result[0], Chunk::Measure(m) if m[0].values().0 == 2.0));

    // Parentheses with content before and after
    let result = parse_rich("add butter (softened) to bowl", &["butter"]);
    assert!(result
        .iter()
        .any(|c| matches!(c, Chunk::Ing(s) if s == "butter")));
    // Note: "softened" in parens becomes a text chunk

    // Nested parenthetical with measurement
    let result = parse_rich("use oil (about 1/4 cup) for frying", &[]);
    let has_quarter_cup = result
        .iter()
        .any(|c| matches!(c, Chunk::Measure(m) if (m[0].values().0 - 0.25).abs() < 0.01));
    assert!(has_quarter_cup, "Should parse '1/4 cup' in parentheses");
}

// ============================================================================
// Known Quirks / Future Improvements (skipped tests)
// ============================================================================

#[test]
#[ignore = "hyphen interpreted as range separator, not compound unit"]
fn test_rich_text_hyphenated_units() {
    // "2-inch" should parse as 2 inches, not "2" with "-inch" as leftover text
    let result = parse_rich("cut into 2-inch pieces", &[]);
    let has_inch = result
        .iter()
        .any(|c| matches!(c, Chunk::Measure(m) if m[0].values().2 == "inch"));
    assert!(has_inch, "Should parse '2-inch' as 2 inches: {result:?}");

    // Same with fractions
    let result = parse_rich("roll to 1/2-inch thick", &[]);
    let has_half_inch = result.iter().any(|c| {
        matches!(c, Chunk::Measure(m) if m[0].values().2 == "inch" && (m[0].values().0 - 0.5).abs() < 0.01)
    });
    assert!(
        has_half_inch,
        "Should parse '1/2-inch' as 0.5 inches: {result:?}"
    );
}

#[test]
#[ignore = "space consumed before non-unit words after numbers"]
fn test_rich_text_number_followed_by_non_unit() {
    // "12 cookies" should preserve the space: ["12 whole"][" cookies"]
    // Currently produces: ["12 whole"]["cookies"] (missing space)
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
        text_chunks
            .iter()
            .any(|s| s.starts_with(" cookies") || s == &" cookies"),
        "Space before 'cookies' should be preserved: {result:?}"
    );
}

#[test]
#[ignore = "at least does not create a lower-bound range"]
fn test_rich_text_at_least_as_lower_bound() {
    // "at least 2 hours" semantically means "2+ hours" (lower bound)
    // Currently parses as just "2 hours" with "at least" as text
    // Ideally this would create a Measure with lower_value set
    let result = parse_rich("at least 2 hours", &[]);

    // For now, just document the current behavior
    assert_eq!(result.len(), 2);
    assert_eq!(result[0], text("at least "));
    assert_eq!(result[1], measure("hours", 2.0));

    // Future: could have Measure { value: 2.0, lower_bound: true } or similar
}

// ============================================================================
// Measurement Parser Edge Case Tests
// ============================================================================

#[test]
fn test_measurement_edge_cases() {
    let parser = IngredientParser::new();

    // Unit mismatch in ranges - parser handles gracefully
    let ingredient = parser.from_str("1 cup to 2 tbsp flour");
    assert!(ingredient.name.contains("flour") || !ingredient.amounts.is_empty());

    // Plus expression with incompatible units - should still parse something
    let ingredient = parser.from_str("1 cup plus 2 grams flour");
    assert!(!ingredient.name.is_empty());

    // Implicit unit defaults to "whole"
    let ingredient = parser.from_str("2 eggs");
    assert_eq!(ingredient.amounts[0].values().2, "whole");

    // Standard plus expression combines measurements
    let ingredient = parser.from_str("1 cup plus 2 tbsp flour");
    assert_eq!(ingredient.name, "flour");
    assert_eq!(ingredient.amounts.len(), 1);

    // parse_amount error on invalid input
    assert!(parser.parse_amount("not a valid amount").is_err());

    // Range with mismatched units - handles gracefully
    let ingredient = parser.from_str("2 cups to 3 tbsp flour");
    assert!(ingredient.name.contains("flour"));
    assert!(!ingredient.amounts.is_empty());

    // Rich text mode shouldn't parse text numbers like "one"
    let rich_parser = RichParser::new(vec![]);
    let result = rich_parser.parse("add one cup of flour").unwrap();
    assert!(!result.is_empty());
}

// ============================================================================
// Custom Parser Configuration Tests
// ============================================================================

#[test]
fn test_custom_adjectives_extraction() {
    // Test various custom adjective configurations
    let cases: Vec<(&[&str], &str, &str, Option<&str>)> = vec![
        // (adjectives, input, expected_name, expected_modifier_contains)
        (
            &["fresh", "chopped"],
            "2 cups fresh chopped basil",
            "basil",
            Some("fresh"),
        ),
        (&["large"], "2 large eggs", "eggs", None), // "large" is a unit, not extracted
        (
            &["sliced", "thinly sliced"],
            "2 cups thinly sliced onions",
            "onions",
            Some("thinly sliced"),
        ),
    ];

    for (adjectives, input, expected_name, expected_modifier) in cases {
        let parser = IngredientParser::new().with_adjectives(adjectives);
        let ingredient = parser.from_str(input);
        assert_eq!(ingredient.name, expected_name, "Failed for: {input}");
        if let Some(modifier_text) = expected_modifier {
            assert!(
                ingredient
                    .modifier
                    .as_ref()
                    .is_some_and(|m| m.contains(modifier_text)),
                "Expected modifier containing '{modifier_text}' for: {input}"
            );
        }
    }
}

#[test]
fn test_parse_with_trace() {
    let parser = IngredientParser::new();
    let traced = parser.parse_with_trace("2 cups flour");

    assert!(traced.result.is_ok());
    assert_eq!(traced.result.unwrap().name, "flour");
    assert!(!traced.trace.format_tree(false).is_empty());
}
