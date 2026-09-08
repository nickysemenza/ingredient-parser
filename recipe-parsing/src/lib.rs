//! Parse complete recipe sections independently of their web or EPUB source.

use ingredient::rich_text::{Chunk, Rich, RichParseError, RichParser};
use ingredient::{Ingredient, IngredientParser, ParseOptions};
use recipe_types::RecipeSection;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize, PartialEq)]
pub struct ParsedSection {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub ingredients: Vec<Ingredient>,
    pub instructions: Vec<Rich>,
}

#[derive(Debug, Deserialize, Serialize, PartialEq)]
pub struct ParsedRecipe {
    pub sections: Vec<ParsedSection>,
}

/// Observations from the same execution that produced the ingredient.
#[derive(Debug)]
pub struct IngredientObservation {
    pub decomposition: Option<ingredient::Decomposition>,
    pub stages: Option<ingredient::trace::StageReport>,
    pub trace: Option<ingredient::trace::ParseTrace>,
}

#[derive(Debug, Serialize)]
pub struct InstructionDiagnostic {
    pub section: usize,
    pub instruction: usize,
    pub message: String,
}

#[derive(Debug)]
pub struct RecipeExecution {
    pub recipe: ParsedRecipe,
    /// Section/ingredient positions match the parsed recipe exactly.
    pub observations: Vec<Vec<IngredientObservation>>,
    pub instruction_diagnostics: Vec<InstructionDiagnostic>,
}

/// Execute ingredient parsing once, then share its configuration and all names
/// with instruction parsing. Failed instructions remain authored text.
pub fn execute_sections(
    sections: &[RecipeSection],
    parser: &IngredientParser,
    options: ParseOptions,
) -> RecipeExecution {
    let mut observations = Vec::with_capacity(sections.len());
    let mut parsed = Vec::with_capacity(sections.len());
    for section in sections {
        let mut section_observations = Vec::with_capacity(section.ingredients.len());
        let ingredients = section
            .ingredients
            .iter()
            .map(|line| {
                let execution = parser.parse_line(line, options);
                section_observations.push(IngredientObservation {
                    decomposition: execution.decomposition,
                    stages: execution.stages,
                    trace: execution.trace,
                });
                execution.ingredient
            })
            .collect();
        observations.push(section_observations);
        parsed.push(ParsedSection {
            name: section.name.clone(),
            ingredients,
            instructions: Vec::new(),
        });
    }
    let names = parsed
        .iter()
        .flat_map(|s| s.ingredients.iter().map(|i| i.name.clone()));
    let rich = RichParser::with_parser(names, parser);
    let mut instruction_diagnostics = Vec::new();
    for (section_index, (source, target)) in sections.iter().zip(&mut parsed).enumerate() {
        target.instructions = source
            .instructions
            .iter()
            .enumerate()
            .map(|(index, line)| {
                let (chunks, diagnostic) = preserve_instruction(line, rich.parse(line));
                if let Some(message) = diagnostic {
                    instruction_diagnostics.push(InstructionDiagnostic {
                        section: section_index,
                        instruction: index,
                        message,
                    });
                }
                chunks
            })
            .collect();
    }
    RecipeExecution {
        recipe: ParsedRecipe { sections: parsed },
        observations,
        instruction_diagnostics,
    }
}

fn preserve_instruction(
    line: &str,
    result: Result<Rich, RichParseError>,
) -> (Rich, Option<String>) {
    match result {
        Ok(chunks) => (chunks, None),
        Err(error) => (vec![Chunk::Text(line.to_string())], Some(error.to_string())),
    }
}

/// Compatibility entry point using the default parser without observations.
pub fn parse_sections(sections: &[RecipeSection]) -> Vec<ParsedSection> {
    execute_sections(sections, &IngredientParser::new(), ParseOptions::default())
        .recipe
        .sections
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configured_execution_preserves_sections_and_shared_names() {
        let source = vec![
            RecipeSection {
                name: Some("Sauce".into()),
                ingredients: vec!["2 glugs oil".into()],
                instructions: vec![],
            },
            RecipeSection {
                name: Some("Finish".into()),
                ingredients: vec![],
                instructions: vec!["Add 2 glugs oil.".into(), "".into()],
            },
        ];
        let parser = IngredientParser::new().with_units(&["glug", "glugs"]);
        let execution = execute_sections(
            &source,
            &parser,
            ParseOptions {
                decomposition: true,
                trace: ingredient::TraceDetail::Full,
            },
        );
        assert_eq!(execution.recipe.sections.len(), 2);
        assert_eq!(execution.recipe.sections[1].instructions.len(), 2);
        assert!(
            execution.recipe.sections[1].instructions[0]
                .iter()
                .any(|c| matches!(c, Chunk::Ing(name) if name == "oil"))
        );
        assert!(execution.recipe.sections[1].instructions[0].iter().any(
            |c| matches!(c, Chunk::Measure(ms) if ms.iter().any(|m| m.unit().to_str() == "glug"))
        ));
        assert!(execution.observations[0][0].trace.is_some());
        assert!(execution.observations[0][0].decomposition.is_some());
        assert!(execution.instruction_diagnostics.is_empty());
        let plain = execute_sections(&source, &parser, ParseOptions::default());
        assert_eq!(plain.recipe, execution.recipe);
        assert!(plain.observations[0][0].trace.is_none());
        assert!(plain.observations[0][0].decomposition.is_none());
    }

    #[test]
    fn failed_instruction_keeps_text_and_diagnostic() {
        let raw = "Stir 😀";
        let (chunks, diagnostic) = preserve_instruction(
            raw,
            Err(RichParseError::Parse {
                input: raw.into(),
                reason: "test failure".into(),
            }),
        );
        assert_eq!(chunks, vec![Chunk::Text(raw.into())]);
        assert!(diagnostic.is_some_and(|d| d.contains("test failure")));
    }
}
