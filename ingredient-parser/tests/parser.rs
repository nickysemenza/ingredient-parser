use ingredient::{self, ingredient::Ingredient, unit::Measure, IngredientParser};
macro_rules! test_parse_ingredient {
    ($test_name:ident, $input:expr, $expected_output:expr) => {
        #[test]
        fn $test_name() {
            assert_eq!(
                (IngredientParser::new(false)).parse_ingredient($input),
                Ok(("", $expected_output))
            );
        }
    };
}
macro_rules! test_parsing_equals {
    ($test_name:ident, $left:expr, $right:expr) => {
        #[test]
        fn $test_name() {
            assert_eq!(
                (IngredientParser::new(false)).parse_ingredient($left),
                (IngredientParser::new(false)).parse_ingredient($right),
            );
        }
    };
}

test_parsing_equals!(test_amount_range, "1/2-1 cup", "1/2 - 1 cup");
test_parsing_equals!(test_amount_range_vfrac, "2¼-2.5 cups", "2 ¼ - 2.5 cups");
test_parsing_equals!(test_amount_range_g, "78g to 104g", "78g - 104g");
test_parsing_equals!(test_unitless, "1 cinnamon stick", "1 whole cinnamon stick");
test_parsing_equals!(multiply, "2 x 200g flour", "400g flour");

test_parse_ingredient!(
    ingredient_parse_no_amounts,
    "egg",
    Ingredient {
        name: "egg".to_string(),
        amounts: vec![],
        modifier: None,
    }
);
test_parse_ingredient!(
    ingredient_parse_no_unit,
    "1 egg",
    Ingredient {
        name: "egg".to_string(),
        amounts: vec![Measure::parse_new("whole", 1.0)],
        modifier: None,
    }
);
test_parse_ingredient!(
    ingredient_parse_no_unit_multi_name_adj,
    "1 cinnamon stick, crushed",
    Ingredient {
        name: "cinnamon stick".to_string(),
        amounts: vec![Measure::parse_new("whole", 1.0)],
        modifier: Some("crushed".to_string()),
    }
);
test_parse_ingredient!(
    test_sum,
    "1 tablespoon plus 1 teaspoon olive oil",
    Ingredient {
        name: "olive oil".to_string(),
        amounts: vec![Measure::parse_new("teaspoon", 4.0),],
        modifier: None
    }
);
test_parse_ingredient!(
    multi1,
    "12 cups all purpose flour, lightly sifted",
    Ingredient {
        name: "all purpose flour".to_string(),
        amounts: vec![Measure::parse_new("cups", 12.0)],
        modifier: Some("lightly sifted".to_string()),
    }
);

test_parse_ingredient!(
    multi2,
    "1¼  cups / 155.5 grams flour",
    Ingredient {
        name: "flour".to_string(),
        amounts: vec![
            Measure::parse_new("cups", 1.25),
            Measure::parse_new("grams", 155.5),
        ],
        modifier: None,
    }
);

test_parse_ingredient!(
    multi3,
    "0.25 ounces (1 packet, about 2 teaspoons) instant or rapid rise yeast",
    Ingredient {
        name: "instant or rapid rise yeast".to_string(),
        amounts: vec![
            Measure::parse_new("ounces", 0.25),
            Measure::parse_new("packet", 1.0),
            Measure::parse_new("teaspoons", 2.0),
        ],
        modifier: None
    }
);
test_parse_ingredient!(
    multi4,
    "6 ounces unsalted butter (1½ sticks; 168.75g)",
    Ingredient {
        name: "unsalted butter".to_string(),
        amounts: vec![
            Measure::parse_new("ounces", 6.0),
            Measure::parse_new("sticks", 1.5),
            Measure::parse_new("g", 168.75),
        ],
        modifier: None
    }
);
test_parse_ingredient!(
    multi5,
    "½ pound 2 sticks; 227 g unsalted butter, room temperature",
    Ingredient {
        name: "unsalted butter".to_string(),
        amounts: vec![
            Measure::parse_new("pound", 0.5),
            Measure::parse_new("sticks", 2.0),
            Measure::parse_new("g", 227.0),
        ],
        modifier: Some("room temperature".to_string())
    }
);

test_parse_ingredient!(
    test_unit_without_number,
    "pinch nutmeg",
    Ingredient {
        name: "nutmeg".to_string(),
        amounts: vec![Measure::parse_new("pinch", 1.0),],
        modifier: None
    }
);

test_parse_ingredient!(
    // "whole" can sometimes be an ingredient
    test_parse_whole_wheat_ambigious,
    "100 grams whole wheat flour",
    Ingredient {
        name: "whole wheat flour".to_string(),
        amounts: vec![Measure::parse_new("grams", 100.0),],
        modifier: None
    }
);

test_parsing_equals!(
    test_unit_without_number_of,
    "pinch nutmeg",
    "pinch of nutmeg"
);

test_parse_ingredient!(
    test_parse_ingredient_cloves,
    "1 clove garlic, grated",
    Ingredient {
        name: "garlic".to_string(),
        amounts: vec![Measure::parse_new("clove", 1.0),],
        modifier: Some("grated".to_string())
    }
);
// todo: doesn't work
// test_parse_ingredient!(
//     cloves_2,
//     "1 clove, grated",
//     Ingredient {
//         name: "clove".to_string(),
//         amounts: vec![Measure::parse_new("whole", 1.0),],
//         modifier: Some("grated".to_string())
//     }
// );

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
fn test_ingredient_parse_multi() {
    assert_eq!(
        (IngredientParser::new(false)).parse_ingredient("1 ½ cups/192 grams all-purpose flour"),
        (IngredientParser::new(false)).parse_ingredient("1 1/2 cups / 192 grams all-purpose flour")
    );
}
#[test]
fn test_weird_chars() {
    vec![
        "confectioners’ sugar",
        "confectioners' sugar",
        "gruyère",
        "Jalapeños",
    ]
    .into_iter()
    .for_each(|n| {
        assert_eq!(
            (IngredientParser::new(false))
                .parse_ingredient(&format!("2 cups/240 grams {n}, sifted")),
            Ok((
                "",
                Ingredient {
                    name: n.to_string(),
                    amounts: vec![
                        Measure::parse_new("cups", 2.0),
                        Measure::parse_new("grams", 240.0)
                    ],
                    modifier: Some("sifted".to_string())
                }
            ))
        );
    });
}
#[test]
fn test_parse_ing_upepr_range() {
    assert_eq!(
        (IngredientParser::new(false)).parse_ingredient("78g to 104g cornmeal"),
        Ok((
            "",
            Ingredient {
                name: "cornmeal".to_string(),
                amounts: vec![Measure::parse_new_with_upper("g", 78.0, 104.0),],
                modifier: None
            }
        ))
    );
    assert_eq!(
        (IngredientParser::new(false)).parse_ingredient("78g to 104g cornmeal"),
        (IngredientParser::new(false)).parse_ingredient("78 to 104g cornmeal"),
    )
}
#[test]
fn test_unit_period_mixed_case() {
    assert_eq!(
        (IngredientParser::new(false)).parse_ingredient("1 Tbsp. flour"),
        (IngredientParser::new(false)).parse_ingredient("1 tbsp flour"),
    );
    assert_eq!(
        (IngredientParser::new(false)).parse_ingredient("12 cloves of garlic, peeled"),
        Ok((
            "",
            Ingredient {
                name: "garlic".to_string(),
                amounts: vec![Measure::parse_new("cloves", 12.0),],
                modifier: Some("peeled".to_string())
            }
        ))
    );
}
