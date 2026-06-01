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
            (&rest[1..close], rest[close + 1..].trim_start())
        } else {
            // Bare hyphenated size adjective: "10-ounce", "1½-inch". Take the
            // first whitespace-delimited token; require it to contain a hyphen so
            // a normal "2 cups flour" doesn't match here.
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
                if !CONTAINER_NOUNS.contains(&last_lower.as_str()) {
                    return Err(reject());
                }
                let name = after[..after.len() - last_word.len()].trim_end();
                (last_lower, name)
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
