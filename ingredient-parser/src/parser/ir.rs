//! Private source-bearing parse representation. Text origins are carried through
//! explicit slices/edits; no output-text search reconstructs field provenance.
use crate::unit::Measure;
use crate::{Field, Ingredient};
use std::ops::Range;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum ModifierKind {
    Raw,
    Prep,
    Alternative,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ModifierPart {
    pub kind: ModifierKind,
    pub value: String,
    pub origins: Vec<Range<usize>>,
}
impl ModifierPart {
    pub(crate) fn raw(value: String) -> Self {
        Self {
            kind: ModifierKind::Raw,
            value,
            origins: vec![],
        }
    }
    pub(crate) fn prep(value: String) -> Self {
        Self {
            kind: ModifierKind::Prep,
            value,
            origins: vec![],
        }
    }
    pub(crate) fn alternative(value: String) -> Self {
        Self {
            kind: ModifierKind::Alternative,
            value,
            origins: vec![],
        }
    }
    pub(crate) fn text(&self) -> &str {
        &self.value
    }
    pub(crate) fn at(mut self, origins: Vec<Range<usize>>) -> Self {
        self.origins = origins;
        self
    }
    pub(crate) fn at_range(self, origin: Range<usize>) -> Self {
        self.at(vec![origin])
    }
}

/// A text view whose bytes retain their original positions. Inserted separators
/// have no origin.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct SourceText {
    pub text: String,
    pub map: Vec<Option<usize>>,
}
impl SourceText {
    pub(crate) fn append(&mut self, text: &str, offset: usize) {
        for (i, ch) in text.char_indices() {
            if ch.is_whitespace() {
                if !self.text.is_empty() && !self.text.ends_with(' ') {
                    self.text.push(' ');
                    self.map.push(Some(offset + i));
                }
            } else {
                self.text.push(ch);
                self.map
                    .extend((0..ch.len_utf8()).map(|j| Some(offset + i + j)));
            }
        }
    }
    pub(crate) fn trim(&mut self) {
        let start = self.text.len() - self.text.trim_start().len();
        let end = self.text.trim_end().len().max(start);
        self.text = self.text[start..end].to_string();
        self.map = self.map[start..end].to_vec();
    }
    pub(crate) fn origins(&self, range: Range<usize>) -> Vec<Range<usize>> {
        ranges(&self.map[range])
    }
}

fn ranges(map: &[Option<usize>]) -> Vec<Range<usize>> {
    let mut out: Vec<Range<usize>> = Vec::new();
    for offset in map.iter().flatten() {
        if let Some(last) = out.last_mut()
            && last.end == *offset
        {
            last.end += 1;
        } else {
            out.push(*offset..offset + 1);
        }
    }
    out
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct ParsedIngredient {
    pub name: String,
    pub amounts: Vec<Measure>,
    pub modifier: Vec<ModifierPart>,
    pub optional: bool,
    pub unresolved_quantity: bool,
    pub source: String,
    pub name_map: Vec<Option<usize>>,
    pub measure_spans: Vec<Range<usize>>,
}
impl ParsedIngredient {
    pub(crate) fn set_name(&mut self, mut text: SourceText) {
        text.trim();
        self.name = text.text;
        self.name_map = text.map;
    }
    pub(crate) fn name_origins(&self, range: Range<usize>) -> Vec<Range<usize>> {
        self.name_map.get(range).map(ranges).unwrap_or_default()
    }
    /// Remove exactly the selected name bytes, preserving the origins of both
    /// surviving slices. This is the only primitive name-local extraction needs.
    pub(crate) fn remove_name(&mut self, range: Range<usize>) -> Vec<Range<usize>> {
        let origins = self.name_origins(range.clone());
        let left = self.name[..range.start].trim_end();
        let right_raw = &self.name[range.end..];
        let right = right_raw.trim_start();
        let right_start = range.end + right_raw.len() - right.len();
        let mut name = left.to_string();
        let mut map = self.name_map.get(..left.len()).unwrap_or_default().to_vec();
        if !left.is_empty() && !right.is_empty() {
            name.push(' ');
            map.push(None);
        }
        name.push_str(right);
        map.extend_from_slice(self.name_map.get(right_start..).unwrap_or_default());
        self.name = name;
        self.name_map = map;
        origins
    }
    pub(crate) fn extract_name(&mut self, range: Range<usize>, kind: ModifierKind) {
        let value = self.name[range.clone()].trim().to_string();
        let origins = self.remove_name(range);
        self.push_modifier(ModifierPart {
            kind,
            value,
            origins,
        });
    }
    pub(crate) fn rebase(&mut self, source: &str, offset: usize) {
        for origin in self.name_map.iter_mut().flatten() {
            *origin += offset;
        }
        for range in self
            .measure_spans
            .iter_mut()
            .chain(self.modifier.iter_mut().flat_map(|p| p.origins.iter_mut()))
        {
            range.start += offset;
            range.end += offset;
        }
        self.source = source.to_string();
    }
    pub(crate) fn ownership(&self) -> Vec<(Range<usize>, Field)> {
        let mut spans: Vec<_> = self
            .measure_spans
            .iter()
            .cloned()
            .map(|r| (r, Field::Amount))
            .collect();
        spans.extend(ranges(&self.name_map).into_iter().map(|r| (r, Field::Name)));
        spans.extend(
            self.modifier
                .iter()
                .flat_map(|p| p.origins.iter().cloned().map(|r| (r, Field::Modifier))),
        );
        spans
    }
    pub(crate) fn order_modifiers(&mut self) {
        self.modifier
            .sort_by_key(|p| p.origins.first().map(|r| r.start).unwrap_or(usize::MAX));
    }
    pub(crate) fn modifier_string(&self) -> Option<String> {
        let mut out = String::new();
        for part in &self.modifier {
            let text = part.text().trim().trim_start_matches(',').trim();
            if text.is_empty() {
                continue;
            }
            if !out.is_empty() {
                out.push_str(if text.starts_with('(') { " " } else { ", " });
            }
            out.push_str(text);
        }
        (!out.is_empty()).then_some(out)
    }
    pub(crate) fn push_modifier(&mut self, part: ModifierPart) {
        if !part.text().trim().is_empty() {
            self.modifier.push(part);
        }
    }
}
impl From<ParsedIngredient> for Ingredient {
    fn from(parsed: ParsedIngredient) -> Ingredient {
        let modifier = super::refine::strip_wrapping_parens(parsed.modifier_string());
        Ingredient {
            name: parsed.name,
            amounts: parsed.amounts,
            modifier,
            optional: parsed.optional,
            usage: Default::default(),
            parse_notes: Default::default(),
        }
    }
}
