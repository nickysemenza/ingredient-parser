// Run after `make build-demo-wasm`:
// node --experimental-wasm-modules ingredient-wasm/tests/boundary.mjs
import assert from "node:assert/strict";
import {
  parse_recipe,
  parse_ingredient_with_decomposition,
  scale_amount,
  format_amount,
} from "../pkg/ingredient_wasm.js";

// Call the actual generated JS exports, crossing their serde/WASM ABI.
const result = parse_recipe({
  sections: [
    { name: "Sauce", ingredients: ["1 cup flour"], instructions: [] },
    {
      ingredients: [],
      instructions: ["Add 2 tsp to 3 tbsp flour. Bake at 350°F.", ""],
    },
  ],
});
assert.equal(result.sections.length, 2);
assert.equal(result.sections[0].name, "Sauce");
assert.equal(result.sections[1].name, undefined);
assert.equal(result.sections[1].instructions.length, 2);
assert(
  result.sections[1].instructions[0].some(
    (chunk) => chunk.kind === "Measure" && chunk.value.length === 2,
  ),
);
assert(
  result.sections[1].instructions[0].some(
    (chunk) => chunk.kind === "Ing" && chunk.value === "flour",
  ),
);
assert.deepEqual(result.instruction_diagnostics, []);
assert.equal(scale_amount({ unit: "fahrenheit", value: 350 }, 2).value, 350);
assert.equal(scale_amount({ unit: "cup", value: 2 / 3 }, 3).value, 2);
// Canonical wire units must keep their non-scalable kind when reconstructed.
for (const [unit, value, formatted] of [
  ["celsius", 180, "180 celsius"],
  ["cm", 3, "3 cm"],
  ["mm", 5, "5 mm"],
]) {
  const scaled = scale_amount({ unit, value }, 2);
  assert.equal(scaled.unit, unit);
  assert.equal(scaled.value, value);
  assert.equal(format_amount(scaled), formatted);
}


const line = "½ jalapeño, émincé";
const observed = parse_ingredient_with_decomposition(line);
assert.equal(
  observed.decomposition.segments.map((segment) => segment.text).join(""),
  line,
);
console.log("Generated JS/WASM recipe, scaling, and Unicode boundary checks passed.");
