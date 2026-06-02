---
name: corpus-harvest
description: Refine ingredient-parser's accuracy corpus (tests/corpus/corpus.jsonl) by harvesting real ingredient lines from EPUB cookbooks or recipe websites, finding parser gaps, and adding committed/xfail rows. Use when asked to grow the corpus, harvest gaps from cookbooks/EPUBs/URLs, find more parser test cases, or refine corpus.jsonl.
---

<objective>
The accuracy corpus (`ingredient-parser/tests/corpus/corpus.jsonl`, scored by
`tests/accuracy.rs`) is the parser's north-star quality metric. This skill grows and
refines it from REAL ingredient lines — EPUB cookbooks or recipe websites — by finding
lines the parser gets wrong (or misses), labeling the desired parse, and appending
committed regression-guards (parser already correct) and `xfail` rows (documented gaps).

The loop is fiddly: committed rows must match the parser's EXACT f64 output, and new
labels must not contradict existing committed rows or the Design Decisions. Read
`references/corpus-rules.md` — it is the rulebook (schema, exact-f64, labeling, the
suspicious-line heuristic, known false-positives, and lessons). Follow it closely.
</objective>

<quick_start>
1. Build the CLI once: `cargo build -p food-cli` (binary at `./target/debug/food-cli`).
2. Get a dump of `{line,name,amounts,modifier}` JSONL from a source (see <workflow>).
3. Filter to suspicious lines, label per `references/corpus-rules.md`, validate each
   against the live parser, append, and verify with the accuracy test.

The feedback loop you'll run constantly:
  cargo test -p ingredient --test accuracy accuracy_corpus -- --nocapture \
    | grep -E 'exact matches|known gaps|PROMOTE|REGRESSION'
Require 0 REGRESSION. Confirm a single line with: `food-cli parse-ingredient "<line>"`.
</quick_start>

<workflow>
1. EXTRACT a dump JSONL (`{line,name,amounts,modifier}` per ingredient line):
   - EPUB cookbook: `food-cli scrape-epub <path.epub> --dump-parsed > dump.jsonl`
     (needs the env vars in <auth_env> of corpus-rules.md; content-cached → re-runs free).
   - Website: `food-cli scrape <url> --json | jq -r '.sections[].ingredients[]' > lines.txt`
     then `food-cli parse-lines lines.txt > dump.jsonl` (no credentials needed).
   - Re-check after a parser change (FREE): take a prior dump's `line` values into a file
     and `food-cli parse-lines lines.txt > dump.jsonl` — re-parses through the current parser.
   - Library breadth: `food-cli scan-cookbooks <dir> --limit N` ranks miss candidates.

2. DEDUP + FILTER: dedup by `line` (the same line recurs constantly). Write a small,
   throwaway suspicious-line filter tuned to the current parser — see the
   <suspicious_heuristic> in `references/corpus-rules.md`. Most flags will be false
   positives once the parser is decent; that's expected.

3. REVIEW / LABEL each suspicious line per the <labeling_rules> (the label is the
   *desired* parse). Small batches: label by hand. Large batches: fan out review
   sub-agents, each given a slice + the corpus-rules.md content.

4. VALIDATE every candidate against the LIVE parser (`food-cli parse-ingredient "<line>"`):
   - parser already correct → COMMITTED row (copy its exact output, incl. full-precision f64).
   - parser wrong but desired parse is clear → `xfail` row (label = desired, add a terse reason).
   - garbage → drop.

5. CURATE: dedup against existing corpus rows (don't re-add). Group xfail by bug class,
   keep ≤3 representatives per class. Resolve any label conflict toward existing committed
   rows / Design Decisions (see <lessons>). Present the curated set to the user for go/no-go.

6. APPEND + VERIFY + PROMOTE: append rows under a dated `// --- section ---`. Run the
   feedback loop; require 0 REGRESSION. For any `PROMOTE` line, remove that row's `xfail`
   key (targeted edit; see <promote_technique>). Regenerate snapshots if they shift
   (`INSTA_UPDATE=always cargo test -p ingredient --test snapshots trace_tree`;
   `REGEN_SNAPSHOTS=1 cargo test -p recipe-scraper --test integration scrape_from_cache`).
   Full gate: `cargo fmt`, `cargo clippy --workspace --all-targets`, `cargo nextest run
   --workspace`. Commit; do not push unless asked.
</workflow>

<validation>
- `food-cli parse-lines <file>` emits one well-formed JSON object per non-blank line,
  matching `parse-ingredient` for the same line.
- Spot-check: every COMMITTED candidate parses EXACTLY today; every `xfail` candidate
  actually differs from current output (so it's a real gap, not a mislabel).
- After append: accuracy test shows the expected committed/xfail counts and 0 REGRESSION;
  full workspace gate green before committing.
</validation>

<anti_patterns>
- Hand-typing f64 values (e.g. `2.333`) instead of copying the parser's serialized output → silent mismatch.
- Adding a label that contradicts an existing committed row → corpus becomes self-inconsistent.
- Treating brand/product names with digits ("Pierre Ferrand 1840 Cognac", "tipo 00 flour") or "X or Y" ingredient alternatives as gaps → they parse correctly by design.
- Committing a fragile parser hack for a rare construction the Design Decisions reject → drop the case with a `//` rationale instead.
- Bundling/relying on a frozen suspicious-line script → the heuristic drifts; regenerate it per pass.
</anti_patterns>
