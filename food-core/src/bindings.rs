//! TypeScript declarations for every type the portable commands carry. The
//! desktop app appends its native-only types and writes the one checked-in
//! frontend contract (`ui/src/api/generated.ts`).
use serde_json::Value;
use ts_rs::{Config, TS};

use crate::{books, corpus, ingredients as ing, recipes};

/// One declaration and the names of the types it references.
pub type Declaration = (String, Vec<String>);

pub fn config() -> Config {
    // Tauri and the worker both carry JSON, where large integers are numbers.
    Config::new().with_large_int("number")
}

pub fn entry<T: TS + 'static>(cfg: &Config) -> Declaration {
    (
        T::decl(cfg),
        T::dependencies(cfg)
            .into_iter()
            .map(|d| d.ts_name)
            .collect(),
    )
}

pub fn declarations(cfg: &Config) -> Vec<Declaration> {
    use cookbook as cb;
    vec![
        (Value::decl(cfg), vec![]),
        entry::<ing::IngredientRow>(cfg),
        entry::<ing::ReviewReason>(cfg),
        entry::<ing::SegmentField>(cfg),
        entry::<ing::Segment>(cfg),
        entry::<ing::Inspection>(cfg),
        entry::<ing::Stages>(cfg),
        entry::<ing::Rewrite>(cfg),
        entry::<ing::Recognizer>(cfg),
        entry::<ing::Grammar>(cfg),
        entry::<ing::TraceNode>(cfg),
        entry::<ing::TraceStatus>(cfg),
        entry::<recipes::RichChunk>(cfg),
        entry::<recipes::RecipeView>(cfg),
        entry::<recipes::SectionView>(cfg),
        entry::<corpus::CorpusField>(cfg),
        entry::<corpus::CorpusCase>(cfg),
        entry::<corpus::CorpusResult>(cfg),
        entry::<books::OpenedBook>(cfg),
        entry::<books::BookImage>(cfg),
        entry::<books::SourceLine>(cfg),
        entry::<books::OpenedRun>(cfg),
        entry::<ingredient::ingredient::Ingredient>(cfg),
        entry::<ingredient::unit::Measure>(cfg),
        entry::<ingredient::IngredientUsage>(cfg),
        entry::<ingredient::Confidence>(cfg),
        entry::<recipe_types::RecipeTimes>(cfg),
        entry::<cb::RunSummary>(cfg),
        entry::<cb::classify::Classified>(cfg),
        entry::<cb::classify::Classification>(cfg),
        entry::<cb::BookOutline>(cfg),
        entry::<cb::Estimate>(cfg),
        entry::<cb::Progress>(cfg),
        entry::<cb::Phase>(cfg),
        entry::<cb::Eta>(cfg),
        entry::<cb::Extraction>(cfg),
        entry::<cb::Cookbook>(cfg),
        entry::<cb::BookSource>(cfg),
        entry::<cb::Chapter>(cfg),
        entry::<cb::Item>(cfg),
        entry::<cb::Recipe>(cfg),
        entry::<cb::Technique>(cfg),
        entry::<cb::Essay>(cfg),
        entry::<cb::RecipeMeta>(cfg),
        entry::<cb::Section>(cfg),
        entry::<cb::IngredientLine>(cfg),
        entry::<cb::Step>(cfg),
        entry::<cb::Note>(cfg),
        entry::<cb::RecipeRef>(cfg),
        entry::<cb::RefKind>(cfg),
        entry::<cb::RefMethod>(cfg),
        entry::<cb::Edge>(cfg),
        entry::<cb::ImageRef>(cfg),
        entry::<cb::Span>(cfg),
        entry::<cb::RunReport>(cfg),
        entry::<cb::ExtractOptions>(cfg),
        entry::<cb::StageTiming>(cfg),
        entry::<cb::CallRecord>(cfg),
        entry::<cb::CallPurpose>(cfg),
        entry::<cb::CallOutcome>(cfg),
        entry::<cb::Usage>(cfg),
        entry::<cb::ChunkReport>(cfg),
        entry::<cb::ChunkStatus>(cfg),
        entry::<cb::Flag>(cfg),
        entry::<cb::SecondOpinion>(cfg),
        entry::<cb::Chosen>(cfg),
        entry::<cb::CrossCheck>(cfg),
        entry::<cb::Escalation>(cfg),
        entry::<cb::UnresolvedRef>(cfg),
        entry::<cb::ModelUsage>(cfg),
        entry::<cb::EtaSample>(cfg),
        entry::<cb::models::Reasoning>(cfg),
    ]
}

/// The contract file for `declarations`, each exported.
pub fn source(header: &str, declarations: Vec<Declaration>) -> String {
    format!(
        "{header}\n{}\n",
        declarations
            .into_iter()
            .map(|(decl, _)| format!(
                "export {}",
                decl.lines()
                    .map(str::trim_end)
                    .collect::<Vec<_>>()
                    .join("\n")
            ))
            .collect::<Vec<_>>()
            .join("\n")
    )
}

/// A referenced type that is not declared, if any.
pub fn undeclared(declarations: &[Declaration]) -> Option<String> {
    let names: Vec<String> = declarations
        .iter()
        .filter_map(|(decl, _)| {
            decl.strip_prefix("type ")
                .and_then(|rest| rest.split([' ', '<']).next())
                .map(str::to_owned)
        })
        .collect();
    declarations
        .iter()
        .flat_map(|(_, deps)| deps)
        .find(|dep| !names.contains(dep))
        .cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every referenced type is declared, and large integers come out as
    /// `number` (JSON numbers, not bigint).
    #[test]
    fn portable_declarations_are_closed() {
        let declarations = declarations(&config());
        assert_eq!(undeclared(&declarations), None);
        let text = source("", declarations);
        assert!(!text.contains("bigint"), "{text}");
        assert!(text.contains("export type IngredientRow = "));
        assert!(text.contains("export type Extraction = "));
    }
}
