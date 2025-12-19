//! Composite measurement parsing (plus expressions, parenthesized amounts)

use nom::{
    bytes::complete::tag, character::complete::char, error::context, sequence::delimited, Parser,
};

use crate::parser::Res;
use crate::traced_parser;
use crate::unit::Measure;

use super::MeasurementParser;

impl<'a> MeasurementParser<'a> {
    /// Parse measurements enclosed in parentheses: (1 cup)
    pub fn parse_parenthesized_amounts<'b>(&self, input: &'b str) -> Res<&'b str, Vec<Measure>> {
        traced_parser!(
            "parse_parenthesized_amounts",
            input,
            context(
                "parenthesized_amounts",
                delimited(
                    char('('),                          // Opening parenthesis
                    |a| self.parse_measurement_list(a), // Parse measurements inside parentheses
                    char(')'),                          // Closing parenthesis
                ),
            )
            .parse(input),
            |measures: &Vec<Measure>| measures
                .iter()
                .map(|m| m.to_string())
                .collect::<Vec<_>>()
                .join(", "),
            "no parenthesized amounts"
        )
    }

    /// Parse expressions with "plus" that combine two measurements
    ///
    /// For example: "1 cup plus 2 tablespoons"
    pub(super) fn parse_plus_expression<'b>(&self, input: &'b str) -> Res<&'b str, Measure> {
        // Define the structure of a plus expression
        let plus_parser = (
            |a| self.parse_single_measurement(a), // First measurement
            nom::character::complete::space1,     // Required whitespace
            tag("plus"),                          // The "plus" keyword
            nom::character::complete::space1,     // Required whitespace
            |a| self.parse_single_measurement(a), // Second measurement
        );

        traced_parser!(
            "parse_plus_expression",
            input,
            context("plus_expression", plus_parser).parse(input).map(
                |(next_input, (first_measure, _, _, _, second_measure))| {
                    // Add the two measurements together
                    match first_measure.add(second_measure) {
                        Ok(combined) => (next_input, combined),
                        Err(_) => {
                            // If addition fails, just return the first measure as fallback
                            (next_input, first_measure)
                        }
                    }
                },
            ),
            |m: &Measure| m.to_string(),
            "no plus expression"
        )
    }
}
