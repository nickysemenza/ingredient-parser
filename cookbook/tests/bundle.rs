//! Portable archive contract, exercised with authored EPUBs and an offline oracle.
#![cfg(all(feature = "native", not(target_arch = "wasm32")))]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use cookbook::bundle::{BundleManifest, export};
use cookbook::cache::NoCache;
use cookbook::test_support::{Oracle, epub3_roles, oracle_transport};
use cookbook::{Book, CancelToken, ExtractOptions, Extraction, Item};
use std::collections::BTreeSet;
use std::io::{Cursor, Read, Write};
use std::path::Path;
use zip::{ZipArchive, ZipWriter, write::SimpleFileOptions};

async fn fixture(dir: &Path) -> Extraction {
    let bytes = cookbook_fixtures::epub3_nav_pagebreaks().unwrap();
    // An alias with identical bytes proves that deduplication isn't path-based.
    let mut zip = ZipArchive::new(Cursor::new(&bytes)).unwrap();
    let original = zip
        .file_names()
        .find(|name| name.ends_with(".jpg"))
        .unwrap()
        .to_owned();
    let mut photo = Vec::new();
    zip.by_name(&original)
        .unwrap()
        .read_to_end(&mut photo)
        .unwrap();
    let mut writer = ZipWriter::new_append(Cursor::new(bytes.clone())).unwrap();
    writer
        .start_file("alias.jpg", SimpleFileOptions::default())
        .unwrap();
    writer.write_all(&photo).unwrap();
    let bytes = writer.finish().unwrap().into_inner();
    std::fs::write(dir.join("book.epub"), &bytes).unwrap();
    let book = Book::open(bytes, "fixture").unwrap();
    let transport = oracle_transport(
        Oracle::new(epub3_roles()),
        book.lines().clone(),
        book.chunks().to_vec(),
    );
    let mut extraction = book
        .extract(
            &ExtractOptions {
                ladder: vec!["gemini-2.5-flash".into()],
                ..Default::default()
            },
            &transport,
            &NoCache,
            &CancelToken::new(),
            |_| {},
        )
        .await
        .unwrap();
    let image = cookbook::ImageRef {
        path: original,
        mime: "image/jpeg".into(),
        alt: Some("A <photo> & plate".into()),
        caption: Some("Crème & herbs".into()),
        line: None,
    };
    let mut alias = image.clone();
    alias.path = "alias.jpg".into();
    extraction.cookbook.cover = Some(image.clone());
    let recipe = extraction
        .cookbook
        .chapters
        .iter_mut()
        .flat_map(|chapter| &mut chapter.items)
        .find_map(|item| match item {
            Item::Recipe(recipe) => Some(recipe),
            _ => None,
        })
        .unwrap();
    recipe.photos.extend([image, alias]);
    recipe.name = "Crème <script>alert('x')</script> & mushrooms".into();
    extraction.report.incomplete = true;
    save(dir, &extraction);
    extraction
}
fn save(dir: &Path, extraction: &Extraction) {
    std::fs::write(
        dir.join("run.json"),
        serde_json::to_vec_pretty(extraction).unwrap(),
    )
    .unwrap();
}
fn text<R: Read + std::io::Seek>(zip: &mut ZipArchive<R>, name: &str) -> String {
    let mut text = String::new();
    zip.by_name(name)
        .unwrap()
        .read_to_string(&mut text)
        .unwrap();
    text
}

#[rstest::rstest]
#[case(false, false)]
#[case(true, false)]
#[case(false, true)]
#[tokio::test]
async fn round_trip_preserves_tree_assets_and_offline_review(
    #[case] incomplete: bool,
    #[case] cancelled: bool,
) {
    let dir = tempfile::tempdir().unwrap();
    let mut extraction = fixture(dir.path()).await;
    extraction.report.incomplete = incomplete;
    extraction.report.cancelled = cancelled;
    // Defaults leave the configured ladder empty; preview the recorded usage.
    extraction.report.options.ladder.clear();
    save(dir.path(), &extraction);
    let result = export(
        &dir.path().join("run.json"),
        &dir.path().join("book.epub"),
        None,
    )
    .unwrap();
    assert_eq!(Path::new(&result.path).parent(), Some(dir.path()));
    assert!(result.path.ends_with(".cookbook.zip"));
    let mut zip = ZipArchive::new(std::fs::File::open(&result.path).unwrap()).unwrap();
    let manifest: BundleManifest = serde_json::from_str(&text(&mut zip, "manifest.json")).unwrap();
    assert_eq!(manifest, result.manifest);
    assert_eq!(manifest.version, 1);
    assert_eq!(manifest.incomplete, incomplete || cancelled);
    assert_eq!(manifest.source_sha256, extraction.cookbook.source.sha256);
    let restored: Extraction = serde_json::from_str(&text(&mut zip, "extraction.json")).unwrap();
    assert_eq!(restored, extraction);
    let html = text(&mut zip, "index.html");
    assert_eq!(
        html.contains("Incomplete extraction"),
        incomplete || cancelled
    );
    assert!(html.contains("Crème &lt;script&gt;"));
    assert!(html.contains("A &lt;photo&gt; &amp; plate"));
    assert!(!html.contains("<script"));
    for usage in &extraction.report.usage_by_model {
        assert!(html.contains(&usage.model));
    }
    assert!(!html.contains("http://"));
    assert!(!html.contains("https://"));
    let dom = scraper::Html::parse_document(&html);
    let anchors = scraper::Selector::parse("[id]").unwrap();
    let ids: BTreeSet<_> = dom
        .select(&anchors)
        .filter_map(|node| node.value().attr("id"))
        .collect();
    let links = scraper::Selector::parse("a[href^='#']").unwrap();
    for link in dom.select(&links) {
        assert!(ids.contains(link.value().attr("href").unwrap().trim_start_matches('#')));
    }
    let articles = scraper::Selector::parse("article").unwrap();
    assert_eq!(
        dom.select(&articles).count(),
        extraction.cookbook.items().count()
    );
    let img = scraper::Selector::parse("img").unwrap();
    assert_eq!(
        dom.select(&img).count(),
        1 + extraction
            .cookbook
            .items()
            .map(|item| item.photos().len())
            .sum::<usize>()
    );
    let mut unique = BTreeSet::new();
    for asset in &manifest.images {
        if asset.mime == "image/jpeg" {
            assert!(
                asset.path.ends_with(".jpg"),
                "JPEGs use the conventional extension"
            );
        }
        let mut bytes = Vec::new();
        zip.by_name(&asset.path)
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        assert_eq!(asset.sha256, cookbook::epub::open::sha256_hex(&bytes));
        assert_eq!(asset.bytes, bytes.len() as u64);
        unique.insert(&asset.path);
    }
    assert!(
        unique.len() < manifest.images.len(),
        "identical photos share a file"
    );
    assert_eq!(
        zip.len(),
        unique.len() + 3,
        "no EPUB or original markup included"
    );
    // The same source/run produces stable archive contents, even at a new path.
    let second = dir.path().join("second.zip");
    std::fs::write(&second, "previous export").unwrap();
    export(
        &dir.path().join("run.json"),
        &dir.path().join("book.epub"),
        Some(&second),
    )
    .unwrap();
    assert_eq!(
        std::fs::read(&result.path).unwrap(),
        std::fs::read(second).unwrap()
    );
}

#[rstest::rstest]
#[case("source")]
#[case("missing")]
#[case("duplicate-id")]
#[case("mime")]
#[tokio::test]
async fn failed_export_keeps_existing_output_and_cleans_temporary_files(#[case] failure: &str) {
    let dir = tempfile::tempdir().unwrap();
    let mut extraction = fixture(dir.path()).await;
    match failure {
        "source" => extraction.cookbook.source.sha256 = "wrong".into(),
        "missing" => extraction.cookbook.cover.as_mut().unwrap().path = "missing.jpg".into(),
        "mime" => extraction.cookbook.cover.as_mut().unwrap().mime = "text/html".into(),
        "duplicate-id" => {
            let duplicate = extraction.cookbook.items().next().unwrap().clone();
            extraction.cookbook.chapters[0].items.push(duplicate);
        }
        _ => unreachable!(),
    }
    save(dir.path(), &extraction);
    let target = dir.path().join("existing.zip");
    std::fs::write(&target, "previous bundle").unwrap();
    let before = std::fs::read_dir(dir.path()).unwrap().count();
    let error = export(
        &dir.path().join("run.json"),
        &dir.path().join("book.epub"),
        Some(&target),
    )
    .unwrap_err();
    assert!(!error.to_string().is_empty());
    assert_eq!(std::fs::read_to_string(target).unwrap(), "previous bundle");
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), before);
}

#[tokio::test]
async fn default_name_cannot_escape_run_directory_and_inputs_cannot_be_replaced() {
    let dir = tempfile::tempdir().unwrap();
    let mut extraction = fixture(dir.path()).await;
    extraction.cookbook.source.title = "../../Crème / <Pie>".into();
    extraction.report.run_id = "../evil".into();
    save(dir.path(), &extraction);
    let run = dir.path().join("run.json");
    let epub = dir.path().join("book.epub");
    let result = export(&run, &epub, None).unwrap();
    assert_eq!(Path::new(&result.path).parent(), Some(dir.path()));
    for source in [&run, &epub] {
        let before = std::fs::read(source).unwrap();
        assert!(export(&run, &epub, Some(source)).is_err());
        assert_eq!(std::fs::read(source).unwrap(), before);
    }
}

#[tokio::test]
async fn books_without_images_export_without_inventing_assets() {
    let dir = tempfile::tempdir().unwrap();
    let mut extraction = fixture(dir.path()).await;
    extraction.cookbook.cover = None;
    for item in extraction
        .cookbook
        .chapters
        .iter_mut()
        .flat_map(|chapter| &mut chapter.items)
    {
        item.photos_mut().clear();
    }
    save(dir.path(), &extraction);
    let result = export(
        &dir.path().join("run.json"),
        &dir.path().join("book.epub"),
        None,
    )
    .unwrap();
    assert!(result.manifest.images.is_empty());
    let zip = ZipArchive::new(std::fs::File::open(result.path).unwrap()).unwrap();
    assert_eq!(zip.len(), 3);
}
