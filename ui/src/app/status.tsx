// What each workspace tells the shell: a status line, whether a job runs (the
// close guard), and the window title.
import { createContext, useCallback, useContext, useEffect, useMemo, useState, type ReactNode } from "react";
import type { Workspace } from "./router";

export interface Status {
  message: string;
  detail?: string;
  busy?: boolean;
  title?: string;
}

type Reports = Partial<Record<Workspace, Status>>;
const Context = createContext<{ reports: Reports; report: (workspace: Workspace, status: Status) => void } | null>(
  null,
);

export function StatusProvider({ children }: { children: ReactNode }) {
  const [reports, setReports] = useState<Reports>({});
  const report = useCallback((workspace: Workspace, status: Status) =>
    setReports((all) => {
      const was = all[workspace];
      return was && was.message === status.message && was.detail === status.detail && was.busy === status.busy && was.title === status.title
        ? all
        : { ...all, [workspace]: status };
    }), []);
  const value = useMemo(() => ({ reports, report }), [reports, report]);
  return <Context.Provider value={value}>{children}</Context.Provider>;
}

export function useReports(): Reports {
  return useContext(Context)?.reports ?? {};
}

/** Report this workspace's status whenever it changes. */
export function useReportStatus(workspace: Workspace, status: Status) {
  const report = useContext(Context)?.report;
  const { message, detail, busy, title } = status;
  useEffect(() => {
    report?.(workspace, { message, detail, busy, title });
  }, [report, workspace, message, detail, busy, title]);
}
