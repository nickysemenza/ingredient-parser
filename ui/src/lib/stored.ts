import { useEffect, useState } from "react";

/** State remembered across launches. Values that no longer fit `allowed` (or
 *  the initial value's type) fall back to `initial`. */
export function useStored<T>(key: string, initial: T, allowed?: readonly T[]) {
  const [value, setValue] = useState<T>(() => read(key, initial, allowed));
  useEffect(() => {
    try {
      localStorage.setItem(key, JSON.stringify(value));
    } catch {
      /* Preferences are optional when storage is unavailable. */
    }
  }, [key, value]);
  return [value, setValue] as const;
}

export function read<T>(key: string, initial: T, allowed?: readonly T[]): T {
  try {
    const raw = localStorage.getItem(key);
    if (raw === null) return initial;
    const parsed: unknown = JSON.parse(raw);
    return typeof parsed === typeof initial &&
      (typeof parsed !== "number" || Number.isFinite(parsed)) &&
      (!allowed || allowed.includes(parsed as T))
      ? (parsed as T)
      : initial;
  } catch {
    return initial;
  }
}
