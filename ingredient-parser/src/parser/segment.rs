//! Clause segmentation — split post-amount ingredient text into clauses and
//! classify each one.
//!
//! The legacy grammar tail carves the post-amount text at the *first* `", "`
//! (the name grammar cannot cross `','` or `'('`) and leaves everything after it
//! as one opaque modifier string, which a family of refine passes then repairs
//! (recover a head noun stranded behind a prep chain, re-attach an alias
//! parenthetical, graft a shared head off an alternatives list, hoist a
//! measurement parenthetical…). This module replaces that carve-then-repair
//! design with an explicit *segmentation*: the post-amount text is split into
//! clauses at every top-level `", "` / `"; "` boundary, each top-level
//! parenthetical becomes its own sub-clause attached to its host clause, and
//! every clause is classified by an ordered rule table ([`CLASSIFIER`]) so the
//! assembly step (and, later, `--explain`) can reason about the line's
//! structure directly.
//!
//! Splitting and classification are pure over the source text; byte ranges into
//! the source are preserved on every clause so field spans can be re-derived
//! for the decomposition view.

// Parts of the clause model (soft boundaries, per-clause kinds) are consumed by
// the later migration steps (trace/`--explain` integration, decompose spans);
// until cutover they are exercised by unit tests only.
#![allow(dead_code)]

use std::ops::Range;

use nom::Parser as _;
use nom::character::complete::space0;
use nom::combinator::opt;

use crate::IngredientParser;
use crate::parser::ir::{ModifierPart, ParsedIngredient};
use crate::parser::paren::{self, ParenKind};
use crate::parser::token;
use crate::parser::vocab;
use crate::parser::{MeasurementMode, MeasurementParser, Res, parse_ingredient_text};
use crate::unit::Measure;

/// What a single clause *is*, judged from its paren-free text by the ordered
/// [`CLASSIFIER`] table (first matching row wins). Parenthetical sub-clauses
/// are classified separately via [`paren::classify`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ClauseKind {
    /// Every token is a preparation token (participle/adverb/descriptor), with
    /// "and"/"&" connectors allowed between them: "deribbed", "seeded",
    /// "bone-in", "peeled and deveined".
    PrepChain,
    /// The whole clause is an exact known preparation adjective / purpose
    /// phrase ("finely chopped", "to taste", "for garnish").
    KnownPrepPhrase,
    /// "minus <parseable measurement> …" — a subtractive amount clause
    /// ("minus 1 tablespoon flour").
    MinusMeasure,
    /// "for <gerund> …" / "for the …" — a purpose clause
    /// ("for brushing the bread", "for the pans").
    Purpose,
    /// An alternative clause led by "or " / "and/or "
    /// ("or white onion", "and/or rosemary").
    Alternative,
    /// A parenthetical sub-clause, carrying its [`ParenKind`].
    Parenthetical(ParenKind),
    /// Prose — the first word is a modifier stopword ("such as serrano",
    /// "then drained", "plus more for serving").
    Prose,
    /// Default: could be (part of) the ingredient name.
    HeadCandidate,
}

impl ClauseKind {
    /// Stable lowercase label for traces and reports.
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            ClauseKind::PrepChain => "prep_chain",
            ClauseKind::KnownPrepPhrase => "known_prep_phrase",
            ClauseKind::MinusMeasure => "minus_measure",
            ClauseKind::Purpose => "purpose",
            ClauseKind::Alternative => "alternative",
            ClauseKind::Parenthetical(_) => "parenthetical",
            ClauseKind::Prose => "prose",
            ClauseKind::HeadCandidate => "head_candidate",
        }
    }
}

/// A top-level parenthetical attached to its host clause.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ParenClause<'a> {
    /// Byte range of the whole `(...)` (both parens inclusive) in the source.
    pub range: Range<usize>,
    /// The inner text between the parens (untrimmed slice of the source).
    pub inner: &'a str,
    /// The parenthetical's classification.
    pub kind: ParenKind,
}

/// One clause of the post-amount text: a maximal span between top-level
/// `", "` / `"; "` separators, with its top-level parentheticals attached as
/// sub-clauses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Clause<'a> {
    /// Byte range of the clause in the source (separator excluded, surrounding
    /// whitespace included as written).
    pub range: Range<usize>,
    /// The separator that preceded this clause (`""` for the first clause).
    pub sep: &'a str,
    /// The clause's classification (of its paren-free text).
    pub kind: ClauseKind,
    /// Top-level parentheticals inside the clause, in source order.
    pub parens: Vec<ParenClause<'a>>,
    /// The clause text with its top-level parentheticals removed and
    /// whitespace collapsed — the view [`CLASSIFIER`] judged.
    pub stripped: String,
}

impl<'a> Clause<'a> {
    /// The raw clause slice (parens included) out of `source`.
    pub(crate) fn text(&self, source: &'a str) -> &'a str {
        &source[self.range.clone()]
    }
}

/// A soft boundary *inside* a clause — a coordination or purpose seam that does
/// not split the clause, but that assembly may consult (e.g. the trailing
/// or-clause of a shared-head alternatives list, a "for <gerund>" purpose
/// tail).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SoftBoundaryKind {
    /// Word-boundary " or ".
    Or,
    /// Word-boundary " and/or ".
    AndOr,
    /// " such as ".
    SuchAs,
    /// " to taste" (at a word boundary).
    ToTaste,
    /// " for " followed by a gerund (≥5 chars ending "ing") or "the" —
    /// mirroring `refine::prep::extract_purpose_gerund`'s guards.
    ForPurpose,
}

/// A soft boundary occurrence: `at` is the byte offset in the examined text
/// where the boundary's *separator* starts (i.e. the space before "or"/"for"/…).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SoftBoundary {
    pub at: usize,
    pub kind: SoftBoundaryKind,
}

/// Which post-amount pipeline [`IngredientParser`] runs.
///
/// Crate-internal migration switch (exposed `#[doc(hidden)]` so the food-cli
/// shadow harness can construct a `Segmented` parser). `Legacy` is the
/// grammar's carve-at-first-comma tail + the full repair-pass pipeline;
/// `Segmented` is the clause-segmentation path in this module.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SegmentationMode {
    /// The historical path: grammar carves name/modifier at the first `", "`,
    /// refine passes repair the damage.
    #[default]
    Legacy,
    /// The clause-segmentation path: amounts grammar + [`Segmenter::segment`] +
    /// assembly, followed by the kept refine passes.
    Segmented,
}

/// The clause segmenter: borrows the parser's vocab sets so classification
/// matches the refine passes it replaces.
pub(crate) struct Segmenter<'p> {
    adjectives: &'p std::collections::HashSet<String>,
    units: &'p std::collections::HashSet<String>,
}

impl IngredientParser {
    /// A [`Segmenter`] borrowing this parser's adjective/unit vocab.
    pub(crate) fn segmenter(&self) -> Segmenter<'_> {
        Segmenter {
            adjectives: &self.adjectives,
            units: &self.units,
        }
    }
}

// --- Classifier table ----------------------------------------------------------

/// One classifier rule: a kind plus its predicate over the clause's paren-free
/// text. Mirrors the `define_stage_pipeline!` shape (ordered, named, one row per
/// kind) so `--explain` can later show per-clause decisions; kept as a plain
/// const table because [`ClauseKind::Parenthetical`] carries data and is
/// assigned outside this table (parens are classified by [`paren::classify`]).
struct ClassifierRule {
    kind: ClauseKind,
    matches: fn(&Segmenter<'_>, &str) -> bool,
}

/// Ordered classification rules — first match wins; [`ClauseKind::HeadCandidate`]
/// is the fall-through default (no row).
const CLASSIFIER: &[ClassifierRule] = &[
    ClassifierRule {
        kind: ClauseKind::PrepChain,
        matches: is_prep_chain,
    },
    ClassifierRule {
        kind: ClauseKind::KnownPrepPhrase,
        matches: is_known_prep_phrase,
    },
    ClassifierRule {
        kind: ClauseKind::MinusMeasure,
        matches: is_minus_measure,
    },
    ClassifierRule {
        kind: ClauseKind::Purpose,
        matches: is_purpose,
    },
    ClassifierRule {
        kind: ClauseKind::Alternative,
        matches: is_alternative,
    },
    ClassifierRule {
        kind: ClauseKind::Prose,
        matches: is_prose,
    },
];

/// "and"/"&" — a connector allowed between the tokens of a prep chain
/// ("peeled and deveined"), mirroring `recover_head_noun_from_modifier`.
fn is_connector(word: &str) -> bool {
    let wl = token::norm(word);
    wl == "and" || wl == "&"
}

/// Every token is a prep token, with connectors allowed strictly *between*
/// prep tokens (never leading or trailing). At least one prep token required.
fn is_prep_chain(_seg: &Segmenter<'_>, text: &str) -> bool {
    let words: Vec<&str> = text.split_whitespace().collect();
    let (Some(first), Some(last)) = (words.first(), words.last()) else {
        return false; // empty clause
    };
    if is_connector(first) || is_connector(last) {
        return false;
    }
    words
        .iter()
        .all(|w| token::is_prep_token(w) || is_connector(w))
}

/// The whole clause is an exact known adjective/purpose phrase (the same
/// membership test `fix_leading_prep_phrase` used on the displaced name).
fn is_known_prep_phrase(seg: &Segmenter<'_>, text: &str) -> bool {
    let trimmed = text.trim();
    !trimmed.is_empty() && seg.adjectives.contains(&trimmed.to_lowercase())
}

/// "minus <parseable measurement>…" — the shape `fix_leading_minus_clause`
/// repaired. The measurement must parse; whether a head noun follows is the
/// assembly step's concern.
fn is_minus_measure(seg: &Segmenter<'_>, text: &str) -> bool {
    let Some(rest) = text
        .trim_start()
        .strip_prefix("minus ")
        .or_else(|| text.trim_start().strip_prefix("Minus "))
    else {
        return false;
    };
    let mp = MeasurementParser::new(seg.units, MeasurementMode::IngredientList);
    match mp.parse_measurement_list(rest) {
        Ok((_, measures)) => !measures.is_empty(),
        Err(_) => false,
    }
}

/// "for <gerund>…" / "for the …" — mirrors `extract_purpose_gerund`'s guards
/// (gerund ≥5 chars ending "ing", all alphabetic; or the article "the").
fn is_purpose(_seg: &Segmenter<'_>, text: &str) -> bool {
    let mut words = text.split_whitespace();
    let Some(first) = words.next() else {
        return false;
    };
    if !first.eq_ignore_ascii_case("for") {
        return false;
    }
    let next = words.next().unwrap_or("");
    is_gerund(next) || next.eq_ignore_ascii_case("the")
}

/// A purpose gerund per `extract_purpose_gerund`: ≥5 chars, ends "ing", all
/// alphabetic ("brushing", "serving" — but not "icing"… which is 5 chars and
/// does qualify; the length guard only rejects short non-gerunds like "king").
fn is_gerund(word: &str) -> bool {
    word.len() >= 5 && word.ends_with("ing") && word.chars().all(char::is_alphabetic)
}

/// An alternative clause: led by "or " or "and/or " (a comma-split tail of an
/// alternatives list, e.g. "or melted coconut oil").
fn is_alternative(_seg: &Segmenter<'_>, text: &str) -> bool {
    let trimmed = text.trim_start();
    let lead = trimmed.split_whitespace().next().unwrap_or("");
    lead.eq_ignore_ascii_case("or") || lead.eq_ignore_ascii_case("and/or")
}

/// Prose: the first word is a modifier stopword ("such as serrano",
/// "then drained", "plus more for serving") — the same test the recover passes
/// used to tell a prose modifier from a head noun.
fn is_prose(_seg: &Segmenter<'_>, text: &str) -> bool {
    let Some(first) = text.split_whitespace().next() else {
        return false;
    };
    vocab::MODIFIER_STOPWORDS.contains(&token::norm(first).as_str())
}

// --- Splitting ------------------------------------------------------------------

/// Byte offsets (in `source`) of every top-level clause separator: a `", "` or
/// `"; "` at paren depth zero. Returns `(offset, separator_len)` pairs;
/// separators inside parentheses never split.
fn separator_offsets(source: &str) -> Vec<(usize, usize)> {
    let bytes = source.as_bytes();
    let mut depth = 0usize;
    let mut out = Vec::new();
    for (i, &b) in bytes.iter().enumerate() {
        match b {
            b'(' => depth += 1,
            b')' => depth = depth.saturating_sub(1),
            b',' | b';' if depth == 0 => {
                if bytes.get(i + 1) == Some(&b' ') {
                    out.push((i, 2));
                }
            }
            _ => {}
        }
    }
    out
}

impl Segmenter<'_> {
    /// Split `source` into classified clauses. Top-level `", "` / `"; "`
    /// separators split (parens never do); each clause's top-level
    /// parentheticals attach to it as [`ParenClause`] sub-clauses and are
    /// removed from the text the classifier judges.
    pub(crate) fn segment<'a>(&self, source: &'a str) -> Vec<Clause<'a>> {
        let mut clauses = Vec::new();
        let mut start = 0usize;
        let mut sep: &str = "";
        for (off, len) in separator_offsets(source) {
            clauses.push(self.build_clause(source, start..off, sep));
            sep = &source[off..off + len];
            start = off + len;
        }
        clauses.push(self.build_clause(source, start..source.len(), sep));
        clauses
    }

    /// Build one clause: attach its top-level parens, strip them from the
    /// classified text, and classify.
    fn build_clause<'a>(&self, source: &'a str, range: Range<usize>, sep: &'a str) -> Clause<'a> {
        let text = &source[range.clone()];
        let parens: Vec<ParenClause<'a>> = paren::spans(text)
            .map(|s| ParenClause {
                range: range.start + s.range.start..range.start + s.range.end,
                inner: s.inner,
                kind: paren::classify(s.inner, Some(self.units)),
            })
            .collect();

        // The classifier judges the clause text with its parens excised.
        let stripped = if parens.is_empty() {
            crate::parser::normalize::collapse_whitespace(text)
        } else {
            let mut buf = String::with_capacity(text.len());
            let mut cursor = range.start;
            for p in &parens {
                buf.push_str(&source[cursor..p.range.start]);
                cursor = p.range.end;
            }
            buf.push_str(&source[cursor..range.end]);
            crate::parser::normalize::collapse_whitespace(&buf)
        };

        let kind = self.classify(&stripped);
        Clause {
            range,
            sep,
            kind,
            parens,
            stripped,
        }
    }

    /// Classify a clause's paren-free text with the ordered [`CLASSIFIER`]
    /// table; [`ClauseKind::HeadCandidate`] when no rule matches.
    pub(crate) fn classify(&self, stripped: &str) -> ClauseKind {
        for rule in CLASSIFIER {
            if (rule.matches)(self, stripped) {
                return rule.kind;
            }
        }
        ClauseKind::HeadCandidate
    }
}

// --- Soft boundaries --------------------------------------------------------------

/// Find the soft boundaries inside a clause's text: word-boundary " or " /
/// " and/or " / " such as " / " to taste", and " for " when followed by a
/// gerund or "the" (see [`SoftBoundaryKind`]). Purely informational for the
/// splitter — soft boundaries do not split a clause; assembly consults them.
pub(crate) fn soft_boundaries(text: &str) -> Vec<SoftBoundary> {
    crate::lazy_regex!(SOFT, r"(?i)\s+(and/or|or|such\s+as|to\s+taste|for)(\s+|$)");
    let mut out = Vec::new();
    let mut cursor = 0usize;
    while let Some(m) = SOFT.captures_at(text, cursor) {
        let Some(whole) = m.get(0) else { break };
        let Some(word) = m.get(1) else { break };
        cursor = word.end();
        let keyword = word.as_str().to_lowercase();
        let kind = match keyword.as_str() {
            "and/or" => Some(SoftBoundaryKind::AndOr),
            "or" => Some(SoftBoundaryKind::Or),
            "to taste" => Some(SoftBoundaryKind::ToTaste),
            "for" => {
                // Only a purpose "for": followed by a gerund or "the".
                let next = text[whole.end()..].split_whitespace().next().unwrap_or("");
                (is_gerund(next) || next.eq_ignore_ascii_case("the"))
                    .then_some(SoftBoundaryKind::ForPurpose)
            }
            s if s.split_whitespace().collect::<Vec<_>>() == ["such", "as"] => {
                Some(SoftBoundaryKind::SuchAs)
            }
            s if s.split_whitespace().collect::<Vec<_>>() == ["to", "taste"] => {
                Some(SoftBoundaryKind::ToTaste)
            }
            _ => None,
        };
        if let Some(kind) = kind {
            out.push(SoftBoundary {
                at: whole.start(),
                kind,
            });
        }
    }
    out
}

// --- Segmented parse path -------------------------------------------------------

impl IngredientParser {
    /// Parse an ingredient line via the segmented path: the same leading
    /// amounts grammar as the legacy tail (`opt(measurement_list) → space0 →
    /// opt(bracketed_amounts) → space0`), then clause segmentation + assembly
    /// over the remaining text. Always consumes the whole line (mirroring the
    /// legacy grammar's `not_line_ending` tail).
    pub(crate) fn parse_ingredient_segmented<'a>(
        &self,
        input: &'a str,
    ) -> Res<&'a str, ParsedIngredient> {
        let mp = MeasurementParser::new(&self.units, MeasurementMode::IngredientList);
        let (rest, (primary, _, bracketed, _)) = (
            opt(|a| mp.parse_measurement_list(a)),
            space0,
            opt(|a| mp.parse_bracketed_amounts(a)),
            space0,
        )
            .parse(input)?;
        let amounts: Vec<Measure> = [primary, bracketed]
            .into_iter()
            .flatten()
            .flatten()
            .collect();
        let clauses = self.segmenter().segment(rest);
        Ok(("", self.assemble(rest, &clauses, amounts, &mp)))
    }

    /// Assemble the post-amount clauses into a [`ParsedIngredient`].
    ///
    /// The head carve is grammar-equivalent (name = the leading run of
    /// ingredient-text characters; an adjacent amounts parenthetical hoists;
    /// one following `", "` is consumed) so the segmented path is byte-faithful
    /// to the legacy tail wherever no structural repair applies. On top of
    /// that, a leading prep-chain (per the clause classification) resolves the
    /// real head noun *here* instead of leaving it to the
    /// `recover_head_noun_from_modifier` repair pass.
    fn assemble(
        &self,
        source: &str,
        clauses: &[Clause<'_>],
        mut amounts: Vec<Measure>,
        mp: &MeasurementParser<'_>,
    ) -> ParsedIngredient {
        // Head carve: the name is the longest leading run the legacy name
        // grammar would take (stops at ',', '(' and other punctuation).
        let name_end = parse_ingredient_text(source)
            .map(|(_, chunk)| chunk.len())
            .unwrap_or(0);
        let mut after = name_end;

        // The grammar's post-name parenthesized-amounts slot: a paren
        // immediately after the name text hoists when it parses as amounts.
        if source[after..].starts_with('(')
            && let Ok((rem, measures)) = mp.parse_parenthesized_amounts(&source[after..])
        {
            amounts.extend(measures);
            after = source.len() - rem.len();
        }
        // The grammar's single `opt(tag(", "))` before the modifier tail.
        if source[after..].starts_with(", ") {
            after += 2;
        }

        let name = source[..name_end].trim();
        let tail = source[after..].trim();

        // Leading prep-chain: the first clause is pure preparation tokens and
        // the head noun lives further right — resolve it now (the segmented
        // replacement for the `recover_head_noun_from_modifier` repair).
        if let Some(parsed) = self.assemble_prep_chain_head(name, tail, &amounts) {
            return parsed;
        }

        // Default assembly: name as carved; every remaining clause becomes a
        // modifier part in source order. `", "`-separated clauses are separate
        // parts (modifier_string re-joins them with `", "`, so the lowering is
        // byte-identical to the legacy single-raw tail); any other separator
        // ("; ", or a mid-clause carve point) is preserved verbatim by merging
        // into the previous part.
        let modifier = tail_parts(source, clauses, after)
            .into_iter()
            .map(ModifierPart::Raw)
            .collect();
        ParsedIngredient {
            name: name.to_string(),
            amounts,
            modifier,
            optional: false,
        }
    }

    /// Resolve a head noun that sits to the right of a leading preparation
    /// chain: `"deribbed, seeded, and roughly chopped fresh hot green chiles,
    /// such as serrano"` → name `"fresh hot green chiles"`, one `Prep` part
    /// `"deribbed, seeded, and roughly chopped"`, and the trailing clause as a
    /// `Raw` part. A faithful port of the legacy
    /// `recover_head_noun_from_modifier` pass, applied at assembly time.
    ///
    /// Gated exactly like the legacy pipeline reaches that pass:
    /// - the carved name must be a *pure* prep chain (every token a prep
    ///   token — connectors disqualify, as they did in the legacy name), and
    /// - the name must not be an exact known adjective phrase (the legacy
    ///   `fix_leading_prep_phrase` pass would have claimed it first), and
    /// - a head noun must exist in the tail whose first word is not a
    ///   modifier stopword.
    fn assemble_prep_chain_head(
        &self,
        name: &str,
        tail: &str,
        amounts: &[Measure],
    ) -> Option<ParsedIngredient> {
        if name.is_empty() || tail.is_empty() {
            return None;
        }
        if !name.split_whitespace().all(token::is_prep_token) {
            return None;
        }
        // An exact known adjective phrase is the legacy `fix_leading_prep_phrase`
        // shape — leave it to that pass so the swap semantics stay identical.
        if self.adjectives.contains(&name.to_lowercase()) {
            return None;
        }

        // Find the head noun: the first tail token that is neither a prep token
        // nor a connector.
        let head_start = token::offsets(tail)
            .find(|(_, w)| !token::is_prep_token(w) && !is_connector(w))
            .map(|(off, _)| off)?;
        let rest = &tail[head_start..];
        let first_word = rest.split_whitespace().next().unwrap_or("");
        if vocab::MODIFIER_STOPWORDS.contains(&token::norm(first_word).as_str()) {
            return None;
        }

        // The head noun runs to the next clause boundary.
        let mut end = rest.len();
        for pat in vocab::CLAUSE_BOUNDARIES {
            if let Some(p) = rest.find(pat) {
                end = end.min(p);
            }
        }
        let head_noun = rest[..end].trim();
        if head_noun.is_empty() {
            return None;
        }
        let trailing = rest[end..]
            .trim_start_matches(|c: char| c == ',' || c.is_whitespace())
            .trim();

        let consumed = tail[..head_start].trim().trim_end_matches(',').trim();
        let prep = if consumed.is_empty() {
            name.to_string()
        } else {
            format!("{name}, {consumed}")
        };

        let mut modifier = vec![ModifierPart::Prep(prep)];
        if !trailing.is_empty() {
            modifier.push(ModifierPart::Raw(trailing.to_string()));
        }
        Some(ParsedIngredient {
            name: head_noun.to_string(),
            amounts: amounts.to_vec(),
            modifier,
            optional: false,
        })
    }
}

/// Split the modifier tail (everything from byte `from` on) into one string per
/// `", "`-separated clause, preserving any *other* separator ("; ", or the
/// bytes between a mid-clause carve point and the next clause) verbatim inside
/// a part. Joining the returned parts with `", "` reproduces the source tail
/// byte-for-byte (modulo end-trimming), which keeps the lowered modifier string
/// identical to the legacy single-raw capture.
fn tail_parts(source: &str, clauses: &[Clause<'_>], from: usize) -> Vec<String> {
    let mut parts: Vec<String> = Vec::new();
    for clause in clauses {
        if clause.range.end <= from {
            continue;
        }
        if parts.is_empty() {
            // First part: include everything from the carve point (which may
            // sit mid-clause, or on separator bytes the carve did not consume).
            parts.push(source[from..clause.range.end].to_string());
        } else if clause.sep == ", " {
            parts.push(source[clause.range.clone()].to_string());
        } else if let Some(prev) = parts.last_mut() {
            // Non-comma separator: preserve it verbatim inside the part.
            prev.push_str(clause.sep);
            prev.push_str(&source[clause.range.clone()]);
        }
    }
    parts.retain(|p| !p.trim().is_empty());
    parts
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    fn parser() -> IngredientParser {
        IngredientParser::new()
    }

    fn segment_texts(source: &str) -> Vec<String> {
        let parser = parser();
        parser
            .segmenter()
            .segment(source)
            .iter()
            .map(|c| c.text(source).trim().to_string())
            .collect()
    }

    fn segment_kinds(source: &str) -> Vec<(String, ClauseKind)> {
        let parser = parser();
        parser
            .segmenter()
            .segment(source)
            .iter()
            .map(|c| (c.stripped.clone(), c.kind))
            .collect()
    }

    // ── splitting ───────────────────────────────────────────────────────────

    #[rstest]
    // Simple comma split.
    #[case("flour, sifted", &["flour", "sifted"])]
    // Semicolon splits too.
    #[case("flour; sifted", &["flour", "sifted"])]
    // A comma inside a parenthetical never splits.
    #[case("chicken thighs (8 to 12 thighs, trimmed), halved", &["chicken thighs (8 to 12 thighs, trimmed)", "halved"])]
    // A bare comma without a trailing space does not split (mirrors the legacy
    // grammar's `opt(tag(", "))`).
    #[case("1,000 grams flour", &["1,000 grams flour"])]
    // Multiple clauses keep source order.
    #[case("deribbed, seeded, and roughly chopped fresh hot green chiles, such as serrano",
           &["deribbed", "seeded", "and roughly chopped fresh hot green chiles", "such as serrano"])]
    fn splits_at_top_level_boundaries(#[case] source: &str, #[case] expected: &[&str]) {
        assert_eq!(segment_texts(source), expected, "source: {source:?}");
    }

    #[test]
    fn clause_ranges_index_into_source() {
        let source = "purple (red) cabbage (about 1 pound), cored";
        let p = parser();
        let clauses = p.segmenter().segment(source);
        assert_eq!(clauses.len(), 2);
        for c in &clauses {
            // Range slices back to the text.
            assert_eq!(&source[c.range.clone()], c.text(source));
            for pc in &c.parens {
                assert_eq!(&source[pc.range.clone()], format!("({})", pc.inner));
            }
        }
        // First clause carries both parens, classified.
        assert_eq!(clauses[0].parens.len(), 2);
        assert_eq!(clauses[0].parens[0].kind, ParenKind::Alias);
        assert_eq!(clauses[0].parens[1].kind, ParenKind::Amount);
        // Stripped text excises the parens.
        assert_eq!(clauses[0].stripped, "purple cabbage");
        // Separators are recorded on the following clause.
        assert_eq!(clauses[0].sep, "");
        assert_eq!(clauses[1].sep, ", ");
    }

    // ── classification ─────────────────────────────────────────────────────

    #[rstest]
    // PrepChain: pure participle/descriptor chains, connectors allowed between.
    #[case("deribbed", ClauseKind::PrepChain)]
    #[case("seeded", ClauseKind::PrepChain)]
    #[case("bone-in", ClauseKind::PrepChain)]
    #[case("skin-on", ClauseKind::PrepChain)]
    #[case("peeled and deveined", ClauseKind::PrepChain)]
    #[case("very finely chopped", ClauseKind::PrepChain)]
    // KnownPrepPhrase: exact vocab phrases that aren't pure -ed/-ly chains.
    #[case("to taste", ClauseKind::KnownPrepPhrase)]
    #[case("for garnish", ClauseKind::KnownPrepPhrase)]
    #[case("fresh", ClauseKind::KnownPrepPhrase)]
    // MinusMeasure: subtractive amount clause.
    #[case("minus 1 tablespoon flour", ClauseKind::MinusMeasure)]
    #[case("minus 1 tablespoon", ClauseKind::MinusMeasure)]
    // Purpose: "for <gerund>" / "for the …" (but fixed vocab phrases like
    // "for garnish" classify as KnownPrepPhrase above).
    #[case("for brushing the bread", ClauseKind::Purpose)]
    #[case("for the pans", ClauseKind::Purpose)]
    // Alternative: an or-led clause.
    #[case("or melted coconut oil", ClauseKind::Alternative)]
    #[case("and/or rosemary", ClauseKind::Alternative)]
    // Prose: stopword-led clause.
    #[case("such as serrano", ClauseKind::Prose)]
    #[case("then drained", ClauseKind::Prose)]
    #[case("plus more for serving", ClauseKind::Prose)]
    // HeadCandidate: everything else.
    #[case("fresh hot green chiles", ClauseKind::HeadCandidate)]
    #[case("toasted walnuts", ClauseKind::HeadCandidate)]
    #[case("flour", ClauseKind::HeadCandidate)]
    #[case(
        "and roughly chopped fresh hot green chiles",
        ClauseKind::HeadCandidate
    )]
    // "minus" without a parseable measurement is not MinusMeasure (and "minus"
    // is not a stopword, so it stays a head candidate).
    #[case("minus the seeds", ClauseKind::HeadCandidate)]
    // "for bread" is not a purpose clause (no gerund, no article).
    #[case("for bread", ClauseKind::Prose)]
    fn classifies_clause_kinds(#[case] text: &str, #[case] expected: ClauseKind) {
        let p = parser();
        assert_eq!(p.segmenter().classify(text), expected, "text: {text:?}");
    }

    /// Classification of whole witness lines' post-amount text (the
    /// ORDER_CONSTRAINTS witnesses, post-amount).
    #[rstest]
    #[case("chopped, toasted walnuts",
           &[("chopped", ClauseKind::PrepChain), ("toasted walnuts", ClauseKind::HeadCandidate)])]
    #[case("deribbed, seeded, and roughly chopped fresh hot green chiles, such as serrano",
           &[("deribbed", ClauseKind::PrepChain),
             ("seeded", ClauseKind::PrepChain),
             ("and roughly chopped fresh hot green chiles", ClauseKind::HeadCandidate),
             ("such as serrano", ClauseKind::Prose)])]
    #[case("chopped red or white onion",
           &[("chopped red or white onion", ClauseKind::HeadCandidate)])]
    #[case("canola, vegetable, or melted coconut oil",
           &[("canola", ClauseKind::HeadCandidate),
             ("vegetable", ClauseKind::HeadCandidate),
             ("or melted coconut oil", ClauseKind::Alternative)])]
    #[case("minus 1 tablespoon flour",
           &[("minus 1 tablespoon flour", ClauseKind::MinusMeasure)])]
    #[case("finely chopped, raw pistachios",
           &[("finely chopped", ClauseKind::PrepChain), ("raw pistachios", ClauseKind::HeadCandidate)])]
    fn classifies_witness_lines(#[case] source: &str, #[case] expected: &[(&str, ClauseKind)]) {
        let got = segment_kinds(source);
        let want: Vec<(String, ClauseKind)> =
            expected.iter().map(|(t, k)| (t.to_string(), *k)).collect();
        assert_eq!(got, want, "source: {source:?}");
    }

    // ── segmented path vs legacy path ───────────────────────────────────────

    /// The segmented path must produce the same public result as the legacy
    /// path (the corpus-wide check lives in the food-cli `corpus shadow`
    /// harness; these are the in-crate witnesses, including every
    /// ORDER_CONSTRAINTS line).
    #[rstest]
    #[case("2 cups flour")]
    #[case("1 cup flour, sifted")]
    #[case("salt")]
    #[case("2 cups chopped, toasted walnuts")]
    #[case("1/2 cup deribbed, seeded, and roughly chopped fresh hot green chiles, such as serrano")]
    #[case("2 cups spinach chopped into ribbons")]
    #[case("1 teaspoon grated or finely chopped lemon zest")]
    #[case("chopped red or white onion")]
    #[case("chopped parsley for garnish for brushing the bread")]
    #[case("½ cup minus 1 tablespoon flour")]
    #[case("1 medium purple (red) cabbage (about 1 pound)")]
    #[case("1 cup canola, vegetable, or melted coconut oil")]
    #[case("3 tomatoes (about 2 cups), diced")]
    #[case("2 boneless, skinless chicken thighs")]
    #[case("bone-in, skin-on chicken legs")]
    #[case("1 pound feta (crumbled)")]
    #[case("salt and pepper to taste")]
    #[case("1 garlic clove, minced")]
    #[case("3 medium carrots")]
    #[case("Juice of 1 lemon")]
    #[case("(1 cup walnuts, toasted)")]
    #[case("Butter — 2 tablespoons")]
    #[case("1,000 grams (about 6 cups) quartered and pitted nectarines")]
    #[case("2/3 cup (85 grams) finely chopped, raw pistachios")]
    #[case("")]
    fn segmented_matches_legacy(#[case] line: &str) {
        let legacy = IngredientParser::new();
        let segmented =
            IngredientParser::new().with_segmentation_mode(crate::SegmentationMode::Segmented);
        assert_eq!(
            segmented.from_str(line),
            legacy.from_str(line),
            "segmented != legacy for {line:?}"
        );
    }

    /// Segmented mode preserves the from_str infallibility invariant: never a
    /// panic, and a name-only fallback rather than an empty name with leftover
    /// modifier text.
    #[rstest]
    #[case("!!! ???")]
    #[case(", chopped")]
    #[case("(")]
    #[case("))((")]
    fn segmented_never_panics_or_strands_name(#[case] line: &str) {
        let segmented =
            IngredientParser::new().with_segmentation_mode(crate::SegmentationMode::Segmented);
        let ing = segmented.from_str(line);
        let legacy = IngredientParser::new().from_str(line);
        assert_eq!(ing, legacy, "segmented != legacy for {line:?}");
    }

    // ── soft boundaries ─────────────────────────────────────────────────────

    #[rstest]
    #[case("red or white onion", &[(3, SoftBoundaryKind::Or)])]
    #[case("thyme and/or rosemary", &[(5, SoftBoundaryKind::AndOr)])]
    #[case("chiles such as serrano", &[(6, SoftBoundaryKind::SuchAs)])]
    #[case("salt to taste", &[(4, SoftBoundaryKind::ToTaste)])]
    #[case("olive oil for brushing the bread", &[(9, SoftBoundaryKind::ForPurpose)])]
    #[case("butter for the pans", &[(6, SoftBoundaryKind::ForPurpose)])]
    // "for" without a gerund/article is not a purpose boundary.
    #[case("flour for bread", &[])]
    // No boundary at all.
    #[case("plain flour", &[])]
    // Multiple boundaries report in order.
    #[case("red or white onion for serving",
           &[(3, SoftBoundaryKind::Or), (18, SoftBoundaryKind::ForPurpose)])]
    fn finds_soft_boundaries(#[case] text: &str, #[case] expected: &[(usize, SoftBoundaryKind)]) {
        let got: Vec<(usize, SoftBoundaryKind)> = soft_boundaries(text)
            .into_iter()
            .map(|b| (b.at, b.kind))
            .collect();
        let want: Vec<(usize, SoftBoundaryKind)> = expected.to_vec();
        assert_eq!(got, want, "text: {text:?}");
    }
}
