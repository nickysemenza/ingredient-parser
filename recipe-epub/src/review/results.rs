//! Comparable latest-run snapshots. Processing success is not source fidelity.
use super::{ReviewRun, store};
use crate::EpubError;
use serde::Serialize;
use std::{collections::BTreeMap, path::Path};

#[derive(Debug, Serialize)]
pub struct ModelBookResult {
    pub latest: store::RunSummary,
    pub runs: usize,
    pub failed_chunks: usize,
    pub pending_chunks: usize,
    pub content_review_flags: usize,
    pub processing_success_rate: Option<f64>,
    pub attempts: Option<usize>,
    pub failed_attempts: Option<usize>,
}
#[derive(Debug, Serialize)]
pub struct ModelBookResults {
    pub rows: Vec<ModelBookResult>,
    pub unreadable: Vec<String>,
}

pub fn list(book: Option<&Path>) -> Result<ModelBookResults, EpubError> {
    Ok(summarize(store::list(book)?))
}
fn summarize(summaries: Vec<store::RunSummary>) -> ModelBookResults {
    let mut groups = BTreeMap::<_, Vec<_>>::new();
    for row in summaries {
        // A run with no successful output still belongs beside the same configured
        // model/prompt. Preserve distinct inherited or historically unknown provenance.
        let mut configurations = row.configurations.clone();
        configurations.push(format!("{} / {}", row.model, row.prompt_version));
        configurations.sort();
        configurations.dedup();
        groups
            .entry((
                row.epub_sha256.clone(),
                row.model.clone(),
                row.prompt_version.clone(),
                configurations,
            ))
            .or_default()
            .push(row);
    }
    let mut report = ModelBookResults {
        rows: vec![],
        unreadable: vec![],
    };
    for mut rows in groups.into_values() {
        rows.sort_by(|a, b| b.created_at.cmp(&a.created_at).then(a.path.cmp(&b.path)));
        let count = rows.len();
        let Some(latest) = rows.into_iter().next() else {
            continue;
        };
        match ReviewRun::read(&latest.path) {
            Ok(run) => report.rows.push(snapshot(&run, &latest.path, count)),
            Err(error) => report
                .unreadable
                .push(format!("{}: {error}", latest.path.display())),
        }
    }
    report.rows.sort_by(|a, b| {
        a.latest
            .title
            .cmp(&b.latest.title)
            .then(a.latest.model.cmp(&b.latest.model))
            .then(a.latest.prompt_version.cmp(&b.latest.prompt_version))
    });
    report
}
fn snapshot(run: &ReviewRun, path: &Path, runs: usize) -> ModelBookResult {
    let latest = store::summary(run, path);
    let issues = super::quality::issues(run);
    let failed_chunks = issues.iter().filter(|i| i.kind == "failed_chunk").count();
    let pending_chunks = latest
        .total
        .saturating_sub(latest.completed + failed_chunks);
    let processed = latest.completed + failed_chunks;
    let known_attempts =
        !run.charges.is_empty() && run.charges.iter().all(|c| !c.attempts.is_empty());
    ModelBookResult {
        processing_success_rate: (processed > 0)
            .then(|| latest.completed as f64 / processed as f64),
        attempts: known_attempts.then(|| run.charges.iter().map(|c| c.attempts.len()).sum()),
        failed_attempts: known_attempts.then(|| {
            run.charges
                .iter()
                .flat_map(|c| &c.attempts)
                .filter(|a| a.error.is_some())
                .count()
        }),
        latest,
        runs,
        failed_chunks,
        pending_chunks,
        content_review_flags: issues
            .iter()
            .filter(|i| i.kind != "failed_chunk" && i.kind != "unextracted_chunk")
            .count(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn separates_prompts_and_does_not_hide_latest_failure() -> Result<(), Box<dyn std::error::Error>>
    {
        let root = std::env::temp_dir().join(format!("model-results-{}", store::new_id()));
        std::fs::create_dir_all(&root)?;
        let mut run = ReviewRun::inspect(&recipe_epub_fixtures::cookbook_epub()?, "book", "model")?;
        let mut rows = vec![];
        for (index, prompt, success) in [(1, "v1", true), (2, "v1", false), (3, "v2", true)] {
            run.prompt_version = prompt.into();
            run.chunks[0].model = Some(run.model.clone());
            run.chunks[0].prompt_version = Some(prompt.into());
            run.chunks[0].output = success.then(Vec::new);
            run.chunks[0].error = (!success).then(|| "request timeout".into());
            let path = root.join(format!("{index}.json"));
            std::fs::write(&path, serde_json::to_vec(&run)?)?;
            let mut row = store::summary(&run, &path);
            row.created_at = Some(index);
            rows.push(row);
        }
        let report = summarize(rows);
        assert_eq!(report.rows.len(), 2);
        assert_eq!(report.rows[0].runs, 2);
        assert_eq!(report.rows[0].failed_chunks, 1);
        assert_eq!(report.rows[0].processing_success_rate, Some(0.0));
        assert_eq!(report.rows[1].processing_success_rate, Some(1.0));
        std::fs::remove_dir_all(root)?;
        Ok(())
    }
    #[test]
    fn distinguishes_partial_success_pending_and_unknown_attempts()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut run = ReviewRun::inspect(&recipe_epub_fixtures::cookbook_epub()?, "book", "model")?;
        let empty = snapshot(&run, Path::new("run.json"), 1);
        assert_eq!(empty.processing_success_rate, None);
        assert_eq!(empty.attempts, None);
        run.chunks[0].output = Some(vec![]);
        let partial = snapshot(&run, Path::new("run.json"), 2);
        assert_eq!(partial.processing_success_rate, Some(1.0));
        assert_eq!(partial.latest.completed, 1);
        assert_eq!(partial.pending_chunks, run.chunks.len() - 1);
        run.chunks[0].output = None;
        run.chunks[0].error = Some("request timeout".into());
        let failed = snapshot(&run, Path::new("run.json"), 2);
        assert_eq!(failed.failed_chunks, 1);
        assert_eq!(failed.processing_success_rate, Some(0.0));
        run.chunks[0].error = Some("request pending; reservation retained if interrupted".into());
        assert_eq!(snapshot(&run, Path::new("run.json"), 2).failed_chunks, 0);
        Ok(())
    }

    #[test]
    fn inherited_success_is_not_attributed_to_an_unproven_requested_model()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut run =
            ReviewRun::inspect(&recipe_epub_fixtures::cookbook_epub()?, "book", "new model")?;
        run.chunks[0].output = Some(vec![]);
        run.chunks[0].model = Some("parent model".into());
        run.chunks[0].prompt_version = Some("parent prompt".into());
        let row = snapshot(&run, Path::new("child.json"), 1);
        assert_eq!(row.latest.configurations.len(), 2);
        assert!(
            row.latest
                .configurations
                .contains(&"parent model / parent prompt".into())
        );
        assert!(
            row.latest
                .configurations
                .iter()
                .any(|c| c.starts_with("new model / "))
        );
        Ok(())
    }
}
