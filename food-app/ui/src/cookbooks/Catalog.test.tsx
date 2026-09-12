import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { api } from "../bridge";
import { CatalogPanel } from "./Catalog";
vi.mock("../bridge", () => ({
  api: { catalogStatus: vi.fn(), catalog: vi.fn(), cancelExtraction: vi.fn() },
}));
beforeEach(() => {
  vi.mocked(api.catalogStatus).mockResolvedValue(null);
});
afterEach(() => {
  cleanup();
  vi.resetAllMocks();
});
it("defaults to Opus and does not start cataloguing on open", async () => {
  render(<CatalogPanel paths={["book.epub"]} onError={vi.fn()} />);
  await waitFor(() =>
    expect(api.catalogStatus).toHaveBeenCalledWith("book.epub"),
  );
  expect(api.catalog).not.toHaveBeenCalled();
  vi.mocked(api.catalog).mockResolvedValue([]);
  fireEvent.click(screen.getByRole("button", { name: "Build catalog" }));
  await waitFor(() =>
    expect(api.catalog).toHaveBeenCalledWith(
      ["book.epub"],
      { reader: "claude-cli/opus", auditor: null, force: false },
      expect.any(Function),
    ),
  );
});
it("surfaces allowance failure without changing models or retrying", async () => {
  vi.mocked(api.catalog).mockRejectedValue(new Error("Allowance exhausted"));
  const onError = vi.fn();
  render(<CatalogPanel paths={["book.epub"]} onError={onError} />);
  fireEvent.click(screen.getByRole("button", { name: "Build catalog" }));
  expect(await screen.findByText("Error: Allowance exhausted")).toBeVisible();
  expect(api.catalog).toHaveBeenCalledTimes(1);
  expect(screen.getByLabelText("Reader")).toHaveValue("claude-cli/opus");
});
