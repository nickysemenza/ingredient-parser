//! Text-artifact normalization with occurrence-level source mappings.
//!
//! Only typography and extraction artifacts belong here: whitespace, list
//! bullets and footnote glyphs. Optionality, references, quantity qualifiers and
//! other ingredient meaning are resolved from source-bearing parser structure.

use std::borrow::Cow;

/// A circled-number glyph (①②③ …) used as a footnote/technique-note marker in
/// some cookbooks (e.g. Claire Saffitz's *Dessert Person*). They're not part of
/// the ingredient, so they're stripped during normalization rather than leaking
/// into the name or modifier.
fn is_footnote_marker(c: char) -> bool {
    matches!(c,
        '\u{2460}'..='\u{2473}'   // ① .. ⑳  circled 1–20
        | '\u{2474}'..='\u{2487}' // ⑴ .. ⒈  parenthesized / full-stop digits
        | '\u{2488}'..='\u{249B}'
        | '\u{24EA}'              // ⓪ circled zero
        | '\u{24F5}'..='\u{24FF}' // double-circled / negative-circled digits
        | '\u{2776}'..='\u{2793}' // dingbat negative/sans-serif circled digits
    )
}

/// Replace non-breaking spaces (common in PDF/EPUB-extracted text) with ASCII
/// spaces so the grammar's space handling works.
fn strip_nbsp(input: &str) -> Cow<'_, str> {
    rewrite_text(RewriteId::Nbsp, input)
}

/// Drop footnote markers (e.g. "rye flour ①" → "rye flour ").
fn strip_footnote_markers(input: &str) -> Cow<'_, str> {
    rewrite_text(RewriteId::FootnoteMarkers, input)
}

/// Strip a trailing footnote marker — an ASCII asterisk or dagger left at the
/// end of a line ("shredded zucchini (see note)*" → "shredded zucchini (see
/// note)"). Anchored to the end so a mid-name asterisk is untouched; the Unicode
/// circled-digit markers are handled by `strip_footnote_markers`.
fn strip_trailing_footnote_markers(input: &str) -> Cow<'_, str> {
    rewrite_text(RewriteId::TrailingFootnoteMarkers, input)
}

/// Strip a leading list-bullet glyph — an en/em-dash, bullet, middot, or
/// asterisk followed by whitespace — that some cookbooks (e.g. hotpot ingredient
/// lists in *The Food of Sichuan*) prefix to each ingredient line. Left in place
/// it lands at the head of the name ("– shiitake mushrooms"). The trailing
/// whitespace requirement keeps a hyphenated/negative leading token untouched.
fn strip_leading_bullet(input: &str) -> Cow<'_, str> {
    rewrite_text(RewriteId::LeadingBullet, input)
}

type Rewrite = fn(&str) -> Cow<'_, str>;

crate::define_stage_pipeline! {
    enum RewriteId,
    struct RewriteEntry,
    const REWRITES: &[RewriteEntry],
    type Rewrite = Rewrite,
    trace: pub(crate) REWRITE_TRACE_NAMES,
    (Nbsp, "strip_nbsp", strip_nbsp),
    (LeadingBullet, "strip_leading_bullet", strip_leading_bullet),
    (FootnoteMarkers, "strip_footnote_markers", strip_footnote_markers),
    (
        TrailingFootnoteMarkers,
        "strip_trailing_footnote_markers",
        strip_trailing_footnote_markers
    ),

}

/// Apply one rewrite to the accumulator, preserving its owned-ness: a borrowed
/// result means the rewrite changed nothing, so the accumulator is kept as-is
/// (no allocation on the common path). When tracing, a rewrite that *did* change
/// the line emits a before→after node so `--explain` shows which rewrite fired.
fn apply_rewrite<'a>(acc: Cow<'a, str>, rewrite: &RewriteEntry) -> Cow<'a, str> {
    let RewriteEntry { run, .. } = *rewrite;
    match run(acc.as_ref()) {
        Cow::Owned(rewritten) => {
            crate::trace::trace_on_change(
                crate::trace::Stage::Normalize,
                rewrite.id().as_str(),
                acc.as_ref(),
                &rewritten,
                true,
            );
            Cow::Owned(rewritten)
        }
        Cow::Borrowed(_) => acc,
    }
}

/// Run all pre-parse rewrites on a raw ingredient line, then collapse any
/// trailing/doubled whitespace a rewrite may have left behind.
pub(super) fn normalize_input(input: &str) -> Cow<'_, str> {
    let mut normalized = Cow::Borrowed(input);
    for rewrite in REWRITES {
        normalized = apply_rewrite(normalized, rewrite);
    }

    if needs_whitespace_collapse(&normalized) {
        Cow::Owned(replace_edits(&normalized, &whitespace_edits(&normalized)).into_owned())
    } else {
        normalized
    }
}

/// Collapse runs of whitespace to single spaces and trim. Shared with the
/// post-parse name cleanup in the refine phase.
pub(super) fn collapse_whitespace(input: &str) -> String {
    // Write words straight into a pre-sized buffer; the old
    // `split_whitespace().collect::<Vec<_>>().join(" ")` allocated a throwaway Vec
    // on every parse (this runs in both normalize and the refine name cleanup).
    let mut out = String::with_capacity(input.len());
    for word in input.split_whitespace() {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(word);
    }
    out
}

/// One occurrence-specific replacement. Ranges refer to the text immediately
/// before this rewrite, never to a later parser result.
#[derive(Debug)]
struct Edit {
    range: std::ops::Range<usize>,
    replacement: String,
}

fn delete(range: std::ops::Range<usize>) -> Edit {
    Edit {
        range,
        replacement: String::new(),
    }
}

fn deletions(regex: &regex::Regex, input: &str) -> Vec<Edit> {
    regex
        .find_iter(input)
        .map(|found| delete(found.range()))
        .collect()
}

/// These occurrence edits are the single implementation for both the plain
/// text and provenance-aware paths. Captured text is left in place; only the
/// actual changed region is replaced or deleted.
fn rewrite_edits(id: RewriteId, input: &str) -> Vec<Edit> {
    match id {
        RewriteId::Nbsp => input
            .char_indices()
            .filter(|&(_, c)| c == '\u{a0}')
            .map(|(start, c)| Edit {
                range: start..start + c.len_utf8(),
                replacement: " ".to_string(),
            })
            .collect(),
        RewriteId::FootnoteMarkers => input
            .char_indices()
            .filter(|&(_, c)| is_footnote_marker(c))
            .map(|(start, c)| delete(start..start + c.len_utf8()))
            .collect(),
        RewriteId::LeadingBullet => {
            crate::lazy_regex!(
                LEADING_BULLET,
                r"^\s*[-\u{2013}\u{2014}\u{2022}\u{00B7}\u{2219}*]\s+"
            );
            deletions(&LEADING_BULLET, input)
        }
        RewriteId::TrailingFootnoteMarkers => {
            crate::lazy_regex!(TRAILING_MARK, r"\s*[*\u{2020}\u{2021}]+\s*$");
            deletions(&TRAILING_MARK, input)
        }
    }
}

fn replace_edits<'a>(input: &'a str, edits: &[Edit]) -> Cow<'a, str> {
    if edits.is_empty() {
        return Cow::Borrowed(input);
    }
    let mut output = String::with_capacity(input.len());
    let mut cursor = 0;
    for edit in edits {
        output.push_str(&input[cursor..edit.range.start]);
        output.push_str(&edit.replacement);
        cursor = edit.range.end;
    }
    output.push_str(&input[cursor..]);
    Cow::Owned(output)
}

fn rewrite_text(id: RewriteId, input: &str) -> Cow<'_, str> {
    replace_edits(input, &rewrite_edits(id, input))
}

/// Normalized parser input with authored origins for every output byte. Bytes
/// belonging to one UTF-8 character share its complete authored character range;
/// rewritten bytes own the explicit replaced occurrence. Deleted text has no
/// surviving origin. Construct this only when source observation is requested.
pub(super) struct NormalizedSource<'a> {
    pub text: Cow<'a, str>,
    origins: Vec<std::ops::Range<usize>>,
}

impl NormalizedSource<'_> {
    /// Project a normalized byte range into ordered, disjoint authored ranges.
    /// Adjacent surviving characters coalesce; deleted gaps never do.
    pub fn project(&self, range: std::ops::Range<usize>) -> Vec<std::ops::Range<usize>> {
        let mut output: Vec<std::ops::Range<usize>> = Vec::new();
        for origin in self.origins.get(range).unwrap_or_default() {
            if let Some(last) = output.last_mut()
                && origin.start <= last.end
            {
                last.end = last.end.max(origin.end);
            } else {
                output.push(origin.clone());
            }
        }
        output
    }

    fn apply(&mut self, edits: &[Edit]) {
        if edits.is_empty() {
            return;
        }
        let rewritten = replace_edits(&self.text, edits).into_owned();
        let mut origins = Vec::with_capacity(rewritten.len());
        let mut cursor = 0;
        for edit in edits {
            origins.extend_from_slice(&self.origins[cursor..edit.range.start]);
            // A literal replacement is attributed to precisely the occurrence
            // it replaces. No token lookup or output alignment is involved.
            let projected = self.project(edit.range.clone());
            // A collapsed whitespace slot represents the first surviving
            // whitespace occurrence, never a bounding span across deleted text.
            if let Some(first) = projected.first() {
                origins.extend(std::iter::repeat_n(first.clone(), edit.replacement.len()));
            }
            cursor = edit.range.end;
        }
        origins.extend_from_slice(&self.origins[cursor..]);
        self.text = Cow::Owned(rewritten);
        self.origins = origins;
    }
}

fn whitespace_edits(input: &str) -> Vec<Edit> {
    crate::lazy_regex!(WHITESPACE, r"\s+");
    WHITESPACE
        .find_iter(input)
        .filter_map(|found| {
            let replacement = if found.start() == 0 || found.end() == input.len() {
                ""
            } else {
                " "
            };
            (found.as_str() != replacement).then(|| Edit {
                range: found.range(),
                replacement: replacement.to_string(),
            })
        })
        .collect()
}

fn needs_whitespace_collapse(input: &str) -> bool {
    input.chars().any(|c| c.is_whitespace() && c != ' ')
        || input.as_bytes().windows(2).any(|w| w == b"  ")
        || input.starts_with(' ')
        || input.ends_with(' ')
}

/// Run the same lexical edits as the ordinary path, retaining authored origins.
pub(super) fn normalize_source(input: &str) -> NormalizedSource<'_> {
    let mut source = NormalizedSource {
        text: Cow::Borrowed(input),
        origins: input
            .char_indices()
            .flat_map(|(start, c)| std::iter::repeat_n(start..start + c.len_utf8(), c.len_utf8()))
            .collect(),
    };
    for rewrite in REWRITES {
        let edits = rewrite_edits(rewrite.id(), &source.text);
        if !edits.is_empty() {
            let before = source.text.to_string();
            source.apply(&edits);
            crate::trace::trace_on_change(
                crate::trace::Stage::Normalize,
                rewrite.id().as_str(),
                &before,
                &source.text,
                true,
            );
        }
    }
    if needs_whitespace_collapse(&source.text) {
        source.apply(&whitespace_edits(&source.text));
    }
    source
}

#[cfg(test)]
mod tests;
