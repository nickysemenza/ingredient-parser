use std::cmp::Reverse;

#[allow(deprecated)]
use nom::{
    bytes::complete::tag,
    character::complete::{not_line_ending, space0, space1},
    combinator::{opt, verify},
    error::context,
    multi::many1,
    Parser,
};

use super::normalize::{collapse_whitespace, normalize_input, strip_optional_note};
use super::recognize::lift_inline_descriptive_paren;
use crate::parser::{parse_ingredient_text, parse_unit_text, MeasurementParser, Res};
use crate::trace;
use crate::traced_parser;
use crate::unit::{self, Measure};
use crate::{Ingredient, IngredientParser};

impl IngredientParser {
    pub(crate) fn parse_ingredient_line(&self, input: &str) -> Ingredient {
        let normalized = normalize_input(input);
        self.parse_normalized_ingredient(normalized.as_ref())
    }

    pub(crate) fn parse_ingredient_line_with_trace(
        &self,
        input: &str,
    ) -> trace::ParseWithTrace<Ingredient> {
        let normalized = normalize_input(input);
        let input = normalized.as_ref();

        trace::enable_tracing();
        let result = self.parse_normalized_ingredient(input);
        let trace = trace::disable_tracing(input);

        trace::ParseWithTrace {
            result: Ok(result),
            trace,
        }
    }

    fn parse_normalized_ingredient(&self, input: &str) -> Ingredient {
        // An "(optional)" note marks the whole ingredient optional, e.g.
        // "Grated zest of 1 lemon (optional)" or, mid-line, "almonds (optional),
        // coarsely chopped". Strip it before parsing and set the flag, so it
        // neither pollutes the name/modifier nor blocks a trailing weight from
        // being hoisted. (A *whole-line* parenthesized ingredient is handled
        // separately below.)
        let (cleaned, is_optional) = strip_optional_note(input);
        let mut ingredient = self.parse_normalized_ingredient_inner(&cleaned);
        if is_optional {
            ingredient.optional = true;
        }
        ingredient
    }

    fn parse_normalized_ingredient_inner(&self, input: &str) -> Ingredient {
        if let Some(ingredient) = self.try_parse_optional_ingredient(input) {
            return ingredient;
        }

        if let Some(ingredient) = self.try_parse_trailing_amount_format(input) {
            return ingredient;
        }

        if let Some(ingredient) = self.try_parse_x_of_construction(input) {
            return ingredient;
        }

        self.parse_core_ingredient(input)
            // Reject a "successful" parse that lost the ingredient name into the
            // modifier (seen on real recipes: a decimal comma in "1,000 grams
            // ... nectarines", a leading prep word, etc.) — the graceful
            // fallback is better than a name-less ingredient with garbled text.
            // A bare quantity like "1/2-1 cup" legitimately has no name, so only
            // fall back when the empty name coincides with leftover modifier text.
            .filter(|ingredient| {
                let name_empty = ingredient.name.trim().is_empty();
                let has_modifier = ingredient
                    .modifier
                    .as_deref()
                    .is_some_and(|m| !m.trim().is_empty());
                !(name_empty && has_modifier)
            })
            .unwrap_or_else(|| fallback_ingredient(input))
    }

    pub(super) fn parse_core_ingredient(&self, input: &str) -> Option<Ingredient> {
        // A descriptive parenthetical sitting *between* name words — e.g. the
        // "(70° to 80°F)" in "room-temperature (70° to 80°F) water" or the
        // "(¼ inch / 6 mm)" in "sliced (¼ inch / 6 mm) green onions" — breaks the
        // name grammar. Lift it out to the modifier and parse the cleaned line,
        // so the real name and amounts survive. Scoped to temperature/distance
        // asides flanked by name text, so mass/volume parentheticals like
        // "(190 grams)" stay hoisted as amounts and "4 (½-inch) slices" (count +
        // size) is untouched.
        if let Some((cleaned, aside)) = lift_inline_descriptive_paren(input) {
            let mut ingredient = self
                .parse_ingredient(&cleaned)
                .ok()
                .map(|(_, ingredient)| self.postprocess_ingredient(ingredient))?;
            append_modifier(&mut ingredient.modifier, &aside);
            ingredient.modifier = clean_modifier(ingredient.modifier);
            return Some(ingredient);
        }

        self.parse_ingredient(input)
            .ok()
            .map(|(_, ingredient)| self.postprocess_ingredient(ingredient))
    }

    fn postprocess_ingredient(&self, mut ingredient: Ingredient) -> Ingredient {
        self.fix_leading_prep_phrase(&mut ingredient);
        self.fix_leading_minus_clause(&mut ingredient);
        self.extract_leading_prep_alternative(&mut ingredient);
        self.extract_adjectives_from_name(&mut ingredient);
        ingredient.name = collapse_whitespace(&ingredient.name);
        self.extract_alternative_from_name(&mut ingredient);
        self.extract_secondary_amounts_from_modifier(&mut ingredient);
        ingredient.modifier = strip_wrapping_parens(clean_modifier(ingredient.modifier));
        ingredient
    }

    /// Recover from a leading prep phrase that displaced the ingredient name.
    ///
    /// A line like "2/3 cup finely chopped, raw pistachios" parses with the
    /// text *before* the comma as the name and the text *after* as the modifier,
    /// yielding name="finely chopped" / modifier="raw pistachios" — backwards.
    /// When the whole name is a single known prep phrase and a modifier is
    /// present, swap them so the prep phrase becomes the modifier and the real
    /// name is restored. The exact-match guard keeps descriptive names (e.g.
    /// "raw pistachios, finely chopped", where the name isn't a prep phrase) from
    /// ever being touched.
    fn fix_leading_prep_phrase(&self, ingredient: &mut Ingredient) {
        let name = ingredient.name.trim();
        if name.is_empty() || !self.adjectives.contains(&name.to_lowercase()) {
            return;
        }
        let Some(modifier) = ingredient
            .modifier
            .as_deref()
            .map(str::trim)
            .filter(|m| !m.is_empty())
        else {
            return;
        };
        let prep = name.to_string();
        ingredient.name = modifier.to_string();
        ingredient.modifier = Some(prep);
    }

    /// Recover from a leading subtractive clause that displaced the name, e.g.
    /// "½ cup minus 1 tablespoon flour" parses with "½ cup" as the amount and
    /// "minus 1 tablespoon flour" as the name. When the name begins with "minus"
    /// followed by a parseable measurement, move "minus <measure>" into the
    /// modifier and restore the real name ("flour"). The primary amount is left
    /// as stated (the subtraction isn't applied numerically).
    fn fix_leading_minus_clause(&self, ingredient: &mut Ingredient) {
        let name = ingredient.name.clone();
        let Some(rest) = name
            .strip_prefix("minus ")
            .or_else(|| name.strip_prefix("Minus "))
        else {
            return;
        };
        let mp = MeasurementParser::new(&self.units, self.is_rich_text);
        let Ok((remaining, measures)) = mp.parse_measurement_list(rest) else {
            return;
        };
        if measures.is_empty() || remaining.trim().is_empty() {
            return;
        }
        let consumed = rest[..rest.len() - remaining.len()].trim();
        let clause = format!("minus {consumed}");
        ingredient.name = remaining.trim().to_string();
        match ingredient.modifier.take() {
            Some(m) if !m.trim().is_empty() => {
                ingredient.modifier = Some(format!("{clause}, {m}"));
            }
            _ => ingredient.modifier = Some(clause),
        }
    }

    /// Parse a complete ingredient line including amounts, name, and modifiers.
    ///
    /// This method only captures the raw grammar shape. Cleanup such as adjective
    /// extraction, alternative extraction, and secondary amount extraction happens
    /// in the higher-level ingredient pipeline.
    #[tracing::instrument(name = "parse_ingredient")]
    pub(crate) fn parse_ingredient<'a>(&self, input: &'a str) -> Res<&'a str, Ingredient> {
        let mp = MeasurementParser::new(&self.units, self.is_rich_text);

        let ingredient_format = (
            opt(|a| mp.parse_measurement_list(a)),
            space0,
            opt(|a| mp.parse_bracketed_amounts(a)),
            space0,
            opt((|a| self.adjective(a), space1)),
            opt(many1(parse_ingredient_text)),
            opt(|a| mp.parse_parenthesized_amounts(a)),
            opt(tag(", ")),
            not_line_ending,
        );

        traced_parser!(
            "parse_ingredient",
            input,
            context("ingredient", ingredient_format).parse(input).map(
                |(
                    next_input,
                    (
                        primary_amounts,
                        _,
                        bracketed_amounts,
                        _,
                        adjective,
                        name_chunks,
                        paren_amounts,
                        _,
                        modifier_text,
                    ),
                )| {
                    (
                        next_input,
                        Ingredient {
                            name: raw_name(name_chunks),
                            amounts: merge_amounts(
                                primary_amounts,
                                bracketed_amounts,
                                paren_amounts,
                            ),
                            modifier: raw_modifier(adjective, modifier_text),
                            optional: false,
                        },
                    )
                },
            ),
            |i: &Ingredient| i.name.clone(),
            "parse failed"
        )
    }

    /// Parse and validate an adjective string.
    fn adjective<'a>(&self, input: &'a str) -> Res<&'a str, String> {
        traced_parser!(
            "adjective",
            input,
            context(
                "adjective",
                verify(parse_unit_text, |s: &str| {
                    self.adjectives.contains(&s.to_lowercase())
                }),
            )
            .parse(input)
            .map(|(rest, s)| (rest, s.to_string())),
            |s: &String| s.clone(),
            "not an adjective"
        )
    }

    fn extract_adjectives_from_name(&self, ingredient: &mut Ingredient) {
        let mut name = ingredient.name.clone();
        let mut name_lower = name.to_lowercase();
        let mut found_adjectives: Vec<&String> = self
            .adjectives
            .iter()
            .filter(|adj| name_lower.contains(adj.as_str()))
            .collect();
        found_adjectives.sort_by_key(|adj| Reverse(adj.len()));

        for adjective in found_adjectives {
            let Some(pos) = name_lower.find(adjective.as_str()) else {
                continue;
            };

            let end = pos + adjective.len();
            // `pos`/`end` are byte offsets into the lowercased name. Lowercasing
            // can change byte lengths for some Unicode (e.g. 'İ' -> "i̇"), so these
            // offsets may not fall on char boundaries in the original `name`.
            // Skip rather than panic when slicing `name` would split a char.
            if !name.is_char_boundary(pos) || !name.is_char_boundary(end) {
                continue;
            }

            // Require a whitespace/string-edge boundary on both sides, so an
            // adjective embedded in a larger token is left alone (e.g. "chopped"
            // inside "well-chopped" must not corrupt the name into "well-").
            let before_boundary = name[..pos]
                .chars()
                .next_back()
                .is_none_or(char::is_whitespace);
            let after_boundary = name[end..].chars().next().is_none_or(char::is_whitespace);
            if !before_boundary || !after_boundary {
                continue;
            }

            append_modifier(&mut ingredient.modifier, adjective);

            let before = name[..pos].trim();
            let after = name[end..].trim();
            let mut new_name = String::with_capacity(name.len());
            if !before.is_empty() {
                new_name.push_str(before);
                if !after.is_empty() {
                    new_name.push(' ');
                }
            }
            if !after.is_empty() {
                new_name.push_str(after);
            }

            name = new_name.trim().to_string();
            name_lower = name.to_lowercase();
        }

        ingredient.name = name;
    }

    /// Recover a leading preparation *alternative* that displaced the name, e.g.
    /// "grated or finely chopped lemon zest" parses with "grated or finely
    /// chopped lemon zest" as the name. When the name begins with
    /// "`<participle> or <known-adjective>`" — a prep word (typically `-ed`),
    /// "or", then a recognized adjective phrase — that whole prefix is a
    /// preparation note. Move it to the modifier and keep the trailing head noun
    /// as the name ("lemon zest", modifier "grated or finely chopped").
    ///
    /// Guarded tightly so genuine two-ingredient alternatives ("basil or chopped
    /// parsley") are left alone: the first word must look like a participle
    /// (`-ed`) or be a known adjective, the word after "or" must be a known
    /// adjective phrase, and a head noun must remain.
    fn extract_leading_prep_alternative(&self, ingredient: &mut Ingredient) {
        let name = ingredient.name.trim().to_string();
        let words: Vec<&str> = name.split_whitespace().collect();
        if words.len() < 4 || words[1].to_lowercase() != "or" {
            return;
        }
        let first = words[0].to_lowercase();
        let first_is_prep = first.ends_with("ed") || self.adjectives.contains(&first);
        if !first.chars().all(char::is_alphabetic) || !first_is_prep {
            return;
        }
        // A known adjective phrase (two words then one) immediately after "or".
        let two = format!(
            "{} {}",
            words[2].to_lowercase(),
            words.get(3).map(|w| w.to_lowercase()).unwrap_or_default()
        );
        let adj_len = if words.len() >= 5 && self.adjectives.contains(&two) {
            2
        } else if self.adjectives.contains(&words[2].to_lowercase()) {
            1
        } else {
            return;
        };
        let name_start = 2 + adj_len;
        if name_start >= words.len() {
            return;
        }
        let prefix = words[..name_start].join(" ");
        ingredient.name = words[name_start..].join(" ");
        append_modifier(&mut ingredient.modifier, &prefix);
    }

    fn extract_alternative_from_name(&self, ingredient: &mut Ingredient) {
        let (name, alternative) = extract_alternative(&ingredient.name);
        ingredient.name = name;
        if let Some(alternative) = alternative {
            append_modifier(&mut ingredient.modifier, &alternative);
        }
    }

    fn extract_secondary_amounts_from_modifier(&self, ingredient: &mut Ingredient) {
        let Some(modifier) = ingredient.modifier.as_ref() else {
            return;
        };

        let (secondary_amounts, cleaned_modifier) =
            extract_secondary_amounts(modifier, &self.units);
        ingredient.amounts.extend(secondary_amounts);
        ingredient.modifier = clean_modifier(Some(cleaned_modifier));
    }
}

fn fallback_ingredient(input: &str) -> Ingredient {
    Ingredient {
        name: input.trim().to_string(),
        amounts: vec![],
        modifier: None,
        optional: false,
    }
}

fn raw_name(name_chunks: Option<Vec<&str>>) -> String {
    name_chunks.unwrap_or_default().join("").trim().to_string()
}

fn raw_modifier(adjective: Option<(String, &str)>, modifier_text: &str) -> Option<String> {
    let mut modifier = modifier_text.to_owned();
    if let Some((adjective, _)) = adjective {
        modifier.push_str(&adjective);
    }
    clean_modifier(Some(modifier))
}

fn merge_amounts(
    primary_amounts: Option<Vec<Measure>>,
    bracketed_amounts: Option<Vec<Measure>>,
    paren_amounts: Option<Vec<Measure>>,
) -> Vec<Measure> {
    let mut amounts = Vec::new();
    if let Some(primary_amounts) = primary_amounts {
        amounts.extend(primary_amounts);
    }
    if let Some(bracketed_amounts) = bracketed_amounts {
        amounts.extend(bracketed_amounts);
    }
    if let Some(paren_amounts) = paren_amounts {
        amounts.extend(paren_amounts);
    }
    amounts
}

fn append_modifier(modifier: &mut Option<String>, addition: &str) {
    if addition.is_empty() {
        return;
    }

    match modifier {
        Some(modifier) if !modifier.is_empty() => {
            modifier.push_str(", ");
            modifier.push_str(addition);
        }
        Some(modifier) => modifier.push_str(addition),
        None => *modifier = Some(addition.to_string()),
    }
}

/// Strip a single pair of parentheses that wraps the *entire* modifier, e.g.
/// "(softened)" -> "softened". Modifiers with internal parentheses or only
/// partial wrapping are left untouched.
fn strip_wrapping_parens(modifier: Option<String>) -> Option<String> {
    let modifier = modifier?;
    let trimmed = modifier.trim();
    if let Some(inner) = trimmed.strip_prefix('(').and_then(|s| s.strip_suffix(')')) {
        if !inner.contains('(') && !inner.contains(')') {
            let inner = inner.trim();
            return (!inner.is_empty()).then(|| inner.to_string());
        }
    }
    Some(modifier)
}

fn clean_modifier(modifier: Option<String>) -> Option<String> {
    modifier.and_then(|modifier| {
        let trimmed = modifier.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

/// Extract alternative ingredients from the name (e.g., "garlic or 1 teaspoon garlic powder")
///
/// Returns `(cleaned_name, optional_alternative)` where:
/// - `cleaned_name`: The ingredient name with alternative removed
/// - `optional_alternative`: The alternative portion to be added to modifier
fn extract_alternative(name: &str) -> (String, Option<String>) {
    use regex::Regex;
    use std::sync::LazyLock;

    static ALTERNATIVE_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
        #[allow(clippy::expect_used)]
        Regex::new(r"(?i)\s+or\s+(\d+|[½¼¾⅓⅔⅕⅖⅗⅘⅙⅚⅛⅜⅝⅞]|a\s+|an\s+)")
            .expect("invalid alternative pattern regex")
    });

    let Some(matched) = ALTERNATIVE_PATTERN.find(name) else {
        return (name.to_string(), None);
    };

    let (ingredient_part, alternative_part) = name.split_at(matched.start());
    let alternative = alternative_part.trim();
    if alternative.is_empty() {
        return (name.to_string(), None);
    }

    (
        ingredient_part.trim().to_string(),
        Some(alternative.to_string()),
    )
}

/// Extract secondary amounts from modifier patterns like "(from about 15 sprigs)".
///
/// Returns `(extracted_amounts, cleaned_modifier)` where:
/// - `extracted_amounts`: `Vec<Measure>` parsed from the pattern
/// - `cleaned_modifier`: The modifier with the pattern removed
fn extract_secondary_amounts(
    modifier: &str,
    units: &std::collections::HashSet<String>,
) -> (Vec<Measure>, String) {
    use regex::Regex;
    use std::sync::LazyLock;

    static SECONDARY_AMOUNT_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
        #[allow(clippy::expect_used)]
        Regex::new(r"\((?:from\s+)?(?:about|approximately|roughly|around)\s+([^)]+)\)")
            .expect("invalid secondary amount regex")
    });

    let Some(caps) = SECONDARY_AMOUNT_PATTERN.captures(modifier) else {
        return (vec![], modifier.to_string());
    };

    let Some(full_match) = caps.get(0) else {
        return (vec![], modifier.to_string());
    };
    let Some(amount_match) = caps.get(1) else {
        return (vec![], modifier.to_string());
    };
    let amount_text = amount_match.as_str().trim();

    let mp = MeasurementParser::new(units, false);
    let Ok((remaining, measures)) = mp.parse_measurement_list(amount_text) else {
        return (vec![], modifier.to_string());
    };

    // A *dimension* aside like "(about 3-inch)" inside a prep phrase ("cut into
    // long (about 3-inch) strips") describes shape, not a secondary quantity.
    // Leave it in the modifier rather than hoisting a spurious inch amount.
    let is_distance = |m: &Measure| match m.unit() {
        unit::Unit::Inch => true,
        unit::Unit::Other(s) => super::measurement::guards::is_distance_unit(s),
        _ => false,
    };
    if measures.iter().any(is_distance) {
        return (vec![], modifier.to_string());
    }

    let remaining_trimmed = remaining.trim();
    let is_simple_remaining = remaining_trimmed.is_empty()
        || (remaining_trimmed.split_whitespace().count() == 1
            && remaining_trimmed.chars().all(char::is_alphabetic));

    if !is_simple_remaining || measures.is_empty() {
        return (vec![], modifier.to_string());
    }

    let cleaned = format!(
        "{}{}",
        &modifier[..full_match.start()],
        &modifier[full_match.end()..]
    )
    .trim()
    .to_string();

    (measures, cleaned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[rstest]
    // Fully wrapped: outer parens are stripped.
    #[case::simple("(sifted)", Some("sifted"))]
    #[case::with_percent("(70% cacao)", Some("70% cacao"))]
    #[case::inner_trimmed("(  softened  )", Some("softened"))]
    // Not wrapped, or only partially: left untouched.
    #[case::plain("softened", Some("softened"))]
    #[case::open_only("(partial", Some("(partial"))]
    #[case::close_only("partial)", Some("partial)"))]
    // Internal parens must NOT be collapsed (would merge distinct clauses).
    #[case::two_groups("(a) and (b)", Some("(a) and (b)"))]
    #[case::nested("(note (nested))", Some("(note (nested))"))]
    // An empty group collapses away entirely.
    #[case::empty("()", None)]
    fn test_strip_wrapping_parens(#[case] input: &str, #[case] expected: Option<&str>) {
        assert_eq!(
            strip_wrapping_parens(Some(input.to_string())),
            expected.map(str::to_string)
        );
    }

    #[test]
    fn test_strip_wrapping_parens_none() {
        assert_eq!(strip_wrapping_parens(None), None);
    }
}
