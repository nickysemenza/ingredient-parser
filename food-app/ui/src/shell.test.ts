import { describe, expect, it } from "vitest";
import { readRecentRuns } from "./shell";

describe("recent runs", () => {
  it("recovers from malformed or incompatible stored preferences", () => {
    for (const raw of [
      null,
      "{",
      "null",
      "123",
      "{}",
      '[null, {}, {"path": 1, "name":"bad"}]',
    ]) {
      expect(readRecentRuns(raw)).toEqual([]);
    }
  });
  it("deduplicates paths, preserves order, and bounds the list", () => {
    const run = { path: "/run.json", name: "My cookbook" };
    const entries = [
      run,
      run,
      ...Array.from({ length: 12 }, (_, index) => ({
        path: `/run-${index}.json`,
        name: `Run ${index}`,
      })),
    ];
    const result = readRecentRuns(JSON.stringify(entries));
    expect(result).toHaveLength(8);
    expect(result[0]).toEqual(run);
    expect(result[1].path).toBe("/run-0.json");
  });
});
