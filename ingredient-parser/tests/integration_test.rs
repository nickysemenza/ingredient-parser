use ingredient::{self, Amount, Ingredient, IngredientParser};
#[test]
fn test_many() {
    let tests: Vec<(&str, Ingredient)> = vec![
        (
            "12 cups all purpose flour, lightly sifted",
            Ingredient {
                name: "all purpose flour".to_string(),
                amounts: vec![Amount::new("cups", 12.0)],
                modifier: Some("lightly sifted".to_string()),
            },
        ),
        (
            "14 tablespoons/200 grams unsalted butter, cut into pieces",
            Ingredient {
                name: "unsalted butter".to_string(),
                amounts: vec![
                    Amount {
                        unit: "tablespoons".to_string(),
                        value: 14.0,
                        upper_value: None,
                    },
                    Amount {
                        unit: "grams".to_string(),
                        value: 200.0,
                        upper_value: None,
                    },
                ],
                modifier: Some("cut into pieces".to_string()),
            },
        ),
        (
            "6 cups vegetable stock, more if needed",
            Ingredient {
                name: "vegetable stock".to_string(),
                amounts: vec![Amount {
                    unit: "cups".to_string(),
                    value: 6.0,
                    upper_value: None,
                }],
                modifier: Some("more if needed".to_string()),
            },
        ),
        (
            "1/4 cup crème fraîche",
            Ingredient {
                name: "crème fraîche".to_string(),
                amounts: vec![Amount::new("cup", 0.25)],
                modifier: None,
            },
        ),
    ];

    for x in &tests {
        let parser = IngredientParser::new(false);
        let res = parser.parse_ingredient(x.0).unwrap().1;
        assert_eq!(res, x.1, "Failed to parse {}", x.0);
    }
}
