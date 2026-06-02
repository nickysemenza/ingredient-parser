// Tests legitimately unwrap on known-good fixtures.
#![allow(clippy::unwrap_used)]

use std::io::{Cursor, Write};

use recipe_epub::{
    extract_cookbook_with, EpubError, ExtractedRecipe, MockExtractor, Options, RecipeMeta,
    RecipeSection,
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
  </metadata>
  <manifest>
    <item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/>
    <item id="front" href="front.xhtml" media-type="application/xhtml+xml"/>
    <item id="chap" href="chapter.xhtml" media-type="application/xhtml+xml"/>
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
</body></html>"#;

// One chapter doc with two recipes (Dessert-Person style <p class> paragraphs).
const CHAPTER: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<html xmlns="http://www.w3.org/1999/xhtml"><body>
  <p class="rt">Pancakes</p>
  <p class="ril">1 cup flour</p>
  <p class="ril">2 eggs</p>
  <p class="rp">Mix and cook on a griddle.</p>
  <p class="rt">Omelette</p>
  <p class="ril">3 eggs</p>
  <p class="rp">Whisk and fry in butter.</p>
</body></html>"#;

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
    use recipe_epub::{book_metadata, classify_by_tags, CookbookGuess};

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
    assert!(snaps
        .iter()
        .all(|s| s.done <= s.total && s.cached <= s.done));
}
