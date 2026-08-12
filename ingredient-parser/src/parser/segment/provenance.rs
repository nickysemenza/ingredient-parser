//! Optional final-field provenance carried by the segment-stage IR.
//!
//! The ordinary parse path stores `None` and pays no allocation cost. A
//! diagnostic execution enables this record, after which assembly repairs and
//! name refinement reconcile authored byte ranges alongside the text they move.

use std::ops::Range;

use crate::parser::ir::ParsedIngredient;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AssemblyProvenance {
    source: String,
    pub(crate) name: Vec<Range<usize>>,
    pub(crate) amounts: Vec<Range<usize>>,
    pub(crate) modifier: Vec<Range<usize>>,
}

impl AssemblyProvenance {
    pub(crate) fn new(
        source: &str,
        name: Vec<Range<usize>>,
        amounts: Vec<Range<usize>>,
        modifier: Vec<Range<usize>>,
    ) -> Self {
        Self {
            source: source.to_string(),
            name,
            amounts,
            modifier,
        }
    }

    /// Reconcile final Name and Modifier origins after a structural move.
    /// Matching is token-based so reordered and discontinuous text keeps its
    /// authored byte ranges; inserted join punctuation remains synthetic.
    pub(crate) fn reconcile(&mut self, parsed: &ParsedIngredient) {
        let tokens = source_tokens(&self.source);
        let mut claimed = vec![false; tokens.len()];
        for (index, (_, range)) in tokens.iter().enumerate() {
            if self.amounts.iter().any(|amount| overlaps(amount, range)) {
                claimed[index] = true;
            }
        }

        self.name = match_tokens(&self.source, &tokens, &mut claimed, &parsed.name);
        self.modifier = match parsed.modifier_string() {
            Some(modifier) => match_tokens(&self.source, &tokens, &mut claimed, &modifier),
            None => Vec::new(),
        };
    }

    /// Claim newly hoisted amount parentheticals before reconciling text fields.
    pub(crate) fn claim_new_amounts(&mut self, previous_count: usize, parsed: &ParsedIngredient) {
        if parsed.amounts.len() <= previous_count {
            return;
        }
        let bytes = self.source.as_bytes();
        let mut start = None;
        for (index, byte) in bytes.iter().copied().enumerate() {
            match byte {
                b'(' => start = Some(index),
                b')' => {
                    if let Some(open) = start.take() {
                        let range = open..index + 1;
                        let text = &self.source[range.clone()];
                        if text
                            .chars()
                            .any(|c| c.is_ascii_digit() || crate::fraction::is_vulgar(c))
                            && !self.amounts.iter().any(|amount| overlaps(amount, &range))
                        {
                            self.amounts.push(range);
                        }
                    }
                }
                _ => {}
            }
        }
        self.amounts.sort_by_key(|range| range.start);
    }
}

fn overlaps(left: &Range<usize>, right: &Range<usize>) -> bool {
    left.start < right.end && right.start < left.end
}

fn source_tokens(source: &str) -> Vec<(String, Range<usize>)> {
    let mut out = Vec::new();
    let mut start = None;
    for (index, ch) in source.char_indices() {
        if ch.is_alphanumeric() || crate::fraction::is_vulgar(ch) {
            start.get_or_insert(index);
        } else if let Some(token_start) = start.take() {
            out.push((
                source[token_start..index].to_lowercase(),
                token_start..index,
            ));
        }
    }
    if let Some(token_start) = start {
        out.push((
            source[token_start..].to_lowercase(),
            token_start..source.len(),
        ));
    }
    out
}

fn target_tokens(text: &str) -> impl Iterator<Item = String> + '_ {
    text.split(|ch: char| !ch.is_alphanumeric() && !crate::fraction::is_vulgar(ch))
        .filter(|token| !token.is_empty())
        .map(str::to_lowercase)
}

fn match_tokens(
    authored: &str,
    source: &[(String, Range<usize>)],
    claimed: &mut [bool],
    target: &str,
) -> Vec<Range<usize>> {
    let mut ranges = Vec::new();
    let mut cursor = 0;
    for wanted in target_tokens(target) {
        let found = (cursor..source.len())
            .chain(0..cursor)
            .find(|index| !claimed[*index] && source[*index].0 == wanted);
        let Some(index) = found else {
            continue;
        };
        claimed[index] = true;
        ranges.push(source[index].1.clone());
        cursor = index + 1;
    }
    ranges.sort_by_key(|range| range.start);
    coalesce(authored, ranges)
}

fn coalesce(authored: &str, ranges: Vec<Range<usize>>) -> Vec<Range<usize>> {
    let mut out: Vec<Range<usize>> = Vec::new();
    for range in ranges {
        if let Some(previous) = out.last_mut()
            && previous.end <= range.start
        {
            let gap = &authored[previous.end..range.start];
            if gap.chars().all(|ch| !ch.is_alphanumeric()) {
                previous.end = range.end;
                continue;
            }
        }
        out.push(range);
    }
    out
}
