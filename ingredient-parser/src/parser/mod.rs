//! Parser modules for ingredient parsing
//!
//! This module contains the core parsing logic organized into focused sub-modules.

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
pub(crate) use measurement::MeasurementParser;
