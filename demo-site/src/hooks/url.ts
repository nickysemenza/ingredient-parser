import { useEffect, useState } from "react";

/* ── URL state helpers ─────────────────────────────────────────
   Inputs are persisted to the query string so demo links are
   shareable. Each writer reads the current params first, so the
   independent effects never clobber each other's keys. */
export const getUrlParam = (key: string): string | null =>
  new URLSearchParams(window.location.search).get(key);

export const setUrlParam = (key: string, value: string | null) => {
  const params = new URLSearchParams(window.location.search);
  if (value === null || value === "") {
    params.delete(key);
  } else {
    params.set(key, value);
  }
  const qs = params.toString();
  window.history.replaceState(null, "", qs ? `?${qs}` : window.location.pathname);
};

/* Persist a value to the query string, debounced: a replaceState per
   keystroke trips Safari's 100-calls-per-30s rate limit (SecurityError). */
export const useUrlParamSync = (key: string, value: string | null) => {
  useEffect(() => {
    const timer = setTimeout(() => setUrlParam(key, value), 500);
    return () => clearTimeout(timer);
  }, [key, value]);
};

/* useState seeded from `?key=`, debounced-persisted back on change.
   Use for plain string inputs; values needing parsing (e.g. the numeric
   scale factor) read `getUrlParam` and call `useUrlParamSync` directly. */
export const useUrlState = (
  key: string,
  fallback: string
): [string, React.Dispatch<React.SetStateAction<string>>] => {
  const [value, setValue] = useState(() => getUrlParam(key) ?? fallback);
  useUrlParamSync(key, value);
  return [value, setValue];
};
