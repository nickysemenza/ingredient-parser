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
//! Read the report top-down and match the first rule that fits:
//!
//! - **A text artifact the parser should never see** — a non-breaking space,
//!   leading bullet, footnote glyph, cross-reference, equivalence aside, or a
//!   leading determiner ("the …"). → add a rewrite to [`normalize::REWRITES`].
//! - **A whole-line shape the pipeline can't express** — "Juice of 1 lemon",
//!   "Flour — 2 cups", an outer-parenthesized optional ingredient. → add a
//!   recognizer to [`recognize::RECOGNIZERS`].
//! - **A new unit, qualifier, or prep word** — "fl oz", "scant", "rib",
//!   "spatchcocked". → add it to [`vocab`] (and, for units/qualifiers that need
//!   grammar, [`measurement::single`]).
//! - **The line's clause structure was read wrong** — a clause was
//!   misclassified (`segment:` shows each clause's kind), a head noun stayed
//!   stranded in the modifier, a parenthetical hoisted (or didn't) as a
//!   secondary amount, an alias paren fell off the name. → adjust the
//!   [`segment`] classifier/assembly (`segment::CLASSIFIER`,
//!   `segment::ASSEMBLY_REPAIRS`).
//! - **The name itself needs work** — a prep adjective stuck to the name, a
//!   purpose clause or "X or Y" alternative needs splitting out, a size or
//!   produce count-unit should be claimed. The `segment`-assembled name and
//!   the final `result:` name differ, or *should*. → add/adjust a pass in
//!   [`refine::REFINE_PIPELINE`]. (`--explain` lists each refine pass that
//!   fired.)
//! - **Last resort: string surgery to unblock the parse** — e.g. lifting a
//!   mid-name dimensional aside so the name carve doesn't stall. → a "lift"
//!   rewrite in [`normalize`].
//!
//! The `normalize::REWRITES`, `recognize::RECOGNIZERS`, and `refine::REFINE_PIPELINE`
//! tables (built via [`stage::define_stage_pipeline!`](crate::define_stage_pipeline))
//! and the `segment::CLASSIFIER` / `segment::ASSEMBLY_REPAIRS` tables are each
//! an ordered, named, one-line-to-extend source of truth; the refine order is
//! load-bearing (see [`refine`]). Always add a corpus row for the fix
//! (`tests/corpus/corpus.jsonl`).

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
    Res, byte_aligned_lowercase, parse_ingredient_text, parse_unit_text, text_number,
    thousands_number,
};
pub(crate) use measurement::guards::is_distance_unit;
pub(crate) use measurement::{MeasurementMode, MeasurementParser};
