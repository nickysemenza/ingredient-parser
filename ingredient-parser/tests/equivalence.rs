//! Semantic equivalences exercised through the public ingredient parser.
use ingredient::{Field, IngredientParser, from_str, unit::Measure};
use rstest::rstest;

#[rstest]
#[case("1/2 cup flour", "½ cup flour")]
#[case("1 1/2 cups flour", "1½ cups flour")]
#[case("2 tablespoons oil", "2 tbsp oil")]
#[case("2 cups flour", "2 cup flour")]
#[case("2 cups flour, sifted", "  2  cups  flour,  sifted  ")]
#[case("2 cups flour", "• 2 cups flour")]
#[case("2 cups chopped onions", "chopped onions — 2 cups")]
#[case("1/3-2/3 cup flour", "⅓–⅔ cup flour")]
fn equivalent_spellings_preserve_semantics(#[case] left: &str, #[case] right: &str) {
    assert_eq!(from_str(left), from_str(right), "{left:?} versus {right:?}");
}

#[rstest]
#[case("1 cup chopped walnuts", "(1 cup chopped walnuts)")]
#[case("Juice of 1 lemon", "(Juice of 1 lemon)")]
#[case("chopped walnuts — 1 cup", "(chopped walnuts — 1 cup)")]
#[case("1 cup walnuts, chopped", "1 cup walnuts (optional), chopped")]
fn optional_wrappers_only_change_optionality(#[case] plain: &str, #[case] wrapped: &str) {
    let mut expected = from_str(plain);
    expected.optional = true;
    assert_eq!(from_str(wrapped), expected);
}

#[test]
fn configured_units_use_the_same_structural_path() {
    let parser = IngredientParser::new().with_units(&["glug", "glugs"]);
    assert_eq!(
        parser.from_str("2 glugs chopped herbs"),
        parser.from_str("chopped herbs — 2 glugs")
    );
}

#[test]
fn configured_containers_compose_with_package_sizes() {
    let parser = IngredientParser::new().with_units(&["bundle", "bundles"]);
    let parsed = parser.from_str("2 × 200g (7oz) bundles of herbs");
    assert_eq!(parsed.name, "herbs");
    assert_eq!(
        parsed.amounts,
        [
            Measure::new("bundle", 2.0),
            Measure::new("g", 200.0),
            Measure::new("oz", 7.0),
        ]
    );

    let described = parser.from_str("2 medium (200g) bundles of herbs");
    assert_eq!(described.name, "herbs");
    assert_eq!(described.modifier.as_deref(), Some("medium"));
    assert_eq!(
        described.amounts,
        [Measure::new("bundle", 2.0), Measure::new("g", 200.0)]
    );
    let decomposition = parser.decompose("2 medium (200g) bundles of herbs");
    for span in &decomposition.spans {
        assert_eq!(&decomposition.source[span.range.clone()], span.text);
    }
    assert!(
        decomposition
            .spans
            .windows(2)
            .all(|pair| pair[0].range.end <= pair[1].range.start)
    );
}

#[test]
fn package_separators_and_connectors_are_case_insensitive() {
    assert_eq!(
        from_str("2 × 200g BLOCKS OF tempeh"),
        from_str("2 × 200g blocks of tempeh")
    );
}

#[test]
fn terminal_count_names_remain_ingredient_scoped() {
    let parser = IngredientParser::new().with_units(&["glug", "glugs"]);
    let ingredient = parser.from_str("4 glugs, toasted");
    assert_eq!(ingredient.name, "glugs");
    assert_eq!(ingredient.amounts, [Measure::new("whole", 4.0)]);
    assert_eq!(ingredient.modifier.as_deref(), Some("toasted"));

    // Standalone amount parsing retains its amount-only contract even though an
    // ingredient line with no following food conservatively treats the terminal
    // count noun as its name.
    assert_eq!(
        parser
            .parse_amount("4 glugs")
            .map_err(|error| error.to_string()),
        Ok(vec![Measure::new("glug", 4.0)])
    );

    let decomposition = parser.decompose("4 glugs");
    let spans: Vec<_> = decomposition
        .spans
        .iter()
        .map(|span| (span.field, span.text.as_str()))
        .collect();
    assert_eq!(spans, [(Field::Amount, "4"), (Field::Name, "glugs")]);

    // A following ingredient head keeps the configured word in its unit role.
    let followed = parser.from_str("4 glugs, sparkling water");
    assert_eq!(followed.name, "sparkling water");
    assert_eq!(followed.amounts, [Measure::new("glug", 4.0)]);
}

#[test]
fn terminal_default_count_name_composes_with_modifier() {
    let ingredient = from_str("4 cloves, toasted");
    assert_eq!(ingredient.name, "cloves");
    assert_eq!(ingredient.amounts, [Measure::new("whole", 4.0)]);
    assert_eq!(ingredient.modifier.as_deref(), Some("toasted"));
}

#[test]
fn known_units_remain_valid_standalone_amounts() {
    let ingredient = from_str("4 ounces");
    assert_eq!(ingredient.name, "");
    assert_eq!(ingredient.amounts, [Measure::new("oz", 4.0)]);
}
