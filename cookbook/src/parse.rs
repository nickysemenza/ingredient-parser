//! Ingredient-line parsing through the core `ingredient` parser: one parser
//! per run, one call per line.

use ingredient::{Confidence, Ingredient, IngredientParser};

pub struct LineParser(IngredientParser);

impl Default for LineParser {
    fn default() -> Self {
        Self::new()
    }
}

impl LineParser {
    pub fn new() -> Self {
        Self(IngredientParser::new())
    }

    /// The structured reading of a verbatim line and how sure the parser is.
    pub fn parse(&self, raw: &str) -> (Ingredient, Confidence) {
        let parsed = self.0.from_str(raw);
        let confidence = parsed.parse_notes.confidence;
        (parsed, confidence)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_amounts_and_reports_confidence() {
        let p = LineParser::new();
        let (i, c) = p.parse("2 cups all-purpose flour, sifted");
        assert_eq!(i.name, "all-purpose flour");
        assert_eq!(i.amounts.len(), 1);
        assert_eq!(c, Confidence::High);
        let (_, c) = p.parse("Kosher salt");
        assert_eq!(c, Confidence::Medium);
    }
}
