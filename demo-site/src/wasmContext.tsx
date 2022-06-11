import { createContext, useEffect, useState } from "react";

export type { RichItem } from "./wasm/pkg";
export type wasm = typeof import("./wasm/pkg/ingredient_wasm");
import init, * as wasmContent from "./wasm/pkg/ingredient_wasm";

export const WasmContext = createContext<wasm | undefined>(undefined);
// https://github.com/rustwasm/wasm-pack/issues/911#issuecomment-1044017752
export const WasmContextProvider: React.FC<{
  children?: React.ReactNode;
}> = ({ children }) => {
  const [state, setState] = useState<wasm>();
  useEffect(() => {
    const fetchWasm = async () => {
      console.time("wasm-load");
      await init();
      setState(wasmContent);
      console.timeEnd("wasm-load");
    };
    fetchWasm();
  }, []);

  return <WasmContext.Provider value={state}>{children}</WasmContext.Provider>;
};
