// Tests legitimately unwrap on known-good fixtures.
#![allow(clippy::unwrap_used)]

use std::io::{Cursor, Write};

use recipe_epub::{
    CookbookRecipeExt, EpubError, ExtractedRecipe, MockExtractor, MockMatch, Options, RecipeMeta,
    RecipeSection, extract_cookbook_with,
};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

const CONTAINER: &str = r#"<?xml version="1.0"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles>
    <rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>"#;

const OPF: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<package xmlns="http://www.idpf.org/2007/opf" unique-identifier="bookid" version="2.0">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:title>Test Cookbook</dc:title>
    <dc:creator>Test Author</dc:creator>
    <dc:subject>Cooking</dc:subject>
    <dc:subject>Italian</dc:subject>
    <dc:identifier id="bookid">urn:uuid:test-cookbook</dc:identifier>
    <dc:language>en</dc:language>
    <meta name="cover" content="coverimg"/>
  </metadata>
  <manifest>
    <item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/>
    <item id="front" href="front.xhtml" media-type="application/xhtml+xml"/>
    <item id="chap" href="chapter.xhtml" media-type="application/xhtml+xml"/>
    <item id="coverimg" href="images/cover.jpg" media-type="image/jpeg"/>
    <item id="p1" href="images/p1.jpg" media-type="image/jpeg"/>
    <item id="p2" href="images/p2.jpg" media-type="image/jpeg"/>
  </manifest>
  <spine toc="ncx">
    <itemref idref="front"/>
    <itemref idref="chap"/>
  </spine>
</package>"#;

const NCX: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<ncx xmlns="http://www.daisy.org/z3986/2005/ncx/" version="2005-1">
  <head/>
  <docTitle><text>Test Cookbook</text></docTitle>
  <navMap>
    <navPoint id="n1" playOrder="1"><navLabel><text>Introduction</text></navLabel><content src="front.xhtml"/></navPoint>
    <navPoint id="n2" playOrder="2"><navLabel><text>Breakfast</text></navLabel><content src="chapter.xhtml"/></navPoint>
  </navMap>
</ncx>"#;

// Front matter — no recipe markers.
const FRONT: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<html xmlns="http://www.w3.org/1999/xhtml"><body>
  <h1>Introduction</h1>
  <p>Welcome to the Test Cookbook. This is just a friendly intro with no recipes.</p>
  <svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink"><image xlink:href="images/cover.jpg"/></svg>
</body></html>"#;

// One chapter doc with two recipes (Dessert-Person style <p class> paragraphs),
// each introduced by its own hero <figure><img> (the common cookbook layout).
const CHAPTER: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<html xmlns="http://www.w3.org/1999/xhtml"><body>
  <figure><img src="images/p1.jpg" alt="Stack of pancakes"/></figure>
  <p class="rt">Pancakes</p>
  <p class="ril">1 cup flour</p>
  <p class="ril">2 eggs</p>
  <p class="rp">Mix and cook on a griddle.</p>
  <figure><img src="images/p2.jpg" alt="Folded omelette"/></figure>
  <p class="rt">Omelette</p>
  <p class="ril">3 eggs</p>
  <p class="rp">Whisk and fry in butter.</p>
</body></html>"#;

// Stand-in image bytes (the readers return raw bytes; decoding happens in the UI).
const COVER_JPG: &[u8] = b"\xff\xd8\xff\xe0COVERJPEG";
const P1_JPG: &[u8] = b"\xff\xd8\xff\xe0PANCAKEJPEG";
const P2_JPG: &[u8] = b"\xff\xd8\xff\xe0OMELETTEJPEG";

/// Build a minimal valid EPUB (zip) in memory from the fixtures above.
fn build_epub() -> Vec<u8> {
    let mut zw = ZipWriter::new(Cursor::new(Vec::new()));
    let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);

    // `mimetype` must be the first entry and stored uncompressed.
    zw.start_file("mimetype", stored).unwrap();
    zw.write_all(b"application/epub+zip").unwrap();
    for (name, body) in [
        ("META-INF/container.xml", CONTAINER),
        ("OEBPS/content.opf", OPF),
        ("OEBPS/toc.ncx", NCX),
        ("OEBPS/front.xhtml", FRONT),
        ("OEBPS/chapter.xhtml", CHAPTER),
    ] {
        zw.start_file(name, deflated).unwrap();
        zw.write_all(body.as_bytes()).unwrap();
    }
    for (name, body) in [
        ("OEBPS/images/cover.jpg", COVER_JPG),
        ("OEBPS/images/p1.jpg", P1_JPG),
        ("OEBPS/images/p2.jpg", P2_JPG),
    ] {
        zw.start_file(name, deflated).unwrap();
        zw.write_all(body).unwrap();
    }
    zw.finish().unwrap().into_inner()
}

fn er(title: &str, ings: &[&str], steps: &[&str]) -> ExtractedRecipe {
    ExtractedRecipe {
        meta: RecipeMeta {
            title: title.to_string(),
            ..Default::default()
        },
        sections: vec![RecipeSection {
            name: None,
            ingredients: ings.iter().map(|s| s.to_string()).collect(),
            instructions: steps.iter().map(|s| s.to_string()).collect(),
        }],
    }
}

#[test]
fn reads_book_metadata_from_opf() {
    use recipe_epub::{CookbookGuess, book_metadata, classify_by_tags};

    // book_metadata reads from a path, so stage the in-memory epub on disk.
    let bytes = build_epub();
    let path = std::env::temp_dir().join(format!("recipe-epub-meta-{}.epub", std::process::id()));
    std::fs::write(&path, &bytes).unwrap();

    let meta = book_metadata(&path).unwrap();
    std::fs::remove_file(&path).ok();

    assert_eq!(meta.title, "Test Cookbook");
    assert_eq!(meta.authors, vec!["Test Author"]);
    assert_eq!(meta.subjects, vec!["Cooking", "Italian"]);
    assert_eq!(classify_by_tags(&meta), CookbookGuess::Yes);
}

#[tokio::test]
async fn extracts_recipes_skipping_front_matter() {
    // The mock returns a recipe for each title-needle present in a chunk's text.
    let mock = MockExtractor::new(vec![
        (
            "Pancakes".to_string(),
            vec![er(
                "Pancakes",
                &["1 cup flour", "2 eggs"],
                &["Mix and cook on a griddle."],
            )],
        ),
        (
            "Omelette".to_string(),
            vec![er("Omelette", &["3 eggs"], &["Whisk and fry in butter."])],
        ),
    ]);

    let bytes = build_epub();
    let recipes = extract_cookbook_with(&bytes, "test.epub", &Options::default(), &mock, |_| {})
        .await
        .unwrap();

    // Front matter contributed nothing; both chapter recipes came through in order.
    assert_eq!(recipes.len(), 2);
    assert_eq!(recipes[0].meta.title, "Pancakes");
    assert_eq!(recipes[1].meta.title, "Omelette");

    // Ingredient strings are preserved verbatim for the downstream nom parser.
    assert_eq!(
        recipes[0].sections[0].ingredients,
        vec!["1 cup flour", "2 eggs"]
    );
    assert_eq!(
        recipes[0].sections[0].instructions,
        vec!["Mix and cook on a griddle."]
    );

    // url carries the source + originating doc fragment.
    assert!(
        recipes[0].url.starts_with("test.epub#") && recipes[0].url.contains(".xhtml"),
        "url was {}",
        recipes[0].url
    );
}

#[tokio::test]
async fn binds_hero_photos_and_reads_cover() {
    // Each recipe is introduced by its own hero figure; binding is by title
    // proximity, so each recipe gets *its* image, not the other's.
    let mock = MockExtractor::new(vec![
        (
            "Pancakes".to_string(),
            vec![er(
                "Pancakes",
                &["1 cup flour", "2 eggs"],
                &["Mix and cook on a griddle."],
            )],
        ),
        (
            "Omelette".to_string(),
            vec![er("Omelette", &["3 eggs"], &["Whisk and fry in butter."])],
        ),
    ]);
    let bytes = build_epub();
    let recipes = extract_cookbook_with(&bytes, "test.epub", &Options::default(), &mock, |_| {})
        .await
        .unwrap();

    // Each recipe bound the hero introducing it (src resolved against the doc dir).
    let pancakes = recipes.iter().find(|r| r.meta.title == "Pancakes").unwrap();
    let omelette = recipes.iter().find(|r| r.meta.title == "Omelette").unwrap();
    let p_hero = pancakes.image.as_ref().unwrap();
    assert_eq!(p_hero.path, "OEBPS/images/p1.jpg");
    assert_eq!(p_hero.mime, "image/jpeg");
    assert_eq!(p_hero.alt.as_deref(), Some("Stack of pancakes"));
    assert_eq!(
        omelette.image.as_ref().map(|i| i.path.as_str()),
        Some("OEBPS/images/p2.jpg")
    );

    // The hero bytes materialize from the EPUB (the lazy half of the reference).
    let (data, mime) = recipe_epub::read_image(&bytes, &p_hero.path).unwrap();
    assert_eq!(data, P1_JPG);
    assert!(mime.contains("jpeg"));

    // The cover resolves via the OPF `<meta name="cover">` and reads its bytes.
    let cover = recipe_epub::cover_image_ref(&bytes).unwrap();
    assert_eq!(cover.path, "OEBPS/images/cover.jpg");
    let (cover_ref, items) = recipe_epub::collect_recipe_images(&bytes, &recipes);
    assert_eq!(
        cover_ref.as_ref().map(|c| c.path.as_str()),
        Some("OEBPS/images/cover.jpg")
    );
    // One open yields the cover + two distinct heroes = 3 image blobs.
    assert_eq!(items.len(), 3);
    assert!(
        items
            .iter()
            .any(|(p, b)| p == "OEBPS/images/cover.jpg" && b == COVER_JPG)
    );
}

#[tokio::test]
async fn parses_verbatim_strings_with_core_parser() {
    // End-to-end with the unchanged ScrapedRecipe::parse() (the nom ingredient parser).
    let mock = MockExtractor::new(vec![(
        "Pancakes".to_string(),
        vec![er(
            "Pancakes",
            &["1 cup flour", "2 eggs"],
            &["Mix and cook."],
        )],
    )]);
    let bytes = build_epub();
    let recipes = extract_cookbook_with(&bytes, "test.epub", &Options::default(), &mock, |_| {})
        .await
        .unwrap();
    let parsed = recipes[0].parse();
    assert_eq!(parsed.sections[0].ingredients.len(), 2);
    // "1 cup flour" parses to name "flour" with a cup amount.
    assert_eq!(parsed.sections[0].ingredients[0].name, "flour");
    assert!(!parsed.sections[0].ingredients[0].amounts.is_empty());
}

#[tokio::test]
async fn bad_zip_is_open_error() {
    let mock = MockExtractor::new(vec![]);
    let err = extract_cookbook_with(
        b"definitely not a zip",
        "x.epub",
        &Options::default(),
        &mock,
        |_| {},
    )
    .await
    .unwrap_err();
    assert!(matches!(err, EpubError::Open(_)), "got {err:?}");
}

#[tokio::test]
async fn progress_sink_reports_each_chunk() {
    use std::sync::Mutex;

    let mock = MockExtractor::new(vec![
        (
            "Pancakes".to_string(),
            vec![er("Pancakes", &["1 cup flour"], &["Mix."])],
        ),
        (
            "Omelette".to_string(),
            vec![er("Omelette", &["3 eggs"], &["Fry."])],
        ),
    ]);
    let bytes = build_epub();

    // Record every snapshot the sink receives (it fires from concurrent tasks).
    let snaps = Mutex::new(Vec::<recipe_epub::ExtractProgress>::new());
    recipe_epub::extract_cookbook_with(&bytes, "test.epub", &Options::default(), &mock, |p| {
        snaps.lock().unwrap().push(p);
    })
    .await
    .unwrap();

    let snaps = snaps.into_inner().unwrap();
    // One initial (done == 0) snapshot, then one per finished chunk.
    let total = snaps[0].total;
    assert!(total > 0, "total should be known up front");
    assert_eq!(snaps.len(), total + 1, "init snapshot + one per chunk");
    assert_eq!(snaps[0].done, 0);
    // The final snapshot reports all chunks done; `done` never exceeds `total`.
    let last = snaps.last().unwrap();
    assert_eq!(last.done, total);
    assert!(
        snaps
            .iter()
            .all(|s| s.done <= s.total && s.cached <= s.done)
    );
}

// ── the continuation contract, end to end ───────────────────────────────────

/// Build an epub whose single recipe body is long enough to force a mid-recipe
/// hard split, so the second chunk carries a `title_hint`.
fn build_long_recipe_epub() -> Vec<u8> {
    let line = "x".repeat(140);
    // Comfortably past CHUNK_BUDGET + CHUNK_SLACK (12000 + 6000).
    let body: String = std::iter::repeat_n(line.as_str(), 200)
        .map(|l| format!("<p>{l}</p>"))
        .collect();
    let chapter = format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<html xmlns="http://www.w3.org/1999/xhtml"><body>
<h1>Lone Long Recipe</h1>{body}</body></html>"#
    );

    let mut zw = ZipWriter::new(Cursor::new(Vec::new()));
    let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    zw.start_file("mimetype", stored).unwrap();
    zw.write_all(b"application/epub+zip").unwrap();
    for (name, body) in [
        ("META-INF/container.xml", CONTAINER),
        ("OEBPS/content.opf", OPF),
        ("OEBPS/toc.ncx", NCX),
        ("OEBPS/front.xhtml", FRONT),
        ("OEBPS/chapter.xhtml", chapter.as_str()),
    ] {
        zw.start_file(name, deflated).unwrap();
        zw.write_all(body.as_bytes()).unwrap();
    }
    zw.finish().unwrap().into_inner()
}

/// The hint → re-emit → merge contract, driven offline for the first time.
///
/// A hard split severs a recipe; the continuation chunk inherits the recipe's
/// title as its `title_hint`; the model is asked (via that hint) to re-emit the
/// same titled recipe; `assemble` merges the halves by lowercased title. Each
/// hop had a test. The contract did not — the mock could only match on chunk
/// text, so nothing could stand in for "a model that responds to the hint".
///
/// This is the bug class the project keeps rediscovering: a dropped tail, or a
/// second half landing as its own untitled recipe.
#[tokio::test]
async fn continuation_chunk_merges_back_into_one_recipe() {
    let bytes = build_long_recipe_epub();

    let head = ExtractedRecipe {
        meta: RecipeMeta {
            title: "Lone Long Recipe".to_string(),
            notes: vec!["from the head chunk".to_string()],
            ..Default::default()
        },
        sections: vec![RecipeSection {
            name: None,
            ingredients: vec!["2 cups flour".to_string()],
            instructions: vec!["Mix.".to_string()],
        }],
    };
    // What a model does when handed "Section title: Lone Long Recipe": it
    // re-emits the SAME title, carrying only the tail it can see.
    let tail = ExtractedRecipe {
        meta: RecipeMeta {
            title: "Lone Long Recipe".to_string(),
            notes: vec!["do-ahead note from the tail chunk".to_string()],
            ..Default::default()
        },
        sections: vec![RecipeSection {
            name: None,
            ingredients: vec![],
            instructions: vec!["Bake.".to_string()],
        }],
    };

    let mock = MockExtractor::with_rules(vec![
        (MockMatch::Text("Lone Long Recipe".to_string()), vec![head]),
        (
            MockMatch::TitleHint("Lone Long Recipe".to_string()),
            vec![tail],
        ),
    ]);

    let recipes = extract_cookbook_with(&bytes, "long.epub", &Options::default(), &mock, |_| {})
        .await
        .unwrap();

    assert_eq!(
        recipes.len(),
        1,
        "the tail must merge into the head, not land as a second recipe: {:#?}",
        recipes.iter().map(|r| &r.meta.title).collect::<Vec<_>>()
    );
    let merged = &recipes[0];
    assert_eq!(merged.meta.title, "Lone Long Recipe");
    assert!(
        merged
            .meta
            .notes
            .iter()
            .any(|n| n.contains("do-ahead note from the tail chunk")),
        "the tail chunk's note was dropped: {:#?}",
        merged.meta.notes
    );
    assert!(
        merged
            .meta
            .notes
            .iter()
            .any(|n| n.contains("from the head chunk")),
        "the head chunk's note was dropped: {:#?}",
        merged.meta.notes
    );
}

#[tokio::test]
async fn review_run_cache_only_replay_and_resume_preserve_source_gaps() {
    use recipe_epub::review::{ReviewRun, RunOptions, extract_run};
    let dir = std::env::temp_dir().join(format!("review-offline-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let out = dir.join("run.json");
    let mut run = ReviewRun::inspect(&build_epub(), "fixture.epub", "unpriced-no-backend").unwrap();
    assert!(!run.documents.is_empty());
    let original_ids: Vec<_> = run.chunks.iter().map(|c| c.id.clone()).collect();
    extract_run(
        &mut run,
        &RunOptions {
            cache_dir: Some(dir.join("empty-cache")),
            ..Default::default()
        },
        &out,
    )
    .await
    .unwrap();
    assert!(run.incomplete());
    assert!(
        run.chunks
            .iter()
            .all(|c| c.error.as_deref() == Some("cache miss (network disabled)"))
    );
    let mut loaded = ReviewRun::read(&out).unwrap();
    loaded.replay().unwrap();
    assert!(loaded.recipes.is_empty());
    assert_eq!(
        original_ids,
        loaded
            .chunks
            .iter()
            .map(|c| c.id.clone())
            .collect::<Vec<_>>()
    );
    assert_eq!(loaded.reserved_usd, 0.0);
    let before = std::fs::read(&out).unwrap();
    assert!(
        extract_run(
            &mut loaded,
            &RunOptions {
                chunks: vec!["missing".into()],
                ..Default::default()
            },
            &out
        )
        .await
        .is_err()
    );
    assert_eq!(before, std::fs::read(&out).unwrap());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn complete_method_schema_and_coverage_guard() {
    let schema = recipe_epub::recipes_tool_schema();
    assert_eq!(
        schema["properties"]["recipes"]["items"]["properties"]["sections"]["items"]["required"],
        serde_json::json!(["ingredients", "instructions"])
    );
    let chunk = recipe_epub::Chunk {
        title_hint: None,
        doc_path: "recipe.xhtml".into(),
        links: vec![],
        images: vec![],
        text: format!(
            "Soup\n1 cup water\nHeat {}\nMix {}",
            "the water in a saucepan until it reaches a gentle simmer. ".repeat(3),
            "the ingredients carefully and continue stirring until combined. ".repeat(3)
        ),
    };
    let payload = serde_json::json!({"recipes":[{"title":"Soup","sections":[{"ingredients":["1 cup water"],"instructions":[]}]}]});
    let rt = tokio::runtime::Runtime::new().unwrap();
    let result = rt.block_on(recipe_epub::try_extract_chunk_detailed_for_chunk(
        &chunk,
        || async {
            Ok(recipe_epub::CallResult {
                input: Some(payload.clone()),
                usage: Default::default(),
                truncated: false,
            })
        },
    ));
    assert!(result.is_err());
    let mut empty_test = chunk;
    empty_test.text.push_str("\nSERVES 2\n2g salt\n3g pepper");
    let missing = rt.block_on(recipe_epub::try_extract_chunk_detailed_for_chunk(
        &empty_test,
        || async {
            Ok(recipe_epub::CallResult {
                input: Some(serde_json::json!({"recipes":[]})),
                usage: Default::default(),
                truncated: false,
            })
        },
    ));
    assert!(missing.is_err());
}

#[tokio::test]
async fn review_budget_version_and_decisions_are_guarded() {
    use recipe_epub::review::{
        ReviewDecision, ReviewDecisions, ReviewRun, RunOptions, extract_run,
    };
    let dir = std::env::temp_dir().join(format!("review-guards-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let out = dir.join("run.json");
    let mut run = ReviewRun::inspect(&build_epub(), "fixture.epub", "gemini-2.5-flash").unwrap();
    extract_run(
        &mut run,
        &RunOptions {
            allow_network: true,
            cache_dir: Some(dir.join("cache")),
            budget_usd: 0.0,
            ..Default::default()
        },
        &out,
    )
    .await
    .unwrap();
    assert_eq!(run.reserved_usd, 0.0);
    assert!(run.chunks.iter().all(|c| c.output.is_none()));
    assert!(
        run.chunks
            .iter()
            .any(|c| c.error.as_deref() == Some("budget exhausted before request"))
    );
    let notes = dir.join("review.json");
    let mut decisions = ReviewDecisions::read(&notes, &run).unwrap();
    decisions.documents.insert(
        run.documents[0].path.clone(),
        ReviewDecision {
            status: "Uncertain".into(),
            note: "Check source photo".into(),
        },
    );
    decisions.save(&notes).unwrap();
    assert_eq!(
        ReviewDecisions::read(&notes, &run)
            .unwrap()
            .documents
            .values()
            .next()
            .unwrap()
            .note,
        "Check source photo"
    );
    let mut other = run.clone();
    other.epub_sha256 = "different".into();
    assert!(ReviewDecisions::read(&notes, &other).is_err());
    run.version += 1;
    run.save(&out).unwrap();
    assert!(ReviewRun::read(&out).is_err());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn offline_replay_uses_source_captions_and_rejects_foreign_annotations() {
    use recipe_epub::review::{ImageText, ReviewRun, SourceImageText};
    let mut run = ReviewRun::inspect(&build_epub(), "fixture.epub", "offline").unwrap();
    for chunk in &mut run.chunks {
        chunk.output = Some(if chunk.source.text.contains("Pancakes") {
            vec![er(
                "Pancakes",
                &["1 cup flour", "2 eggs"],
                &["Mix and cook on a griddle."],
            )]
        } else {
            vec![]
        });
    }
    run.image_text = Some(SourceImageText {
        epub_sha256: run.epub_sha256.clone(),
        method: "synthetic captions".into(),
        images: vec![
            ImageText {
                path: "OEBPS/images/p1.jpg".into(),
                captions: vec![],
            },
            ImageText {
                path: "OEBPS/images/p2.jpg".into(),
                captions: vec!["Pancakes".into()],
            },
        ],
    });
    run.replay().unwrap();
    assert_eq!(
        run.recipes[0].image.as_ref().unwrap().path,
        "OEBPS/images/p2.jpg"
    );
    assert_eq!(
        run.parsed,
        serde_json::to_value(
            run.recipes
                .iter()
                .map(CookbookRecipeExt::parse)
                .collect::<Vec<_>>()
        )
        .unwrap()
    );
    run.image_text.as_mut().unwrap().epub_sha256 = "another source".into();
    assert!(run.replay().is_err());
}

#[test]
fn inheriting_a_run_preserves_unchanged_outputs_and_invalidates_changed_chunks() {
    use recipe_epub::review::ReviewRun;
    let mut parent = ReviewRun::inspect(&build_epub(), "fixture.epub", "old-model").unwrap();
    for c in &mut parent.chunks {
        c.output = Some(vec![]);
    }
    parent.reserved_usd = 0.125;
    let mut fresh = ReviewRun::inspect(&build_epub(), "fixture.epub", "new-model").unwrap();
    fresh.chunks[0].source.text.push_str(" Changed cleaning.");
    let run = fresh
        .inherit_outputs(&parent, std::path::Path::new("parent.json"))
        .unwrap();
    assert!(run.chunks[0].output.is_none());
    assert!(
        run.chunks[1..]
            .iter()
            .all(|c| c.output.is_some() && c.model.as_deref() == Some("old-model"))
    );
    assert_eq!(run.model, "new-model");
    assert_eq!(run.reserved_usd, 0.125);
    assert_eq!(run.parent.as_deref(), Some("parent.json"));
    fresh.epub_sha256 = "another book".into();
    assert!(
        fresh
            .inherit_outputs(&parent, std::path::Path::new("parent.json"))
            .is_err()
    );
}

#[test]
fn review_discrepancies_include_ingredient_extra_and_exact_recipe_failures() {
    use recipe_epub::review::{ReviewRun, discrepancies_by_source};
    let mut run = ReviewRun::inspect(&build_epub(), "fixture.epub", "offline").unwrap();
    run.chunks[0].output = Some(vec![er("Pancakes", &["1 cup flour"], &["Mix."])]);
    run.replay().unwrap();
    let source = run.recipes[0].url.rsplit_once('#').unwrap().1;
    let evaluation = serde_json::json!({
        "rows":[{"source":source,"issues":[]}],
        "extra_recipes":[{"source":run.recipes[0].url}],
        "mismatches":[{"path":"recipes/0/meta/title","want":"Other","got":"Pancakes"},
            {"path":"recipes/1","want":{"url":"epub://fixture#missing.xhtml"},"got":null}],
        "ingredients":{"rows":[{"want":{"recipe_index":0},"fields":[true,false,true,true,true]},
            {"want":{"recipe_index":0},"fields":[true,true,true,true,true]}]}
    });
    let issues = discrepancies_by_source(&run, &evaluation);
    assert_eq!(issues[source].len(), 3);
    assert_eq!(issues["missing.xhtml"].len(), 1);
    assert!(
        discrepancies_by_source(
            &run,
            &serde_json::json!({"rows":[{"source":source,"issues":[]}]})
        )
        .is_empty()
    );
}

#[test]
fn source_inspection_retains_svg_cover_images() {
    let source = recipe_epub::source::inspect_source(&build_epub()).unwrap();
    assert_eq!(source[0].images[0].path, "OEBPS/images/cover.jpg");
    assert_eq!(source[0].images[0].mime, "image/jpeg");
}

#[test]
fn saved_name_statistics_count_occurrences_and_recipes_without_reparsing() {
    use recipe_epub::review::{
        ReviewRun,
        stats::{NameSort, ingredient_stats},
    };
    let mut run = ReviewRun::inspect(&build_epub(), "fixture.epub", "offline").unwrap();
    run.chunks[0].output = Some(vec![
        er(
            "First",
            &["1 tsp salt", "2 tsp salt", "1 tsp Salt"],
            &["Mix."],
        ),
        er("Second", &["1 tsp salt", "mystery"], &["Mix."]),
    ]);
    run.replay().unwrap();
    // Stored parsing may come from older code. Statistics must not rerun it.
    run.parsed[0]["sections"][0]["ingredients"][2]["name"] = serde_json::json!("Salt");
    run.parsed[1]["sections"][0]["ingredients"][1]["name"] = serde_json::json!("");
    let stats = ingredient_stats(&run).unwrap();
    assert_eq!(stats.total_occurrences, 5);
    assert_eq!(stats.unique_names, 3);
    assert_eq!(stats.singleton_names, 2);
    assert_eq!(stats.total_recipes, 2);
    assert_eq!(stats.names[0].name, "salt");
    assert_eq!(stats.names[0].occurrences, 3);
    assert_eq!(stats.names[0].recipes, 2);
    assert_eq!(stats.names[0].distinct_inputs, 2);
    assert_eq!(stats.names[0].examples[2].recipe_index, 1);
    assert_eq!(stats.names[0].examples[2].input, "1 tsp salt");
    assert_eq!(
        stats
            .select("SALT", None, NameSort::Name)
            .iter()
            .map(|n| n.name.as_str())
            .collect::<Vec<_>>(),
        ["Salt", "salt"]
    );
    assert_eq!(stats.select("", Some(1), NameSort::Occurrences).len(), 2);
    assert_eq!(stats.select("", None, NameSort::Recipes)[0].recipes, 2);
    assert!(stats.select("absent", None, NameSort::Name).is_empty());
    assert!(!stats.complete);
    run.parsed[0]["sections"][0]["ingredients"] = serde_json::json!([]);
    assert!(ingredient_stats(&run).is_err());
    run.recipes.clear();
    run.parsed = serde_json::json!([]);
    assert_eq!(ingredient_stats(&run).unwrap().total_occurrences, 0);
}
