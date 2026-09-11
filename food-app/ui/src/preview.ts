export const PREVIEW_DEBOUNCE_MS = 200;

/**
 * Debounce native preflight calls and ignore replies for superseded options.
 * The returned disposer is used by React effect cleanup as well as callers
 * that close the extraction dialog before the timer fires.
 */
export function schedulePreview<T>(
  load: () => Promise<T>,
  apply: (value: T) => void,
  fail: (error: unknown) => void,
  debounceMs = PREVIEW_DEBOUNCE_MS,
): () => void {
  let active = true;
  const timer = window.setTimeout(() => {
    void load().then(
      (value) => {
        if (active) apply(value);
      },
      (error: unknown) => {
        if (active) fail(error);
      },
    );
  }, debounceMs);
  return () => {
    active = false;
    window.clearTimeout(timer);
  };
}
