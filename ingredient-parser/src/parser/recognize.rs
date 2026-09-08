//! Compositional whole-line shapes, peeled iteratively before one core parse.
//! Each layer retains its authored range and contributes a structural effect;
//! nested shapes do not recurse through the ingredient parser.

use super::ir::{ModifierPart, ParsedIngredient};
use crate::IngredientParser;
use crate::parser::{MeasurementMode, MeasurementParser};
use crate::unit::{self, Measure};
use std::ops::Range;

struct Shape<'a> {
    inner: &'a str,
    effect: Effect,
}
enum Effect {
    Optional,
    Description {
        span: Range<usize>,
    },
    Trailing {
        amounts: Vec<Measure>,
        spans: Vec<Range<usize>>,
        descriptions: Vec<Range<usize>>,
    },
    Component {
        phrase: String,
        span: Range<usize>,
    },
}

impl IngredientParser {
    pub(super) fn parse_shape(&self, input: &str) -> Option<ParsedIngredient> {
        let mut current = input;
        let mut layers = Vec::new();
        loop {
            let shape = RECOGNIZERS.iter().find_map(|recognizer| {
                let result = (recognizer.run)(self, current);
                crate::trace::trace_attempt(recognizer.id().as_str(), current, result, |shape| {
                    shape.inner.to_string()
                })
            });
            let Some(shape) = shape else {
                break;
            };
            layers.push((current, shape.effect));
            current = shape.inner;
        }
        let parse_core = |source: &str| {
            self.parse_ingredient_ir(source)
                .ok()
                .map(|(_, mut parsed)| {
                    parsed.rebase(input, source.as_ptr() as usize - input.as_ptr() as usize);
                    parsed
                })
        };
        let mut parsed = parse_core(current);
        // Keep amount groups separate while unwinding: prepending each group to
        // a growing vector would make deeply nested trailing forms quadratic.
        let mut trailing = Vec::<Vec<Measure>>::new();
        for (source, effect) in layers.into_iter().rev() {
            let valid = parsed.as_ref().is_some_and(|inner| match &effect {
                Effect::Optional => {
                    !inner.name.is_empty() || !inner.amounts.is_empty() || !trailing.is_empty()
                }
                Effect::Component { .. } => {
                    !inner.name.trim().is_empty()
                        && (!inner.amounts.is_empty() || !trailing.is_empty())
                }
                Effect::Trailing { .. } | Effect::Description { .. } => true,
            });
            if !valid {
                parsed = parse_core(source);
                trailing.clear();
                continue;
            }
            let Some(inner) = parsed.as_mut() else {
                continue;
            };
            let offset = source.as_ptr() as usize - input.as_ptr() as usize;
            match effect {
                Effect::Optional => inner.optional = true,
                Effect::Trailing {
                    amounts,
                    spans,
                    descriptions,
                } => {
                    if !amounts.is_empty() {
                        trailing.push(amounts);
                    }
                    inner.measure_spans.extend(
                        spans
                            .into_iter()
                            .map(|span| offset + span.start..offset + span.end),
                    );
                    for span in descriptions {
                        inner.modifier.push(
                            ModifierPart::raw(source[span.clone()].trim().to_string())
                                .at_range(offset + span.start..offset + span.end),
                        );
                    }
                }
                Effect::Description { span } => inner.modifier.push(
                    ModifierPart::raw(source[span.clone()].trim().to_string())
                        .at_range(offset + span.start..offset + span.end),
                ),
                Effect::Component { phrase, span } => inner.modifier.push(
                    ModifierPart::raw(phrase).at_range(offset + span.start..offset + span.end),
                ),
            }
        }
        parsed.map(|mut parsed| {
            let mut amounts: Vec<Measure> = trailing.into_iter().rev().flatten().collect();
            amounts.append(&mut parsed.amounts);
            parsed.amounts = amounts;
            parsed
        })
    }

    /// Try to parse an optional ingredient format: "(amount ingredient, modifier)"
    ///
    /// When an entire ingredient line is wrapped in parentheses, it indicates
    /// the ingredient is optional. This is common in cookbooks like Joy of Cooking.
    fn try_parse_optional_ingredient<'a>(&self, input: &'a str) -> Option<Shape<'a>> {
        let mut inner = input.trim();
        if !inner.starts_with('(') {
            return None;
        }
        // Pair once, then peel wrappers by offset without rescanning each layer.
        let mut closes = vec![None; input.len()];
        let mut stack = Vec::new();
        for (offset, byte) in input.bytes().enumerate() {
            if byte == b'(' {
                stack.push(offset);
            } else if byte == b')'
                && let Some(open) = stack.pop()
            {
                closes[open] = Some(offset);
            }
        }
        let mut wrapped = false;
        while inner.starts_with('(') && inner.len() >= 2 {
            let start = inner.as_ptr() as usize - input.as_ptr() as usize;
            if closes[start] != Some(start + inner.len() - 1) {
                break;
            }
            inner = inner[1..inner.len() - 1].trim();
            wrapped = true;
        }
        if !wrapped {
            return None;
        }
        Some(Shape {
            inner,
            effect: Effect::Optional,
        })
    }

    /// Try to parse ingredient with trailing amount format: "Name — AMOUNT"
    ///
    /// This handles professional/European cookbook formats where the amount
    /// comes at the end after an em-dash, en-dash, or double hyphen.
    fn try_parse_trailing_amount_format<'a>(&self, input: &'a str) -> Option<Shape<'a>> {
        let separators = [" — ", " – ", " -- "];
        let mp = MeasurementParser::new(&self.units, MeasurementMode::IngredientList);

        for sep in separators {
            let Some(pos) = input.rfind(sep) else {
                continue;
            };

            let name_part = &input[..pos];
            let amount_part = &input[pos + sep.len()..];

            let Ok((remaining, amounts)) = mp.parse_measurement_list(amount_part) else {
                continue;
            };

            if amounts.is_empty() || !remaining.trim().is_empty() {
                continue;
            }

            let span = pos + sep.len()..input.len();
            let effect = if amounts.iter().all(|m| is_descriptive_unit(m.unit())) {
                Effect::Description { span }
            } else {
                let descriptions: Vec<_> = if amounts.iter().any(|m| is_descriptive_unit(m.unit()))
                {
                    super::segment::dimensional_spans(amount_part)
                        .into_iter()
                        .map(|range| span.start + range.start..span.start + range.end)
                        .collect()
                } else {
                    Vec::new()
                };
                let mut spans = Vec::new();
                let mut cursor = span.start;
                for description in &descriptions {
                    if cursor < description.start {
                        spans.push(cursor..description.start);
                    }
                    cursor = cursor.max(description.end);
                }
                if cursor < span.end {
                    spans.push(cursor..span.end);
                }
                let amounts = amounts
                    .into_iter()
                    .filter(|m| !is_descriptive_unit(m.unit()))
                    .collect();
                Effect::Trailing {
                    amounts,
                    spans,
                    descriptions,
                }
            };
            return Some(Shape {
                inner: name_part.trim(),
                effect,
            });
        }

        None
    }

    /// Try to parse an "X of/from N item" construction such as "Juice of 1 lemon",
    /// "Grated zest of 2 limes", "Finely grated zest from 1 lemon", "Peel of 1
    /// grapefruit", "Seeds scraped from 1 vanilla bean", or "Leaves from 3 sprigs
    /// thyme". These describe a component derived from a countable item; the item
    /// becomes the name (with its count), and the leading phrase ("juice of",
    /// "seeds scraped from", ...) moves into the modifier.
    fn try_parse_x_of_construction<'a>(&self, input: &'a str) -> Option<Shape<'a>> {
        let trimmed = input.trim();

        // Find the leading "… of " / "… from " clause whose pivot is immediately
        // followed by a number (e.g. "Seeds scraped from 1 …"). Uses the EARLIEST
        // qualifying pivot across both separators.
        // A qualifying phrase has at most five words; searching beyond its
        // sixth token cannot find a valid pivot and needlessly rescans long tails.
        let prefix_end = crate::parser::token::offsets(trimmed)
            .nth(5)
            .map_or(trimmed.len(), |(offset, _)| offset);
        let lower = crate::parser::byte_aligned_lowercase(&trimmed[..prefix_end])?;
        let pivot_end = [" of ", " from "]
            .iter()
            .filter_map(|sep| {
                lower.find(sep).and_then(|pos| {
                    let after = pos + sep.len();
                    // A number must follow the separator: a digit/vulgar fraction
                    // or a spelled-out count ("one lemon"). This keeps normal
                    // names with "of"/"from" (e.g. "cream of tartar", "heart of
                    // palm") from being captured.
                    let tail = &trimmed[after..];
                    let starts_number = tail
                        .chars()
                        .next()
                        .is_some_and(|c| c.is_ascii_digit() || crate::fraction::is_vulgar(c))
                        || crate::parser::text_number(tail).is_ok();
                    starts_number.then_some(after)
                })
            })
            .min()?;

        let phrase = trimmed[..pivot_end].trim();
        // Guard against a bare leading pivot ("of 1 lemon") with no descriptor.
        if phrase.is_empty() || phrase.split_whitespace().count() > 5 {
            return None;
        }

        let rest = trimmed[pivot_end..].trim_start();
        let start = trimmed.as_ptr() as usize - input.as_ptr() as usize;
        Some(Shape {
            inner: rest,
            effect: Effect::Component {
                phrase: phrase.to_string(),
                span: start..start + phrase.len(),
            },
        })
    }
}

type Recognizer = for<'a> fn(&IngredientParser, &'a str) -> Option<Shape<'a>>;

crate::define_stage_pipeline! {
    pub(crate) enum RecognizerId,
    struct RecognizerEntry,
    const RECOGNIZERS: &[RecognizerEntry],
    type Recognizer = Recognizer,
    trace: pub(crate) RECOGNIZER_TRACE_NAMES,
    (OptionalWrapped, "optional_wrapped", IngredientParser::try_parse_optional_ingredient),
    (
        TrailingAmount,
        "trailing_amount",
        IngredientParser::try_parse_trailing_amount_format
    ),
    (
        XOfConstruction,
        "x_of_construction",
        IngredientParser::try_parse_x_of_construction
    ),
}

fn is_descriptive_unit(unit: &unit::Unit) -> bool {
    matches!(
        unit,
        unit::Unit::Inch | unit::Unit::Fahrenheit | unit::Unit::Celsius
    ) || matches!(unit, unit::Unit::Other(name) if crate::parser::is_distance_unit(name))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]
    use super::*;
    use rstest::rstest;

    /// Generated nesting checks algorithmic depth and ownership, rather than
    /// duplicating the corpus's fixed ingredient accuracy examples.
    #[rstest]
    #[case(false)]
    #[case(true)]
    fn deeply_nested_shapes_do_not_recurse(#[case] optional: bool) {
        let parser = IngredientParser::new();
        let depth = 2048;
        let source = if optional {
            format!("{}flour{}", "(".repeat(depth), ")".repeat(depth))
        } else {
            format!("flour{}", " — 1 cup".repeat(depth))
        };
        let parsed = parser
            .parse_shape(&source)
            .expect("generated shape should resolve");
        assert_eq!(parsed.optional, optional);
        assert_eq!(parsed.amounts.len(), if optional { 0 } else { depth });
        assert!(parsed.ownership().iter().all(|(range, _)| {
            range.end <= source.len()
                && source.is_char_boundary(range.start)
                && source.is_char_boundary(range.end)
        }));
    }

    #[test]
    fn trailing_layers_keep_outer_to_inner_amount_order() {
        let parser = IngredientParser::new();
        let parsed = parser
            .parse_shape("flour — 1 cup — 2 tablespoons")
            .expect("nested trailing shape should resolve");
        assert_eq!(
            parsed.amounts,
            vec![Measure::new("tablespoon", 2.0), Measure::new("cup", 1.0)]
        );
    }
}
