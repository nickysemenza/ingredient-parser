#![allow(clippy::unwrap_used)]
use ingredient::{self, ingredient::Ingredient, unit::Measure, IngredientParser};
#[test]
fn test_many() {
    let tests: Vec<(&str, Ingredient)> = vec![
        (
            "12 cups all purpose flour, lightly sifted",
            Ingredient {
                name: "all purpose flour".to_string(),
                amounts: vec![Measure::parse_new("cups", 12.0)],
                modifier: Some("lightly sifted".to_string()),
            },
        ),
        (
            "14 tablespoons/200 grams unsalted butter, cut into pieces",
            Ingredient {
                name: "unsalted butter".to_string(),
                amounts: vec![
                    Measure::parse_new("tablespoons", 14.0),
                    Measure::parse_new("grams", 200.0),
                ],
                modifier: Some("cut into pieces".to_string()),
            },
        ),
        (
            "6 cups vegetable stock, more if needed",
            Ingredient {
                name: "vegetable stock".to_string(),
                amounts: vec![Measure::parse_new("cups", 6.0)],
                modifier: Some("more if needed".to_string()),
            },
        ),
        (
            "1/4 cup crème fraîche",
            Ingredient {
                name: "crème fraîche".to_string(),
                amounts: vec![Measure::parse_new("cup", 0.25)],
                modifier: None,
            },
        ),
        (
            "⅔ cup (167ml) cold water",
            Ingredient {
                name: "cold water".to_string(),
                amounts: vec![
                    Measure::parse_new("cup", 2.0 / 3.0),
                    Measure::parse_new("ml", 167.0),
                ],
                modifier: None,
            },
        ),
        (
            "1 tsp freshly ground black pepper",
            Ingredient {
                name: "black pepper".to_string(),
                amounts: vec![Measure::parse_new("tsp", 1.0)],
                modifier: Some("freshly ground".to_string()),
            },
        ),
        (
            "1 tsp chopped pepper",
            Ingredient {
                name: "pepper".to_string(),
                amounts: vec![Measure::parse_new("tsp", 1.0)],
                modifier: Some("chopped".to_string()),
            },
        ),
    ];

    for x in &tests {
        let parser = IngredientParser::new(false);
        let res = parser.from_str(x.0);
        assert_eq!(res, x.1, "Failed to parse {}", x.0);
    }
}
