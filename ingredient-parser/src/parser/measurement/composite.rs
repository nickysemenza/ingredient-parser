//! Composite measurement parsing (plus expressions, parenthesized amounts)

use nom::{
    branch::alt, bytes::complete::tag, character::complete::char, error::context,
    sequence::delimited, Parser,
};

use crate::parser::Res;
use crate::traced_parser;
use crate::unit::Measure;

use super::MeasurementParser;

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
