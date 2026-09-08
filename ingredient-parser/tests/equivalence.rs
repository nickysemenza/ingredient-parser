//! Semantic equivalences exercised through the public ingredient parser.
use ingredient::{IngredientParser, from_str};
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
