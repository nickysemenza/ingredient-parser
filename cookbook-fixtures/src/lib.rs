//! Deterministic in-memory EPUB cookbooks whose shapes come from real books.
//!
//! Every fixture is generated from embedded XHTML by an in-memory ZIP writer:
//! no filesystem, clock, randomness, network, or model calls are involved, and
//! ZIP entry order, timestamps, and permissions are fixed, so repeated
//! generation is byte-identical.
//!
//! The recipes are invented. What is borrowed from real cookbooks is the
//! *markup shape* — the specific ways publisher and conversion pipelines break
//! the assumption that one spine document holds one recipe with a title at the
//! top:
//!
//! | Fixture | Shape | Defining trap |
//! |---|---|---|
//! | [`cookbook_epub`] | plain synthetic EPUB 2 | none; the easy baseline |
//! | [`split_spine`] | Calibre page-split EPUB 2 | one recipe cut across two spine documents |
//! | [`epub3_nav_pagebreaks`] | publisher-native EPUB 3 | many recipes per document, split only by pagebreak spans, behind a caption that names a different recipe |
//! | [`typographic_variations`] | Calibre-converted EPUB 2 | titles shredded into small-caps runs, classes that describe type rather than structure |
//!
//! Each shape fixture ships an [`Expected`] answer key so tests assert against
//! shared ground truth instead of restating counts:
//!
//! ```
//! let bytes = cookbook_fixtures::split_spine()?;
//! let expected = cookbook_fixtures::split_spine_expected();
//! assert!(bytes.starts_with(b"PK"));
//! assert_eq!(expected.standalone_recipes().count(), 2);
//! # Ok::<(), zip::result::ZipError>(())
//! ```
//!
//! [`EpubBuilder`] is public too, for a test that needs a shape none of the
//! four fixtures covers.

mod builder;
mod expected;
mod fixtures;

pub use builder::{EpubBuilder, EpubVersion};
pub use expected::{CrossRef, Expected, ExpectedRecipe};
pub use fixtures::{
    cookbook_epub, epub3_nav_pagebreaks, epub3_nav_pagebreaks_expected, split_spine,
    split_spine_expected, typographic_variations, typographic_variations_expected,
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::{COOKBOOK_DOCS, EPUB3_NAV_DOCS, SPLIT_SPINE_DOCS, TYPOGRAPHIC_DOCS};
    use std::io::{Cursor, Read};
    use zip::CompressionMethod;

    type TestResult = Result<(), Box<dyn std::error::Error>>;
    type Fixture = fn() -> zip::result::ZipResult<Vec<u8>>;

    /// Every fixture, paired with its builder so determinism can be rechecked.
    fn all_fixtures() -> [(&'static str, Fixture); 4] {
        [
            ("cookbook_epub", cookbook_epub as Fixture),
            ("split_spine", split_spine as Fixture),
            ("epub3_nav_pagebreaks", epub3_nav_pagebreaks as Fixture),
            ("typographic_variations", typographic_variations as Fixture),
        ]
    }

    /// Read one archive entry as text.
    fn read_entry(bytes: &[u8], path: &str) -> Result<String, Box<dyn std::error::Error>> {
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes.to_vec()))?;
        let mut text = String::new();
        archive.by_name(path)?.read_to_string(&mut text)?;
        Ok(text)
    }

    #[test]
    fn fixtures_are_byte_identical_across_builds() -> TestResult {
        for (name, build) in all_fixtures() {
            let first = build()?;
            let second = build()?;
            assert_eq!(first, second, "{name} is not reproducible");
            assert!(first.starts_with(b"PK"), "{name} is not a ZIP archive");
        }
        Ok(())
    }

    #[test]
    fn mimetype_is_the_first_stored_entry() -> TestResult {
        for (name, build) in all_fixtures() {
            let bytes = build()?;
            let mut archive = zip::ZipArchive::new(Cursor::new(bytes))?;
            let mut entry = archive.by_index(0)?;
            assert_eq!(entry.name(), "mimetype", "{name}");
            assert_eq!(entry.compression(), CompressionMethod::Stored, "{name}");
            assert_eq!(entry.extra_data(), Some([].as_slice()), "{name}");
            let mut text = String::new();
            entry.read_to_string(&mut text)?;
            assert_eq!(text, "application/epub+zip", "{name}");
        }
        Ok(())
    }

    #[test]
    fn every_declared_entry_is_present() -> TestResult {
        for (name, build) in all_fixtures() {
            let bytes = build()?;
            let mut archive = zip::ZipArchive::new(Cursor::new(bytes.clone()))?;
            for path in ["mimetype", "META-INF/container.xml", "OEBPS/content.opf"] {
                assert!(archive.by_name(path).is_ok(), "{name} is missing {path}");
            }
            let opf = read_entry(&bytes, "OEBPS/content.opf")?;
            // Anything the manifest names must really be in the archive.
            for href in manifest_hrefs(&opf) {
                let path = format!("OEBPS/{href}");
                assert!(
                    archive.by_name(&path).is_ok(),
                    "{name} manifests {href} but the archive has no {path}"
                );
            }
        }
        Ok(())
    }

    #[test]
    fn spine_follows_insertion_order() -> TestResult {
        /// A fixture's bytes, its spine-document table, and its answer key.
        type Case<'a> = (&'a str, Vec<u8>, &'a [(&'a str, &'a str)], Expected);
        let cases: [Case<'_>; 3] = [
            (
                "split_spine",
                split_spine()?,
                &SPLIT_SPINE_DOCS,
                split_spine_expected(),
            ),
            (
                "epub3_nav_pagebreaks",
                epub3_nav_pagebreaks()?,
                &EPUB3_NAV_DOCS,
                epub3_nav_pagebreaks_expected(),
            ),
            (
                "typographic_variations",
                typographic_variations()?,
                &TYPOGRAPHIC_DOCS,
                typographic_variations_expected(),
            ),
        ];
        for (name, bytes, docs, expected) in cases {
            let opf = read_entry(&bytes, "OEBPS/content.opf")?;
            let inserted: Vec<&str> = docs.iter().map(|(path, _)| *path).collect();
            assert_eq!(spine_hrefs(&opf), inserted, "{name} spine order");
            assert_eq!(
                expected.spine,
                inserted.as_slice(),
                "{name} Expected::spine"
            );
            assert!(
                opf.contains(&format!("<dc:title>{}</dc:title>", expected.book_title)),
                "{name} title"
            );
            assert!(opf.contains(expected.identifier), "{name} identifier");
        }
        // The baseline cookbook has no `Expected`, so check it against its table.
        let opf = read_entry(&cookbook_epub()?, "OEBPS/content.opf")?;
        let inserted: Vec<&str> = COOKBOOK_DOCS.iter().map(|(path, _)| *path).collect();
        assert_eq!(spine_hrefs(&opf), inserted);
        Ok(())
    }

    #[test]
    fn v2_fixtures_use_ncx_and_no_nav() -> TestResult {
        for (name, build) in [
            ("split_spine", split_spine as Fixture),
            ("typographic_variations", typographic_variations as Fixture),
        ] {
            let bytes = build()?;
            let opf = read_entry(&bytes, "OEBPS/content.opf")?;
            assert!(opf.contains("version=\"2.0\""), "{name}");
            assert!(opf.contains("<spine toc=\"ncx\">"), "{name}");
            assert!(!opf.contains("properties=\"nav\""), "{name}");
            let ncx = read_entry(&bytes, "OEBPS/toc.ncx")?;
            assert!(ncx.contains("<navMap>"), "{name}");
            let mut archive = zip::ZipArchive::new(Cursor::new(bytes))?;
            assert!(archive.by_name("OEBPS/nav.xhtml").is_err(), "{name}");
        }
        Ok(())
    }

    #[test]
    fn v2_cover_is_declared_with_a_meta_element() -> TestResult {
        // EPUB 2 has no manifest `properties`, so the cover is named by a
        // `<meta>` pointing at the image's manifest id.
        let opf = read_entry(&split_spine()?, "OEBPS/content.opf")?;
        let cover = opf
            .split("<meta name=\"cover\" content=\"")
            .nth(1)
            .and_then(|rest| rest.split('"').next())
            .unwrap_or_default();
        assert!(!cover.is_empty(), "no cover meta");
        assert!(opf.contains(&format!(
            "<item id=\"{cover}\" href=\"images/00036.jpg\" media-type=\"image/jpeg\"/>"
        )));
        Ok(())
    }

    #[test]
    fn v3_fixture_has_nav_and_page_list() -> TestResult {
        let bytes = epub3_nav_pagebreaks()?;
        let opf = read_entry(&bytes, "OEBPS/content.opf")?;
        assert!(opf.contains("version=\"3.0\""));
        assert!(opf.contains(
            "<item id=\"nav\" href=\"nav.xhtml\" media-type=\"application/xhtml+xml\" properties=\"nav\"/>"
        ));
        assert!(opf.contains("properties=\"cover-image\""));
        assert!(opf.contains("<meta property=\"dcterms:modified\">"));
        let nav = read_entry(&bytes, "OEBPS/nav.xhtml")?;
        assert!(nav.contains("epub:type=\"toc\""));
        assert!(nav.contains("epub:type=\"page-list\""));
        for page in [77, 78, 79, 111, 327, 330] {
            assert!(nav.contains(&format!(">{page}</a>")), "page {page} missing");
        }
        // Chapter entries nest their recipes.
        assert!(nav.contains("<li><a href=\"xhtml/c02.xhtml\">Pies and Tarts</a><ol>\n"));
        // The NCX is present alongside the nav document, as publishers ship it.
        assert!(read_entry(&bytes, "OEBPS/toc.ncx")?.contains("<navMap>"));
        Ok(())
    }

    #[test]
    fn nav_and_ncx_nest_by_depth() -> TestResult {
        let ncx = read_entry(&epub3_nav_pagebreaks()?, "OEBPS/toc.ncx")?;
        // Six entries, sequential playOrder, chapters wrapping their recipes.
        for order in 1..=6 {
            assert!(ncx.contains(&format!("playOrder=\"{order}\"")), "{order}");
        }
        assert!(!ncx.contains("playOrder=\"7\""));
        assert_eq!(ncx.matches("<navPoint ").count(), 6);
        assert_eq!(ncx.matches("</navPoint>").count(), 6);
        // "Pies and Tarts" is still open when its first recipe starts.
        let chapter = ncx.find("Pies and Tarts").unwrap_or(usize::MAX);
        let recipe = ncx.find("Cranberry-Pomegranate Mousse Pie").unwrap_or(0);
        let closing = ncx[..recipe].matches("</navPoint>").count();
        assert!(chapter < recipe);
        assert_eq!(closing, 0, "the chapter navPoint closed too early");
        Ok(())
    }

    // -----------------------------------------------------------------------
    // `Expected` cannot drift from the assets
    // -----------------------------------------------------------------------

    /// Count non-overlapping occurrences of each needle.
    fn tally(text: &str, needles: &[&str]) -> usize {
        needles
            .iter()
            .map(|needle| text.matches(needle).count())
            .sum()
    }

    /// Contents of every `<span class="bold">…</span>` in document order.
    fn bold_labels(xhtml: &str) -> Vec<&str> {
        const OPEN: &str = "<span class=\"bold\">";
        let mut out = Vec::new();
        let mut rest = xhtml;
        while let Some(start) = rest.find(OPEN) {
            rest = &rest[start + OPEN.len()..];
            if let Some(end) = rest.find("</span>") {
                out.push(&rest[..end]);
                rest = &rest[end..];
            }
        }
        out
    }

    /// Split `xhtml` at every marker, returning one slice per marker.
    ///
    /// Falls back to the whole document when no marker appears, so a
    /// title-less prose document still yields exactly one segment.
    fn segments<'a>(xhtml: &'a str, markers: &[&str]) -> Vec<&'a str> {
        let mut starts: Vec<usize> = Vec::new();
        for marker in markers {
            let mut from = 0;
            while let Some(found) = xhtml.get(from..).and_then(|rest| rest.find(marker)) {
                starts.push(from + found);
                from += found + marker.len();
            }
        }
        if starts.is_empty() {
            return vec![xhtml];
        }
        starts.sort_unstable();
        starts
            .iter()
            .enumerate()
            .filter_map(|(index, start)| {
                let end = starts.get(index + 1).copied().unwrap_or(xhtml.len());
                xhtml.get(*start..end)
            })
            .collect()
    }

    fn doc_text(docs: &[(&str, &'static str)], path: &str) -> &'static str {
        docs.iter()
            .find(|(candidate, _)| *candidate == path)
            .map_or("", |(_, xhtml)| *xhtml)
    }

    /// Cross-references listed in `Expected` must really be in the markup.
    fn assert_cross_refs(recipe: &ExpectedRecipe, text: &str) {
        for reference in recipe.cross_references {
            if reference.linked {
                assert!(
                    text.contains(&format!("href=\"{}\"", reference.target)),
                    "{}: no link to {}",
                    recipe.title,
                    reference.target
                );
            }
            assert!(
                text.contains(reference.text),
                "{}: no reference text {:?}",
                recipe.title,
                reference.text
            );
        }
    }

    #[test]
    fn split_spine_expected_matches_assets() {
        let expected = split_spine_expected();
        assert_eq!(expected.recipes.len(), 3);
        for recipe in expected.recipes {
            // A page-split recipe spans several documents; counting over all of
            // them is how the reader has to see it too.
            let text: String = recipe
                .docs
                .iter()
                .map(|path| doc_text(&SPLIT_SPINE_DOCS, path))
                .collect();
            assert_eq!(
                tally(&text, &["class=\"calibre_21\"", "class=\"calibre_22\""]),
                recipe.ingredient_lines,
                "{} ingredients",
                recipe.title
            );
            // A procedure paragraph opens with a bold step number, or with the
            // bold `TO MAKE` lead-in an essay uses instead. `DO AHEAD` and
            // `NOTE` trailers are bold too and deliberately excluded.
            let procedures = bold_labels(&text)
                .into_iter()
                .filter(|label| {
                    *label == "TO MAKE"
                        || (!label.is_empty() && label.bytes().all(|byte| byte.is_ascii_digit()))
                })
                .count();
            assert_eq!(procedures, recipe.steps, "{} steps", recipe.title);
            assert!(text.contains(recipe.title), "{} title", recipe.title);
            assert_cross_refs(recipe, &text);
        }
    }

    #[test]
    fn epub3_expected_matches_assets() {
        let expected = epub3_nav_pagebreaks_expected();
        assert_eq!(expected.recipes.len(), 5);
        for (path, xhtml) in EPUB3_NAV_DOCS {
            // Recipes and variations both begin with an `rt`-family title, so
            // the titles are the only boundaries inside a chapter document.
            let blocks = segments(xhtml, &["<p class=\"rt\"", "<p class=\"rt-small\""]);
            let recipes: Vec<&ExpectedRecipe> = expected
                .recipes
                .iter()
                .filter(|recipe| recipe.docs.contains(&path))
                .collect();
            assert_eq!(blocks.len(), recipes.len(), "{path} block count");
            for (recipe, block) in recipes.iter().zip(blocks) {
                assert_eq!(
                    tally(block, &["class=\"rilf\"", "class=\"ril\""]),
                    recipe.ingredient_lines,
                    "{} ingredients",
                    recipe.title
                );
                assert_eq!(
                    tally(block, &["class=\"rpf\"", "class=\"rp\""]),
                    recipe.steps,
                    "{} steps",
                    recipe.title
                );
                assert!(block.contains(recipe.title), "{} title", recipe.title);
                assert_cross_refs(recipe, block);
            }
        }
        // The phantom-title trap: the caption before the first title names a
        // recipe that lives further down the document, and belongs to neither.
        let c02 = doc_text(&EPUB3_NAV_DOCS, "xhtml/c02.xhtml");
        let prefix = c02.split("<p class=\"rt\"").next().unwrap_or_default();
        assert!(prefix.contains("Sour Cherry Pie"));
        assert!(prefix.contains("href=\"c02.xhtml#page_111\""));
        for recipe in expected.recipes {
            assert!(
                !recipe
                    .cross_references
                    .iter()
                    .any(|reference| reference.target == "c02.xhtml#page_111"),
                "the caption link must not be attributed to {}",
                recipe.title
            );
        }
    }

    #[test]
    fn typographic_expected_matches_assets() {
        let expected = typographic_variations_expected();
        assert_eq!(expected.recipes.len(), 5);
        for (path, xhtml) in TYPOGRAPHIC_DOCS {
            let blocks = segments(
                xhtml,
                &["<p class=\"sub-head-fp\"", "<p class=\"star-list\""],
            );
            let recipes: Vec<&ExpectedRecipe> = expected
                .recipes
                .iter()
                .filter(|recipe| recipe.docs.contains(&path))
                .collect();
            assert_eq!(blocks.len(), recipes.len(), "{path} block count");
            for (recipe, block) in recipes.iter().zip(blocks) {
                assert_eq!(
                    tally(block, &["class=\"hang\""]),
                    recipe.ingredient_lines,
                    "{} ingredients",
                    recipe.title
                );
                // `noindentsp` is prose, and does not match `class="indentsp"`.
                assert_eq!(
                    tally(block, &["class=\"indentsp\""]),
                    recipe.steps,
                    "{} steps",
                    recipe.title
                );
                assert_cross_refs(recipe, block);
            }
        }
        // Titles are shredded into small-caps runs: the raw DOM text is upper
        // case, so `Expected::title` is the recased form, not a substring.
        let part0011 = doc_text(&TYPOGRAPHIC_DOCS, "text/part0011.html");
        assert!(part0011.contains(
            "<p class=\"sub-head-fp\" id=\"page190\">P<small class=\"calibre9\">OLENTA</small></p>"
        ));
        assert!(!part0011.contains("Polenta with Fresh Corn"));
    }

    #[test]
    fn essays_and_variations_are_distinguishable() {
        for expected in [
            split_spine_expected(),
            epub3_nav_pagebreaks_expected(),
            typographic_variations_expected(),
        ] {
            for recipe in expected.recipes {
                assert!(
                    !(recipe.is_essay && recipe.is_variation),
                    "{} cannot be both",
                    recipe.title
                );
                if recipe.is_essay {
                    assert_eq!(recipe.ingredient_lines, 0, "{}", recipe.title);
                }
            }
            assert!(expected.standalone_recipes().count() >= 2);
        }
    }

    // -----------------------------------------------------------------------
    // OPF helpers
    // -----------------------------------------------------------------------

    fn attribute<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
        let start = tag.find(&format!("{name}=\""))? + name.len() + 2;
        let rest = tag.get(start..)?;
        let end = rest.find('"')?;
        rest.get(..end)
    }

    fn manifest_hrefs(opf: &str) -> Vec<&str> {
        opf.split("<item ")
            .skip(1)
            .filter_map(|tag| attribute(tag, "href"))
            .collect()
    }

    fn spine_hrefs(opf: &str) -> Vec<&str> {
        let items: Vec<(&str, &str)> = opf
            .split("<item ")
            .skip(1)
            .filter_map(|tag| Some((attribute(tag, "id")?, attribute(tag, "href")?)))
            .collect();
        let spine = opf.split("<spine").nth(1).unwrap_or_default();
        spine
            .split("<itemref ")
            .skip(1)
            .filter_map(|tag| attribute(tag, "idref"))
            .filter_map(|idref| {
                items
                    .iter()
                    .find(|(id, _)| *id == idref)
                    .map(|(_, href)| *href)
            })
            .collect()
    }
}
