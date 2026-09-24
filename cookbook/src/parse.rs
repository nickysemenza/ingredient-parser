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
