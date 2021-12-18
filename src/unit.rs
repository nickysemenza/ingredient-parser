use std::collections::HashSet;
use std::iter::FromIterator;

pub fn is_valid(s: &str) -> bool {
    let units: Vec<String> = [
        "oz",
        "ml",
        "ounces",
        "ounce",
        "grams",
        "gram",
        "whole",
        "cups",
        "cup",
        "teaspoons",
        "packet",
        "sticks",
        "stick",
        "cloves",
        "clove",
        "g",
        "$",
        "bunch",
        "head",
        "large",
        "medium",
        "package",
        "pounds",
        "quarts",
        "recipe",
        "slice",
        "standard",
        "tablespoon",
        "tablespoons",
        "tbsp",
        "teaspoon",
        "tsp",
        "kcal",
        "lb",
        "dollars",
        "dollar",
        "cent",
        "cents",
    ]
    .iter()
    .map(|&s| s.into())
    .collect();

    let m: HashSet<String> = HashSet::from_iter(units.iter().cloned());
    return m.contains(&s.to_lowercase());
}
