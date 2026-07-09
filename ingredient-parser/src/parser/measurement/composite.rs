//! Composite measurement parsing (plus expressions, parenthesized amounts)

use nom::{
    Parser,
    branch::alt,
    bytes::complete::{tag, take_till},
    character::complete::{char, space0, space1},
    combinator::verify,
    error::{ParseError, context},
    sequence::delimited,
};
use nom_language::error::VerboseError;

use crate::parser::Res;
use crate::traced_parser;
use crate::unit::Measure;

use super::guards::find_matching_paren;
use super::{DEFAULT_UNIT, MeasurementParser};

use crate::parser::vocab::CONTAINER_NOUNS;

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
                // The close arm tolerates a *space-separated, digit-free*
                // descriptive tail before ')': "(8 ounces by weight)" parses
                // "8 ounces" and discards " by weight" rather than failing the
                // whole delimited parse (which would abort the entire line). Two
                // load-bearing guards keep real content from being silently eaten:
                //   - the space requirement keeps "(70% cacao)" failing here (the
                //     "%" abuts the bare "70", which must stay a modifier, not
                //     become a unitless amount);
                //   - the digit-free requirement keeps a quantity clause like
                //     "(2 sticks minus 1 tablespoon)" failing here so it stays a
                //     modifier instead of collapsing to just "2 sticks".
                // Only a wordy descriptor ("by weight"/"by volume"/"packed") drops.
                delimited(
                    char(open),
                    |a| self.parse_measurement_list(a),
                    alt((
                        char(close).map(|_| ()),
                        (
                            space1,
                            verify(take_till(|c: char| c == close), |s: &str| {
                                !s.chars().any(|c| c.is_ascii_digit())
                            }),
                            char(close),
                        )
                            .map(|_| ()),
                    )),
                ),
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
    /// "1 (28-ounce) can", or "2 (14.5 oz) cans", and the paren-less hyphenated
    /// form "`<count> <N-unit> <container>`" such as "One 10-ounce disk" — all
    /// producing `[<count> <container>, <size>]`, e.g. "1 (1-ounce) piece ginger"
    /// → `[1 piece, 1 oz]`, "2 (14.5 oz) cans tomatoes" → `[2 can, 14.5 oz]`, and
    /// "One 10-ounce disk Pie Dough" → `[1 disk, 10 oz]`.
    ///
    /// A container noun must follow the size. That requirement keeps arbitrary
    /// parentheticals like "(not defrosted)" from matching — so a bare
    /// "1 (14.5 oz) of stock" still falls through to [`parse_parenthesized_amounts`].
    /// For the paren-less form the size must be a *hyphenated* adjective
    /// ("10-ounce"), so a plain "2 cups flour" isn't mistaken for this shape.
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

        // Leading count, e.g. "1" or "One".
        let (rest, value) = self.parse_value(input).map_err(|_| reject())?;
        let (rest, _) = space0::<_, VerboseError<&str>>(rest).map_err(|_| reject())?;

        // Extract the size string and the remainder after it, for either the
        // parenthesized form "(…)" or the bare hyphenated adjective "10-ounce".
        let (inner, after) = if rest.starts_with('(') {
            // Find the matching close paren (handles nesting).
            let close = find_matching_paren(rest).ok_or_else(reject)?;
            (&rest[1..close], rest[close + 1..].trim_start())
        } else {
            // Bare hyphenated size adjective: "10-ounce", "1½-inch" (or "-inch"
            // when the count already consumed the fraction as its value). Such a
            // token always begins with a digit, a vulgar fraction, or the hyphen
            // itself, so gate on the first char in O(1) BEFORE scanning for the
            // token boundary. Without this gate, input with no whitespace — e.g.
            // each element of a long slash-separated amount list "1/1/1/…" — makes
            // `find(char::is_whitespace)` walk to the end on every element,
            // turning the whole list parse quadratic. The gate rejects those
            // (they can't be a numeric size adjective) at the first character.
            let starts_size_adjective = rest
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_digit() || c == '-' || crate::fraction::is_vulgar(c));
            if !starts_size_adjective {
                return Err(reject());
            }
            // The first whitespace-delimited token; require it to contain a hyphen
            // so a normal "2 cups flour" doesn't match here.
            let tok_end = rest.find(char::is_whitespace).unwrap_or(rest.len());
            let size = &rest[..tok_end];
            if !size.contains('-') {
                return Err(reject());
            }
            (size, rest[tok_end..].trim_start())
        };

        // A container noun must follow the size. Usually it comes immediately
        // after ("piece ginger"), but it can also trail the name
        // ("halibut fillets" = 4 fillets of halibut), so fall back to the last
        // word when the first isn't a container. `after_rest` stays a slice of
        // the input so the parser can return the unconsumed remainder.
        let first_end = after.find(char::is_whitespace).unwrap_or(after.len());
        let first_word = after[..first_end].to_lowercase();
        let (container, after_rest): (String, &str) =
            if CONTAINER_NOUNS.contains(&first_word.as_str()) {
                // "piece ginger" → container "piece", remainder "ginger" (drop a
                // connecting " of ", mirroring how units consume a trailing "of").
                let r = after[first_end..].trim_start();
                let remainder = r.strip_prefix("of ").unwrap_or(r);
                (first_word, remainder)
            } else {
                // "halibut fillets" → container = trailing "fillets", name "halibut".
                let last_word = after.rsplit(char::is_whitespace).next().unwrap_or("");
                let last_lower = last_word.to_lowercase();
                if CONTAINER_NOUNS.contains(&last_lower.as_str()) {
                    let name = after[..after.len() - last_word.len()].trim_end();
                    (last_lower, name)
                } else {
                    // No container noun: the count is of whole items that each
                    // carry the parenthetical size, e.g. "1 (3½ to 4 pound)
                    // chicken" → [1 whole, 3.5–4 lb] / "chicken" and "2 (8-ounce)
                    // swordfish steaks, …" → [2 whole, 8 oz] / "swordfish steaks".
                    (DEFAULT_UNIT.to_string(), after)
                }
            };

        // The size must fully parse as a measurement (hyphen → space).
        let inner_norm = inner.replace('-', " ");
        let inner_measures = match self.parse_measurement_list(inner_norm.as_str()) {
            Ok((r, m)) if r.trim().is_empty() && !m.is_empty() => m,
            _ => return Err(reject()),
        };

        let mut measures = Vec::with_capacity(1 + inner_measures.len());
        measures.push(Measure::from_parts(container.as_str(), value.0, value.1));
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

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::super::test_support::units;
    use super::super::{MeasurementMode, MeasurementParser};
    use rstest::{fixture, rstest};
    use std::collections::HashSet;

    #[fixture]
    fn units_fx() -> HashSet<String> {
        units()
    }

    #[rstest]
    #[case::single("(2 cups)", 1)]
    #[case::multiple("(1 cup / 240 ml)", 2)]
    // A trailing descriptor inside the parens is dropped, not fatal.
    #[case::by_weight("(8 ounces by weight)", 1)]
    fn test_parenthesized_amounts(
        units_fx: HashSet<String>,
        #[case] input: &str,
        #[case] expected_count: usize,
    ) {
        let parser = MeasurementParser::new(&units_fx, MeasurementMode::IngredientList);
        let result = parser.parse_parenthesized_amounts(input);
        assert!(result.is_ok());
        let (_, measures) = result.unwrap();
        assert_eq!(measures.len(), expected_count);
    }

    #[rstest]
    // Compatible kinds (volume + volume) are summed into a single measure.
    #[case::word("1 cup plus 2 tbsp", 1)]
    #[case::symbol("½ cup + 2 tbsp", 1)]
    // Incompatible kinds (volume + weight) keep both rather than dropping one.
    #[case::incompatible("1 cup plus 100 grams", 2)]
    fn test_plus_expression(
        units_fx: HashSet<String>,
        #[case] input: &str,
        #[case] expected_len: usize,
    ) {
        let parser = MeasurementParser::new(&units_fx, MeasurementMode::IngredientList);
        let (_, measures) = parser.parse_plus_expression(input).unwrap();
        assert_eq!(measures.len(), expected_len, "input: {input}");
    }

    /// A parenthetical size — hyphenated ("1-ounce") or space form ("14.5 oz") —
    /// is hoisted into a second measure while the count keeps the container unit:
    /// "1 (1-ounce) piece" -> [1 piece, 1 oz]; "2 (14.5 oz) cans" -> [2 can, 14.5 oz].
    #[rstest]
    #[case::piece("1 (1-ounce) piece ginger", 2, "piece")]
    #[case::can("1 (28-ounce) can tomatoes", 2, "can")]
    #[case::space_form("2 (14.5 oz) cans tomatoes", 2, "can")]
    fn test_count_with_parenthetical_size(
        units_fx: HashSet<String>,
        #[case] input: &str,
        #[case] expected_len: usize,
        #[case] first_unit: &str,
    ) {
        let parser = MeasurementParser::new(&units_fx, MeasurementMode::IngredientList);
        let (_, measures) = parser.parse_count_with_parenthetical_size(input).unwrap();
        assert_eq!(measures.len(), expected_len, "input: {input}");
        assert_eq!(measures[0].unit_as_string(), first_unit);
        assert_eq!(measures[1].unit_as_string(), "oz");
    }

    /// Count + parenthetical/hyphenated size with NO container noun: the count
    /// becomes a "whole" amount and the size a second amount, e.g.
    /// "1 (3 ounce) chicken" -> [1 whole, 3 oz] and "One 6-ounce carrot" ->
    /// [1 whole, 6 oz]. (With a container the first unit is the container.)
    #[rstest]
    #[case::paren("1 (3 ounce) chicken")]
    #[case::hyphen("One 6-ounce carrot")]
    fn test_count_with_size_no_container(units_fx: HashSet<String>, #[case] input: &str) {
        let parser = MeasurementParser::new(&units_fx, MeasurementMode::IngredientList);
        let (_, measures) = parser.parse_count_with_parenthetical_size(input).unwrap();
        assert_eq!(measures.len(), 2, "input: {input}");
        assert_eq!(measures[0].unit_as_string(), "whole");
        assert_eq!(measures[1].unit_as_string(), "oz");
    }

    /// A parenthetical that is NOT a size (no parseable measurement inside) is
    /// rejected even when a container noun follows.
    #[rstest]
    fn test_parenthetical_size_rejects_non_size(units_fx: HashSet<String>) {
        let parser = MeasurementParser::new(&units_fx, MeasurementMode::IngredientList);
        assert!(
            parser
                .parse_count_with_parenthetical_size("1 (not defrosted) can tomatoes")
                .is_err()
        );
    }
}
