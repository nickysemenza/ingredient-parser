use super::*;

impl IngredientParser {
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
    pub(super) fn extract_leading_prep_alternative(&self, parsed: &mut ParsedIngredient) {
        let trimmed = parsed.name.trim();
        let words: Vec<&str> = trimmed.split_whitespace().collect();
        if words.len() < 4 {
            return;
        }
        // Every guard below matches tokens against lowercase vocab ("or", known
        // adjectives), so lowercase each token once up front instead of repeating
        // `words[i].to_lowercase()` per check.
        let words_lower: Vec<String> = words.iter().map(|w| w.to_lowercase()).collect();
        if words_lower[1] != "or" {
            return;
        }
        let first = &words_lower[0];
        let first_is_prep = crate::parser::token::is_participle(first, &self.adjectives);
        if !first.chars().all(char::is_alphabetic) || !first_is_prep {
            return;
        }
        // A known adjective phrase (two words then one) immediately after "or".
        // Only build the two-word key when there's room for it — the common
        // short-name case never allocates the `format!`.
        let two_word_adj = words.len() >= 5
            && words_lower.get(3).is_some_and(|w3| {
                self.adjectives
                    .contains(&format!("{} {}", words_lower[2], w3))
            });
        let adj_len = if two_word_adj {
            2
        } else if self.adjectives.contains(&words_lower[2]) {
            1
        } else {
            return;
        };
        let name_start = 2 + adj_len;
        if name_start >= words.len() {
            return;
        }
        let cut = crate::parser::token::offsets(trimmed)
            .nth(name_start)
            .map(|(i, _)| i);
        if let Some(cut) = cut {
            parsed.extract_name(0..cut, ModifierKind::Prep);
        }
    }
}
