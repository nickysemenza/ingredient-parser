//! The evaluation harness: hand-authored expectations per book, scored
//! against an extraction, with the gate the ladder must pass.
//!
//! Expectations are authored from the source (the publisher's contents and
//! the XHTML), never from an extraction, and live outside the repository
//! because the books do.

use serde::{Deserialize, Serialize};

use crate::crosscheck::titles_match;

/// Section names compare leniently: the key writes the group's name, the
/// source may print it with a column heading ("final dough baker's formula").
fn sections_match(got: &[Option<String>], want: &[Option<String>]) -> bool {
    if got.len() != want.len() {
        return false;
    }
    let mut used = vec![false; got.len()];
    want.iter().all(|w| {
        got.iter().enumerate().any(|(i, g)| {
            let hit = !used[i]
                && match (g, w) {
                    (None, None) => true,
                    (Some(g), Some(w)) => g == w || g.starts_with(w) || w.starts_with(g),
                    _ => false,
                };
            if hit {
                used[i] = true;
            }
            hit
        })
    })
}
use crate::model::{Item, RefKind, RefMethod};
use crate::report::{ChunkStatus, Extraction};

/// One book's answer key.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Expectations {
    /// Path to the EPUB (absolute, or relative to the expectations file).
    pub book: String,
    pub sha256: Option<String>,
    /// `publisher-epub3`, `calibre-typographic`, `calibre-page-split`, …
    pub shape: String,
    /// Every recipe title, in reading order.
    pub titles: Vec<String>,
    /// Titled variations with their own ingredient lists.
    pub variants: Vec<Variant>,
    /// Titled things that must not come out as recipes.
    pub not_recipes: Vec<String>,
    pub samples: Vec<Sample>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Variant {
    pub title: String,
    pub of: String,
}

/// A hand-checked recipe.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Sample {
    pub title: String,
    /// Section names in order; `null` for the unnamed main section.
    pub sections: Option<Vec<Option<String>>>,
    pub ingredients: Option<usize>,
    pub steps: Option<usize>,
    pub notes_contain: Vec<String>,
    pub refs: Vec<RefExpect>,
    pub photos: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefExpect {
    /// Substring of the referencing line or step.
    pub line_contains: String,
    /// Title of the referenced recipe.
    pub target: String,
    pub kind: RefKind,
    /// When given, a mismatch is a warning, not a failure.
    pub method: Option<RefMethod>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BookScore {
    pub book: String,
    pub shape: String,
    pub title_recall: f32,
    pub titles: usize,
    pub matched: usize,
    pub missing: Vec<String>,
    pub phantoms: Vec<String>,
    pub not_recipe_leaks: Vec<String>,
    pub variant_accuracy: Option<f32>,
    pub sample_pass: Option<f32>,
    pub sample_failures: Vec<String>,
    pub sample_warnings: Vec<String>,
    pub line_coverage: f32,
    pub wall_ms: u64,
    pub cost_usd: f64,
    pub calls: usize,
    pub escalated: bool,
    pub incomplete: bool,
    /// |actual − estimate midpoint| / actual.
    pub eta_error: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Gate {
    pub pass: bool,
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalReport {
    pub ladder: Vec<String>,
    pub books: Vec<BookScore>,
    pub mean_recall: f32,
    pub total_phantoms: usize,
    pub mean_sample_pass: Option<f32>,
    pub total_cost_usd: f64,
    pub max_wall_ms: u64,
    pub gate: Gate,
}

pub const GATE_MEAN_RECALL: f32 = 0.97;
pub const GATE_PHANTOMS_PER_BOOK: f32 = 1.0;

pub fn score(expected: &Expectations, extraction: &Extraction) -> BookScore {
    let cookbook = &extraction.cookbook;
    let report = &extraction.report;
    let recipes: Vec<_> = cookbook.recipes().collect();
    let mut used = vec![false; recipes.len()];
    let mut matched = 0usize;
    let mut missing = Vec::new();
    for title in &expected.titles {
        match recipes
            .iter()
            .enumerate()
            .find(|(i, r)| !used[*i] && titles_match(title, &r.title))
        {
            Some((i, _)) => {
                used[i] = true;
                matched += 1;
            }
            None => missing.push(title.clone()),
        }
    }
    let mut variant_hits = 0usize;
    for v in &expected.variants {
        if let Some((i, r)) = recipes
            .iter()
            .enumerate()
            .find(|(i, r)| !used[*i] && titles_match(&v.title, &r.title))
        {
            used[i] = true;
            let parent_ok = r
                .variant_of
                .as_deref()
                .and_then(|id| cookbook.item(id))
                .is_some_and(|p| titles_match(&v.of, p.title()));
            if parent_ok {
                variant_hits += 1;
            }
        }
    }
    let phantoms: Vec<String> = recipes
        .iter()
        .enumerate()
        .filter(|(i, _)| !used[*i])
        .map(|(_, r)| r.title.clone())
        .collect();
    let not_recipe_leaks: Vec<String> = expected
        .not_recipes
        .iter()
        .filter(|t| recipes.iter().any(|r| titles_match(t, &r.title)))
        .cloned()
        .collect();

    let mut sample_failures = Vec::new();
    let mut sample_warnings = Vec::new();
    let mut sample_ok = 0usize;
    for s in &expected.samples {
        let Some(r) = recipes.iter().find(|r| titles_match(&s.title, &r.title)) else {
            sample_failures.push(format!("{}: not extracted", s.title));
            continue;
        };
        let mut ok = true;
        let ingredients: usize = r.sections.iter().map(|x| x.ingredients.len()).sum();
        let steps: usize = r.sections.iter().map(|x| x.steps.len()).sum();
        if let Some(n) = s.ingredients
            && ingredients.abs_diff(n) > 1
        {
            ok = false;
            sample_failures.push(format!(
                "{}: {ingredients} ingredient lines, expected {n}",
                s.title
            ));
        }
        if let Some(n) = s.steps
            && steps.abs_diff(n) > 1
        {
            ok = false;
            sample_failures.push(format!("{}: {steps} steps, expected {n}", s.title));
        }
        if let Some(names) = &s.sections {
            // A shared method lives in an unnamed section with no ingredients;
            // answer keys describe ingredient groups, so compare those only.
            let mut got: Vec<Option<String>> = r
                .sections
                .iter()
                .filter(|x| !x.ingredients.is_empty())
                .map(|x| x.name.as_ref().map(|n| n.to_lowercase()))
                .collect();
            let mut want: Vec<Option<String>> = names
                .iter()
                .map(|n| n.as_ref().map(|n| n.to_lowercase()))
                .collect();
            got.sort();
            want.sort();
            if !sections_match(&got, &want) {
                ok = false;
                sample_failures.push(format!("{}: sections {got:?}, expected {want:?}", s.title));
            }
        }
        for needle in &s.notes_contain {
            let hit = r.notes.iter().any(|n| {
                n.text.contains(needle) || n.label.as_deref().is_some_and(|l| l.contains(needle))
            });
            if !hit {
                ok = false;
                sample_failures.push(format!("{}: no note containing {needle:?}", s.title));
            }
        }
        if let Some(n) = s.photos
            && r.photos.len() != n
        {
            ok = false;
            sample_failures.push(format!(
                "{}: {} photos, expected {n}",
                s.title,
                r.photos.len()
            ));
        }
        for e in &s.refs {
            let found = r
                .sections
                .iter()
                .flat_map(|x| {
                    x.ingredients
                        .iter()
                        .filter_map(|l| l.reference.as_ref().map(|q| (l.raw.as_str(), q)))
                        .chain(
                            x.steps
                                .iter()
                                .flat_map(|st| st.refs.iter().map(move |q| (st.text.as_str(), q))),
                        )
                })
                .chain(
                    r.notes
                        .iter()
                        .flat_map(|n| n.refs.iter().map(move |q| (n.text.as_str(), q))),
                )
                .find(|(text, q)| {
                    text.contains(&e.line_contains)
                        && q.kind == e.kind
                        && cookbook
                            .item(&q.target_id)
                            .is_some_and(|t| titles_match(&e.target, t.title()))
                });
            match found {
                None => {
                    ok = false;
                    sample_failures.push(format!(
                        "{}: no {:?} reference from {:?} to {:?}",
                        s.title, e.kind, e.line_contains, e.target
                    ));
                }
                Some((_, q)) => {
                    if let Some(m) = e.method
                        && q.method != m
                    {
                        sample_warnings.push(format!(
                            "{}: reference to {:?} resolved by {:?}, expected {:?}",
                            s.title, e.target, q.method, m
                        ));
                    }
                }
            }
        }
        if ok {
            sample_ok += 1;
        }
    }

    let total_lines: usize = report.chunks.iter().map(|c| c.end - c.start).sum();
    let unassigned: usize = report
        .chunks
        .iter()
        .filter(|c| c.status == ChunkStatus::Failed)
        .map(|c| c.end - c.start)
        .sum();
    let line_coverage = if total_lines == 0 {
        1.0
    } else {
        1.0 - unassigned as f32 / total_lines as f32
    };
    let mid = (report.estimate.wall_ms_low + report.estimate.wall_ms_high) as f32 / 2.0;
    let eta_error =
        (report.wall_ms > 0).then(|| (report.wall_ms as f32 - mid).abs() / report.wall_ms as f32);
    BookScore {
        book: expected.book.clone(),
        shape: expected.shape.clone(),
        title_recall: if expected.titles.is_empty() {
            1.0
        } else {
            matched as f32 / expected.titles.len() as f32
        },
        titles: expected.titles.len(),
        matched,
        missing,
        phantoms,
        not_recipe_leaks,
        variant_accuracy: (!expected.variants.is_empty())
            .then(|| variant_hits as f32 / expected.variants.len() as f32),
        sample_pass: (!expected.samples.is_empty())
            .then(|| sample_ok as f32 / expected.samples.len() as f32),
        sample_failures,
        sample_warnings,
        line_coverage,
        wall_ms: report.wall_ms,
        cost_usd: report.total_cost_usd,
        calls: report.calls.len(),
        escalated: report.escalation.is_some(),
        incomplete: report.incomplete,
        eta_error,
    }
}

pub fn summarize(ladder: Vec<String>, books: Vec<BookScore>) -> EvalReport {
    let n = books.len().max(1) as f32;
    let mean_recall = books.iter().map(|b| b.title_recall).sum::<f32>() / n;
    let total_phantoms = books.iter().map(|b| b.phantoms.len()).sum();
    let with_samples: Vec<f32> = books.iter().filter_map(|b| b.sample_pass).collect();
    let mean_sample_pass = (!with_samples.is_empty())
        .then(|| with_samples.iter().sum::<f32>() / with_samples.len() as f32);
    let total_cost_usd = books.iter().map(|b| b.cost_usd).sum();
    let max_wall_ms = books.iter().map(|b| b.wall_ms).max().unwrap_or(0);
    let mut reasons = Vec::new();
    if books.is_empty() {
        reasons.push("no books evaluated".into());
    }
    if mean_recall < GATE_MEAN_RECALL {
        reasons.push(format!(
            "mean title recall {:.1}% below {:.0}%",
            mean_recall * 100.0,
            GATE_MEAN_RECALL * 100.0
        ));
    }
    if total_phantoms as f32 > GATE_PHANTOMS_PER_BOOK * books.len() as f32 {
        reasons.push(format!(
            "{total_phantoms} phantom recipes over {} books",
            books.len()
        ));
    }
    let leaks: usize = books.iter().map(|b| b.not_recipe_leaks.len()).sum();
    if leaks > 0 {
        reasons.push(format!("{leaks} non-recipes came out as recipes"));
    }
    if books.iter().any(|b| b.line_coverage < 1.0) {
        reasons.push("some chunks failed every model".into());
    }
    EvalReport {
        ladder,
        books,
        mean_recall,
        total_phantoms,
        mean_sample_pass,
        total_cost_usd,
        max_wall_ms,
        gate: Gate {
            pass: reasons.is_empty(),
            reasons,
        },
    }
}

/// Every recipe as a starting point for an answer key: contents titles are
/// the publisher's own list; samples are left for a person to fill in.
pub fn skeleton(book: &crate::Book, path: &str) -> Expectations {
    Expectations {
        book: path.to_string(),
        sha256: Some(book.source().sha256.clone()),
        shape: String::new(),
        titles: crate::crosscheck::nav_recipe_titles(book.lines(), book.nav())
            .into_iter()
            .map(|(t, _)| t)
            .collect(),
        variants: Vec::new(),
        not_recipes: Vec::new(),
        samples: Vec::new(),
    }
}

/// A skeleton seeded from a saved run of the same file: `not_recipes`
/// candidates are the techniques and essays the run produced, and eight
/// `samples` stubs are spread through the contents titles with every count
/// left empty. The author fills the counts from the HTML, never from the
/// run; the run only suggests where to look.
pub fn skeleton_from_run(
    book: &crate::Book,
    path: &str,
    extraction: &Extraction,
) -> crate::error::Result<Expectations> {
    if extraction.cookbook.source.sha256 != book.source().sha256 {
        return Err(crate::Error::Config(format!(
            "the run is of another file (sha256 {}…, the book is {}…)",
            &extraction.cookbook.source.sha256[..8],
            &book.source().sha256[..8]
        )));
    }
    let mut key = skeleton(book, path);
    key.not_recipes = non_recipe_titles(extraction);
    let n = key.titles.len();
    if n > 0 {
        let want = 8.min(n);
        key.samples = (0..want)
            .map(|i| Sample {
                title: key.titles[i * n / want].clone(),
                sections: None,
                ingredients: None,
                steps: None,
                notes_contain: Vec::new(),
                refs: Vec::new(),
                photos: None,
            })
            .collect();
    }
    Ok(key)
}

/// Items an answer key might want to list as not-recipes.
pub fn non_recipe_titles(extraction: &Extraction) -> Vec<String> {
    extraction
        .cookbook
        .items()
        .filter(|i| !matches!(i, Item::Recipe(_)))
        .map(|i| i.title().to_string())
        .collect()
}

#[cfg(test)]
mod section_tests {
    use super::sections_match;

    #[test]
    fn sections_compare_leniently() {
        let got = vec![
            None,
            Some("final dough baker’s formula".to_string()),
            Some("levain".into()),
        ];
        let want = vec![None, Some("final dough".to_string()), Some("levain".into())];
        assert!(sections_match(&got, &want));
        assert!(!sections_match(&got[1..], &want));
        assert!(!sections_match(
            &[Some("poolish".to_string())],
            &[Some("levain".to_string())]
        ));
    }
}
