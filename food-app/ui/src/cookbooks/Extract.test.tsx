import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import type { Estimate, GatewayStatus, OpenedBook } from "../bridge";
import { api } from "../bridge";
import { BookPanel } from "./Extract";

vi.mock("../bridge", () => ({
  api: { estimate: vi.fn(), gateway: vi.fn() },
}));
afterEach(() => {
  cleanup();
  vi.resetAllMocks();
});

const book: OpenedBook = {
  path: "/books/dessert.epub",
  outline: {
    source: {
      label: "dessert",
      sha256: "abc",
      title: "Dessert Fixture",
      authors: ["Fixture Author"],
      identifiers: [],
      subjects: ["Baking"],
      spine_docs: 2,
      lines: 67,
    },
    cover: null,
    chapters: ["Pies and Tarts", "Foundational Recipes"],
    nav_recipe_titles: 4,
    chunks: 1,
    lines: 67,
  },
  classified: {
    classification: "ambiguous",
    score: 0.74,
    method: "structure",
    reasons: ["26 of 67 lines look like quantities (38.8%)"],
    quantity_lines: 26,
    ingredient_runs: 4,
    nav_recipe_titles: 4,
  },
  runs: [],
  openMs: 12,
};
const estimate: Estimate = {
  chunks: 1,
  lines: 67,
  chars: 4754,
  cache_hits: 0,
  input_tokens: 2720,
  output_tokens: 307,
  calls_low: 2,
  calls_high: 3,
  wall_ms_low: 7616,
  wall_ms_high: 22233,
  cost_usd_low: 0.0015835,
  cost_usd_high: 0.0028848,
  ladder: ["gemini-2.5-flash", "claude-haiku-4-5"],
  concurrency: 16,
  assumptions: ["at most 25% of chunks need a second opinion"],
};
const gateway = (configured: boolean): GatewayStatus => ({
  configured,
  baseUrl: configured ? "https://gateway.example/v1" : null,
  configPath: "/Users/me/Library/Application Support/gateway.env",
  cacheDir: null,
  runsDir: null,
  ladder: ["gemini-2.5-flash"],
  error: configured ? null : "CLOUDFLARE_AI_GATEWAY_BASE_URL is unset",
});
const panel = () => (
  <BookPanel
    book={book}
    extracting={false}
    progress={null}
    onExtract={vi.fn()}
    onCancel={vi.fn()}
    onOpenRun={vi.fn()}
    onError={vi.fn()}
  />
);

it("shows the estimate before any extraction, with its assumptions", async () => {
  vi.mocked(api.estimate).mockResolvedValue(estimate);
  vi.mocked(api.gateway).mockResolvedValue(gateway(true));
  render(panel());
  expect(await screen.findByText("$0.0016–$0.0029")).toBeVisible();
  expect(screen.getByText("7.6 s–22.2 s")).toBeVisible();
  expect(screen.getByText("1 (0 already cached)")).toBeVisible();
  expect(
    screen.getByText("at most 25% of chunks need a second opinion"),
  ).toBeVisible();
  expect(screen.getByText("ambiguous")).toBeVisible();
  expect(screen.getByRole("button", { name: "Extract" })).toBeEnabled();
  expect(api.estimate).toHaveBeenCalledWith("/books/dessert.epub");
});

it("refuses to extract without gateway credentials and says where they go", async () => {
  vi.mocked(api.estimate).mockResolvedValue(estimate);
  vi.mocked(api.gateway).mockResolvedValue(gateway(false));
  render(panel());
  const extract = await screen.findByRole("button", { name: "Extract" });
  expect(extract).toBeDisabled();
  expect(
    screen.getByText(/Gateway credentials are not configured/),
  ).toBeVisible();
  expect(
    screen.getByText("/Users/me/Library/Application Support/gateway.env"),
  ).toBeVisible();
});
