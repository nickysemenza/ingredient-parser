use super::*;

impl IngredientParser {
    /// Postfix produce count-units: "1 medium garlic clove" -> name "garlic",
    /// amount `{clove:1}`, with leading descriptors ("medium") moved to the
    /// modifier. Only fires for the curated [`vocab::POSTFIX_PRODUCE_UNITS`]
    /// pairs and only when the count is a count (or absent), so
    /// weights/volumes and idioms like "cinnamon stick" / "wood ear mushroom"
    /// are untouched.
    ///
    /// [`vocab::POSTFIX_PRODUCE_UNITS`]: crate::parser::vocab::POSTFIX_PRODUCE_UNITS
    pub(super) fn extract_postfix_produce_unit(&self, parsed: &mut ParsedIngredient) {
        // The count must be a plain count (the default count unit) or
        // there must be no amount at all; a real volume/weight lead means the
        // trailing word isn't acting as the count unit.
        let whole_idx = parsed
            .amounts
            .iter()
            .position(|m| matches!(m.unit(), unit::Unit::Whole));
        if whole_idx.is_none() && !parsed.amounts.is_empty() {
            return;
        }

        let name_lower = parsed.name.to_lowercase();
        for (food, units) in crate::parser::vocab::POSTFIX_PRODUCE_UNITS {
            for unit_word in *units {
                let suffix = format!("{food} {unit_word}");
                if name_lower != suffix && !name_lower.ends_with(&format!(" {suffix}")) {
                    continue;
                }
                // `suffix` is ASCII produce, so lowercasing preserved byte
                // lengths and this offset is a valid char boundary in `name`.
                let food_start = parsed.name.len() - suffix.len();
                let measure = whole_idx
                    .map(|i| parsed.amounts[i].relabel_unit(unit_word))
                    .unwrap_or_else(|| Measure::new(unit_word, 1.0));
                match whole_idx {
                    Some(i) => parsed.amounts[i] = measure,
                    None => parsed.amounts.push(measure),
                }
                let food_end = food_start + food.len();
                let unit_origins = parsed.remove_name(food_end..parsed.name.len());
                parsed.measure_spans.extend(unit_origins);
                if food_start > 0 {
                    parsed.extract_name(0..food_start, ModifierKind::Prep);
                }
                return;
            }
        }
    }

    /// Consume a leading size descriptor as the count *unit* for an explicitly
    /// counted item: "3 medium carrots" -> `{medium:3}` carrots, "2 extra large
    /// eggs" -> `{extra large:2}` eggs. This lets the size map to USDA portion data
    /// through the unit graph — a bare `{whole}` count is not a USDA portion key,
    /// the size is (for example, a mapping can relate "1 each" to "1 large").
    ///
    /// Fires only when a real `Unit::Whole` count is present (so "medium heat", a
    /// no-count "medium onion", and "2 cups large onion" are all untouched), the
    /// name begins with a [`vocab::SIZE_UNIT_WORDS`] token, and a head noun follows
    /// that isn't a connector or another size word (so the "medium or large …"
    /// range stays as authored text). Runs after
    /// [`Self::extract_postfix_produce_unit`] so a produce count unit ("1 medium
    /// garlic clove" -> `{clove:1}`) wins and this pass then skips it.
    ///
    /// [`vocab::SIZE_UNIT_WORDS`]: crate::parser::vocab::SIZE_UNIT_WORDS
    pub(super) fn extract_size_unit_from_name(&self, parsed: &mut ParsedIngredient) {
        // Require an explicit whole count — the load-bearing gate. No count means
        // there is no portion to size ("medium heat" has no amount).
        let Some(idx) = parsed
            .amounts
            .iter()
            .position(|m| matches!(m.unit(), unit::Unit::Whole))
        else {
            return;
        };

        let name = parsed.name.trim();
        let name_lower = name.to_lowercase();
        // SIZE_UNIT_WORDS is ordered longest-first, so "extra large"/"extra-large"
        // win over "large"; the trailing-whitespace check rejects "larger"/"jumbos".
        let mut matched: Option<&str> = None;
        for w in crate::parser::vocab::SIZE_UNIT_WORDS {
            if let Some(rest) = name_lower.strip_prefix(*w)
                && rest.starts_with(char::is_whitespace)
            {
                matched = Some(*w);
                break;
            }
        }
        let Some(size) = matched else {
            return;
        };

        // `size` is ASCII, so its byte length indexes `name` (original case) too.
        let rest = name[size.len()..].trim();
        if rest.is_empty() {
            return;
        }
        // A connector or a second size word means this is a range ("medium or large
        // carrots") or malformed — leave the whole phrase in the name.
        let next = rest.split_whitespace().next().unwrap_or("").to_lowercase();
        if next == "or"
            || next == "and"
            || crate::parser::vocab::SIZE_WORDS.contains(&next.as_str())
        {
            return;
        }

        // Only the two "extra large" spellings canonicalize to "extra large".
        // Match them exactly rather than on an "extra" prefix, so a future
        // SIZE_UNIT_WORDS entry like "extra small" is never mis-mapped to large.
        let unit_str = match size {
            "extra large" | "extra-large" => "extra large",
            other => other,
        };
        let m = &parsed.amounts[idx];
        parsed.amounts[idx] = m.relabel_unit(unit_str);
        let origins = parsed.remove_name(0..size.len());
        parsed.measure_spans.extend(origins);
    }
}
