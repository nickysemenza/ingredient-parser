//! Source-review signals, not claims of recipe completeness or model accuracy.
use super::ReviewRun;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QualityIssue {
    pub kind: String,
    pub source: String,
    pub chunk: Option<String>,
    pub recipe: Option<usize>,
    pub message: String,
    pub detail: Option<String>,
}

/// Heuristic only: an imperative paragraph may be a method misplaced in notes.
/// Never silently move text or reject a valid optional note based on this signal.
fn method_like_note(note: &str) -> bool {
    let word = note
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    note.len() >= 60
        && matches!(
            word.as_str(),
            "add"
                | "bake"
                | "beat"
                | "blend"
                | "boil"
                | "combine"
                | "cook"
                | "drain"
                | "fold"
                | "freeze"
                | "heat"
                | "knead"
                | "mix"
                | "place"
                | "pour"
                | "remove"
                | "roast"
                | "stir"
                | "transfer"
                | "whisk"
        )
}

pub fn issues(run: &ReviewRun) -> Vec<QualityIssue> {
    let mut issues = vec![];
    for (i, chunk) in run.chunks.iter().enumerate() {
        if chunk.output.is_none() {
            let failed = chunk
                .error
                .as_deref()
                .is_some_and(|e| e != "cache miss (network disabled)");
            issues.push(QualityIssue {
                kind: if failed { "failed_chunk" } else { "unextracted_chunk" }.into(),
                source: chunk.source.doc_path.clone(), chunk: Some(chunk.id.clone()), recipe: None,
                message: if failed { "This source chunk could not be extracted; recipes or recipe parts may be missing." }
                    else { "This source chunk has not been extracted; its recipe coverage is unknown." }.into(),
                detail: None,
            });
            continue;
        }
        let yields = chunk
            .source
            .text
            .lines()
            .filter(|line| {
                let line = line.trim().to_ascii_lowercase();
                line.starts_with("serves ")
                    || line.starts_with("makes ")
                    || line.starts_with("yields ")
            })
            .count();
        if yields > chunk.output.as_ref().map_or(0, Vec::len) {
            issues.push(QualityIssue { kind: "possible_missing_recipe".into(), source: chunk.source.doc_path.clone(), chunk: Some(chunk.id.clone()), recipe: None,
                message: "The source has more serves/makes labels than extracted recipes. Check for omitted recipes or component yields.".into(), detail: None });
        }
        if let Some(hint) = &chunk.source.title_hint {
            let matches =
                |title: &str| crate::continuation_title(title) == crate::continuation_title(hint);
            let head = i
                .checked_sub(1)
                .and_then(|p| run.chunks[p].output.as_ref())
                .and_then(|r| r.last());
            let tail = chunk.output.as_ref().and_then(|r| r.first());
            if !head.is_some_and(|r| matches(&r.meta.title))
                || !tail.is_some_and(|r| matches(&r.meta.title))
            {
                issues.push(QualityIssue { kind: "unresolved_continuation".into(), source: chunk.source.doc_path.clone(), chunk: Some(chunk.id.clone()), recipe: None,
                    message: "A source continuation could not be joined to its preceding recipe. Review both chunks.".into(), detail: Some(hint.clone()) });
            }
        }
    }
    for (i, recipe) in run.recipes.iter().enumerate() {
        let source = recipe
            .url
            .rsplit_once('#')
            .map(|(_, s)| s)
            .unwrap_or_default()
            .to_owned();
        if recipe.sections.iter().any(|s| !s.ingredients.is_empty())
            && recipe.sections.iter().all(|s| s.instructions.is_empty())
        {
            issues.push(QualityIssue { kind: "missing_method".into(), source: source.clone(), chunk: None, recipe: Some(i), message: "This recipe has ingredients but no method. Check the source and adjacent chunks.".into(), detail: Some(recipe.meta.title.clone()) });
        }
        for section in &recipe.sections {
            if !section.ingredients.is_empty()
                && section.name.as_deref().is_some_and(|name| {
                    matches!(
                        name.trim().to_ascii_lowercase().as_str(),
                        "equipment" | "special equipment" | "equipment needed" | "you also need"
                    )
                })
            {
                issues.push(QualityIssue { kind: "possible_equipment_in_ingredients".into(), source: source.clone(), chunk: None, recipe: Some(i), message: "An equipment-like source heading contains ingredients. Check whether these are food, wrappers, or tools.".into(), detail: section.name.clone() });
            }
        }
        for note in recipe
            .meta
            .notes
            .iter()
            .filter(|note| method_like_note(note))
        {
            issues.push(QualityIssue { kind: "possible_method_in_notes".into(), source: source.clone(), chunk: None, recipe: Some(i), message: "Possible method step stored in notes. Check whether this action is required or optional.".into(), detail: Some(note.clone()) });
        }
    }
    issues
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn flags_method_shaped_notes_without_reclassifying_source()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut run = ReviewRun::inspect(
            &recipe_epub_fixtures::cookbook_epub()?,
            "book",
            "gemini-2.5-flash",
        )?;
        assert!(issues(&run).iter().all(|i| i.kind == "unextracted_chunk"));
        run.chunks[0].output = Some(vec![]);
        run.chunks[0].source.title_hint = Some("Prior recipe".into());
        assert!(
            issues(&run)
                .iter()
                .any(|i| i.kind == "unresolved_continuation")
        );
        let note = "Pour the mixture into the prepared molds and freeze until completely set before unmolding.";
        run.recipes.push(serde_json::from_value(serde_json::json!({
            "meta":{"title":"Frozen dessert","notes":[note]},
            "sections":[{"ingredients":["1 cup milk"],"instructions":[]}],
            "source":"book","url":"book#chapter.xhtml","references":[]
        }))?);
        let checks = issues(&run);
        assert!(
            checks
                .iter()
                .any(|i| i.kind == "missing_method" && i.recipe == Some(0))
        );
        assert!(
            checks
                .iter()
                .any(|i| i.kind == "possible_method_in_notes" && i.source == "chapter.xhtml")
        );
        assert_eq!(run.recipes[0].meta.notes, vec![note]);
        run.recipes[0].sections[0].name = Some("You also need".into());
        assert!(
            issues(&run)
                .iter()
                .any(|i| i.kind == "possible_equipment_in_ingredients")
        );
        run.chunks[0].source.text = "Serves 4\n1 cup milk".into();
        assert!(
            issues(&run)
                .iter()
                .any(|i| i.kind == "possible_missing_recipe")
        );
        assert!(method_like_note(
            "Pour the mixture into the prepared molds and freeze until completely set before unmolding."
        ));
        assert!(!method_like_note(
            "If you prefer, serve this with a spoonful of yogurt on the side instead of cream."
        ));
        assert!(!method_like_note(
            "Storage: freeze leftovers for up to one month in a covered container."
        ));
        Ok(())
    }
}
