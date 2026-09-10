import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { ModelResults } from "./ModelResults";
import { api } from "./bridge";
vi.mock("./bridge", () => ({ api: { results: vi.fn() } }));
afterEach(() => { cleanup(); vi.resetAllMocks(); });
it("shows loading, an empty result, and no invented success rate", async () => {
  vi.mocked(api.results).mockResolvedValue({ rows: [], unreadable: [] });
  render(<ModelResults onOpen={vi.fn()} />);
  expect(screen.getByRole("status")).toHaveTextContent("Loading model results");
  expect(await screen.findByText("No extraction results yet.")).toBeVisible();
  expect(screen.queryByText("100.0%")).not.toBeInTheDocument();
});
it("reports a failed load and refreshes without retaining stale results", async () => {
  vi.mocked(api.results).mockRejectedValueOnce(new Error("disk unavailable"))
    .mockResolvedValueOnce({ rows: [], unreadable: ["missing.json: unreadable"] });
  render(<ModelResults onOpen={vi.fn()} />);
  expect(await screen.findByRole("alert")).toHaveTextContent("disk unavailable");
  fireEvent.click(screen.getByRole("button", { name: "Refresh results" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("missing.json: unreadable");
  expect(screen.queryByText(/disk unavailable/)).not.toBeInTheDocument();
});
