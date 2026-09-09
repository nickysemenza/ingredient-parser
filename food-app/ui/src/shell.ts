import { useEffect, useState } from "react";

export interface WorkspaceStatus {
  title: string;
  canReview: boolean;
  message: string;
  detail: string;
}
export interface ReviewAction {
  id: number;
  command: string;
}
export interface RecentRun {
  path: string;
  name: string;
}
const key = "v1:recent-runs";
export function readRecentRuns(raw: string | null): RecentRun[] {
  try {
    const value: unknown = JSON.parse(raw ?? "[]");
    if (!Array.isArray(value)) return [];
    const paths = new Set<string>();
    return value
      .filter((item): item is RecentRun => {
        if (
          !item ||
          typeof item !== "object" ||
          typeof item.path !== "string" ||
          !item.path.trim() ||
          typeof item.name !== "string" ||
          !item.name.trim() ||
          paths.has(item.path)
        )
          return false;
        paths.add(item.path);
        return true;
      })
      .slice(0, 8);
  } catch {
    return [];
  }
}
export function useRecentRuns() {
  const [runs, setRuns] = useState<RecentRun[]>(() => {
    try {
      return readRecentRuns(localStorage.getItem(key));
    } catch {
      return [];
    }
  });
  useEffect(() => {
    try {
      localStorage.setItem(key, JSON.stringify(runs));
    } catch {
      /* Optional preference. */
    }
  }, [runs]);
  return {
    runs,
    remember: (path: string, name: string) =>
      setRuns((previous) =>
        [{ path, name }, ...previous.filter((run) => run.path !== path)].slice(
          0,
          8,
        ),
      ),
    clear: () => setRuns([]),
  };
}
