//! Extract a structured recipe tree from an EPUB cookbook.
//!
//! The book is read into one cleaned line stream, cut into chunks, and each
//! chunk is sent to a model that answers with *line indices* — Rust copies the
//! text, so nothing is ever paraphrased. Deterministic validation and a
//! table-of-contents cross-check run on every book; a second model re-reads
//! only what was flagged. The result is a [`Cookbook`]: chapters of recipes,
//! techniques and essays with dependency edges, photos, and provenance, plus a
//! [`RunReport`] recording every call, its cost, and its outcome.
//!
//! Hosts differ only in the transport that carries a fully built gateway
//! request: native code uses reqwest, the browser hands it to a JavaScript
//! callback.

pub mod chunk;
mod cost;
pub mod epub;
mod error;
pub mod lines;
pub mod model;
pub mod report;

pub use cost::Usage;
pub use error::{Error, Result};
pub use model::*;
pub use report::*;

#[cfg(all(test, feature = "typescript"))]
mod typescript_tests {
    use ts_rs::TS;

    #[test]
    fn book_tree_declares_in_typescript() {
        let cfg = ts_rs::Config::default();
        let decl = super::Cookbook::decl(&cfg);
        assert!(decl.contains("chapters: Array<Chapter>"), "{decl}");
        let line = super::IngredientLine::decl(&cfg);
        assert!(line.contains("parsed: Ingredient"), "{line}");
        assert!(line.contains("ref: RecipeRef | null"), "{line}");
    }
}
