//! Tests for ingredient and rich text parsing

#![allow(clippy::unwrap_used)]

#[macro_use]
mod common;

use ingredient::{
    from_str,
    ingredient::Ingredient,
    rich_text::{Chunk, RichParser},
    unit::Measure,
    IngredientParser,
};

// ============================================================================
// Parsing Equality Tests (two inputs should parse to same result)
// ============================================================================

test_ingredient!(eq: test_amount_range, "1/2-1 cup", "1/2 - 1 cup");
test_ingredient!(eq: test_amount_range_vfrac, "2¼-2.5 cups", "2 ¼ - 2.5 cups");
test_ingredient!(eq: test_amount_range_g, "78g to 104g", "78g - 104g");
test_ingredient!(eq: test_unitless, "1 cinnamon stick", "1 whole cinnamon stick");
test_ingredient!(eq: multiply, "2 x 200g flour", "400g flour");
test_ingredient!(eq: test_unit_without_number_of, "pinch nutmeg", "pinch of nutmeg");

// ============================================================================
// Basic Ingredient Parsing Tests
// ============================================================================

test_ingredient!(
    ingredient_parse_no_amounts,
    "egg",
    Ingredient {
        name: "egg".to_string(),
        amounts: vec![],
        modifier: None,
    }
);

test_ingredient!(
    ingredient_parse_no_unit,
    "1 egg",
    Ingredient {
        name: "egg".to_string(),
        amounts: vec![Measure::new("whole", 1.0)],
        modifier: None,
    }
);

test_ingredient!(
    ingredient_parse_no_unit_multi_name_adj,
    "1 cinnamon stick, crushed",
    Ingredient {
        name: "cinnamon stick".to_string(),
        amounts: vec![Measure::new("whole", 1.0)],
        modifier: Some("crushed".to_string()),
    }
);

test_ingredient!(
    test_sum,
    "1 tablespoon plus 1 teaspoon olive oil",
    Ingredient {
        name: "olive oil".to_string(),
        amounts: vec![Measure::new("teaspoon", 4.0)],
        modifier: None
    }
);

test_ingredient!(
    test_unit_without_number,
    "pinch nutmeg",
    Ingredient {
        name: "nutmeg".to_string(),
        amounts: vec![Measure::new("pinch", 1.0)],
        modifier: None
    }
);

// "whole" can sometimes be an ingredient
test_ingredient!(
    test_parse_whole_wheat_ambigious,
    "100 grams whole wheat flour",
    Ingredient {
        name: "whole wheat flour".to_string(),
        amounts: vec![Measure::new("grams", 100.0)],
        modifier: None
    }
);

test_ingredient!(
    test_parse_ingredient_cloves,
    "1 clove garlic, grated",
    Ingredient {
        name: "garlic".to_string(),
        amounts: vec![Measure::new("clove", 1.0)],
        modifier: Some("grated".to_string())
    }
);

// ============================================================================
// Multi-Amount Parsing Tests
// ============================================================================

test_ingredient!(
    multi1,
    "12 cups all purpose flour, lightly sifted",
    Ingredient {
        name: "all purpose flour".to_string(),
        amounts: vec![Measure::new("cups", 12.0)],
        modifier: Some("lightly sifted".to_string()),
    }
);

test_ingredient!(
    multi2,
    "1¼  cups / 155.5 grams flour",
    Ingredient {
        name: "flour".to_string(),
        amounts: vec![
            Measure::new("cups", 1.25),
            Measure::new("grams", 155.5),
        ],
        modifier: None,
    }
);

test_ingredient!(
    multi3,
    "0.25 ounces (1 packet, about 2 teaspoons) instant or rapid rise yeast",
    Ingredient {
        name: "instant or rapid rise yeast".to_string(),
        amounts: vec![
            Measure::new("ounces", 0.25),
            Measure::new("packet", 1.0),
            Measure::new("teaspoons", 2.0),
        ],
        modifier: None
    }
);

test_ingredient!(
    multi4,
    "6 ounces unsalted butter (1½ sticks; 168.75g)",
    Ingredient {
        name: "unsalted butter".to_string(),
        amounts: vec![
            Measure::new("ounces", 6.0),
            Measure::new("sticks", 1.5),
            Measure::new("g", 168.75),
        ],
        modifier: None
    }
);

test_ingredient!(
    multi5,
    "½ pound 2 sticks; 227 g unsalted butter, room temperature",
    Ingredient {
        name: "unsalted butter".to_string(),
        amounts: vec![
            Measure::new("pound", 0.5),
            Measure::new("sticks", 2.0),
            Measure::new("g", 227.0),
        ],
        modifier: Some("room temperature".to_string())
    }
);

// ============================================================================
// Amount Parsing Tests
// ============================================================================

#[test]
fn test_amount() {
    assert_eq!(
        IngredientParser::new(false).parse_amount("350 °").unwrap(),
        vec![Measure::new("°", 350.0)]
    );
    assert_eq!(
        IngredientParser::new(false).parse_amount("350 °F").unwrap(),
        vec![Measure::new("°f", 350.0)]
    );
}

#[test]
fn test_amount_range_parse() {
    assert_eq!(
        IngredientParser::new(false)
            .parse_amount("2¼-2.5 cups")
            .unwrap(),
        vec![Measure::with_range("cups", 2.25, 2.5)]
    );

    assert_eq!(
        Ingredient::try_from("1-2 cups flour"),
        Ok(Ingredient {
            name: "flour".to_string(),
            amounts: vec![Measure::with_range("cups", 1.0, 2.0)],
            modifier: None,
        })
    );

    let amounts = IngredientParser::new(false)
        .parse_amount("2 ¼ - 2.5 cups")
        .unwrap();
    assert!(!amounts.is_empty(), "Expected at least one measure");
    assert_eq!(format!("{}", amounts[0]), "2.25 - 2.5 cups");

    assert_eq!(
        IngredientParser::new(false)
            .parse_amount("2 to 4 days")
            .unwrap(),
        vec![Measure::with_range("days", 2.0, 4.0)]
    );

    // #30
    assert_eq!(
        IngredientParser::new(false)
            .parse_amount("up to 4 days")
            .unwrap(),
        vec![Measure::with_range("days", 0.0, 4.0)]
    );
}

// ============================================================================
// Display and Formatting Tests
// ============================================================================

#[test]
fn test_no_ingredient_amounts() {
    assert_eq!(
        Ingredient {
            name: "apples".to_string(),
            amounts: vec![],
            modifier: None,
        }
        .to_string(),
        "n/a apples"
    );
}

#[test]
fn test_ingredient_parse() {
    assert_eq!(
        Ingredient::try_from("12 cups flour"),
        Ok(Ingredient {
            name: "flour".to_string(),
            amounts: vec![Measure::new("cups", 12.0)],
            modifier: None,
        })
    );
}

#[test]
fn test_stringy() {
    assert_eq!(
        format!("res: {}", from_str("12 cups flour")),
        "res: 12 cups flour"
    );
    assert_eq!(from_str("one whole egg").to_string(), "1 whole egg");
    assert_eq!(from_str("a tsp flour").to_string(), "1 tsp flour");
}

#[test]
fn test_with_parens() {
    assert_eq!(
        from_str("1 cup (125.5 grams) AP flour, sifted").to_string(),
        "1 cup / 125.5 g AP flour, sifted"
    );
}

// ============================================================================
// Special Characters and Edge Cases
// ============================================================================

#[test]
fn test_weird_chars() {
    vec![
        "confectioners' sugar",
        "confectioners' sugar",
        "gruyère",
        "Jalapeños",
    ]
    .into_iter()
    .for_each(|n| {
        assert_eq!(
            IngredientParser::new(false).from_str(&format!("2 cups/240 grams {n}, sifted")),
            Ingredient {
                name: n.to_string(),
                amounts: vec![
                    Measure::new("cups", 2.0),
                    Measure::new("grams", 240.0)
                ],
                modifier: Some("sifted".to_string())
            }
        );
    });
}

#[test]
fn test_unit_period_mixed_case() {
    assert_eq!(
        IngredientParser::new(false).from_str("1 Tbsp. flour"),
        IngredientParser::new(false).from_str("1 tbsp flour"),
    );
    assert_eq!(
        IngredientParser::new(false).from_str("12 cloves of garlic, peeled"),
        Ingredient {
            name: "garlic".to_string(),
            amounts: vec![Measure::new("cloves", 12.0)],
            modifier: Some("peeled".to_string())
        }
    );
}

// ============================================================================
// Real-World Integration Tests
// ============================================================================

#[test]
fn test_real_world_ingredients() {
    let tests: Vec<(&str, Ingredient)> = vec![
        (
            "14 tablespoons/200 grams unsalted butter, cut into pieces",
            Ingredient {
                name: "unsalted butter".to_string(),
                amounts: vec![
                    Measure::new("tablespoons", 14.0),
                    Measure::new("grams", 200.0),
                ],
                modifier: Some("cut into pieces".to_string()),
            },
        ),
        (
            "6 cups vegetable stock, more if needed",
            Ingredient {
                name: "vegetable stock".to_string(),
                amounts: vec![Measure::new("cups", 6.0)],
                modifier: Some("more if needed".to_string()),
            },
        ),
        (
            "1/4 cup crème fraîche",
            Ingredient {
                name: "crème fraîche".to_string(),
                amounts: vec![Measure::new("cup", 0.25)],
                modifier: None,
            },
        ),
        (
            "⅔ cup (167ml) cold water",
            Ingredient {
                name: "cold water".to_string(),
                amounts: vec![
                    Measure::new("cup", 2.0 / 3.0),
                    Measure::new("ml", 167.0),
                ],
                modifier: None,
            },
        ),
        (
            "1 tsp freshly ground black pepper",
            Ingredient {
                name: "black pepper".to_string(),
                amounts: vec![Measure::new("tsp", 1.0)],
                modifier: Some("freshly ground".to_string()),
            },
        ),
    ];

    for (input, expected) in tests {
        let res = IngredientParser::new(false).from_str(input);
        assert_eq!(res, expected, "Failed to parse: {input}");
    }
}

// ============================================================================
// Rich Text Parsing Tests
// ============================================================================

#[test]
fn test_rich_text() {
    assert_eq!(
        RichParser {
            ingredient_names: vec![],
            ip: IngredientParser::new(true),
        }
        .parse("hello 1 cups foo bar")
        .unwrap(),
        vec![
            Chunk::Text("hello ".to_string()),
            Chunk::Measure(vec![Measure::new("cups", 1.0)]),
            Chunk::Text(" foo bar".to_string())
        ]
    );
    assert_eq!(
        RichParser {
            ingredient_names: vec!["bar".to_string()],
            ip: IngredientParser::new(true),
        }
        .parse("hello 1 cups foo bar")
        .unwrap(),
        vec![
            Chunk::Text("hello ".to_string()),
            Chunk::Measure(vec![Measure::new("cups", 1.0)]),
            Chunk::Text(" foo ".to_string()),
            Chunk::Ing("bar".to_string())
        ]
    );
    assert_eq!(
        RichParser {
            ingredient_names: vec![],
            ip: IngredientParser::new(true),
        }
        .parse("2-2 1/2 cups foo' bar")
        .unwrap(),
        vec![
            Chunk::Measure(vec![Measure::with_range("cups", 2.0, 2.5)]),
            Chunk::Text(" foo' bar".to_string())
        ]
    );
}

#[test]
fn test_rich_text_space() {
    assert_eq!(
        RichParser {
            ingredient_names: vec!["foo bar".to_string()],
            ip: IngredientParser::new(true),
        }
        .parse("hello 1 cups foo bar")
        .unwrap(),
        vec![
            Chunk::Text("hello ".to_string()),
            Chunk::Measure(vec![Measure::new("cups", 1.0)]),
            Chunk::Text(" ".to_string()),
            Chunk::Ing("foo bar".to_string()),
        ]
    );
}

#[test]
fn test_rich_upper_amount() {
    assert_eq!(
        RichParser {
            ingredient_names: vec![],
            ip: IngredientParser::new(true),
        }
        .parse("store for 1-2 days")
        .unwrap(),
        vec![
            Chunk::Text("store for ".to_string()),
            Chunk::Measure(vec![Measure::with_range("days", 1.0, 2.0)]),
        ]
    );
    assert_eq!(
        RichParser {
            ingredient_names: vec!["water".to_string()],
            ip: IngredientParser::new(true),
        }
        .parse("add 1 cup water and store for at most 2 days")
        .unwrap(),
        vec![
            Chunk::Text("add ".to_string()),
            Chunk::Measure(vec![Measure::new("cup", 1.0)]),
            Chunk::Text(" ".to_string()),
            Chunk::Ing("water".to_string()),
            Chunk::Text(" and store for".to_string()),
            Chunk::Measure(vec![Measure::with_range("days", 0.0, 2.0)]),
        ]
    );
}

#[test]
fn test_rich_dimensions() {
    assert_eq!(
        RichParser {
            ingredient_names: vec![],
            ip: IngredientParser::new(true),
        }
        .parse(r#"9" x 13""#)
        .unwrap(),
        vec![
            Chunk::Measure(vec![Measure::new(r#"""#, 9.0)]),
            Chunk::Text(" x ".to_string()),
            Chunk::Measure(vec![Measure::new(r#"""#, 13.0)]),
        ]
    );
}
