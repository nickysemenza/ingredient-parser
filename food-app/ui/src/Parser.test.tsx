import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { Parser } from "./Parser";
import { api, type IngredientResult } from "./bridge";
vi.mock("./bridge", async () => {
  const actual = await vi.importActual<typeof import("./bridge")>("./bridge");
  return {
    ...actual,
    api: { ...actual.api, parse: vi.fn(), inspect: vi.fn(), corpus: vi.fn() },
  };
});
const row = (
  lineNumber: number,
  input: string,
  name: string,
): IngredientResult => ({
  lineNumber,
  input,
  name,
  amounts: ["2 cups"],
  modifier: null,
  optional: false,
  usage: "Primary",
  confidence: "High",
  reviewReasons: [],
  json: { name },
});
const rows = [
  row(1, "2 cups zucchini", "zucchini"),
  row(2, "2 cups apples", "apples"),
];
beforeEach(() => {
  localStorage.clear();
  vi.mocked(api.parse).mockResolvedValue(rows);
  vi.mocked(api.inspect).mockImplementation(async (input) => ({
    result: rows.find((r) => r.input === input) ?? rows[0],
    stages: "normalize → result",
    trace: null,
    traceText: "",
    jaegerJson: "{}",
  }));
});
afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});
const props = { onError: vi.fn(), onBusy: vi.fn() };
describe("Parser evidence identity", () => {
  it("keeps selected line through sorting and clears stale evidence on edit", async () => {
    render(<Parser {...props} />);
    fireEvent.click(screen.getByRole("button", { name: "Parse" }));
    await screen.findByRole("listbox", { name: "Parsed ingredients" });
    await screen.findByRole("region", { name: "Ingredient inspector" });
    expect(
      within(
        screen.getByRole("region", { name: "Ingredient inspector" }),
      ).getByText("2 cups zucchini"),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Name" }));
    expect(screen.getAllByRole("option")[1]).toHaveAttribute(
      "aria-selected",
      "true",
    );
    expect(
      within(
        screen.getByRole("region", { name: "Ingredient inspector" }),
      ).getByText("2 cups zucchini"),
    ).toBeInTheDocument();
    fireEvent.change(
      screen.getByRole("textbox", { name: "Ingredient lines" }),
      { target: { value: "3 cups flour" } },
    );
    expect(
      screen.queryByRole("region", { name: "Ingredient inspector" }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("listbox", { name: "Parsed ingredients" }),
    ).not.toBeInTheDocument();
  });
  it("ignores a pending result after changing source", async () => {
    let resolve: (v: IngredientResult[]) => void = () => {};
    vi.mocked(api.parse).mockReturnValue(
      new Promise((done) => {
        resolve = done;
      }),
    );
    render(<Parser {...props} />);
    fireEvent.click(screen.getByRole("button", { name: "Parse" }));
    fireEvent.click(screen.getByRole("tab", { name: "Recipe URL" }));
    await act(async () => resolve(rows));
    expect(
      screen.queryByRole("listbox", { name: "Parsed ingredients" }),
    ).not.toBeInTheDocument();
    expect(
      screen.getByRole("heading", { name: "Inspect a web recipe" }),
    ).toBeInTheDocument();
  });
  it("inspects corpus evidence without losing corpus context", async () => {
    vi.mocked(api.corpus).mockResolvedValue({
      path: null,
      cases: [
        {
          lineNumber: 1,
          section: "test",
          input: "2 cups zucchini",
          status: "Regression",
          reason: null,
          fields: [
            {
              field: "name",
              matches: false,
              expected: "squash",
              actual: "zucchini",
            },
          ],
        },
      ],
    });
    render(<Parser {...props} />);
    fireEvent.click(screen.getByRole("tab", { name: "Corpus" }));
    fireEvent.click(screen.getByRole("button", { name: "Load & score" }));
    await screen.findByRole("button", { name: "Inspect ingredient" });
    fireEvent.click(screen.getByRole("button", { name: "Inspect ingredient" }));
    await screen.findByRole("region", { name: "Ingredient inspector" });
    expect(screen.getByRole("tab", { name: "Corpus" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
    expect(
      screen.getByRole("listbox", { name: "Corpus rows" }),
    ).toBeInTheDocument();
    fireEvent.click(
      screen.getByRole("button", { name: "Back to field comparison" }),
    );
    expect(
      screen.getByRole("heading", { name: "Field comparison" }),
    ).toBeInTheDocument();
  });
});
