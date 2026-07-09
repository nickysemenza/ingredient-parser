use std::collections::HashSet;

use crate::{
    IngredientParser, Res,
    parser::measurement::single::leading_qualifier,
    parser::{MeasurementMode, MeasurementParser},
    unit::Measure,
};
use nom::{Parser, branch::alt, character::complete::satisfy, error::context, multi::many0};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value")]
pub enum Chunk {
    Measure(Vec<Measure>),
    Text(String),
    Ing(String),
}
pub type Rich = Vec<Chunk>;

/// Error returned by [`RichParser::parse`] when the underlying nom grammar fails
/// to consume the input.
///
/// In practice `parse` is effectively infallible on real prose — the grammar's
/// `text_chunk` fallback accepts any character — but the nom combinator surface
/// is fallible, so the failure is surfaced as a typed error (matching the crate's
/// `thiserror`-based [`IngredientError`](crate::IngredientError)) rather than a
/// bare `String`.
#[derive(thiserror::Error, Debug, Clone, PartialEq)]
pub enum RichParseError {
    /// The rich-text grammar failed on `input`; `reason` carries the nom error.
    #[error("unable to parse '{input}': {reason}")]
    Parse { input: String, reason: String },
}
fn condense_text(r: Rich) -> Rich {
    let mut result = Vec::with_capacity(r.len());
    for chunk in r {
        match (&mut result.last_mut(), &chunk) {
            (Some(Chunk::Text(prev)), Chunk::Text(new)) => {
                prev.push_str(new);
            }
            _ => result.push(chunk),
        }
    }
    result
}
/// Full name first (longest), then drop one leading word at a time, so the head
/// noun is always the shortest variant. Generic — no vocab, no word lists.
/// "extra virgin olive oil" -> ["extra virgin olive oil", "virgin olive oil",
/// "olive oil", "oil"]; "red onion" -> ["red onion", "onion"].
fn name_variants(name: &str) -> Vec<String> {
    let toks: Vec<&str> = name.split_whitespace().collect();
    (0..toks.len()).map(|i| toks[i..].join(" ")).collect()
}

/// Is `c` an ASCII alphanumeric character? Word boundaries are defined by
/// `char::is_alphanumeric` elsewhere; suffix/prefix folding only cares about the
/// ASCII letter run, which is what cooking-prose plurals and casing live in.
fn is_word_char(c: char) -> bool {
    c.is_alphanumeric()
}

/// Whether the byte at `idx` (or the string edge) is a word boundary in `s`.
fn boundary_at(s: &str, idx: usize) -> bool {
    s[idx..].chars().next().is_none_or(|c| !is_word_char(c))
}

/// Whether the char *before* `idx` (or string start) is a word boundary.
fn boundary_before(s: &str, idx: usize) -> bool {
    s[..idx]
        .chars()
        .next_back()
        .is_none_or(|c| !is_word_char(c))
}

/// Recipe steps often open with an imperative verb spelled like an ingredient
/// ("Oil the pan", "Salt to taste", "Cream together"). Case-insensitive matching
/// at sentence start would false-positive on those; block when the tail matches.
fn is_imperative_continuation(tail: &str) -> bool {
    tail.starts_with(" the ")
        || tail.starts_with(" to ")
        || tail.starts_with(" together")
        || tail.starts_with(" well")
        || tail.starts_with(" until ")
        || tail.starts_with(" for ")
}

/// Case-insensitive match at the start of a text chunk only — catches
/// sentence-initial capitalized mentions ("Parsley makes…") without folding case
/// mid-sentence ("the best Parsley around") or on imperative openers ("Oil the
/// pan"). Uses char-wise comparison so multibyte haystacks never slice mid-char.
fn find_candidate_case_insensitive_at_start(
    haystack: &str,
    candidate: &str,
) -> Option<(usize, usize)> {
    if candidate.is_empty() {
        return None;
    }
    if !boundary_before(haystack, 0) {
        return None;
    }
    let mut h_iter = haystack.char_indices().peekable();
    for c in candidate.chars() {
        let (_, h) = h_iter.next()?;
        if !h.eq_ignore_ascii_case(&c) {
            return None;
        }
    }
    let base_end = h_iter.peek().map(|(i, _)| *i).unwrap_or(haystack.len());
    let tail = &haystack[base_end..];
    if is_imperative_continuation(tail) {
        return None;
    }
    if tail.starts_with("es") && boundary_at(haystack, base_end + 2) {
        return Some((0, base_end + 2));
    }
    if tail.starts_with('s') && boundary_at(haystack, base_end + 1) {
        return Some((0, base_end + 1));
    }
    if boundary_at(haystack, base_end) {
        return Some((0, base_end));
    }
    None
}

/// Locate `candidate` in `haystack`, returning the matched **surface span**
/// `(pos, end)` in `haystack` byte offsets, or `None`.
///
/// Generic — no vocab, no word lists. One extension over plain exact match,
/// preserving the word-boundary guard (so "oil" never matches inside "broil"):
///
/// **Optional trailing plural suffix.** After the base candidate, an optional
/// `"es"` (preferred) or `"s"` is consumed *iff* a word boundary holds after it
/// — so name "egg" matches prose "eggs" but not "eggshell", and "lime" does not
/// punch through "limestone". The returned span is sliced out of the original
/// `haystack`, so the caller emits the prose's own suffix ("eggs", "tomatoes").
///
/// Matching is **case-sensitive** except at chunk start, where a guarded
/// case-insensitive pass catches sentence-initial capitalized mentions without
/// mis-highlighting imperative step openers ("Oil the pan") or mid-sentence
/// capitals ("the best Parsley around").
fn find_candidate(haystack: &str, candidate: &str) -> Option<(usize, usize)> {
    let clen = candidate.len();
    for (pos, m) in haystack.match_indices(candidate) {
        let base_end = pos + m.len();
        debug_assert_eq!(m.len(), clen);

        // Leading boundary: char before `pos` must be a non-word char / edge.
        if !boundary_before(haystack, pos) {
            continue;
        }

        // Trailing boundary, with optional plural suffix. Check "es" before "s"
        // so "tomato"->"tomatoes" consumes the pair as a unit.
        let tail = &haystack[base_end..];
        if tail.starts_with("es") && boundary_at(haystack, base_end + 2) {
            return Some((pos, base_end + 2));
        }
        if tail.starts_with('s') && boundary_at(haystack, base_end + 1) {
            return Some((pos, base_end + 1));
        }
        if boundary_at(haystack, base_end) {
            return Some((pos, base_end));
        }
        // Otherwise this occurrence fails the trailing guard; `match_indices`
        // keeps scanning for a later, boundary-valid one.
    }
    find_candidate_case_insensitive_at_start(haystack, candidate)
}

// find any text chunks which have an ingredient name as a substring in them.
// if so, split on the ingredient name, giving it it's own `Chunk::Ing`.
//
// Repeatedly splits at the EARLIEST match across all names, so extraction is
// independent of the order names are listed in and repeated names are all
// found ("Add sugar and flour" highlights both for names ["flour", "sugar"]).
fn extract_ingredients(r: Rich, ingredient_names: &[String]) -> Rich {
    // Match the full name *and* every trailing sub-phrase, so prose that refers
    // to an ingredient by its head noun ("red onion" -> "onion") still
    // highlights. Built once; the longest variant still wins via the tie-break
    // below, so the full phrase fires when prose spells it out.
    let candidates: Vec<String> = ingredient_names
        .iter()
        .flat_map(|n| name_variants(n))
        .collect();

    r.into_iter()
        .flat_map(|s| match s {
            Chunk::Text(text) => {
                let mut text_or_ing_res = vec![];
                let mut rest = text.as_str();

                // Earliest word-boundary match across all candidates; ties go to
                // the longer candidate so "sea salt" wins over "salt" and the
                // full name wins over its head-noun variant. `find_candidate`
                // returns the matched *surface span* `(pos, end)` in `haystack`
                // (NOT the candidate string), so the emitted Ing carries the
                // prose's own plural suffix ("eggs", "tomatoes"). The boundary
                // guard (non-alphanumeric or string edge on both sides) stops a
                // short candidate like "oil" matching inside "broil"; for
                // plurals it is re-checked *past* the suffix.
                let earliest = |haystack: &str| {
                    candidates
                        .iter()
                        .filter(|c| !c.is_empty())
                        .filter_map(|c| {
                            find_candidate(haystack, c).map(|(pos, end)| (pos, end, c.len()))
                        })
                        // Tie-break on *candidate* length (not span length) so the
                        // longest name wins regardless of any plural suffix the
                        // prose happens to add.
                        .min_by_key(|(pos, _end, clen)| (*pos, std::cmp::Reverse(*clen)))
                };

                while let Some((pos, end, _clen)) = earliest(rest) {
                    if pos > 0 {
                        text_or_ing_res.push(Chunk::Text(rest[..pos].to_string()));
                    }
                    // Surface text actually matched in the prose, including any
                    // plural suffix.
                    text_or_ing_res.push(Chunk::Ing(rest[pos..end].to_string()));
                    rest = &rest[end..];
                }
                if !rest.is_empty() {
                    // ignore empty
                    text_or_ing_res.push(Chunk::Text(rest.to_string()));
                }

                text_or_ing_res
            }
            other => vec![other],
        })
        .collect()
}

fn amounts_chunk<'a>(units: &HashSet<String>, input: &'a str) -> Res<&'a str, Vec<Chunk>> {
    // Always use rich text mode (true) for instruction parsing
    let mp = MeasurementParser::new(units, MeasurementMode::RichText);
    let (next_input, measures) =
        context("amounts_chunk", |a| mp.parse_measurement_list(a)).parse(input)?;

    // The measurement parser swallows a leading approximation qualifier
    // ("about ", "roughly ", …) and a trailing sentence boundary (". " / "." /
    // " of") around the measure. Those are noise for ingredient amounts but
    // real prose in instructions, so re-emit them as Text instead of deleting
    // them — otherwise the measure glues onto the next sentence (e.g.
    // "...foamy, about 3 minutes. Continue" → "...foamy, 3 minutesContinue").
    let consumed = &input[..input.len() - next_input.len()];
    let leading_len = match leading_qualifier(input) {
        Ok((rest, ())) => input.len() - rest.len(),
        Err(_) => 0,
    };

    let mut chunks = Vec::with_capacity(3);
    if leading_len > 0 {
        chunks.push(Chunk::Text(input[..leading_len].to_string()));
    }
    chunks.push(Chunk::Measure(measures));
    let trailing = trailing_boundary(consumed);
    if !trailing.is_empty() {
        chunks.push(Chunk::Text(trailing.to_string()));
    } else {
        // A unitless bare count ("12 cookies") consumes the trailing space via
        // `space0` but never uses it; re-emit it so the next word keeps its
        // leading space (otherwise "12 cookies" → measure + "cookies").
        let stripped = consumed.trim_end_matches(char::is_whitespace);
        if stripped.len() < consumed.len() {
            chunks.push(Chunk::Text(consumed[stripped.len()..].to_string()));
        }
    }
    Ok((next_input, chunks))
}

/// The sentence boundary the measure parser's `optional_period_or_of` swallows
/// after a measure, so rich text can re-emit it as prose.
fn trailing_boundary(consumed: &str) -> &'static str {
    if consumed.ends_with(". ") {
        ". "
    } else if consumed.ends_with(" of") {
        " of"
    } else if consumed.ends_with('.') {
        "."
    } else {
        ""
    }
}

fn text_chunk(input: &str) -> Res<&str, Vec<Chunk>> {
    parse_rich_char(input).map(|(next_input, res)| (next_input, vec![Chunk::Text(res)]))
}
/// Parse a single text character for rich text (recipe instructions).
///
/// Allows: alphanumeric, whitespace, plus additional punctuation
/// (commas, parentheses, semicolons, colons, slashes, etc.)
///
/// Note: This is more permissive than `parser::helpers::parse_ingredient_text()` which is
/// designed for ingredient names only.
fn parse_rich_char(input: &str) -> Res<&str, String> {
    satisfy(|c| match c {
        '-' | '\u{2014}' | '\'' | '\u{2019}' | '.' | '\\' => true,
        ',' | '(' | ')' | ';' | '#' | '/' | ':' | '!' => true,
        c => c.is_alphanumeric() || c.is_whitespace(),
    })
    .parse(input)
    .map(|(next_input, res)| (next_input, res.to_string()))
}
/// Parse some rich text that has some parsable [Measure] scattered around in it. Useful for displaying text with fancy formatting.
/// returns [Rich]
/// ```
/// use ingredient::{unit::Measure, rich_text::{RichParser, Chunk}};
/// assert_eq!(
/// RichParser::new(Vec::<String>::new()).parse("hello 1 cups foo bar").unwrap(),
/// vec![
///     Chunk::Text("hello ".to_string()),
///     Chunk::Measure(vec![Measure::new("cups", 1.0)]),
///     Chunk::Text(" foo bar".to_string())
/// ]
/// );
/// ```
#[derive(Clone, PartialEq, Debug, Default)]
pub struct RichParser {
    ingredient_names: Vec<String>,
    ip: IngredientParser,
}
impl RichParser {
    /// Create a new RichParser for parsing recipe instructions
    ///
    /// # Arguments
    ///
    /// * `ingredient_names` - List of ingredient names to highlight in the text
    ///
    /// # Example
    /// ```
    /// use ingredient::rich_text::RichParser;
    ///
    /// // Accepts anything iterable of string-likes — no `.to_string()` needed:
    /// let parser = RichParser::new(["flour", "sugar"]);
    /// let chunks = parser.parse("Add 2 cups flour").unwrap();
    /// ```
    pub fn new<I, S>(ingredient_names: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            ingredient_names: ingredient_names.into_iter().map(Into::into).collect(),
            ip: IngredientParser::new(),
        }
    }

    #[tracing::instrument]
    pub fn parse(&self, input: &str) -> Result<Rich, RichParseError> {
        let units = self.ip.units();
        match context(
            "amts",
            many0(alt((|a| amounts_chunk(units, a), text_chunk))),
        )
        .parse(input)
        {
            Ok((_, res)) => {
                let flat: Rich = res.into_iter().flatten().collect();
                Ok(extract_ingredients(
                    condense_text(flat),
                    &self.ingredient_names,
                ))
            }
            Err(e) => Err(RichParseError::Parse {
                input: input.to_string(),
                reason: e.to_string(),
            }),
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use rstest::{fixture, rstest};

    #[fixture]
    fn parser() -> RichParser {
        RichParser::new(Vec::<String>::new())
    }

    #[fixture]
    fn parser_with_ingredients() -> RichParser {
        RichParser::new(["flour", "sugar"])
    }

    // ============================================================================
    // RichParser Basic Tests
    // ============================================================================

    #[rstest]
    fn test_rich_parse_error_display() {
        // The Display text is public API (callers were promised the old
        // Result<_, String> message verbatim) — pin it. The Err branch in
        // parse() itself is practically unreachable (many0 + a catch-all
        // text_chunk), matching the old String-error behavior.
        let err = RichParseError::Parse {
            input: "2 cups flour".into(),
            reason: "boom".into(),
        };
        assert_eq!(err.to_string(), "unable to parse '2 cups flour': boom");
    }

    #[rstest]
    fn test_rich_parser_basic(parser: RichParser) {
        let result = parser.parse("hello 1 cup foo bar").unwrap();
        assert_eq!(result.len(), 3);
        assert!(matches!(result[0], Chunk::Text(_)));
        assert!(matches!(result[1], Chunk::Measure(_)));
        assert!(matches!(result[2], Chunk::Text(_)));
    }

    #[rstest]
    fn test_rich_parser_empty_input(parser: RichParser) {
        let result = parser.parse("").unwrap();
        assert!(result.is_empty());
    }

    #[rstest]
    fn test_rich_parser_only_text(parser: RichParser) {
        let result = parser.parse("just some text").unwrap();
        assert_eq!(result.len(), 1);
        assert!(matches!(&result[0], Chunk::Text(s) if s == "just some text"));
    }

    #[rstest]
    fn test_rich_parser_bullet_proof_text(parser: RichParser) {
        // "bullet-proof" should NOT be parsed as containing a measurement
        let result = parser.parse("This bullet-proof recipe translates").unwrap();
        // Should be a single text chunk with no measurements extracted
        let has_measure = result.iter().any(|c| matches!(c, Chunk::Measure(_)));
        assert!(
            !has_measure,
            "bullet-proof should not extract measurements: {result:?}"
        );
    }

    #[rstest]
    #[case("Reinforce the edges of the tart")]
    #[case("infused with vanilla")]
    #[case("nantucket cranberries")]
    fn test_rich_parser_no_inf_nan_words(parser: RichParser, #[case] input: &str) {
        // nom's float parser accepts "inf"/"infinity"/"nan"; words containing
        // them must NOT extract a (non-finite) measurement.
        let result = parser.parse(input).unwrap();
        let has_measure = result.iter().any(|c| matches!(c, Chunk::Measure(_)));
        assert!(!has_measure, "should not extract a measurement: {result:?}");
    }

    #[rstest]
    fn test_rich_parser_multiple_measures(parser: RichParser) {
        let result = parser.parse("Mix 1 cup flour with 2 tbsp sugar").unwrap();
        let measures: Vec<_> = result
            .iter()
            .filter(|c| matches!(c, Chunk::Measure(_)))
            .collect();
        assert_eq!(measures.len(), 2);
    }

    /// Regression: the measure parser swallows a leading qualifier ("about ")
    /// and a trailing sentence boundary (". "); in instruction prose those must
    /// be preserved, or the measure glues onto the next sentence
    /// ("...foamy, about 3 minutes. Continue" → "...3 minutesContinue").
    #[rstest]
    fn test_rich_parser_preserves_qualifier_and_boundary(parser: RichParser) {
        let result = parser
            .parse("melt butter until foamy, about 3 minutes. Continue cooking")
            .unwrap();
        // The measure is still extracted (highlighted).
        assert!(
            result.iter().any(|c| matches!(c, Chunk::Measure(_))),
            "no measure extracted: {result:?}"
        );
        // "about " is kept as prose rather than deleted (condensed into the
        // preceding text run).
        assert!(
            result
                .iter()
                .any(|c| matches!(c, Chunk::Text(t) if t.ends_with("about "))),
            "leading qualifier dropped: {result:?}"
        );
        // The ". " sentence boundary survives, so "Continue" isn't glued on.
        assert!(
            result
                .iter()
                .any(|c| matches!(c, Chunk::Text(t) if t.starts_with(". Continue"))),
            "sentence boundary dropped: {result:?}"
        );
    }

    /// The reconstructed plain text (measures rendered via `Display`) must
    /// contain the original sentence boundary — no two words concatenated.
    #[rstest]
    #[case(
        "melt butter until foamy, about 3 minutes. Continue cooking",
        "3 minutes. Continue"
    )]
    #[case(
        "stir on low until creamy, about 2 minutes. Add the egg",
        "2 minutes. Add"
    )]
    #[case(
        "bake until set, 10 minutes. Transfer to a wire rack",
        "10 minutes. Transfer"
    )]
    fn test_rich_parser_no_run_on(
        parser: RichParser,
        #[case] input: &str,
        #[case] must_contain: &str,
    ) {
        let result = parser.parse(input).unwrap();
        let reconstructed: String = result
            .iter()
            .map(|c| match c {
                Chunk::Text(t) | Chunk::Ing(t) => t.clone(),
                Chunk::Measure(ms) => ms
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(" "),
            })
            .collect();
        assert!(
            reconstructed.contains(must_contain),
            "run-on at boundary: expected {must_contain:?} in {reconstructed:?}"
        );
    }

    #[rstest]
    fn test_rich_parser_with_ingredients(parser_with_ingredients: RichParser) {
        let result = parser_with_ingredients
            .parse("Add 2 cups flour and sugar")
            .unwrap();

        let has_flour = result
            .iter()
            .any(|c| matches!(c, Chunk::Ing(s) if s == "flour"));
        let has_sugar = result
            .iter()
            .any(|c| matches!(c, Chunk::Ing(s) if s == "sugar"));
        assert!(has_flour);
        assert!(has_sugar);
    }

    #[test]
    fn test_rich_parser_default() {
        let parser: RichParser = Default::default();
        let result = parser.parse("1 cup").unwrap();
        assert!(!result.is_empty());
    }

    // ============================================================================
    // Condense Text Tests
    // ============================================================================

    #[test]
    fn test_condense_text_adjacent() {
        let chunks = vec![
            Chunk::Text("hello ".to_string()),
            Chunk::Text("world".to_string()),
        ];
        let condensed = condense_text(chunks);
        assert_eq!(condensed.len(), 1);
        assert!(matches!(&condensed[0], Chunk::Text(s) if s == "hello world"));
    }

    #[test]
    fn test_condense_text_mixed() {
        let chunks = vec![
            Chunk::Text("hello ".to_string()),
            Chunk::Measure(vec![Measure::new("cup", 1.0)]),
            Chunk::Text(" world".to_string()),
        ];
        let condensed = condense_text(chunks);
        assert_eq!(condensed.len(), 3);
    }

    // ============================================================================
    // Extract Ingredients Tests
    // ============================================================================

    #[test]
    fn test_extract_ingredients() {
        let chunks = vec![Chunk::Text("Add flour and sugar".to_string())];
        let names = vec!["flour".to_string(), "sugar".to_string()];
        let result = extract_ingredients(chunks, &names);

        let has_flour = result
            .iter()
            .any(|c| matches!(c, Chunk::Ing(s) if s == "flour"));
        let has_sugar = result
            .iter()
            .any(|c| matches!(c, Chunk::Ing(s) if s == "sugar"));
        assert!(has_flour);
        assert!(has_sugar);
    }

    #[test]
    fn test_extract_ingredients_empty_name() {
        let chunks = vec![Chunk::Text("some text".to_string())];
        let names = vec!["".to_string(), "flour".to_string()];
        let result = extract_ingredients(chunks, &names);
        assert!(!result.is_empty());
    }

    /// Plural matching slices `haystack` by byte offsets (base match + optional
    /// suffix); multibyte prose (Turkish dotted-İ, accents, emoji) must never
    /// split a char or panic. Regression guard for the surface-slice fix.
    #[rstest]
    #[case("İstanbul eggs İ", "egg")]
    #[case("naïve broil café", "oil")]
    #[case("eggs😀more", "egg")]
    #[case("café. Parsley", "parsley")]
    #[case("İ", "i")]
    fn test_extract_multibyte_no_panic(#[case] input: &str, #[case] name: &str) {
        let chunks = vec![Chunk::Text(input.to_string())];
        let _ = extract_ingredients(chunks, &[name.to_string()]);
    }

    /// Extraction must be independent of the order names are listed: with
    /// names ["flour", "sugar"], "Add sugar and flour" must highlight BOTH
    /// (the old one-pass-per-name scan never revisited the prefix, leaving
    /// "sugar" as plain text).
    #[test]
    fn test_extract_ingredients_order_independent() {
        let chunks = vec![Chunk::Text("Add sugar and flour".to_string())];
        let names = vec!["flour".to_string(), "sugar".to_string()];
        let result = extract_ingredients(chunks, &names);
        assert_eq!(
            result,
            vec![
                Chunk::Text("Add ".to_string()),
                Chunk::Ing("sugar".to_string()),
                Chunk::Text(" and ".to_string()),
                Chunk::Ing("flour".to_string()),
            ]
        );
    }

    /// Every occurrence of a repeated name is extracted, not just the first.
    #[test]
    fn test_extract_ingredients_repeated_name() {
        let chunks = vec![Chunk::Text("Add flour then more flour".to_string())];
        let names = vec!["flour".to_string()];
        let result = extract_ingredients(chunks, &names);
        let flour_count = result
            .iter()
            .filter(|c| matches!(c, Chunk::Ing(s) if s == "flour"))
            .count();
        assert_eq!(flour_count, 2);
    }

    /// Plural-suffix matching, with the boundary guard re-checked past the
    /// suffix. Each case: (input, name, expected chunks). Surface text must
    /// carry the prose's own plural, not the candidate.
    ///
    /// Sentence-initial capitalized mentions use guarded case-insensitive
    /// matching (see `find_candidate_case_insensitive_at_start`); mid-sentence
    /// capitals and imperative openers stay plain text.
    #[rstest]
    // plural: name "egg" matches prose "eggs", surface keeps the plural.
    #[case::plural_s("beat the eggs", "egg", vec![
        Chunk::Text("beat the ".into()), Chunk::Ing("eggs".into())])]
    // "es" plural form.
    #[case::plural_es("dice the tomatoes", "tomato", vec![
        Chunk::Text("dice the ".into()), Chunk::Ing("tomatoes".into())])]
    // boundary re-checked past suffix: must NOT punch through "eggplant".
    #[case::no_eggplant("slice the eggplant", "egg", vec![
        Chunk::Text("slice the eggplant".into())])]
    // "s" branch must not leak into "limestone".
    #[case::no_limestone("set on limestone", "lime", vec![
        Chunk::Text("set on limestone".into())])]
    // apostrophe-s is a boundary, not a plural to absorb.
    #[case::apostrophe_s("trim the egg's shell", "egg", vec![
        Chunk::Text("trim the ".into()), Chunk::Ing("egg".into()), Chunk::Text("'s shell".into())])]
    // Sentence-initial capitalized head noun matches via guarded case-insensitive pass.
    #[case::cap_start_match("Parsley is nice", "parsley", vec![
        Chunk::Ing("Parsley".into()), Chunk::Text(" is nice".into())])]
    // same guard after a sentence terminator: still no case-fold.
    #[case::cap_after_period_no_match("Stir well. Onion adds depth", "onion", vec![
        Chunk::Text("Stir well. Onion adds depth".into())])]
    // the imperative-verb false positive that case-folding would introduce:
    // "Oil" opening a step must stay text, not highlight name "olive oil".
    #[case::imperative_verb_guard("Oil the pan well", "olive oil", vec![
        Chunk::Text("Oil the pan well".into())])]
    #[case::imperative_salt_to("Salt to taste", "salt", vec![
        Chunk::Text("Salt to taste".into())])]
    #[case::cap_start_plural_es("Tomatoes are ripe", "tomato", vec![
        Chunk::Ing("Tomatoes".into()), Chunk::Text(" are ripe".into())])]
    // must-preserve: "oil" never matches inside "broil" (leading boundary).
    #[case::broil_guard("broil until charred", "oil", vec![
        Chunk::Text("broil until charred".into())])]
    // lowercase exact still works unchanged.
    #[case::lower_exact("add the onion", "onion", vec![
        Chunk::Text("add the ".into()), Chunk::Ing("onion".into())])]
    fn test_extract_plural_and_case(
        #[case] input: &str,
        #[case] name: &str,
        #[case] expected: Vec<Chunk>,
    ) {
        let chunks = vec![Chunk::Text(input.to_string())];
        let result = extract_ingredients(chunks, &[name.to_string()]);
        assert_eq!(result, expected, "input={input:?} name={name:?}");
    }

    // ============================================================================
    // parse_rich_char() Parser Tests
    // ============================================================================

    #[rstest]
    #[case::exclamation("hello!")]
    #[case::parenthesis("(test)")]
    #[case::semicolon(";")]
    #[case::colon(":")]
    #[case::comma(",")]
    #[case::slash("/")]
    #[case::hash("#")]
    fn test_parse_rich_char_special_chars(#[case] input: &str) {
        assert!(parse_rich_char(input).is_ok());
    }

    // ============================================================================
    // Cooking Terms Should Not Be Measurements
    // ============================================================================

    #[rstest]
    #[case::medium_speed("on medium speed until done")]
    #[case::high_heat("over high heat for 5 minutes")]
    #[case::low_heat("simmer on low heat")]
    #[case::medium_high_heat("cook over medium heat")]
    #[case::small_sheet_tray("on a small sheet tray")]
    #[case::large_pot("in a large pot")]
    #[case::medium_bowl("in a medium bowl")]
    #[case::small_saucepan("in a small saucepan")]
    fn test_cooking_terms_not_measurements(parser: RichParser, #[case] input: &str) {
        let result = parser.parse(input).unwrap();
        // "medium speed", "high heat", etc. should NOT produce a Measure chunk
        // with units like "medium" or "high"
        let has_size_measure = result.iter().any(|c| {
            matches!(c, Chunk::Measure(measures) if measures.iter().any(|m| {
                let unit_str = m.unit().to_string().to_lowercase();
                matches!(unit_str.as_str(), "small" | "medium" | "large" | "high" | "low")
            }))
        });
        assert!(
            !has_size_measure,
            "Should not parse cooking terms as measurements: {input:?} -> {result:?}"
        );
    }

    // ============================================================================
    // Chunk Tests
    // ============================================================================

    #[test]
    fn test_chunk_clone() {
        let chunk = Chunk::Text("test".to_string());
        let cloned = chunk.clone();
        assert_eq!(chunk, cloned);
    }

    // Regression test: numbers followed by periods and capitalized words (like oven temps)
    // should be parsed as measurements, not rejected as step numbers
    #[test]
    fn test_oven_temperature_parsing() {
        let parser = RichParser::new(Vec::<String>::new());
        let result = parser.parse("Heat oven to 375. Combine flour").unwrap();
        // "375" should be parsed as a Measure with Whole unit
        let has_375 = result.iter().any(|c| match c {
            Chunk::Measure(measures) => measures.iter().any(|m| (m.value() - 375.0).abs() < 0.01),
            _ => false,
        });
        assert!(has_375, "Should parse 375 as a measurement: {result:?}");
    }
}
