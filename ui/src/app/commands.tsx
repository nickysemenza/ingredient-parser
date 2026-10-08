// The command palette's registry. The shell contributes navigation and
// appearance; each workspace contributes its own actions while mounted.
import { createContext, useContext, useEffect, useMemo, useRef, useState, type ReactNode } from "react";

/** Asks the Cookbooks workspace to show its open dialog (File › Open…). */
export const OPEN_BOOK = "open-book";

export interface Command {
  id: string;
  title: string;
  group: string;
  icon?: ReactNode;
  shortcut?: string;
  keywords?: string[];
  run: () => void;
}

type Registry = { add: (owner: string, commands: Command[]) => void; remove: (owner: string) => void };
const RegistryContext = createContext<Registry | null>(null);
const CommandsContext = createContext<Command[]>([]);

export function CommandProvider({ children }: { children: ReactNode }) {
  const [owners, setOwners] = useState<Record<string, Command[]>>({});
  const registry = useMemo<Registry>(
    () => ({
      add: (owner, commands) => setOwners((all) => ({ ...all, [owner]: commands })),
      remove: (owner) =>
        setOwners((all) => {
          const { [owner]: _, ...rest } = all;
          return rest;
        }),
    }),
    [],
  );
  const commands = useMemo(() => Object.values(owners).flat(), [owners]);
  return (
    <RegistryContext.Provider value={registry}>
      <CommandsContext.Provider value={commands}>{children}</CommandsContext.Provider>
    </RegistryContext.Provider>
  );
}

export const useCommandList = () => useContext(CommandsContext);

/** Contribute commands while mounted. Re-registers when `deps` change; the
 *  latest `run` closures are always used. */
export function useCommands(owner: string, commands: Command[], deps: unknown[]) {
  const registry = useContext(RegistryContext);
  const latest = useRef(commands);
  latest.current = commands;
  useEffect(() => {
    registry?.add(
      owner,
      latest.current.map((command, index) => ({ ...command, run: () => latest.current[index]?.run() })),
    );
    // oxlint-disable-next-line react-hooks/exhaustive-deps
  }, [registry, owner, ...deps]);
  useEffect(() => () => registry?.remove(owner), [registry, owner]);
}
