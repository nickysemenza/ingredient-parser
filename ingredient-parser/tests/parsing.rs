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
    ];

    for (input, expected) in test_cases {
        let result = IngredientParser::new(false).from_str(input);
        assert_eq!(result, expected, "Failed to parse: {input}");
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

    for (left, right) in equivalent_pairs {
        assert_eq!(
            IngredientParser::new(false).from_str(left),
            IngredientParser::new(false).from_str(right),
            "Expected '{left}' == '{right}'"
        );
    }
}

// ============================================================================
// Amount Parsing Tests
// ============================================================================

#[test]
fn test_amount_parsing() {
    let parser = IngredientParser::new(false);

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
        // Multiple amounts
        ("1 cup / 240 ml", vec![Measure::new("cup", 1.0), Measure::new("ml", 240.0)]),
        ("2 tbsp; 30 ml", vec![Measure::new("tbsp", 2.0), Measure::new("ml", 30.0)]),
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
    RichParser {
        ingredient_names: ingredient_names.iter().map(|s| s.to_string()).collect(),
        ip: IngredientParser::new(true),
    }
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
    assert_eq!(parse_rich("hello 1 cups foo bar", &[]), vec![text("hello "), measure("cups", 1.0), text(" foo bar")]);

    // With ingredient name extraction
    assert_eq!(parse_rich("hello 1 cups foo bar", &["bar"]), vec![text("hello "), measure("cups", 1.0), text(" foo "), ing("bar")]);

    // Multi-word ingredient names
    assert_eq!(parse_rich("hello 1 cups foo bar", &["foo bar"]), vec![text("hello "), measure("cups", 1.0), text(" "), ing("foo bar")]);

    // Ranges in rich text
    assert_eq!(parse_rich("2-2 1/2 cups foo' bar", &[]), vec![measure_range("cups", 2.0, 2.5), text(" foo' bar")]);

    // "store for X days" pattern
    assert_eq!(parse_rich("store for 1-2 days", &[]), vec![text("store for "), measure_range("days", 1.0, 2.0)]);

    // Complex sentence with ingredient and time
    assert_eq!(
        parse_rich("add 1 cup water and store for at most 2 days", &["water"]),
        vec![text("add "), measure("cup", 1.0), text(" "), ing("water"), text(" and store for"), measure_range("days", 0.0, 2.0)]
    );

    // Dimensions (inches)
    assert_eq!(parse_rich(r#"9" x 13""#, &[]), vec![measure(r#"""#, 9.0), text(" x "), measure(r#"""#, 13.0)]);

    // Temperature
    assert_eq!(parse_rich("preheat to 350 °F", &[]), vec![text("preheat to "), measure("°f", 350.0)]);

    // Multiple ingredients
    assert_eq!(
        parse_rich("mix 2 cups flour and 1 tsp salt", &["flour", "salt"]),
        vec![text("mix "), measure("cups", 2.0), text(" "), ing("flour"), text(" and "), measure("tsp", 1.0), text(" "), ing("salt")]
    );

    // Unicode fractions
    assert_eq!(parse_rich("add ½ cup sugar", &["sugar"]), vec![text("add "), measure("cup", 0.5), text(" "), ing("sugar")]);

    // No measures, just text
    assert_eq!(parse_rich("stir until smooth", &[]), vec![text("stir until smooth")]);

    // Ingredient at start (note: empty text prefix is expected)
    assert_eq!(parse_rich("butter should be soft", &["butter"]), vec![text(""), ing("butter"), text(" should be soft")]);

    // Time durations
    assert_eq!(parse_rich("bake for 25-30 minutes", &[]), vec![text("bake for "), measure_range("minutes", 25.0, 30.0)]);
    assert_eq!(parse_rich("let rest 1-2 hours", &[]), vec![text("let rest "), measure_range("hours", 1.0, 2.0)]);
}
