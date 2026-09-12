//! Write independent shape fixtures and answer keys for live backend comparisons.
//! Usage: cargo run -p cookbook --example catalog_fixtures -- /tmp/catalog-eval
use cookbook::eval::{Expectations, Sample, Variant};
use cookbook_fixtures::{
    Expected, epub3_nav_pagebreaks, epub3_nav_pagebreaks_expected, split_spine,
    split_spine_expected, typographic_variations, typographic_variations_expected,
};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dir = std::env::args()
        .nth(1)
        .ok_or("Supply an output directory")?;
    let dir = std::path::PathBuf::from(dir);
    std::fs::create_dir_all(dir.join("expectations"))?;
    let cases: [(&str, Vec<u8>, Expected); 3] = [
        ("split-spine", split_spine()?, split_spine_expected()),
        (
            "publisher",
            epub3_nav_pagebreaks()?,
            epub3_nav_pagebreaks_expected(),
        ),
        (
            "typographic",
            typographic_variations()?,
            typographic_variations_expected(),
        ),
    ];
    for (slug, bytes, expected) in cases {
        let path = dir.join(format!("{slug}.epub"));
        std::fs::write(&path, bytes)?;
        let mut key = Expectations {
            book: path.canonicalize()?.display().to_string(),
            shape: slug.into(),
            ..Default::default()
        };
        let mut parent = String::new();
        for recipe in expected.recipes {
            if recipe.is_essay {
                key.not_recipes.push(recipe.title.into());
                continue;
            }
            if recipe.is_variation {
                key.variants.push(Variant {
                    title: recipe.title.into(),
                    of: parent.clone(),
                });
            } else {
                key.titles.push(recipe.title.into());
                parent = recipe.title.into();
            }
            key.samples.push(Sample {
                title: recipe.title.into(),
                ingredients: Some(recipe.ingredient_lines),
                steps: Some(recipe.steps),
                ..Default::default()
            });
        }
        std::fs::write(
            dir.join("expectations").join(format!("{slug}.json")),
            serde_json::to_string_pretty(&key)?,
        )?;
    }
    println!("{}", dir.display());
    Ok(())
}
