import { useCallback, useEffect, useRef, useState } from "react";
import { toast } from "sonner";
import { message } from "../api";

export interface Query<T> {
  value: T | undefined;
  error: string | undefined;
  loading: boolean;
}

/** Run `fn` whenever `deps` change; the previous value stays visible while
 *  the next one loads, and only the latest answer is kept. `null` clears. */
export function useQuery<T>(fn: (() => Promise<T>) | null, deps: unknown[], delay = 0): Query<T> {
  const [state, setState] = useState<Query<T>>({ value: undefined, error: undefined, loading: false });
  const latest = useRef(fn);
  latest.current = fn;
  // oxlint-disable-next-line react-hooks/exhaustive-deps
  useEffect(() => {
    const run = latest.current;
    if (!run) {
      setState({ value: undefined, error: undefined, loading: false });
      return;
    }
    let live = true;
    setState((was) => ({ ...was, loading: true }));
    const timer = setTimeout(() => {
      run().then(
        (value) => live && setState({ value, error: undefined, loading: false }),
        (error: unknown) => live && setState((was) => ({ ...was, error: message(error), loading: false })),
      );
    }, delay);
    return () => {
      live = false;
      clearTimeout(timer);
    };
    // oxlint-disable-next-line react-hooks/exhaustive-deps
  }, deps);
  return state;
}

/** A user-triggered job: only the latest run's answer is accepted, and
 *  failures become toasts. */
export function useAction() {
  const generation = useRef(0);
  const [busy, setBusy] = useState(false);
  const run = useCallback(async <T>(fn: () => Promise<T>, accept: (value: T) => void) => {
    const id = ++generation.current;
    setBusy(true);
    try {
      const value = await fn();
      if (id === generation.current) accept(value);
    } catch (error) {
      if (id === generation.current) toast.error(message(error));
    } finally {
      if (id === generation.current) setBusy(false);
    }
  }, []);
  const cancel = useCallback(() => {
    generation.current++;
    setBusy(false);
  }, []);
  return { run, busy, cancel };
}
