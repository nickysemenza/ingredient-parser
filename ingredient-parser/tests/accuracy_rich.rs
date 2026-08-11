//! Rich-text parser accuracy corpus — the instruction-prose counterpart to
//! `corpus.jsonl` (which scores `from_str` on ingredient lines).
//!
//! `tests/corpus/rich_text.jsonl` holds real instruction strings, each with a
//! human-labeled expected chunk sequence (`RichParser::parse` output). The flat
//! ingredient-corpus schema can't express an interleaved `Text`/`Measure`/`Ing`
//! sequence, so rich text gets its own ratchet here.
//!
//! Two row classes (mirroring `accuracy.rs`):
//! - **Committed rows** (no `xfail`): MUST match exactly. A mismatch fails this
//!   test — the per-case regression guard.
//! - **Known gaps** (`"xfail": "reason"`): a mismatch is tolerated and reported;
//!   when the parser improves enough to pass, a `PROMOTE` hint prints.
//!
//! Behavioral properties a chunk sequence can't express (e.g. a measure's
//! `MeasureKind`/scalability) stay in `parsing.rs` rstests.
//!
//! Seeded by hand. The deferred wire-up is harvesting: `scan-cookbooks` already
//! surfaces low-confidence *ingredient* lines; the same loop can mine instruction
//! prose into candidate rows here.

#![allow(clippy::unwrap_used)]
// Test-harness code: a malformed corpus line should fail the test loudly.
#![allow(clippy::panic)]

use ingredient::rich_text::{Chunk, RichParser};
use ingredient_corpus::rich::RichRow;

/// Every well-formed row; a malformed line fails the suite loudly. The strict
/// policy is shared (`into_rows_strict`) so the two ratchets cannot drift.
fn load() -> Vec<RichRow> {
    match ingredient_corpus::rich::parse(ingredient_corpus::embedded_rich()).into_rows_strict() {
        Ok(rows) => rows,
        Err(problems) => panic!("invalid rich_text row(s):\n{problems}"),
    }
}

#[test]
fn accuracy_rich_text() {
    let rows = load();
    let total = rows.len();
    assert!(total > 0, "rich-text corpus is empty");

    let mut exact = 0usize;
    let mut known_gaps = 0usize;
    let mut regressions: Vec<(String, String)> = Vec::new();
    let mut promotable: Vec<String> = Vec::new();

    for row in rows {
        let RichRow {
            input,
            ingredients,
            chunks,
            xfail,
        } = row;
        let want: Vec<Chunk> = chunks.into_iter().map(Chunk::from).collect();
        let got = RichParser::new(ingredients).parse(&input).unwrap();

        if got == want {
            exact += 1;
            if xfail.is_some() {
                promotable.push(input);
            }
            continue;
        }
        if xfail.is_some() {
            known_gaps += 1;
        } else {
            regressions.push((input, format!("got {got:?}\n        want {want:?}")));
        }
    }

    let pct = 100.0 * exact as f64 / total as f64;
    eprintln!("\n========== Rich-text accuracy corpus ==========");
    eprintln!("rows:           {total}");
    eprintln!("exact matches:  {exact} ({pct:.1}%)");
    eprintln!("known gaps:     {known_gaps} (xfail)");
    eprintln!("===============================================\n");

    for input in &promotable {
        eprintln!("PROMOTE (xfail now passes — remove `xfail`): {input}");
    }
    for (input, diff) in &regressions {
        eprintln!("REGRESSION: {input}\n    got/want: {diff}");
    }

    assert!(
        regressions.is_empty(),
        "{} non-xfail rich-text row(s) mismatch — see report above",
        regressions.len()
    );
}
