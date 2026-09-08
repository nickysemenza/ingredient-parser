//! Source-bearing structural resolution of ingredient clauses.
//!
//! Split and classify before assigning fields. A leading preparation clause
//! cannot become the name merely because it precedes the first comma, and an
//! inline parenthetical cannot strand the rest of a name in a modifier. Dimension
//! tokens are masked in place so the measurement grammar sees only ingredient
//! measures while all attribution retains its original byte coordinates.

use std::ops::Range;

use nom::Parser as _;
use nom::character::complete::space0;
use nom::combinator::opt;

use crate::IngredientParser;
use crate::parser::ir::{ModifierKind, ModifierPart, ParsedIngredient, SourceText};
use crate::parser::normalize::collapse_whitespace;
use crate::parser::paren::{self, ParenKind};
use crate::parser::token;
use crate::parser::vocab;
use crate::parser::{MeasurementMode, MeasurementParser, Res};
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
    /// Parsed once during classification and consumed by structural assembly.
    pub measures: Vec<Measure>,
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
/// kind). Parenthetical classifications carry their own measure payloads and
/// are resolved separately.
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

/// A subtractive quantity clause. The measure must parse; structural
/// assembly determines whether food-bearing text follows it.
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
    let mut words = trimmed.split_whitespace();
    let lead = words.next().unwrap_or("");
    let next = words.next().unwrap_or("");
    (lead.eq_ignore_ascii_case("or") || lead.eq_ignore_ascii_case("and/or"))
        && !["more", "less", "as", "to", "if"]
            .iter()
            .any(|word| next.eq_ignore_ascii_case(word))
}

/// Prose: the first word is a modifier stopword ("such as serrano",
/// "then drained", "plus more for serving") — the same test the recover passes
/// used to tell a prose modifier from a head noun.
fn is_prose(_seg: &Segmenter<'_>, text: &str) -> bool {
    let Some(first) = text.split_whitespace().next() else {
        return false;
    };
    vocab::MODIFIER_STOPWORDS.contains(&token::norm(first).as_str())
        || ((first.eq_ignore_ascii_case("or") || first.eq_ignore_ascii_case("and/or"))
            && !is_alternative(_seg, text))
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
            .map(|span| {
                let mut kind = paren::classify(span.inner, None);
                let measures = if matches!(kind, ParenKind::Alias | ParenKind::Other) {
                    paren::amounts(span.inner, self.units)
                } else {
                    Vec::new()
                };
                if !measures.is_empty() {
                    kind = ParenKind::Amount;
                }
                ParenClause {
                    range: range.start + span.range.start..range.start + span.range.end,
                    inner: span.inner,
                    kind,
                    measures,
                }
            })
            .collect();

        // The classifier judges the clause text with its parens excised.
        let stripped = if parens.is_empty() {
            collapse_whitespace(text)
        } else {
            let mut buf = String::with_capacity(text.len());
            let mut cursor = range.start;
            for p in &parens {
                buf.push_str(&source[cursor..p.range.start]);
                cursor = p.range.end;
            }
            buf.push_str(&source[cursor..range.end]);
            collapse_whitespace(&buf)
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
// --- Structural resolution ----------------------------------------------------

impl IngredientParser {
    pub(crate) fn parse_ingredient_segmented<'a>(
        &self,
        input: &'a str,
    ) -> Res<&'a str, ParsedIngredient> {
        // Establish branch scope before interpreting any parenthetical amounts.
        let authored = input;
        let alternative = quantified_alternative(input, &self.units);
        let input = alternative.map_or(input, |start| input[..start].trim_end_matches([',', ' ']));
        let mp = MeasurementParser::new(&self.units, MeasurementMode::IngredientList);
        let container_descriptor = container_descriptor(input, &mp);
        // Only ingredient-side dimensions are removed from the grammar view.
        // A preparation tail keeps its dimensions inside the authored phrase.
        let dimension_limit = ingredient_dimension_limit(input);
        let dimensions = dimensional_spans(input)
            .into_iter()
            .filter(|range| range.start < dimension_limit)
            .collect::<Vec<_>>();
        // These classified constructs have roles independent of the leading
        // quantity. Hide their slots from the measurement view without deleting
        // source text; assembly consumes their payloads below.
        let semantic_parens: Vec<_> = paren::spans(authored)
            .filter_map(|span| {
                let kind = paren::classify(span.inner, None);
                matches!(
                    kind,
                    ParenKind::CrossReference
                        | ParenKind::NoteReference
                        | ParenKind::Optional
                        | ParenKind::MinusEquivalence
                )
                .then_some((span.range, kind, span.inner))
            })
            .collect();
        let optional_word = optional_suffix(authored);
        let mut ignored: Vec<_> = dimensions
            .iter()
            .cloned()
            .chain(semantic_parens.iter().map(|(r, _, _)| r.clone()))
            .chain(optional_word.iter().cloned())
            .chain(container_descriptor.iter().cloned())
            .collect();
        ignored.sort_by_key(|r| r.start);
        ignored.dedup();
        let mut syntax = input.to_string();
        for range in &ignored {
            if range.end > syntax.len() {
                continue;
            }
            syntax.replace_range(range.clone(), &" ".repeat(range.len()));
        }
        let terminal_name = terminal_count_name(&syntax, &mp, &self.segmenter());
        let measure_input = terminal_name.map_or(syntax.as_str(), |start| &syntax[..start]);
        let view = leading_measure_view(measure_input.trim_start());
        let measure_start = view.as_ptr() as usize - syntax.as_ptr() as usize;
        let Ok((mut rest, (primary, _, bracketed, _))) = (
            opt(|a| mp.parse_measurement_list(a)),
            space0,
            opt(|a| mp.parse_bracketed_amounts(a)),
            space0,
        )
            .parse(view)
        else {
            return Err(nom::Err::Error(nom_language::error::VerboseError {
                errors: vec![(
                    input,
                    nom_language::error::VerboseErrorKind::Context("measurement"),
                )],
            }));
        };
        if let Some(start) = terminal_name {
            rest = &syntax[start..];
        }
        // An unconsumed arithmetic operator followed by another quantity is
        // an unsupported amount expression, not the beginning of a food name.
        if rest
            .strip_prefix('+')
            .is_some_and(|tail| starts_quantity(tail.trim_start()))
        {
            return Err(nom::Err::Error(nom_language::error::VerboseError {
                errors: vec![(
                    input,
                    nom_language::error::VerboseErrorKind::Context(
                        "incomplete quantity expression",
                    ),
                )],
            }));
        }
        let mut parsed = ParsedIngredient {
            source: authored.to_string(),
            amounts: [primary, bracketed]
                .into_iter()
                .flatten()
                .flatten()
                .collect(),
            optional: optional_word.is_some()
                || semantic_parens
                    .iter()
                    .any(|(_, kind, _)| *kind == ParenKind::Optional),
            ..Default::default()
        };
        if let Some(span) = container_descriptor {
            parsed.push_modifier(ModifierPart::raw(input[span.clone()].to_string()).at_range(span));
        }
        // "N batches of X" expresses N recipes. Resolve its count-unit role
        // while the authored words still have explicit source positions.
        if let Some(index) = parsed
            .amounts
            .iter()
            .position(|m| matches!(m.unit(), crate::unit::Unit::Whole))
        {
            let words: Vec<_> = token::offsets(rest).take(3).collect();
            if let [(_, batch), (of_start, of), (name_start, _)] = words.as_slice()
                && (batch.eq_ignore_ascii_case("batch") || batch.eq_ignore_ascii_case("batches"))
                && of.eq_ignore_ascii_case("of")
            {
                let base = rest.as_ptr() as usize - syntax.as_ptr() as usize;
                parsed.amounts[index] = parsed.amounts[index].relabel_unit("recipe");
                ignored.push(base + of_start..base + name_start);
                rest = &rest[*name_start..];
            }
        }
        // A counted trailing container can leave a middle slice ("halibut"
        // in "4 (6-ounce) halibut fillets"). Length subtraction would then
        // attribute the food to the container's source occurrence.
        let base = if rest.is_empty() {
            syntax.len()
        } else {
            rest.as_ptr() as usize - syntax.as_ptr() as usize
        };
        let rest_end = base + rest.len();
        if rest_end < syntax.len() {
            parsed.measure_spans.push(rest_end..syntax.len());
        }
        ignored.sort_by_key(|r| r.start);
        let mut cursor = measure_start;
        for range in &ignored {
            if range.start >= base {
                break;
            }
            let start = range.start.max(measure_start);
            if cursor < start {
                parsed.measure_spans.push(cursor..start);
            }
            cursor = cursor.max(range.end.min(base));
        }
        if cursor < base {
            parsed.measure_spans.push(cursor..base);
        }
        for range in &dimensions {
            parsed.push_modifier(
                ModifierPart::raw(
                    input[range.clone()]
                        .trim_matches(['(', ')'])
                        .trim()
                        .to_string(),
                )
                .at(vec![range.clone()]),
            );
        }
        for (range, kind, inner) in &semantic_parens {
            if *kind == ParenKind::Optional
                && let Some(note) = paren::optional_note(inner)
                && !paren::is_cross_reference(note)
                && !paren::is_note_reference(note)
            {
                let start = note.as_ptr() as usize - authored.as_ptr() as usize;
                parsed.push_modifier(
                    ModifierPart::raw(note.to_string()).at_range(start..start + note.len()),
                );
            }
            if *kind == ParenKind::MinusEquivalence && parsed.amounts.is_empty() {
                parsed.unresolved_quantity = true;
                parsed.push_modifier(
                    ModifierPart::raw(inner.trim().to_string()).at(vec![range.clone()]),
                );
            }
        }
        let clauses = self.segmenter().segment(rest);
        trace_clauses(rest, &clauses);
        let mut found_name = false;
        let mut leading_prep = false;
        let mut coordination_end = None;
        for (clause_index, clause) in clauses.iter().enumerate() {
            let coordinated_name = coordination_end.is_some_and(|end| clause_index <= end)
                || (!found_name && opaque_coordination_end(&clauses, clause_index, &mp).is_some());
            let mut text = SourceText::default();
            let mut cursor = clause.range.start;
            for p in &clause.parens {
                text.append(&rest[cursor..p.range.start], base + cursor);
                let origin = base + p.range.start..base + p.range.end;
                match p.kind {
                    ParenKind::CrossReference | ParenKind::NoteReference => {}
                    ParenKind::Optional => parsed.optional = true,
                    ParenKind::Amount => {
                        parsed.amounts.extend(p.measures.iter().cloned());
                        parsed.measure_spans.push(origin);
                    }
                    ParenKind::Alias if coordinated_name => {
                        text.append(&rest[p.range.clone()], base + p.range.start);
                    }
                    ParenKind::Descriptive | ParenKind::Alias | ParenKind::Other if found_name => {
                        // Parenthetical details in a descriptive tail stay in
                        // their host phrase: "cut into (1-inch) pieces".
                        text.append(&rest[p.range.clone()], base + p.range.start);
                    }
                    ParenKind::Alias
                        if !found_name
                            && !self.adjectives.contains(&p.inner.trim().to_lowercase())
                            // A unit without its quantity is incomplete measure
                            // evidence, not an alias for the following food.
                            && !crate::unit::is_valid(&self.units, p.inner.trim())
                            && rest[p.range.end..clause.range.end]
                                .trim()
                                .chars()
                                .next()
                                .is_some_and(char::is_alphabetic) =>
                    {
                        text.append(&rest[p.range.clone()], base + p.range.start);
                    }
                    _ => parsed.push_modifier(
                        ModifierPart::raw(p.inner.trim().to_string()).at(vec![origin]),
                    ),
                }
                // Preserve a separator between words surrounding an omitted aside.
                if !text.text.is_empty() && !text.text.ends_with(' ') {
                    text.text.push(' ');
                    text.map.push(None);
                }
                cursor = p.range.end;
            }
            text.append(&rest[cursor..clause.range.end], base + cursor);
            text.trim();
            if text.text.is_empty() {
                continue;
            }
            if found_name {
                if coordination_end.is_some_and(|end| clause_index <= end) {
                    let separator_start = base + clause.range.start - clause.sep.len();
                    let mut combined = SourceText {
                        text: std::mem::take(&mut parsed.name),
                        map: std::mem::take(&mut parsed.name_map),
                    };
                    combined.append(clause.sep, separator_start);
                    combined.text.push_str(&text.text);
                    combined.map.extend(text.map);
                    parsed.set_name(combined);
                } else {
                    let origins = text.origins(0..text.text.len());
                    let part = if clause.kind == ClauseKind::Alternative {
                        ModifierPart::alternative(text.text)
                    } else {
                        ModifierPart::raw(text.text)
                    };
                    parsed.push_modifier(part.at(origins));
                }
                continue;
            }
            if matches!(
                clause.kind,
                ClauseKind::PrepChain | ClauseKind::KnownPrepPhrase
            ) && clauses
                .iter()
                .any(|c| c.range.start > clause.range.start && c.kind == ClauseKind::HeadCandidate)
            {
                let origins = text.origins(0..text.text.len());
                parsed.push_modifier(ModifierPart::prep(text.text).at(origins));
                leading_prep = true;
                continue;
            }
            parsed.set_name(text);
            coordination_end = opaque_coordination_end(&clauses, clause_index, &mp);
            if clause.kind == ClauseKind::MinusMeasure {
                let prefix = parsed.name.split_once(' ').map(|(_, r)| r).unwrap_or("");
                if let Ok((remaining, _)) = mp.parse_measurement_list(prefix)
                    && !remaining.trim().is_empty()
                {
                    let cut = parsed.name.len() - remaining.len();
                    parsed.extract_name(0..cut, ModifierKind::Raw);
                }
            } else if leading_prep {
                let head = token::offsets(&parsed.name)
                    .find(|(_, w)| !token::is_prep_token(w) && !is_connector(w))
                    .map(|(i, _)| i)
                    .unwrap_or(0);
                if head > 0 {
                    parsed.extract_name(0..head, ModifierKind::Prep);
                }
            }
            found_name = true;
        }
        if let Some(start) = alternative {
            let mut branch = SourceText::default();
            let mut cursor = start;
            for range in ignored.iter().filter(|r| r.start >= start) {
                if cursor < range.start {
                    branch.append(&authored[cursor..range.start], cursor);
                }
                cursor = cursor.max(range.end);
            }
            branch.append(&authored[cursor..], cursor);
            branch.trim();
            let origins = branch.origins(0..branch.text.len());
            parsed.push_modifier(ModifierPart::alternative(branch.text).at(origins));
        }
        Ok(("", parsed))
    }
}

/// A size before a parenthetical package weight describes its following
/// container: "2 medium (8-ounce) cones". Hide only that descriptive source
/// slot from the measurement view, leaving the count/size/container grammar to
/// resolve the measures. A size directly before a food remains a count unit.
fn container_descriptor(input: &str, mp: &MeasurementParser<'_>) -> Option<Range<usize>> {
    let paren = paren::spans(input).next()?;
    let before = input[..paren.range.start].trim_end();
    let size = super::vocab::SIZE_UNIT_WORDS.iter().find(|size| {
        before.len() > size.len()
            && before
                .get(before.len() - size.len()..)
                .is_some_and(|tail| tail.eq_ignore_ascii_case(size))
            && before[..before.len() - size.len()].ends_with(char::is_whitespace)
    })?;
    let start = before.len() - size.len();
    let (remaining, measures) = mp.parse_measurement_list(before[..start].trim()).ok()?;
    if !remaining.is_empty()
        || measures.len() != 1
        || !matches!(measures[0].unit(), crate::unit::Unit::Whole)
    {
        return None;
    }
    let after = input[paren.range.end..].trim_start();
    let word = after.split_whitespace().next()?.to_lowercase();
    (mp.is_container_unit(&word)
        && !paren::amounts(&paren.inner.replace('-', " "), mp.units).is_empty())
    .then_some(start..before.len())
}

/// With no following food, an authored count noun is the ingredient's name:
/// "4 cloves" must not become a nameless garlic-clove measure. This is a
/// structural boundary for ingredient lines only; standalone amount parsing
/// and rich-text measurements still recognize the configured count unit.
fn terminal_count_name(
    input: &str,
    mp: &MeasurementParser<'_>,
    segmenter: &Segmenter<'_>,
) -> Option<usize> {
    let end = separator_offsets(input)
        .first()
        .map_or(input.len(), |(offset, _)| *offset);
    let word = input[..end].split_whitespace().next_back()?;
    let lower = word.to_lowercase();
    if !crate::unit::is_addon_unit(mp.units, &lower)
        || crate::unit::Unit::is_known(&crate::unit::singular(&lower))
    {
        return None;
    }
    if end < input.len()
        && segmenter.segment(input).iter().skip(1).any(|clause| {
            !matches!(
                clause.kind,
                ClauseKind::PrepChain | ClauseKind::KnownPrepPhrase | ClauseKind::Purpose
            )
        })
    {
        return None;
    }
    let start = word.as_ptr() as usize - input.as_ptr() as usize;
    let prefix = input[..start].trim();
    let (remaining, measures) = mp.parse_measurement_list(prefix).ok()?;
    (remaining.is_empty()
        && measures.len() == 1
        && matches!(measures[0].unit(), crate::unit::Unit::Whole))
    .then_some(start)
}

fn trace_clauses(source: &str, clauses: &[Clause<'_>]) {
    if !crate::trace::is_diagnostics_enabled() {
        return;
    }
    for clause in clauses {
        crate::trace::trace_on_change(
            crate::trace::Stage::Segment,
            clause.kind.as_str(),
            clause.text(source).trim(),
            &clause.stripped,
            true,
        );
    }
}

pub(crate) const SEGMENT_TRACE_NAMES: &[&str] = &[
    "prep_chain",
    "known_prep_phrase",
    "minus_measure",
    "purpose",
    "alternative",
    "parenthetical",
    "prose",
    "head_candidate",
];

/// Lexical dimensions occupy their source positions but do not participate in
/// the ingredient measure grammar. Masking preserves byte coordinates; unlike
/// lifting text to the end, it cannot change clause order or lose provenance.
pub(super) fn dimensional_spans(input: &str) -> Vec<Range<usize>> {
    let mut spans: Vec<_> = paren::spans(input)
        .filter(|p| paren::is_descriptive(p.inner))
        .map(|p| p.range)
        .collect();
    crate::lazy_regex!(
        DIMENSION,
        r"(?i)(?:[0-9¼½¾⅐⅑⅒⅓⅔⅕⅖⅗⅘⅙⅚⅛⅜⅝⅞]+(?:[./–—-][0-9¼½¾⅐⅑⅒⅓⅔⅕⅖⅗⅘⅙⅚⅛⅜⅝⅞]+)*)(?:\s*[-–]\s*|\s*)(?:inches|inch|in|centimeters?|centimetres?|millimeters?|millimetres?|cm|mm)(?:-(?:thick|long|wide|deep|tall))?\b"
    );
    crate::lazy_regex!(
        TEMPERATURE,
        r"(?i)[0-9]+(?:[.][0-9]+)?(?:\s*(?:[-–]|to)\s*[0-9]+(?:[.][0-9]+)?)?\s*(?:°\s*(?:fahrenheit|celsius|[fc])?|degrees?\s*(?:fahrenheit|celsius|[fc]\b)|fahrenheit|celsius)"
    );
    crate::lazy_regex!(QUOTED_LENGTH, r#"[0-9]+(?:[./][0-9]+)?\s*["″]"#);
    for m in DIMENSION
        .find_iter(input)
        .chain(TEMPERATURE.find_iter(input))
        .chain(QUOTED_LENGTH.find_iter(input))
    {
        if !spans
            .iter()
            .any(|r| r.start <= m.start() && r.end >= m.end())
        {
            spans.push(m.range());
        }
    }
    spans.sort_by_key(|r| r.start);
    spans
}

fn starts_quantity(text: &str) -> bool {
    text.chars()
        .next()
        .is_some_and(|c| c.is_ascii_digit() || crate::fraction::is_vulgar(c))
        || crate::parser::text_number(text).is_ok()
}

/// Locate an explicitly quantified alternative outside parenthetical details.
fn quantified_alternative(input: &str, units: &std::collections::HashSet<String>) -> Option<usize> {
    let parens: Vec<_> = paren::spans(input).map(|p| p.range).collect();
    let parser = MeasurementParser::new(units, MeasurementMode::IngredientList);
    for (start, word) in token::offsets(input) {
        if start == 0 || !(word.eq_ignore_ascii_case("or") || word.eq_ignore_ascii_case("and/or")) {
            continue;
        }
        // Unquantified coordination cannot form a quantity branch. Check this
        // before scanning parenthetical ranges or parsing the primary prefix.
        let tail = input[start + word.len()..].trim_start();
        if !starts_quantity(tail) || parens.iter().any(|range| range.contains(&start)) {
            continue;
        }
        // A numeric range's first endpoint is not an ingredient branch.
        // Require food text after any leading amount in the left-hand side.
        let primary = input[..start].trim_end_matches([',', ' ']);
        let food = parser
            .parse_measurement_list(primary)
            .map_or(primary, |(remaining, _)| remaining);
        if !food.chars().any(char::is_alphabetic) {
            continue;
        }
        if parser
            .parse_measurement_list(tail)
            .is_ok_and(|(remaining, amounts)| !amounts.is_empty() && !remaining.trim().is_empty())
        {
            return Some(start);
        }
    }
    None
}

/// Comma coordination is opaque identity unless a conjunct states its own
/// quantity. Stop scanning at a descriptive clause; never invent a shared head.
fn opaque_coordination_end(
    clauses: &[Clause<'_>],
    start: usize,
    parser: &MeasurementParser<'_>,
) -> Option<usize> {
    for (index, clause) in clauses.iter().enumerate().skip(start + 1) {
        if clause.sep != ", " {
            return None;
        }
        match clause.kind {
            ClauseKind::HeadCandidate | ClauseKind::PrepChain | ClauseKind::KnownPrepPhrase => {}
            ClauseKind::Alternative => {
                let rest = clause
                    .stripped
                    .split_once(' ')
                    .map(|(_, tail)| tail)
                    .unwrap_or("");
                let quantified = parser
                    .parse_measurement_list(rest)
                    .is_ok_and(|(_, measures)| !measures.is_empty());
                return (!quantified).then_some(index);
            }
            _ => return None,
        }
    }
    None
}

/// A dimension after a top-level delimiter belongs to that descriptive tail.
/// Inside the ingredient clause, a preparation preposition also starts a tail.
fn ingredient_dimension_limit(input: &str) -> usize {
    let clause_end = separator_offsets(input)
        .first()
        .map(|(offset, _)| *offset)
        .unwrap_or(input.len());
    token::offsets(&input[..clause_end])
        .find(|(_, word)| matches!(token::norm(word).as_str(), "into" | "for" | "in"))
        .map(|(offset, _)| offset)
        .unwrap_or(clause_end)
}

/// A leading determiner is grammatical only when followed by a quantity.
fn leading_measure_view(input: &str) -> &str {
    let Some((word, rest)) = input.split_once(char::is_whitespace) else {
        return input;
    };
    let rest = rest.trim_start();
    if word.eq_ignore_ascii_case("the")
        && (rest
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_digit() || crate::fraction::is_vulgar(c))
            || crate::parser::text_number(rest).is_ok())
    {
        rest
    } else {
        input
    }
}

/// The terminal optional token modifies the line, rather than its preceding
/// preparation phrase. Its source slot is retained as semantic metadata.
fn optional_suffix(input: &str) -> Option<Range<usize>> {
    let trimmed = input.trim_end();
    let start = trimmed.rfind(char::is_whitespace)? + 1;
    (start > 0
        && trimmed[start..].eq_ignore_ascii_case("optional")
        && trimmed[..start].chars().any(char::is_alphanumeric))
    .then_some(start..trimmed.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[rstest]
    #[case("2-inch piece ginger", &[(0, 6)])]
    #[case("2 carrots, cut into 2-inch pieces", &[])]
    #[case("carrots cut into (1-inch) pieces", &[])]
    fn dimensions_respect_clause_ownership(
        #[case] input: &str,
        #[case] expected: &[(usize, usize)],
    ) {
        let ranges: Vec<_> = dimensional_spans(input)
            .into_iter()
            .filter(|r| r.start < ingredient_dimension_limit(input))
            .map(|r| (r.start, r.end))
            .collect();
        assert_eq!(ranges, expected);
    }

    #[test]
    fn classified_parenthetical_retains_measure_payload_and_range() {
        let parser = IngredientParser::new();
        let clauses = parser.segmenter().segment("tomatoes (about 2 cups), diced");
        let p = &clauses[0].parens[0];
        assert_eq!(p.kind, ParenKind::Amount);
        assert_eq!(p.measures, vec![Measure::new("cup", 2.0)]);
        assert_eq!(p.range, 9..23);
    }
}
