//! Parser modules for ingredient parsing
//!
//! This module contains the core parsing logic organized into focused sub-modules.
//!
//! # Where does a parser fix go?
//!
//! Parsing is a four-stage pipeline: **normalize** (pre-parse string rewrites) →
//! **recognize** (whole-line special forms) → **grammar** (the nom parse) →
//! **refine** (post-parse passes). When a harvested corpus line parses wrong,
//! run it through the stage view to see *which stage* mishandled it — then the
//! fix goes in that stage:
//!
//! ```text
//! cargo run -p food-cli --quiet -- parse-ingredient --explain "<line>"
//! ```
//!
//! Read the report top-down and match the first rule that fits:
//!
//! - **A text artifact the grammar should never see** — a non-breaking space,
//!   leading bullet, footnote glyph, cross-reference, equivalence aside, or a
//!   leading determiner ("the …"). → add a rewrite to [`normalize::REWRITES`].
//! - **A whole-line shape the grammar can't express** — "Juice of 1 lemon",
//!   "Flour — 2 cups", an outer-parenthesized optional ingredient. → add a
//!   recognizer to [`recognize::RECOGNIZERS`].
//! - **A new unit, qualifier, or prep word** — "fl oz", "scant", "rib",
//!   "spatchcocked". → add it to [`vocab`] (and, for units/qualifiers that need
//!   grammar, [`measurement::single`]).
//! - **The `grammar:` line captured the wrong span** — the name leaked into the
//!   modifier (or vice versa), a prep adjective stuck to the name, an
//!   alternative/secondary-amount needs splitting out. The `grammar:` name and
//!   the final `result:` name differ, or *should*. → add/adjust a pass in
//!   [`refine::POST_PASSES`]. (`--explain` lists each refine pass that fired.)
//! - **Last resort: string surgery to unblock the grammar** — e.g. lifting a
//!   mid-name dimensional aside so the name grammar doesn't stall. → a "lift"
//!   rewrite in [`normalize`].
//!
//! The `normalize::REWRITES`, `recognize::RECOGNIZERS`, and `refine::POST_PASSES`
//! lists are each an ordered, named, one-line-to-extend source of truth; the
//! refine order is load-bearing (see [`refine`]). Always add a corpus row for
//! the fix (`tests/corpus/corpus.jsonl`).

pub(crate) mod helpers;
pub(crate) mod ir;
pub(crate) mod measurement;
pub(crate) mod normalize;
pub(crate) mod pipeline;
pub(crate) mod recognize;
pub(crate) mod refine;
pub(crate) mod vocab;

pub(crate) use helpers::parse_amount_string;
pub(crate) use helpers::{
    parse_ingredient_text, parse_unit_text, text_number, thousands_number, Res,
};
pub(crate) use measurement::guards::is_distance_unit;
pub(crate) use measurement::{MeasurementMode, MeasurementParser};
