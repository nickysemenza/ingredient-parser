import { createContext, useEffect, useState } from "react";

export type { RichItem, ScrapedRecipe } from "./wasm/pkg";
export type wasm = typeof import("./wasm/pkg/ingredient_wasm");

export const WasmContext = createContext<wasm | undefined>(undefined);

export const WasmContextProvider: React.FC<{
  children?: React.ReactNode;
}> = ({ children }) => {
  const [state, setState] = useState<wasm>();
  useEffect(() => {
    const fetchWasm = async () => {
      console.time("wasm-load");
      const wasm = await import("./wasm/pkg/ingredient_wasm");
      setState(wasm);
      console.timeEnd("wasm-load");
    };
    fetchWasm();
  }, []);

  return <WasmContext.Provider value={state}>{children}</WasmContext.Provider>;
};
