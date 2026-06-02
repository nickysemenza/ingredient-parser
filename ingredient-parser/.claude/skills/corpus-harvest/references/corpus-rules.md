<corpus_format>
The corpus is `ingredient-parser/tests/corpus/corpus.jsonl` — one JSON object per
line; `//` lines and blank lines are ignored. Scored by `tests/accuracy.rs`.

Row schema:
  {"input": "<raw line, REQUIRED>",
   "name": "<string>",
   "amounts": [{"unit": "<str>", "value": <num>, "upper_value": <num|null>}],
   "modifier": "<string>",
   "optional": <bool>,
   "xfail": "<reason>"}

- `input` is the only required field. Missing name → "", missing amounts → [],
  missing modifier → null, missing optional → false.
- Group rows under a `// --- section ---` comment so the file stays legible.
</corpus_format>

<committed_vs_xfail>
- A row WITHOUT `xfail` is COMMITTED: it must parse EXACTLY as labeled. A mismatch
  fails the test — this is the per-case regression guard. Only commit a row whose
  label equals the live parser's CURRENT output.
- A row WITH `xfail` is a KNOWN GAP: the label is the *desired* parse the parser does
  NOT yet produce. A mismatch is tolerated and reported. When the parser improves so
  an xfail row passes, the test prints `PROMOTE (xfail now passes — remove xfail)`;
  remove the `xfail` key to lock it in as a committed guard.
- So: if the live parser is already correct → commit it. If it's wrong but the desired
  parse is clear and defensible → xfail it (label = what it SHOULD be). If the line is
  garbage (OCR/PUA glyphs, pure cross-ref, non-food) → drop it.
</committed_vs_xfail>

<exact_f64_rule>
`amounts` values are compared with bitwise f64 `==` — NO rounding.
- NEVER hand-type a value like `2.333`. It will not match.
- For a COMMITTED row, copy the value from the parser's OWN serialized output
  (`food-cli parse-ingredient "<line>"` or `parse-lines`). That guarantees round-trip
  equality.
- Repeating fractions need the full shortest round-trip form: ⅓ → `0.3333333333333333`,
  ⅔ → `0.6666666666666666`. ¼ → `0.25`, ⅛ → `0.125` (terminating, fine).
- Footgun: a value that "looks right" from a calculator (e.g. 1⅔ = 1.6666666666666667)
  can differ from how the parser computes it. Always copy the parser's emitted value.
  (A prior committed row had to be dropped for exactly this reason.)
</exact_f64_rule>

<labeling_rules>
Apply the Design Decisions from `ingredient-parser/src/lib.rs` (the doc comment):
- SIZE words (large, medium, small, extra-large, jumbo, ripe) stay in the NAME — they
  describe which variant, not how much. "2 large eggs" → name "large eggs".
- PREP words (chopped, minced, diced, sliced, sifted, melted, softened, grated,
  finely/coarsely chopped, packed, room-temperature, toasted-as-trailing) → MODIFIER,
  whether leading or trailing. "1 cup chopped onion" → name "onion", mod "chopped".
- "whole" is the default unit for countable items with no explicit unit ("2 eggs").
- MULTIPLE units are preserved as SEPARATE amounts, no conversion: "1 cup / 240ml" →
  [1 cup, 240 ml]. Metric/imperial duals, "(N oz / M g)", "[56 G]" all hoist.
- RANGES are ONE Measure with `upper_value`: "2-3 cups" → value 2, upper_value 3.
  Cross-unit ranges ("2 tsp to 2 tbsp") are the exception → two separate amounts.
- PURPOSE / QUALIFIER phrases → MODIFIER: "for garnish", "to taste", "plus more for
  dusting", "or more to taste".
- "X or Y" ALTERNATIVES where Y starts with a number/article → MODIFIER beginning "or …"
  ("4 cloves garlic or 1 teaspoon garlic powder" → mod "or 1 teaspoon garlic powder").
  BUT "X or Y" where both are bare ingredients ("applesauce or pear sauce",
  "lemon or orange zest") is kept WHOLE in the name by design — do NOT split it.
- AMOUNT qualifiers (generous, scant, heaping, heaped, rounded, brimming) are
  DISCARDED, leading or mid-position: "2 generous tablespoons X" → [2 tbsp], no
  "generous" anywhere. (Matches the committed rows; do not re-introduce them.)
- A trailing/mid "(optional)" → set `optional: true` and remove the note.
- "(see this page)" / "(this page)" cross-refs are stripped.
- The parser NEVER fails — unparseable input falls back to name-only.
</labeling_rules>

<suspicious_heuristic>
For bulk filtering a dump (thousands of lines), write a SMALL throwaway filter tuned to
the CURRENT parser — do not rely on a frozen script; the signal drifts as the parser
improves. Flag a line if any of:
- MISS: the line has a digit or vulgar fraction but `amounts` is empty (and it's not a
  recipe-component reference whose only digit is in the modifier like "9-inch pan").
- name starts with a digit or punctuation ( ( - – — , / ).
- name starts with a known UNIT word (leftover unit not consumed).
- name has a concatenation seam: `[a-z][A-Z]` or `)` immediately followed by a non-space.
- modifier starts with `(` or `,`, or has a `)`-glued-to-text seam.
- name/modifier still contains "this page" or "(optional)".
Emit `{...row, "_reasons": [...]}` and a `# N/M suspicious` summary.

KNOWN FALSE POSITIVES — do NOT treat these as gaps:
- Brand/product names with digits: "Pierre Ferrand 1840 Cognac", "tipo 00 flour",
  "U10 scallops", "Chinese 5-spice powder", "Beefeater 24 gin". The digit belongs in
  the name; the parse is correct.
- "X or Y" ingredient alternatives kept in the name (see labeling_rules).
- Recipe-component refs with no quantity ("Graham Cracker Crust (this page), fully baked
  in a 9-inch pan") — name-only is correct.
</suspicious_heuristic>

<promote_technique>
After appending rows, run the accuracy test with `--nocapture` and grep for `PROMOTE`.
For each promoted input (an xfail that now passes), remove the `, "xfail": "..."` key
from that row (a targeted Edit). Usually only a handful per pass — no script needed.
Verify the EXACT-f64 holds: a promoted row becomes a committed guard, so its label must
equal the live output.
</promote_technique>

<lessons>
- NEVER add a label that CONTRADICTS an existing committed row. The corpus must stay
  self-consistent. If a new candidate's "desired" parse conflicts with how a committed
  row is labeled (e.g. whether "grated"/"minced" extracts, whether qualifiers are
  discarded), the established committed convention wins — relabel the candidate to match.
- Some harvested labels are internally inconsistent (e.g. one says keep a count, another
  drops it). Standardize on the design-consistent behavior and fix the outliers.
- Don't fix a gap that needs machinery the design rejects (e.g. a size-word lexicon for
  "1 small or ½ medium celery root" — the design avoids size blocklists). Drop such a
  case with a `//` rationale rather than committing a fragile hack.
- WHERE A TEST GOES: `from_str` accuracy (input → name/amounts/modifier/optional) → this
  corpus. `parse_amount`, `RichParser`, `Display`, custom-parser config, unit/conversion
  behavior → Rust `#[rstest]` tests (the corpus schema can't express those). Trace-tree
  structure → `tests/trace.rs`.
</lessons>

<auth_env>
EPUB extraction (`scrape-epub`, `scan-cookbooks`) reads from the environment:
  ANTHROPIC_BASE_URL                                  (required)
  one of: ANTHROPIC_API_KEY | CF_AIG_TOKEN | AI_GATEWAY_API_KEY
Default model: gemini-2.5-flash (override with --model; claude-*/gpt-* also work).
Extraction is content-hash cached on disk ($TMPDIR/recipe-epub), so re-runs are free.
Do NOT hardcode secrets — read them from the environment. (Live gateway creds, if any,
live in the user's MEMORY, not in the repo.) `parse-lines`, `parse-ingredient`, and the
website `scrape <url>` path need NO credentials.
</auth_env>
