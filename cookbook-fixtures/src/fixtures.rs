//! The fixture books themselves.
//!
//! Each shape fixture imitates the structural habits of one real publisher
//! pipeline. The recipes are invented; only the markup shape is borrowed.

use crate::builder::{EpubBuilder, EpubVersion};
use crate::expected::{CrossRef, Expected, ExpectedRecipe};
use zip::result::ZipResult;

/// Append a fixture's spine documents in table order.
///
/// Every fixture drives its spine from one table so the archive, the test that
/// counts paragraphs in the assets, and `Expected::spine` cannot disagree about
/// which documents exist or what order they are in.
fn with_docs(builder: EpubBuilder, docs: &[(&str, &str)]) -> EpubBuilder {
    docs.iter()
        .fold(builder, |builder, (path, xhtml)| builder.doc(path, xhtml))
}

// ---------------------------------------------------------------------------
// Images
// ---------------------------------------------------------------------------

/// Byte length of the minimal JPEG produced by [`jpeg_1x1`].
const JPEG_LEN: usize = 141;

/// A minimal decodable baseline JPEG: 1x1 pixel, grayscale, mid-tone.
///
/// `quant` fills the quantization table, which is the only thing that differs
/// between the fixture images: it changes the bytes without changing validity,
/// so a test can tell two images apart by content.
fn jpeg_1x1(quant: u8) -> [u8; JPEG_LEN] {
    // SOI, then a DQT header whose 64 table bytes are appended below.
    const HEAD: [u8; 7] = [0xff, 0xd8, 0xff, 0xdb, 0x00, 0x43, 0x00];
    // SOF0 (8-bit, 1x1, one grayscale component), a one-symbol DC Huffman
    // table, a one-symbol AC table, SOS, a two-bit scan (DC zero then
    // end-of-block, padded with ones), and EOI.
    const TAIL: [u8; 70] = [
        0xff, 0xc0, 0x00, 0x0b, 0x08, 0x00, 0x01, 0x00, 0x01, 0x01, 0x01, 0x11, 0x00, //
        0xff, 0xc4, 0x00, 0x14, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, //
        0xff, 0xc4, 0x00, 0x14, 0x10, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, //
        0xff, 0xda, 0x00, 0x08, 0x01, 0x01, 0x00, 0x00, 0x3f, 0x00, //
        0x3f, //
        0xff, 0xd9,
    ];
    let mut out = [0u8; JPEG_LEN];
    let mut index = 0;
    for byte in HEAD {
        out[index] = byte;
        index += 1;
    }
    for _ in 0..64 {
        out[index] = quant;
        index += 1;
    }
    for byte in TAIL {
        out[index] = byte;
        index += 1;
    }
    out
}

// ---------------------------------------------------------------------------
// Synthetic three-recipe cookbook
// ---------------------------------------------------------------------------

/// Spine documents of [`cookbook_epub`], in spine order.
pub(crate) const COOKBOOK_DOCS: [(&str, &str); 3] = [
    (
        "tomato-soup.xhtml",
        include_str!("../assets/cookbook/tomato-soup.xhtml"),
    ),
    (
        "flatbread.xhtml",
        include_str!("../assets/cookbook/flatbread.xhtml"),
    ),
    (
        "lemon-dressing.xhtml",
        include_str!("../assets/cookbook/lemon-dressing.xhtml"),
    ),
];

/// The plain three-recipe synthetic cookbook, as an EPUB 2 archive.
///
/// Chapters, in spine order: roasted tomato soup with white beans and basil,
/// flatbread, and lemon dressing. One heading, one ingredient class, one
/// instruction class, no images — the shape a reader should handle before it
/// handles any of the publisher fixtures.
///
/// Returns ZIP/I/O errors if archive construction fails.
pub fn cookbook_epub() -> ZipResult<Vec<u8>> {
    with_docs(
        EpubBuilder::new("Synthetic cookbook")
            .author("Fixture Author")
            .identifier("urn:cookbook-fixtures:synthetic-cookbook:v1")
            .subject("Cooking")
            .nav_entry(
                "Roasted tomato soup with white beans and basil",
                "tomato-soup.xhtml",
                1,
            )
            .nav_entry("Flatbread", "flatbread.xhtml", 1)
            .nav_entry("Lemon dressing", "lemon-dressing.xhtml", 1),
        &COOKBOOK_DOCS,
    )
    .build()
}

// ---------------------------------------------------------------------------
// a. Calibre page-split EPUB 2
// ---------------------------------------------------------------------------

/// Spine documents of [`split_spine`], in spine order.
pub(crate) const SPLIT_SPINE_DOCS: [(&str, &str); 7] = [
    (
        "index_split_000.html",
        include_str!("../assets/split_spine/index_split_000.html"),
    ),
    (
        "index_split_046.html",
        include_str!("../assets/split_spine/index_split_046.html"),
    ),
    (
        "index_split_047.html",
        include_str!("../assets/split_spine/index_split_047.html"),
    ),
    (
        "index_split_048.html",
        include_str!("../assets/split_spine/index_split_048.html"),
    ),
    (
        "index_split_049.html",
        include_str!("../assets/split_spine/index_split_049.html"),
    ),
    (
        "index_split_050.html",
        include_str!("../assets/split_spine/index_split_050.html"),
    ),
    (
        "index_split_051.html",
        include_str!("../assets/split_spine/index_split_051.html"),
    ),
];

/// A Calibre page-split EPUB 2, the way a Kindle-sourced conversion arrives.
///
/// The defining trap: a recipe is cut across spine documents at a page break.
/// Each title, with its full-bleed photo, sits alone in one tiny document while
/// the yield line, headnote, ingredients, and steps live in the next. A reader
/// that treats one spine document as one recipe finds two title-only recipes
/// and two headless ingredient lists.
///
/// Also present: an ingredient that cross-references another recipe by
/// `filepos` anchor, an essay with a procedure but no ingredient list, a
/// front-matter contents document, and a bare chapter-heading document.
///
/// Returns ZIP/I/O errors if archive construction fails.
pub fn split_spine() -> ZipResult<Vec<u8>> {
    let photo = jpeg_1x1(0x10);
    with_docs(
        EpubBuilder::new("Nothing Special")
            .author("Fixture Author")
            .identifier("urn:cookbook-fixtures:split-spine:v1")
            .subject("Cooking")
            .image("images/00036.jpg", "image/jpeg", &photo)
            .cover_image("images/00036.jpg")
            .nav(false)
            .nav_entry("Vegetables", "index_split_046.html", 1)
            .nav_entry(
                "Tangy Roasted Mushrooms",
                "index_split_047.html#filepos107429",
                2,
            )
            .nav_entry(
                "Sheet-Pan Chicken with Crispy Potatoes",
                "index_split_049.html#filepos118204",
                2,
            ),
        &SPLIT_SPINE_DOCS,
    )
    .build()
}

/// Ground truth for [`split_spine`].
#[must_use]
pub const fn split_spine_expected() -> Expected {
    Expected {
        book_title: "Nothing Special",
        identifier: "urn:cookbook-fixtures:split-spine:v1",
        spine: &[
            "index_split_000.html",
            "index_split_046.html",
            "index_split_047.html",
            "index_split_048.html",
            "index_split_049.html",
            "index_split_050.html",
            "index_split_051.html",
        ],
        recipes: &[
            ExpectedRecipe {
                title: "Tangy Roasted Mushrooms",
                docs: &["index_split_047.html", "index_split_048.html"],
                ingredient_lines: 6,
                steps: 3,
                is_variation: false,
                is_essay: false,
                cross_references: &[],
            },
            ExpectedRecipe {
                title: "Sheet-Pan Chicken with Crispy Potatoes",
                docs: &["index_split_049.html", "index_split_050.html"],
                ingredient_lines: 8,
                steps: 4,
                is_variation: false,
                is_essay: false,
                cross_references: &[CrossRef {
                    text: "this page",
                    target: "index_split_047.html#filepos107429",
                    linked: true,
                }],
            },
            ExpectedRecipe {
                title: "diy martini bar",
                docs: &["index_split_051.html"],
                ingredient_lines: 0,
                // The single `TO MAKE` paragraph. An essay can carry a
                // procedure without becoming a recipe.
                steps: 1,
                is_variation: false,
                is_essay: true,
                cross_references: &[],
            },
        ],
    }
}

// ---------------------------------------------------------------------------
// b. Publisher-native EPUB 3
// ---------------------------------------------------------------------------

/// Spine documents of [`epub3_nav_pagebreaks`], in spine order.
pub(crate) const EPUB3_NAV_DOCS: [(&str, &str); 2] = [
    (
        "xhtml/c02.xhtml",
        include_str!("../assets/epub3_nav/c02.xhtml"),
    ),
    (
        "xhtml/c07.xhtml",
        include_str!("../assets/epub3_nav/c07.xhtml"),
    ),
];

/// A publisher-native EPUB 3 with `nav.xhtml`, a page-list, and pagebreak spans.
///
/// The defining trap: several recipes share one chapter document, and the only
/// boundary between them is a `<p class="rt">` carrying a `doc-pagebreak`
/// span. Before the first title sits a photo caption naming a *different*
/// recipe ("Sour Cherry Pie") and linking to its page — a phantom title for
/// anything that takes the first prominent text in a document as the title.
///
/// Also present: an ingredient line that references a variation of a recipe in
/// another chapter document, a `cnum` footnote mark inside an ingredient, an
/// ingredient-less variation heading, a special-equipment line, and a printed
/// page-list covering pages 77, 78, 79, 111, 327, and 330.
///
/// Returns ZIP/I/O errors if archive construction fails.
pub fn epub3_nav_pagebreaks() -> ZipResult<Vec<u8>> {
    let cover = jpeg_1x1(0x10);
    let hero = jpeg_1x1(0x20);
    with_docs(
        EpubBuilder::new("Dessert Fixture")
            .version(EpubVersion::V3)
            .author("Fixture Author")
            .identifier("urn:cookbook-fixtures:epub3-nav-pagebreaks:v1")
            .subject("Baking")
            .image("images/0551.jpg", "image/jpeg", &cover)
            .image("images/2727.jpg", "image/jpeg", &hero)
            .cover_image("images/0551.jpg")
            .page_list(true)
            .nav_entry("Pies and Tarts", "xhtml/c02.xhtml", 1)
            .nav_entry(
                "Cranberry-Pomegranate Mousse Pie",
                "xhtml/c02.xhtml#page_79",
                2,
            )
            .nav_entry("Sour Cherry Pie", "xhtml/c02.xhtml#page_111", 2)
            .nav_entry("Foundational Recipes", "xhtml/c07.xhtml", 1)
            .nav_entry("Graham Cracker Crust", "xhtml/c07.xhtml#page_327", 2)
            .nav_entry("All-Butter Pie Dough", "xhtml/c07.xhtml#page_330", 2)
            .page(77, "xhtml/c02.xhtml#page_77")
            .page(78, "xhtml/c02.xhtml#page_78")
            .page(79, "xhtml/c02.xhtml#page_79")
            .page(111, "xhtml/c02.xhtml#page_111")
            .page(327, "xhtml/c07.xhtml#page_327")
            .page(330, "xhtml/c07.xhtml#page_330"),
        &EPUB3_NAV_DOCS,
    )
    .build()
}

/// Ground truth for [`epub3_nav_pagebreaks`].
///
/// The page-77 caption's link to `c02.xhtml#page_111` belongs to no recipe: it
/// precedes the first title and is deliberately not listed below, so a test can
/// assert that a reader does not attribute it to the mousse pie.
#[must_use]
pub const fn epub3_nav_pagebreaks_expected() -> Expected {
    Expected {
        book_title: "Dessert Fixture",
        identifier: "urn:cookbook-fixtures:epub3-nav-pagebreaks:v1",
        spine: &["xhtml/c02.xhtml", "xhtml/c07.xhtml"],
        recipes: &[
            ExpectedRecipe {
                title: "Cranberry-Pomegranate Mousse Pie",
                docs: &["xhtml/c02.xhtml"],
                ingredient_lines: 10,
                steps: 6,
                is_variation: false,
                is_essay: false,
                cross_references: &[CrossRef {
                    text: "this page",
                    target: "c07.xhtml#page_327",
                    linked: true,
                }],
            },
            ExpectedRecipe {
                title: "Sour Cherry Pie",
                docs: &["xhtml/c02.xhtml"],
                ingredient_lines: 8,
                steps: 5,
                is_variation: false,
                is_essay: false,
                cross_references: &[],
            },
            ExpectedRecipe {
                title: "Graham Cracker Crust",
                docs: &["xhtml/c07.xhtml"],
                ingredient_lines: 5,
                steps: 3,
                is_variation: false,
                is_essay: false,
                cross_references: &[],
            },
            ExpectedRecipe {
                title: "Speculoos Variation",
                docs: &["xhtml/c07.xhtml"],
                ingredient_lines: 0,
                steps: 0,
                is_variation: true,
                is_essay: false,
                cross_references: &[],
            },
            ExpectedRecipe {
                title: "All-Butter Pie Dough",
                docs: &["xhtml/c07.xhtml"],
                ingredient_lines: 4,
                steps: 4,
                is_variation: false,
                is_essay: false,
                cross_references: &[],
            },
        ],
    }
}

// ---------------------------------------------------------------------------
// c. Calibre-converted EPUB 2 with typographic classes
// ---------------------------------------------------------------------------

/// Spine documents of [`typographic_variations`], in spine order.
pub(crate) const TYPOGRAPHIC_DOCS: [(&str, &str); 3] = [
    (
        "text/part0011.html",
        include_str!("../assets/typographic/part0011.html"),
    ),
    (
        "text/part0012.html",
        include_str!("../assets/typographic/part0012.html"),
    ),
    (
        "text/part0021.html",
        include_str!("../assets/typographic/part0021.html"),
    ),
];

/// A Calibre-converted EPUB 2 whose classes describe typography, not structure.
///
/// The defining trap: titles are set in small caps by shredding them across
/// `<small>` runs, so `part0011.html`'s first title is the markup
/// `P<small>OLENTA</small>`. Its raw DOM text is `POLENTA`; the cleaned title
/// in [`typographic_variations_expected`] is `Polenta`, so a reader has to
/// recase rather than pass the concatenation through.
///
/// Class names say `hang`, `indentsp`, and `noindentsp` rather than
/// `ingredient` and `step`, and `noindentsp` is a prose class despite the
/// shared suffix. Also present: a starred variation heading between two
/// recipes, a drop cap that splits the first word of a headnote, `~` standing
/// in for em dashes, `{}` for parentheses, a linked cross-reference into a
/// back-matter section, a linked cross-reference to another document's page
/// anchor, and an unlinked `{see page 190}` reference.
///
/// Returns ZIP/I/O errors if archive construction fails.
pub fn typographic_variations() -> ZipResult<Vec<u8>> {
    let polenta = jpeg_1x1(0x10);
    let star = jpeg_1x1(0x20);
    with_docs(
        EpubBuilder::new("The Fixture Café Cookbook")
            .author("Fixture Author")
            .identifier("urn:cookbook-fixtures:typographic-variations:v1")
            .subject("Cooking")
            .image("images/00006.jpeg", "image/jpeg", &polenta)
            .image("images/00014.jpeg", "image/jpeg", &star)
            .nav(false)
            .nav_entry("Starchy Dishes", "text/part0011.html", 1)
            .nav_entry("Polenta", "text/part0011.html#page190", 2)
            .nav_entry(
                "Soft Polenta with Braised Greens",
                "text/part0011.html#page204",
                2,
            ),
        &TYPOGRAPHIC_DOCS,
    )
    .build()
}

/// Ground truth for [`typographic_variations`].
///
/// Titles are the cleaned forms. The raw DOM text of the three `sub-head-fp`
/// headings is `POLENTA`, `SOFT POLENTA with BRAISED GREENS`, and `CORN`; the
/// variation heading's raw text is `Variation POLENTA with FRESH CORN`.
#[must_use]
pub const fn typographic_variations_expected() -> Expected {
    Expected {
        book_title: "The Fixture Café Cookbook",
        identifier: "urn:cookbook-fixtures:typographic-variations:v1",
        spine: &[
            "text/part0011.html",
            "text/part0012.html",
            "text/part0021.html",
        ],
        recipes: &[
            ExpectedRecipe {
                title: "Polenta",
                docs: &["text/part0011.html"],
                ingredient_lines: 4,
                steps: 2,
                is_variation: false,
                is_essay: false,
                cross_references: &[CrossRef {
                    text: "here",
                    target: "part0021.html#page_518",
                    linked: true,
                }],
            },
            ExpectedRecipe {
                title: "Polenta with Fresh Corn",
                docs: &["text/part0011.html"],
                ingredient_lines: 3,
                steps: 1,
                is_variation: true,
                is_essay: false,
                cross_references: &[CrossRef {
                    text: "here",
                    target: "part0012.html#page_254",
                    linked: true,
                }],
            },
            ExpectedRecipe {
                title: "Soft Polenta with Braised Greens",
                docs: &["text/part0011.html"],
                ingredient_lines: 5,
                steps: 3,
                is_variation: false,
                is_essay: false,
                cross_references: &[CrossRef {
                    text: "page 190",
                    target: "page 190",
                    linked: false,
                }],
            },
            ExpectedRecipe {
                title: "Corn",
                docs: &["text/part0012.html"],
                ingredient_lines: 3,
                steps: 1,
                is_variation: false,
                is_essay: false,
                cross_references: &[],
            },
            ExpectedRecipe {
                title: "Sources and Resources",
                docs: &["text/part0021.html"],
                ingredient_lines: 0,
                steps: 0,
                is_variation: false,
                is_essay: true,
                cross_references: &[],
            },
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn fixture_images_are_decodable_jpegs() {
        let image = jpeg_1x1(0x10);
        assert_eq!(&image[..2], &[0xff, 0xd8]);
        assert_eq!(&image[JPEG_LEN - 2..], &[0xff, 0xd9]);
        assert_ne!(jpeg_1x1(0x10), jpeg_1x1(0x20));
    }

    #[test]
    fn images_reach_the_archive() -> Result<(), Box<dyn std::error::Error>> {
        let mut archive = zip::ZipArchive::new(Cursor::new(epub3_nav_pagebreaks()?))?;
        for path in ["OEBPS/images/0551.jpg", "OEBPS/images/2727.jpg"] {
            assert_eq!(archive.by_name(path)?.size(), JPEG_LEN as u64);
        }
        Ok(())
    }
}
