//! Clause segmentation — the default post-amount pipeline stage: split the
//! post-amount text into clauses, classify each one, and assemble the parsed
//! ingredient.
//!
//! The legacy grammar tail carved the post-amount text at the *first* `", "`
//! (the name grammar cannot cross `','` or `'('`) and left everything after it
//! as one opaque modifier string, which a family of refine passes then
//! repaired (recover a head noun stranded behind a prep chain, re-attach an
//! alias parenthetical, graft a shared head off an alternatives list, hoist a
//! measurement parenthetical…). This module replaced that carve-then-repair
//! design with an explicit *segmentation*: the post-amount text is split into
//! clauses at every top-level `", "` / `"; "` boundary, each top-level
//! parenthetical becomes its own sub-clause attached to its host clause, and
//! every clause is classified by an ordered rule table ([`CLASSIFIER`]) so
//! assembly and `--explain` can reason about the line's structure directly.
//! Assembly then runs the ordered clause-structure repairs
//! ([`ASSEMBLY_REPAIRS`]) that used to live at the tail of the refine
//! pipeline — refine now only works *inside* the name.
//!
//! Splitting and classification are pure over the source text; byte ranges into
//! the source are preserved on every clause, and the decomposition view's
//! field spans derive from them ([`IngredientParser::segmented_field_spans`]).
//!
//! The legacy path survives behind [`SegmentationMode::Legacy`] purely as the
//! `food-cli corpus shadow` A/B baseline.

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
///
/// Not consumed by production assembly yet: the name-internal "or"/purpose
/// handling stayed with the kept refine passes at cutover. This detector (unit
/// tested below) is the seed for absorbing `extract_alternatives_from_name` /
/// `extract_purpose_gerund` into clause-native logic — a follow-up.
#[allow(dead_code)]
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
#[allow(dead_code)]
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
    /// refine passes repair the damage. Kept as the A/B baseline for the
    /// `corpus shadow` harness.
    Legacy,
    /// The clause-segmentation path (the default): amounts grammar +
    /// [`Segmenter::segment`] + assembly, followed by the refine passes.
    #[default]
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
            b',' | b';' if depth == 0 && bytes.get(i + 1) == Some(&b' ') => {
                out.push((i, 2));
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
/// gerund or "the" (see [`SoftBoundaryKind`]). Purely informational — soft
/// boundaries do not split a clause. (See the note on [`SoftBoundaryKind`]:
/// unit-tested seed for a follow-up, not yet consumed by assembly.)
#[allow(dead_code)]
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

/// The result of the grammar-equivalent head carve over the post-amount text:
/// the name span, an optional hoisted name-adjacent amounts parenthetical, and
/// where the modifier tail begins.
struct Carve {
    /// End of the name run (byte offset into the post-amount source).
    name_end: usize,
    /// Byte range of the hoisted name-adjacent amounts parenthetical (if any),
    /// plus its measures.
    hoisted: Option<(Range<usize>, Vec<Measure>)>,
    /// Where the modifier tail starts (after the paren and one `", "`).
    tail_from: usize,
}

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
        trace_clauses(rest, &clauses);
        let parsed = self.assemble(rest, &clauses, amounts, &mp)?;
        Ok(("", parsed))
    }

    /// The grammar-equivalent head carve: name = the leading run of
    /// ingredient-text characters (stops at ',', '(' and other punctuation);
    /// a paren immediately after the name hoists when it parses as amounts;
    /// one following `", "` is consumed.
    ///
    /// A recoverable paren parse error falls through (the paren stays modifier
    /// text); a nom `Failure` propagates, exactly as the legacy grammar's
    /// `opt(...)` slot behaved — the whole parse then falls back name-only.
    fn carve<'a>(
        &self,
        source: &'a str,
        mp: &MeasurementParser<'_>,
    ) -> Result<Carve, nom::Err<nom_language::error::VerboseError<&'a str>>> {
        let name_end = parse_ingredient_text(source)
            .map(|(_, chunk)| chunk.len())
            .unwrap_or(0);
        let mut after = name_end;

        let mut hoisted = None;
        if source[after..].starts_with('(') {
            match mp.parse_parenthesized_amounts(&source[after..]) {
                Ok((rem, measures)) => {
                    let end = source.len() - rem.len();
                    hoisted = Some((after..end, measures));
                    after = end;
                }
                Err(err @ nom::Err::Failure(_)) | Err(err @ nom::Err::Incomplete(_)) => {
                    return Err(err);
                }
                Err(nom::Err::Error(_)) => {}
            }
        }
        if source[after..].starts_with(", ") {
            after += 2;
        }
        Ok(Carve {
            name_end,
            hoisted,
            tail_from: after,
        })
    }

    /// Assemble the post-amount clauses into a [`ParsedIngredient`].
    ///
    /// The head carve is grammar-equivalent (see [`Self::carve`]) so the
    /// segmented path is byte-faithful to the legacy tail wherever no
    /// structural repair applies.
    fn assemble<'a>(
        &self,
        source: &'a str,
        clauses: &[Clause<'_>],
        mut amounts: Vec<Measure>,
        mp: &MeasurementParser<'_>,
    ) -> Result<ParsedIngredient, nom::Err<nom_language::error::VerboseError<&'a str>>> {
        let carve = self.carve(source, mp)?;
        if let Some((_, measures)) = &carve.hoisted {
            amounts.extend(measures.iter().cloned());
        }
        let after = carve.tail_from;

        let name = source[..carve.name_end].trim();
        let tail = &source[after..];

        // Two repair passes scan the *whole* first raw modifier part with
        // comma-crossing string searches, so their trigger shapes must reach
        // them as one part (splitting would change what they recover):
        // - a pure-prep-chain name triggers `recover_head_noun_from_modifier`
        //   (its head scan skips across `", "`);
        // - a paren-led tail triggers `recover_parenthetical_alias_from_modifier`
        //   (its `find(" (")` head cut crosses `", "` too).
        let keep_tail_whole = tail.trim_start().starts_with('(')
            || (!name.is_empty() && name.split_whitespace().all(token::is_prep_token));

        // Otherwise: every remaining clause becomes a modifier part in source
        // order. `", "`-separated clauses are separate parts (modifier_string
        // re-joins them with `", "`, so the lowering is byte-identical to the
        // legacy single-raw tail); any other separator ("; ", or a mid-clause
        // carve point) is preserved verbatim by merging into the previous part.
        let modifier = if keep_tail_whole {
            if tail.trim().is_empty() {
                Vec::new()
            } else {
                vec![ModifierPart::Raw(tail.to_string())]
            }
        } else {
            tail_part_ranges(source, clauses, after)
                .into_iter()
                .map(|r| ModifierPart::Raw(source[r].to_string()))
                .collect()
        };
        let mut parsed = ParsedIngredient {
            name: name.to_string(),
            amounts,
            modifier,
            optional: false,
        };
        self.run_assembly_repairs(&mut parsed);
        Ok(parsed)
    }

    /// Run the ordered clause-structure repairs on the freshly assembled IR
    /// (see [`ASSEMBLY_REPAIRS`]). Mirrors `run_refine_pass`'s
    /// trace-on-change so `--explain` shows which repairs fired.
    fn run_assembly_repairs(&self, parsed: &mut ParsedIngredient) {
        for (label, repair) in ASSEMBLY_REPAIRS {
            if !crate::trace::is_tracing_enabled() {
                repair(self, parsed);
                continue;
            }
            let before = parsed.clone();
            repair(self, parsed);
            crate::trace::trace_on_change(
                label,
                &before.name,
                &format!(
                    "{} | {}",
                    parsed.name,
                    parsed.modifier_string().as_deref().unwrap_or("-")
                ),
                *parsed != before,
            );
        }
    }

    /// Decomposition field spans for the segmented path — the clause-derived
    /// replacement for the legacy `consumed`-wrapper span capture. Spans index
    /// into `input` (the normalized, optional-stripped line): the leading
    /// amounts region and a hoisted name-adjacent amounts parenthetical are
    /// [`Field::Amount`](crate::Field::Amount) spans, the carved name a
    /// [`Field::Name`](crate::Field::Name) span, and each modifier part its
    /// own [`Field::Modifier`](crate::Field::Modifier) span (a multi-clause
    /// modifier yields multiple spans). Empty when the parse fails.
    pub(crate) fn segmented_field_spans(&self, input: &str) -> Vec<crate::FieldSpan> {
        use crate::{Field, FieldSpan};

        let mp = MeasurementParser::new(&self.units, MeasurementMode::IngredientList);
        let Ok((rest, _)) = (
            opt(|a| mp.parse_measurement_list(a)),
            space0,
            opt(|a| mp.parse_bracketed_amounts(a)),
            space0,
        )
            .parse(input)
        else {
            return Vec::new();
        };
        let Ok(carve) = self.carve(rest, &mp) else {
            return Vec::new();
        };

        // `rest` is a suffix slice of `input`; offset clause/carve ranges by it.
        let base = rest.as_ptr() as usize - input.as_ptr() as usize;
        let span_of = |range: Range<usize>, field: Field| -> Option<FieldSpan> {
            let slice = &input[range.clone()];
            let trimmed = slice.trim();
            if trimmed.is_empty() {
                return None;
            }
            let start = range.start + (trimmed.as_ptr() as usize - slice.as_ptr() as usize);
            Some(FieldSpan {
                field,
                range: start..start + trimmed.len(),
                text: trimmed.to_string(),
            })
        };

        let mut spans = Vec::new();
        // Leading amounts region (primary + bracketed as one span).
        spans.extend(span_of(0..base, Field::Amount));
        // Carved name.
        spans.extend(span_of(base..base + carve.name_end, Field::Name));
        // Hoisted name-adjacent amounts parenthetical.
        if let Some((range, _)) = &carve.hoisted {
            spans.extend(span_of(base + range.start..base + range.end, Field::Amount));
        }
        // Modifier: one span per assembled part (mirrors `assemble`'s
        // keep-whole / split decision so spans match the parts).
        let name = rest[..carve.name_end].trim();
        let tail = &rest[carve.tail_from..];
        let keep_tail_whole = tail.trim_start().starts_with('(')
            || (!name.is_empty() && name.split_whitespace().all(token::is_prep_token));
        if keep_tail_whole {
            spans.extend(span_of(
                base + carve.tail_from..input.len(),
                Field::Modifier,
            ));
        } else {
            let clauses = self.segmenter().segment(rest);
            for r in tail_part_ranges(rest, &clauses, carve.tail_from) {
                spans.extend(span_of(base + r.start..base + r.end, Field::Modifier));
            }
        }
        spans
    }
}

/// Emit one trace node per clause decision (and one per attached
/// parenthetical), so `--explain` and the stage report can show how the
/// segmenter read the line. No-ops when tracing is disabled.
fn trace_clauses(source: &str, clauses: &[Clause<'_>]) {
    if !crate::trace::is_tracing_enabled() {
        return;
    }
    for clause in clauses {
        let text = clause.text(source).trim();
        if text.is_empty() {
            continue;
        }
        crate::trace::trace_on_change(clause.kind.as_str(), text, &clause.stripped, true);
        for paren in &clause.parens {
            crate::trace::trace_on_change(
                ClauseKind::Parenthetical(paren.kind).as_str(),
                &format!("({})", paren.inner),
                paren_kind_label(paren.kind),
                true,
            );
        }
    }
}

/// Stable lowercase label for a [`ParenKind`] (trace preview text).
fn paren_kind_label(kind: ParenKind) -> &'static str {
    match kind {
        ParenKind::CrossReference => "cross_reference",
        ParenKind::NoteReference => "note_reference",
        ParenKind::MinusEquivalence => "minus_equivalence",
        ParenKind::Optional => "optional",
        ParenKind::Descriptive => "descriptive",
        ParenKind::Amount => "amount",
        ParenKind::Alias => "alias",
        ParenKind::Other => "other",
    }
}

/// A clause-structure repair applied at assembly time.
type Repair = fn(&IngredientParser, &mut ParsedIngredient);

/// The ordered clause-structure repairs the segmentation stage owns — the
/// carve-then-repair passes the cutover removed from `REFINE_PIPELINE`. The
/// functions still live in `refine::{recover, alternatives, amounts}` (their
/// guards and unit tests are unchanged); only the caller moved: they now run
/// once at assembly, before the name-internal refine passes, in the same
/// relative order they held in the old pipeline.
const ASSEMBLY_REPAIRS: &[(&str, Repair)] = &[
    (
        "fix_leading_prep_phrase",
        IngredientParser::fix_leading_prep_phrase,
    ),
    (
        "fix_leading_minus_clause",
        IngredientParser::fix_leading_minus_clause,
    ),
    (
        "recover_head_noun_from_modifier",
        IngredientParser::recover_head_noun_from_modifier,
    ),
    (
        "recover_parenthetical_alias_from_modifier",
        IngredientParser::recover_parenthetical_alias_from_modifier,
    ),
    (
        "recover_shared_head_from_alternatives",
        IngredientParser::recover_shared_head_from_alternatives,
    ),
    (
        "extract_secondary_amounts_from_modifier",
        IngredientParser::extract_secondary_amounts_from_modifier,
    ),
];

/// Every label the `segment` stage can emit in a trace — the clause-kind
/// decisions (classifier order) followed by the assembly repairs — the
/// stage's label universe for tooling (mirrors the per-stage `*_TRACE_NAMES`
/// slices).
pub(crate) const SEGMENT_TRACE_NAMES: &[&str] = &[
    "prep_chain",
    "known_prep_phrase",
    "minus_measure",
    "purpose",
    "alternative",
    "parenthetical",
    "prose",
    "head_candidate",
    "fix_leading_prep_phrase",
    "fix_leading_minus_clause",
    "recover_head_noun_from_modifier",
    "recover_parenthetical_alias_from_modifier",
    "recover_shared_head_from_alternatives",
    "extract_secondary_amounts_from_modifier",
];

/// Split the modifier tail (everything from byte `from` on) into one byte range
/// per *cleanly separable* `", "`-separated clause. The lowering contract is
/// strict: `modifier_string` re-joins parts with `", "` (or `" ("` before a
/// parenthesized part) and strips each part's leading commas, so a clause is
/// only emitted as its own range when that join reproduces the source verbatim —
/// it must follow a `", "` separator and its text must be non-empty and not
/// start with whitespace, `'('`, or `','` (nor sit after trailing whitespace).
/// Everything else ("; " separators, empty clauses, paren-led or comma-led
/// clauses) is preserved byte-for-byte by extending the previous range across
/// the separator, which keeps the lowered modifier string identical to the
/// legacy single-raw capture. Ranges are contiguous slices of `source`.
fn tail_part_ranges(source: &str, clauses: &[Clause<'_>], from: usize) -> Vec<Range<usize>> {
    let separable = |clause: &Clause<'_>| {
        if clause.sep != ", " {
            return false;
        }
        let raw = &source[clause.range.clone()];
        // The join must be reversible under modifier_string's per-part
        // trimming: no whitespace abutting the separator on either side
        // ("x , y" must survive verbatim), and the part must not begin with a
        // '(' (joined with " " instead of ", ") or a ',' (stripped as a stray
        // grammar artifact).
        if raw.is_empty()
            || raw.starts_with(|c: char| c.is_whitespace())
            || raw.starts_with('(')
            || raw.starts_with(',')
        {
            return false;
        }
        let sep_start = clause.range.start - clause.sep.len();
        !source[..sep_start]
            .chars()
            .next_back()
            .is_some_and(char::is_whitespace)
    };
    let mut parts: Vec<Range<usize>> = Vec::new();
    for clause in clauses {
        if clause.range.end <= from {
            continue;
        }
        match parts.last_mut() {
            // First part: include everything from the carve point (which may
            // sit mid-clause, or on separator bytes the carve did not consume).
            None => parts.push(from..clause.range.end),
            Some(_) if separable(clause) => {
                parts.push(clause.range.clone());
            }
            // Not cleanly separable: extend the previous range across the
            // separator bytes, preserving them verbatim.
            Some(prev) => {
                prev.end = clause.range.end;
            }
        }
    }
    parts.retain(|r| !source[r.clone()].trim().is_empty());
    parts
}

#[cfg(test)]
#[allow(clippy::expect_used)]
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

    // ── segmented assembly (positive control) ───────────────────────────────

    /// The segmented path must genuinely run the segmenter: a multi-clause
    /// tail assembles into one modifier part per clause (the legacy grammar
    /// captures a single raw string). This pins the assembled IR itself
    /// (pre-refine), proving the mode plumbing exercises the segmenter rather
    /// than silently reproducing the legacy carve.
    #[test]
    fn assembly_splits_tail_into_clause_parts() {
        let p = IngredientParser::new();
        let (_, parsed) = p
            .parse_ingredient_segmented("1 cup flour, sifted, divided")
            .expect("segmented parse");
        assert_eq!(parsed.name, "flour");
        assert_eq!(
            parsed.modifier,
            vec![
                ModifierPart::Raw("sifted".to_string()),
                ModifierPart::Raw("divided".to_string()),
            ]
        );
        // The legacy grammar captures the same tail as ONE raw part.
        let (_, legacy) = p
            .parse_ingredient("1 cup flour, sifted, divided")
            .expect("legacy parse");
        assert_eq!(
            legacy.modifier,
            vec![ModifierPart::Raw("sifted, divided".to_string())]
        );
    }

    /// The clause-structure repairs run at assembly time: the assembled IR
    /// (pre-refine) already has the head noun recovered from a leading prep
    /// chain, and an alias parenthetical re-attached to the name.
    #[test]
    fn assembly_repairs_resolve_clause_structure() {
        let p = IngredientParser::new();

        // Prep-chain head recovery (was recover_head_noun_from_modifier).
        let (_, parsed) = p
            .parse_ingredient_segmented(
                "1/2 cup deribbed, seeded, and roughly chopped fresh hot green chiles, such as serrano",
            )
            .expect("segmented parse");
        assert_eq!(parsed.name, "fresh hot green chiles");
        assert_eq!(
            parsed.modifier,
            vec![
                ModifierPart::Prep("deribbed, seeded, and roughly chopped".to_string()),
                ModifierPart::Raw("such as serrano".to_string()),
            ]
        );

        // Alias re-attachment + secondary-amount hoist (was
        // recover_parenthetical_alias_from_modifier +
        // extract_secondary_amounts_from_modifier). "medium" stays in the
        // assembled name; extract_size_unit_from_name claims it in refine.
        let (_, parsed) = p
            .parse_ingredient_segmented("1 medium purple (red) cabbage (about 1 pound), cored")
            .expect("segmented parse");
        assert_eq!(parsed.name, "medium purple (red) cabbage");
        // The hoist leaves the stray leading comma in the raw part; the
        // lowering strips it (same as the old pipeline pass did).
        assert_eq!(parsed.modifier_string().as_deref(), Some("cored"));
        assert_eq!(parsed.amounts.len(), 2, "pound paren hoisted");
    }

    /// Grammar-equivalent head carve: adjacent amounts parenthetical hoists at
    /// assembly time and the following ", " is consumed.
    #[test]
    fn assembly_hoists_name_adjacent_amount_paren() {
        let p = IngredientParser::new();
        let (_, parsed) = p
            .parse_ingredient_segmented("3 tomatoes (about 2 cups), diced")
            .expect("segmented parse");
        assert_eq!(parsed.name, "tomatoes");
        assert_eq!(parsed.amounts.len(), 2, "paren amounts hoisted");
        assert_eq!(
            parsed.modifier,
            vec![ModifierPart::Raw("diced".to_string())]
        );
    }

    // ── segmented default: end-to-end witnesses ─────────────────────────────

    /// End-to-end witnesses for the segmented default, pinned to the exact
    /// pre-cutover outputs (the migration converged to zero divergences before
    /// the legacy repair passes were absorbed into assembly, so these are the
    /// historical parses, now produced by the segmentation stage). Includes
    /// every shape the absorbed repairs own: prep-chain head recovery, the
    /// leading prep-phrase swap, the minus-clause split, alias re-attachment,
    /// the shared-head graft, and the secondary-amount hoist.
    #[rstest]
    #[case("2 cups flour", "flour", None)]
    #[case("1 cup flour, sifted", "flour", Some("sifted"))]
    #[case("salt", "salt", None)]
    #[case("2 cups chopped, toasted walnuts", "toasted walnuts", Some("chopped"))]
    #[case(
        "1/2 cup deribbed, seeded, and roughly chopped fresh hot green chiles, such as serrano",
        "hot green chiles",
        Some("fresh, deribbed, seeded, and roughly chopped, such as serrano")
    )]
    #[case(
        "2 cups spinach chopped into ribbons",
        "spinach",
        Some("chopped into ribbons")
    )]
    #[case(
        "1 teaspoon grated or finely chopped lemon zest",
        "lemon zest",
        Some("grated or finely chopped")
    )]
    #[case(
        "chopped red or white onion",
        "red onion",
        Some("chopped, or white onion")
    )]
    #[case(
        "chopped parsley for garnish for brushing the bread",
        "parsley",
        Some("chopped, for garnish, for brushing the bread")
    )]
    #[case("½ cup minus 1 tablespoon flour", "flour", Some("minus 1 tablespoon"))]
    #[case(
        "1 medium purple (red) cabbage (about 1 pound)",
        "purple (red) cabbage",
        None
    )]
    #[case(
        "1 cup canola, vegetable, or melted coconut oil",
        "canola oil",
        Some("or vegetable, or melted coconut oil")
    )]
    #[case("3 tomatoes (about 2 cups), diced", "tomatoes", Some("diced"))]
    #[case(
        "2 boneless, skinless chicken thighs",
        "chicken thighs",
        Some("boneless, skinless")
    )]
    #[case(
        "bone-in, skin-on chicken legs",
        "chicken legs",
        Some("bone-in, skin-on")
    )]
    #[case("1 pound feta (crumbled)", "feta", Some("crumbled"))]
    #[case("salt and pepper to taste", "salt and pepper", Some("to taste"))]
    #[case("1 garlic clove, minced", "garlic", Some("minced"))]
    #[case("3 medium carrots", "carrots", None)]
    #[case("Juice of 1 lemon", "lemon", Some("juice of"))]
    #[case("(1 cup walnuts, toasted)", "walnuts", Some("toasted"))]
    #[case("Butter — 2 tablespoons", "Butter", None)]
    #[case(
        "1,000 grams (about 6 cups) quartered and pitted nectarines",
        "quartered and pitted nectarines",
        None
    )]
    #[case(
        "2/3 cup (85 grams) finely chopped, raw pistachios",
        "raw pistachios",
        Some("finely chopped")
    )]
    #[case("", "", None)]
    fn segmented_default_witnesses(
        #[case] line: &str,
        #[case] want_name: &str,
        #[case] want_modifier: Option<&str>,
    ) {
        let ing = IngredientParser::new().from_str(line);
        assert_eq!(ing.name, want_name, "name for {line:?}");
        assert_eq!(
            ing.modifier.as_deref(),
            want_modifier,
            "modifier for {line:?}"
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
        let has_modifier = ing
            .modifier
            .as_deref()
            .is_some_and(|m| !m.trim().is_empty());
        assert!(
            !(ing.name.trim().is_empty() && has_modifier),
            "stranded name for {line:?}: {ing:?}"
        );
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
