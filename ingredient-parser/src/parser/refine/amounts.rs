use super::*;

impl IngredientParser {
    pub(super) fn extract_secondary_amounts_from_modifier(&self, parsed: &mut ParsedIngredient) {
        let Some(modifier) = parsed.modifier_string() else {
            return;
        };

        let (secondary_amounts, cleaned_modifier) =
            extract_secondary_amounts(&modifier, &self.units);
        // Only rewrite the modifier when an amount was actually hoisted; otherwise
        // leave the typed parts untouched (the cleaned string equals the original).
        if secondary_amounts.is_empty() {
            return;
        }
        parsed.amounts.extend(secondary_amounts);
        parsed.modifier = if cleaned_modifier.trim().is_empty() {
            Vec::new()
        } else {
            vec![ModifierPart::Raw(cleaned_modifier)]
        };
    }
}

fn extract_secondary_amounts(
    modifier: &str,
    units: &std::collections::HashSet<String>,
) -> (Vec<Measure>, String) {
    use regex::Regex;
    use std::sync::LazyLock;

    // An explicit approximation aside, anywhere in the modifier: "(about 2 cups)".
    static SECONDARY_AMOUNT_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
        #[allow(clippy::expect_used)]
        Regex::new(r"\((?:from\s+)?(?:about|approximately|roughly|around)\s+([^)]+)\)")
            .expect("invalid secondary amount regex")
    });
    // A bare trailing measure parenthetical: "coarsely chopped (2.1 oz / 60g)" —
    // a weight/volume equivalence stated for the prepped ingredient. Anchored to
    // the end and validated below (the inner text must fully parse as a
    // non-distance measurement), so non-measure asides like "(softened)" or
    // "(70% cacao)" fall through untouched.
    static TRAILING_MEASURE_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
        #[allow(clippy::expect_used)]
        Regex::new(r"\(([^)]+)\)\s*$").expect("invalid trailing-measure regex")
    });

    // The approximation aside wins (it strips the "about" off the amount text);
    // otherwise fall back to a bare trailing measure parenthetical.
    let Some(caps) = SECONDARY_AMOUNT_PATTERN
        .captures(modifier)
        .or_else(|| TRAILING_MEASURE_PATTERN.captures(modifier))
    else {
        return (vec![], modifier.to_string());
    };

    let Some(full_match) = caps.get(0) else {
        return (vec![], modifier.to_string());
    };
    let Some(amount_match) = caps.get(1) else {
        return (vec![], modifier.to_string());
    };
    let amount_text = amount_match.as_str().trim();

    let mp = MeasurementParser::new(units, MeasurementMode::IngredientList);
    let Ok((remaining, measures)) = mp.parse_measurement_list(amount_text) else {
        return (vec![], modifier.to_string());
    };

    // A *dimension* aside like "(about 3-inch)" inside a prep phrase ("cut into
    // long (about 3-inch) strips") describes shape, not a secondary quantity.
    // Leave it in the modifier rather than hoisting a spurious inch amount.
    let is_distance = |m: &Measure| match m.unit() {
        unit::Unit::Inch => true,
        unit::Unit::Other(s) => crate::parser::is_distance_unit(s),
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

    // Collapse, don't just trim: a mid-modifier match ("chopped (about 2 cups)
    // plus more") leaves the spaces on both sides of the excised parenthetical
    // adjacent, which trim() can't fix.
    let cleaned = collapse_whitespace(&format!(
        "{}{}",
        &modifier[..full_match.start()],
        &modifier[full_match.end()..]
    ));

    (measures, cleaned)
}
