//! Exercise the public fixture through the real EPUB reader, without a model.
use recipe_epub::{chunk_epub, epub_metadata};

#[test]
fn generated_cookbook_has_metadata_and_ordered_recipe_sources()
-> Result<(), Box<dyn std::error::Error>> {
    let bytes = recipe_epub_fixtures::cookbook_epub()?;
    let metadata = epub_metadata(&bytes).ok_or("fixture metadata was unreadable")?;
    assert_eq!(metadata.title, "Synthetic cookbook");
    assert_eq!(metadata.authors, ["Fixture Author"]);
    let chunks = chunk_epub(&bytes)?;
    assert_eq!(chunks.len(), 3);
    let expected = [
        (
            "OEBPS/tomato-soup.xhtml",
            "Roasted tomato soup with white beans and basil",
        ),
        ("OEBPS/flatbread.xhtml", "Flatbread"),
        ("OEBPS/lemon-dressing.xhtml", "Lemon dressing"),
    ];
    for (chunk, (path, title)) in chunks.iter().zip(expected) {
        assert_eq!(chunk.doc_path, path);
        assert!(chunk.text.contains(title), "missing recipe title: {title}");
        assert!(chunk.text.contains("Ingredients"));
        assert!(chunk.text.contains("Instructions"));
        assert!(chunk.images.is_empty());
    }
    assert!(chunks[0].text.contains("2 × 400 g cans whole tomatoes"));
    assert!(
        chunks[0]
            .text
            .contains("1/2 tsp crushed red pepper flakes, optional")
    );
    assert!(
        chunks[0]
            .text
            .contains("Stir in the basil just before serving.")
    );
    Ok(())
}
