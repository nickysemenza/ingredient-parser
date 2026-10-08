import { createContext, useContext, useEffect, useSyncExternalStore, type ReactNode } from "react";
import { useStored } from "../lib/stored";

export const APPEARANCES = ["system", "light", "dark"] as const;
export type Appearance = (typeof APPEARANCES)[number];

const query = typeof matchMedia === "function" ? matchMedia("(prefers-color-scheme: dark)") : null;
const subscribe = (notify: () => void) => {
  query?.addEventListener("change", notify);
  return () => query?.removeEventListener("change", notify);
};

type Theme = { appearance: Appearance; setAppearance: (value: Appearance) => void; dark: boolean };
const Context = createContext<Theme>({ appearance: "system", setAppearance: () => {}, dark: false });

/** The chosen appearance and whether it currently resolves to dark. The
 *  first paint is handled in index.html with the same key. */
export function ThemeProvider({ children }: { children: ReactNode }) {
  const [appearance, setAppearance] = useStored<Appearance>("v2:appearance", "system", APPEARANCES);
  const systemDark = useSyncExternalStore(subscribe, () => query?.matches ?? false, () => false);
  const dark = appearance === "dark" || (appearance === "system" && systemDark);
  useEffect(() => {
    document.documentElement.dataset.theme = dark ? "dark" : "light";
  }, [dark]);
  return <Context.Provider value={{ appearance, setAppearance, dark }}>{children}</Context.Provider>;
}

export const useTheme = () => useContext(Context);
