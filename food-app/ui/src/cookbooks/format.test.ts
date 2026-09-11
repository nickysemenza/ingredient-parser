import { describe, expect, it } from "vitest";
import type { Chapter, ChunkReport, Item, Recipe, Span } from "../bridge";
import {
  describeFlag,
  findItem,
  flattenChapters,
  formatAmounts,
  formatCost,
  formatCostRange,
  formatDuration,
  formatDurationRange,
  formatEta,
  formatPercent,
  formatSpan,
  itemCounts,
  orderChunks,
} from "./format";

const span: Span = { start: 0, end: 4, doc_path: "OEBPS/c01.xhtml", page: "7" };
function recipe(id: string, title: string, extra: Partial<Recipe> = {}): Item {
  return {
    kind: "recipe",
    id,
    title,
    name: title,
    meta: {
      description: [],
      recipe_yield: null,
      times: null,
      equipment: [],
      category: null,
      page: null,
    },
    sections: [],
    photos: [],
    notes: [],
    variant_of: null,
    span,
    ...extra,
  };
}
const chunk = (
  id: string,
  status: ChunkReport["status"],
  flags: ChunkReport["flags"],
): ChunkReport => ({
  id,
  start: 0,
  end: 10,
  chars: 100,
  status,
  final_model: "m",
  attempts: 1,
  flags,
  second_opinion: null,
  recipes: 1,
  cached: false,
});

describe("money and duration", () => {
  it("keeps fractions of a cent visible and collapses an equal range", () => {
    expect(formatCost(0.0015835)).toBe("$0.0016");
    expect(formatCost(12.5)).toBe("$12.50");
    expect(formatCost(0.00001)).toBe("<$0.0001");
    expect(formatCost(0)).toBe("$0");
    expect(formatCost(null)).toBe("—");
    expect(formatCostRange(0.0015835, 0.0028848)).toBe("$0.0016–$0.0029");
    expect(formatCostRange(0.002, 0.002)).toBe("$0.0020");
  });
  it("chooses the coarsest unit that still carries information", () => {
    expect(formatDuration(29)).toBe("29 ms");
    expect(formatDuration(7616)).toBe("7.6 s");
    expect(formatDuration(222330)).toBe("3 m 42 s");
    expect(formatDuration(120000)).toBe("2 m");
    expect(formatDuration(-1)).toBe("—");
    expect(formatDurationRange(7616, 22233)).toBe("7.6 s–22.2 s");
    expect(formatDurationRange(0, 0)).toBe("0 ms");
  });
  it("reports the remaining window, not a single invented number", () => {
    expect(
      formatEta({
        remaining_low_ms: 8000,
        remaining_high_ms: 64000,
        projected_cost_usd: 0.2,
      }),
    ).toBe("~8.0 s–1 m 4 s left");
    expect(
      formatEta({
        remaining_low_ms: 0,
        remaining_high_ms: 0,
        projected_cost_usd: 0,
      }),
    ).toBe("finishing");
  });
  it("formats shares and amounts from the parser", () => {
    expect(formatPercent(0.875, 1)).toBe("87.5%");
    expect(formatPercent(null)).toBe("—");
    expect(
      formatAmounts([
        { unit: "cup", value: 1.5, upper_value: null },
        { unit: "g", value: 360, upper_value: null },
      ]),
    ).toBe("1.5 cup / 360 g");
    expect(formatAmounts([{ unit: "clove", value: 2, upper_value: 3 }])).toBe(
      "2–3 clove",
    );
  });
  it("names the source document, page and line range", () => {
    expect(formatSpan(span)).toBe("OEBPS/c01.xhtml · page 7 · lines 0–4");
    expect(formatSpan({ ...span, page: null })).toBe(
      "OEBPS/c01.xhtml · lines 0–4",
    );
  });
});

describe("book tree", () => {
  const chapters: Chapter[] = [
    {
      id: "ch00",
      title: "Pies and Tarts",
      span,
      intro: [],
      items: [
        recipe("000.0001", "Sour Cherry Pie"),
        recipe("000.0022", "Tart"),
      ],
    },
    {
      id: "ch01",
      title: null,
      span,
      intro: [],
      items: [recipe("001.0000", "Graham Cracker Crust")],
    },
  ];
  it("keeps reading order and labels the pre-contents chapter", () => {
    expect(flattenChapters(chapters).map((row) => row.key)).toEqual([
      "ch00",
      "000.0001",
      "000.0022",
      "ch01",
      "001.0000",
    ]);
    const front = flattenChapters(chapters)[3];
    expect(front.kind === "chapter" && front.title).toBe("Front matter");
  });
  it("drops chapters whose items all fail the filter", () => {
    expect(flattenChapters(chapters, "crust").map((row) => row.key)).toEqual([
      "ch01",
      "001.0000",
    ]);
    expect(flattenChapters(chapters, "absent")).toEqual([]);
  });
  it("counts the parts of a recipe and finds an item by id", () => {
    const full = recipe("000.0005", "Mousse Pie", {
      sections: [
        {
          name: null,
          ingredients: [
            {
              raw: "2 cups flour",
              line: 1,
              parsed: {
                name: "flour",
                amounts: [],
                modifier: null,
                usage: "normal",
              },
              confidence: "high",
              ref: null,
            },
            {
              raw: "Graham Cracker Crust",
              line: 2,
              parsed: {
                name: "Graham Cracker Crust",
                amounts: [],
                modifier: null,
                usage: "normal",
              },
              confidence: "medium",
              ref: {
                target_id: "001.0000",
                text: "this page",
                kind: "ingredient",
                method: "anchor",
              },
            },
          ],
          steps: [{ text: "Bake.", line: 3, refs: [] }],
        },
      ],
      photos: [
        {
          path: "a.jpg",
          mime: "image/jpeg",
          alt: null,
          caption: null,
          line: 1,
        },
      ],
    });
    expect(itemCounts(full)).toEqual({
      ingredients: 2,
      steps: 1,
      photos: 1,
      refs: 1,
    });
    expect(findItem(chapters, "001.0000")?.title).toBe("Graham Cracker Crust");
    expect(findItem(chapters, "nope")).toBeUndefined();
  });
});

describe("diagnostics ordering", () => {
  it("puts failed chunks first, then flagged, then the rest in id order", () => {
    const rows = orderChunks([
      chunk("k003", "ok", []),
      chunk("k002", "ok", [{ flag: "truncated" }]),
      chunk("k001", "failed", []),
      chunk("k000", "ok", []),
    ]);
    expect(rows.map((row) => row.id)).toEqual(["k001", "k002", "k000", "k003"]);
  });
  it("explains every flag shape the report can carry", () => {
    expect(
      describeFlag({ flag: "low_amount_parse_rate", rate: 0.25, lines: 8 }),
    ).toBe("low amount parse rate 25% over 8 lines");
    expect(
      describeFlag({ flag: "missing_nav_title", title: "Pie", line: 4 }),
    ).toBe("missing contents title “Pie” (line 4)");
    expect(describeFlag({ flag: "unassigned_lines", count: 3 })).toBe(
      "3 unassigned lines",
    );
  });
});
