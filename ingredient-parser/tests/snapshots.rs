//! Golden snapshot of the parser decision-tree trace. Parse output accuracy
//! is asserted by the labeled corpus rather than snapshots.

use ingredient::IngredientParser;

/// The parser decision-tree trace for a representative line.
#[test]
fn trace_tree() {
    let traced = IngredientParser::new().parse_with_trace("2 cups flour, sifted");
    insta::assert_snapshot!(traced.trace.format_tree(false));
}
