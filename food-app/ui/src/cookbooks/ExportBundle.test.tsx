import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { api, pick } from "../bridge";
import { ExportBundle } from "./ExportBundle";
vi.mock("../bridge", () => ({
  api: { exportBundle: vi.fn() },
  pick: vi.fn(),
  revealFile: vi.fn(),
}));
beforeEach(() => {
  vi.mocked(api.exportBundle).mockResolvedValue({
    path: "/runs/book--run.cookbook.zip",
    manifest: {
      format: "cookbook-bundle",
      version: 1,
      extraction: "extraction.json",
      preview: "index.html",
      source_sha256: "abc",
      run_id: "run",
      incomplete: false,
      images: [],
    },
  });
});
afterEach(() => {
  cleanup();
  vi.resetAllMocks();
});
const onBusy = vi.fn();
const onError = vi.fn();
const panel = (sourcePath: string | null = "/books/book.epub") =>
  render(
    <ExportBundle
      runPath="/runs/run.json"
      sourcePath={sourcePath}
      onBusy={onBusy}
      onError={onError}
    />,
  );
it("picks the source when missing and does nothing when the picker is cancelled", async () => {
  vi.mocked(pick)
    .mockResolvedValueOnce(null)
    .mockResolvedValueOnce("/other/book.epub");
  panel(null);
  fireEvent.click(screen.getByRole("button", { name: "Export bundle" }));
  await waitFor(() => expect(onBusy).toHaveBeenLastCalledWith(false));
  expect(api.exportBundle).not.toHaveBeenCalled();
  expect(onError).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "Export bundle" }));
  await screen.findByRole("status");
  expect(api.exportBundle).toHaveBeenCalledWith(
    "/runs/run.json",
    "/other/book.epub",
  );
});
it("surfaces source mismatch and lets the user choose a replacement without automatic retry", async () => {
  vi.mocked(api.exportBundle).mockRejectedValueOnce(
    new Error("Source EPUB hash does not match"),
  );
  vi.mocked(pick).mockResolvedValue("/correct/book.epub");
  panel();
  fireEvent.click(screen.getByRole("button", { name: "Export bundle" }));
  fireEvent.click(
    await screen.findByRole("button", { name: "Choose source EPUB" }),
  );
  await screen.findByRole("status");
  expect(onError).toHaveBeenCalledWith(
    "Error: Source EPUB hash does not match",
  );
  expect(api.exportBundle).toHaveBeenCalledTimes(2);
  expect(api.exportBundle).toHaveBeenLastCalledWith(
    "/runs/run.json",
    "/correct/book.epub",
  );
});
