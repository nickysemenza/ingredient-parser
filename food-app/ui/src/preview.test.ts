import { afterEach, describe, expect, it, vi } from "vitest";
import { PREVIEW_DEBOUNCE_MS, schedulePreview } from "./preview";

afterEach(() => vi.useRealTimers());

describe("native preflight preview scheduling", () => {
  it("coalesces a burst of option changes into one warm call", async () => {
    vi.useFakeTimers();
    let calls = 0;
    let dispose = () => {};
    for (let i = 0; i < 100; i += 1) {
      dispose();
      dispose = schedulePreview(
        async () => {
          calls += 1;
          return i;
        },
        () => {},
        () => {},
      );
    }
    vi.advanceTimersByTime(PREVIEW_DEBOUNCE_MS - 1);
    expect(calls).toBe(0);
    vi.advanceTimersByTime(1);
    await vi.runAllTimersAsync();
    expect(calls).toBe(1);
  });

  it("suppresses a reply after the option is superseded", async () => {
    vi.useFakeTimers();
    let resolve!: (value: string) => void;
    const applied: string[] = [];
    const dispose = schedulePreview(
      () => new Promise<string>((next) => (resolve = next)),
      (value) => applied.push(value),
      () => {},
    );
    vi.advanceTimersByTime(PREVIEW_DEBOUNCE_MS);
    dispose();
    resolve("stale");
    await vi.runAllTimersAsync();
    expect(applied).toEqual([]);
  });
});
