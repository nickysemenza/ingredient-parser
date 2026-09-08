//! Parser modules for ingredient parsing
//!
//! This module contains the core parsing logic organized into focused sub-modules.
//!
//! # Where does a parser fix go?
//!
//! Parsing is a five-stage pipeline: **normalize** (pre-parse string rewrites) →
//! **recognize** (whole-line special forms) → **grammar** (the nom amounts
//! parse) → **segment** (clause segmentation + assembly of name/modifier) →
//! **refine** (name-internal passes). When a harvested corpus line parses
//! wrong, run it through the stage view to see *which stage* mishandled it —
//! then the fix goes in that stage:
//!
//! ```text
//! cargo run -p food-cli --quiet -- parse-ingredient --explain "<line>"
//! ```
//!
//! Follow the first incorrect interpretation and fix its owning module:
//!
//! - [`normalize`] handles textual artifacts: whitespace, list bullets, and
//!   footnote glyphs. Each edit retains its authored occurrence mapping.
//! - [`recognize`] describes composable whole-line shapes, including trailing
//!   amounts, optional wrappers, and derived components.
//! - [`measurement`] and [`vocab`] own units, qualifiers, and configured
//!   preparation vocabulary shared with rich-text parsing.
//! - [`segment`] resolves clause relationships, references, optional notes,
//!   aliases, dimensions, and parenthetical measures before field assignment.
//! - [`refine`] interprets name-local preparation and count units using the same
//!   source-bearing representation. Extraction transfers source ownership.
//!
//! Keep names opaque and preserve ambiguous coordination. Add acceptance rows
//! in `tests/corpus/corpus.jsonl`; document intentional semantic corrections
//! separately. Replace the incorrect structural rule and delete its superseded
//! repair rather than appending a later correction.

pub(crate) mod helpers;
pub(crate) mod ir;
pub(crate) mod measurement;
pub(crate) mod normalize;
pub(crate) mod paren;
pub(crate) mod pipeline;
pub(crate) mod recognize;
pub(crate) mod refine;
pub(crate) mod segment;
pub(crate) mod stage;
pub(crate) mod token;
pub(crate) mod vocab;

pub(crate) use helpers::parse_amount_string;
pub(crate) use helpers::{
    Res, byte_aligned_lowercase, parse_unit_text, text_number, thousands_number,
};
pub(crate) use measurement::guards::is_distance_unit;
pub(crate) use measurement::{MeasurementMode, MeasurementParser};
