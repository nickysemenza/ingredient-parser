//! Plain, offline review of the exact tree exported as JSON.
use super::BundleManifest;
use crate::{Extraction, ImageRef, Item, RecipeRef, Step};
use maud::{DOCTYPE, Markup, PreEscaped, html};
use std::collections::BTreeMap;

const CSS: &str = "body{max-width:76rem;margin:auto;padding:2rem;font:18px/1.6 system-ui;color:#28251f;background:#fffdf8}a{color:#365f47}h1,h2,h3{line-height:1.2}h1{font-size:2.5rem}h2{margin-top:3rem}article{border-top:1px solid #d6d0c4;padding:1.5rem 0;scroll-margin-top:1rem}figure{display:inline-block;vertical-align:top;margin:1rem 1rem 1rem 0;max-width:100%}img{display:block;max-width:100%;max-height:32rem;height:auto}figcaption,.source{font-size:.85rem;color:#655f54}.notice{padding:1rem;background:#fff1ce;border-left:4px solid #916323}nav ul{columns:2;column-width:20rem}li{break-inside:avoid}p,li{white-space:pre-wrap}pre{white-space:pre-wrap;overflow-wrap:anywhere}details{margin:1rem 0}section{margin-bottom:2rem}@media print{body{background:white;padding:0}nav{display:none}article{break-before:page}img{max-height:20rem}}";

pub(super) fn render(extraction: &Extraction, manifest: &BundleManifest) -> String {
    let book = &extraction.cookbook;
    let models = extraction
        .report
        .usage_by_model
        .iter()
        .map(|usage| usage.model.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    let anchors: BTreeMap<_, _> = book
        .items()
        .enumerate()
        .map(|(i, item)| (item.id(), format!("item-{i}")))
        .collect();
    let images: BTreeMap<_, _> = manifest
        .images
        .iter()
        .map(|image| (image.source_path.as_str(), image.path.as_str()))
        .collect();
    html! {
        (DOCTYPE)
        html lang="en" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                meta http-equiv="Content-Security-Policy" content="default-src 'none'; img-src 'self'; style-src 'unsafe-inline'; base-uri 'none'; form-action 'none'";
                title { (book.source.title) " — cookbook review" }
                style { (PreEscaped(CSS)) }
            }
            body {
                header id="top" {
                    h1 { (book.source.title) }
                    p { (book.source.authors.join(", ")) }
                    p.source { "Run " (manifest.run_id) " · " (book.recipes().count()) " recipes · " (extraction.report.started_at) }
                    p.source { "Models: " @if models.is_empty() { "No model calls recorded" } @else { (models) } }
                    @if manifest.incomplete {
                        p.notice { strong { "Incomplete extraction. " } "Some source content may be missing. Review the diagnostics before using this run." }
                    }
                    @if let Some(cover) = &book.cover { (photos(std::slice::from_ref(cover), &images)) }
                }
                details {
                    summary {
                        "Extraction quality: "
                        (extraction.report.chunks.iter().filter(|chunk| !chunk.flags.is_empty() || chunk.status == crate::report::ChunkStatus::Failed).count())
                        " flagged chunks · " (extraction.report.crosscheck.missing.len()) " missing titles · "
                        (extraction.report.unresolved_refs.len()) " unresolved references"
                    }
                    p { "Missing contents titles: " (extraction.report.crosscheck.missing.len())
                        " · Unresolved references: " (extraction.report.unresolved_refs.len())
                        " · Catalog omissions: " (extraction.report.catalog_missing.len()) }
                    @for chunk in &extraction.report.chunks {
                        @if !chunk.flags.is_empty() || chunk.status == crate::report::ChunkStatus::Failed {
                            p.notice { (chunk.id) ": " (format!("{:?}", chunk.status)) " — " (format!("{:?}", chunk.flags)) }
                        }
                    }
                    @for title in &extraction.report.crosscheck.missing { p { "Missing: " (title) } }
                    @for title in &extraction.report.crosscheck.phantom { p { "Unexpected title: " (title) } }
                    @for title in &extraction.report.catalog_missing { p { "Catalog omission: " (title) } }
                    p { a href="extraction.json" { "Complete extraction and run report (JSON)" } }
                }
                nav aria-label="Contents" {
                    h2 { "Contents" }
                    @for (i, chapter) in book.chapters.iter().enumerate() {
                        h3 { a href=(format!("#chapter-{i}")) { (chapter.title.as_deref().unwrap_or("Opening pages")) } }
                        ul { @for item in &chapter.items {
                            li { a href=(format!("#{}", anchors[item.id()])) { (item.name()) } }
                        } }
                    }
                }
                main {
                    @for (i, chapter) in book.chapters.iter().enumerate() {
                        section id=(format!("chapter-{i}")) {
                            h2 { (chapter.title.as_deref().unwrap_or("Opening pages")) }
                            @for text in &chapter.intro { p { (text) } }
                            @for item in &chapter.items {
                                article id=(&anchors[item.id()]) {
                                    h3 { (item.name()) }
                                    p.source { (item.span().doc_path) " · lines " (item.span().start) "–" (item.span().end)
                                        @if let Some(page) = &item.span().page { " · page " (page) }
                                    }
                                    @match item {
                                        Item::Recipe(recipe) => {
                                            @for text in &recipe.meta.description { p { (text) } }
                                            @if let Some(value) = &recipe.meta.recipe_yield { p { strong { "Yield: " } (value) } }
                                            @if let Some(page) = &recipe.meta.page { p.source { "Printed page: " (page) } }
                                            @if let Some(times) = &recipe.meta.times {
                                                @for (label, value) in [("Active", &times.active), ("Total", &times.total), ("Prep", &times.prep), ("Cook", &times.cook)] {
                                                    @if let Some(value) = value { p { strong { (label) ": " } (value) } }
                                                }
                                            }
                                            @if !recipe.meta.equipment.is_empty() { p { strong { "Equipment: " } (recipe.meta.equipment.join(", ")) } }
                                            @if let Some(parent) = &recipe.variant_of {
                                                @if let Some(anchor) = anchors.get(parent.as_str()) { p { "Variation of " a href=(format!("#{anchor}")) { (book.item(parent).map(|item| item.name()).unwrap_or(parent)) } } }
                                            }
                                            @for section in &recipe.sections {
                                                @if let Some(name) = &section.name { h4 { (name) } }
                                                ul { @for line in &section.ingredients {
                                                    li { (line.raw) @if let Some(reference) = &line.reference { " " (references(std::slice::from_ref(reference), &anchors)) } }
                                                } }
                                                (steps(&section.steps, &anchors))
                                            }
                                            @for note in &recipe.notes {
                                                p { @if let Some(label) = &note.label { strong { (label) ": " } } (note.text) " " (references(&note.refs, &anchors)) }
                                            }
                                        },
                                        Item::Technique(technique) => {
                                            @for text in &technique.description { p { (text) } }
                                            (steps(&technique.steps, &anchors))
                                        },
                                        Item::Essay(essay) => { @for text in &essay.text { p { (text) } } },
                                    }
                                    (photos(item.photos(), &images))
                                    p.source { a href="#top" { "Back to contents" } }
                                }
                            }
                        }
                    }
                }
            }
        }
    }.into_string()
}

fn photos(photos: &[ImageRef], paths: &BTreeMap<&str, &str>) -> Markup {
    html! { @for photo in photos {
        @if let Some(path) = paths.get(photo.path.as_str()) {
            figure {
                img src=(path) alt=(photo.alt.as_deref().unwrap_or("Cookbook photograph")) loading="lazy";
                @if let Some(caption) = &photo.caption { figcaption { (caption) } }
            }
        }
    } }
}
fn references(refs: &[RecipeRef], anchors: &BTreeMap<&str, String>) -> Markup {
    html! { @for reference in refs {
        @if let Some(anchor) = anchors.get(reference.target_id.as_str()) {
            a href=(format!("#{anchor}")) { (reference.text) } " "
        }
    } }
}
fn steps(steps: &[Step], anchors: &BTreeMap<&str, String>) -> Markup {
    html! { ol { @for step in steps { li { (step.text) " " (references(&step.refs, anchors)) } } } }
}
