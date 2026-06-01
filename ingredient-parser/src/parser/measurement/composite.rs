//! Composite measurement parsing (plus expressions, parenthesized amounts)

use nom::{
    branch::alt,
    bytes::complete::tag,
    character::complete::{char, space0},
    error::{context, ParseError},
    sequence::delimited,
    Parser,
};
use nom_language::error::VerboseError;

use crate::parser::Res;
use crate::traced_parser;
use crate::unit::Measure;

use super::MeasurementParser;

/// Container nouns that can follow a parenthesized size, e.g. the "piece" in
/// "1 (1-ounce) piece ginger". Kept narrow so the size-hoisting parser doesn't
/// over-match arbitrary parentheticals.
const CONTAINER_NOUNS: &[&str] = &[
    "piece", "pieces", "can", "cans", "knob", "knobs", "package", "packages", "packet", "packets",
    "bottle", "bottles", "jar", "jars", "block", "blocks", "bunch", "bunches", "head", "heads",
    "stick", "sticks", "fillet", "fillets", "loaf", "chunk", "chunks", "ball", "balls", "box",
    "boxes", "disk", "disks", "wedge", "wedges",
];

impl<'a> MeasurementParser<'a> {
    /// Parse measurements enclosed in matching delimiters
    fn parse_delimited_amounts<'b>(
        &self,
        input: &'b str,
        open: char,
        close: char,
        name: &'static str,
    ) -> Res<&'b str, Vec<Measure>> {
        traced_parser!(
            name,
            input,
            context(
                name,
                delimited(char(open), |a| self.parse_measurement_list(a), char(close),),
            )
            .parse(input),
            |measures: &Vec<Measure>| measures
                .iter()
                .map(|m| m.to_string())
                .collect::<Vec<_>>()
                .join(", "),
            "no delimited amounts"
        )
    }

    /// Parse measurements enclosed in parentheses: (1 cup)
    pub(crate) fn parse_parenthesized_amounts<'b>(
        &self,
        input: &'b str,
    ) -> Res<&'b str, Vec<Measure>> {
        self.parse_delimited_amounts(input, '(', ')', "parenthesized_amounts")
    }

    /// Parse measurements enclosed in square brackets: [56 G]
    ///
    /// Common in professional cookbooks like American Sfoglino where
    /// alternate measurements are shown in brackets: "4 TBSP [56 G] BUTTER"
    pub(crate) fn parse_bracketed_amounts<'b>(&self, input: &'b str) -> Res<&'b str, Vec<Measure>> {
        self.parse_delimited_amounts(input, '[', ']', "bracketed_amounts")
    }

    /// Parse "`<count> (<size>) <container>`" such as "1 (1-ounce) piece",
    /// "1 (28-ounce) can", or "2 (14.5 oz) cans", producing
    /// `[<count> <container>, <inner size>]` — e.g. "1 (1-ounce) piece ginger"
    /// → `[1 piece, 1 oz]` and "2 (14.5 oz) cans tomatoes" → `[2 can, 14.5 oz]`.
    ///
    /// Fires for both the hyphenated size adjective ("1-ounce", "13.5-gram") and
    /// the space form ("14.5 oz"), as long as the parenthetical fully parses as a
    /// measurement *and* a container noun follows. The container-noun requirement
    /// is what keeps arbitrary parentheticals like "(not defrosted)" from matching
    /// — so a bare "1 (14.5 oz) of stock" still falls through to
    /// [`parse_parenthesized_amounts`].
    pub(super) fn parse_count_with_parenthetical_size<'b>(
        &self,
        input: &'b str,
    ) -> Res<&'b str, Vec<Measure>> {
        let reject = || {
            nom::Err::Error(VerboseError::from_error_kind(
                input,
                nom::error::ErrorKind::Verify,
            ))
        };

        // Leading count, e.g. "1".
        let (rest, value) = self.parse_value(input).map_err(|_| reject())?;
        let (rest, _) = space0::<_, VerboseError<&str>>(rest).map_err(|_| reject())?;
        if !rest.starts_with('(') {
            return Err(reject());
        }

        // Find the matching close paren (handles nesting).
        let mut depth = 0usize;
        let mut close = None;
        for (i, c) in rest.char_indices() {
            match c {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        close = Some(i);
                        break;
                    }
                }
                _ => {}
            }
        }
        let close = close.ok_or_else(reject)?;
        let inner = &rest[1..close];

        // Require a container noun immediately after the parenthetical. This is
        // the gate that keeps non-size parentheticals (e.g. "(not defrosted)")
        // from matching — so both the hyphenated "1-ounce" and the space form
        // "14.5 oz" are accepted here.
        let after = rest[close + 1..].trim_start();
        let word_end = after.find(char::is_whitespace).unwrap_or(after.len());
        let container = after[..word_end].to_lowercase();
        if !CONTAINER_NOUNS.contains(&container.as_str()) {
            return Err(reject());
        }
        let after_rest = &after[word_end..];

        // The inner size must fully parse as a measurement (hyphen → space).
        let inner_norm = inner.replace('-', " ");
        let inner_measures = match self.parse_measurement_list(inner_norm.as_str()) {
            Ok((r, m)) if r.trim().is_empty() && !m.is_empty() => m,
            _ => return Err(reject()),
        };

        let mut measures = Vec::with_capacity(1 + inner_measures.len());
        measures.push(Measure::from_parts(container.as_ref(), value.0, value.1));
        measures.extend(inner_measures);
        Ok((after_rest, measures))
    }

    /// Parse expressions with "plus" or "+" that combine two measurements
    ///
    /// For example: "1 cup plus 2 tablespoons" or "½ cup + 2 tablespoons".
    ///
    /// When the two measures are compatible (same kind) they are summed into a
    /// single [`Measure`]. When they are incompatible (e.g. "1 cup plus 100 g"),
    /// both are returned as separate amounts rather than silently dropping one.
    pub(super) fn parse_plus_expression<'b>(&self, input: &'b str) -> Res<&'b str, Vec<Measure>> {
        // Define the structure of a plus expression
        // Accept either the word "plus" or the "+" symbol
        let plus_parser = (
            |a| self.parse_single_measurement(a), // First measurement
            nom::character::complete::space1,     // Required whitespace
            alt((tag("plus"), tag("+"))),         // The "plus" keyword or "+" symbol
            nom::character::complete::space1,     // Required whitespace
            |a| self.parse_single_measurement(a), // Second measurement
        );

        traced_parser!(
            "parse_plus_expression",
            input,
            context("plus_expression", plus_parser).parse(input).map(
                |(next_input, (first_measure, _, _, _, second_measure))| {
                    // Sum compatible measures; otherwise keep both rather than
                    // discarding the second (which loses data the recipe stated).
                    let measures = match first_measure.clone().add(second_measure.clone()) {
                        Ok(combined) => vec![combined],
                        Err(_) => vec![first_measure, second_measure],
                    };
                    (next_input, measures)
                },
            ),
            |measures: &Vec<Measure>| measures
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(" + "),
            "no plus expression"
        )
    }
}
