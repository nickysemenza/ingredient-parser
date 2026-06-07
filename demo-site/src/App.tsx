import { WasmContextProvider } from "./WasmProvider";
import { Demo } from "./Demo";

function App() {
  return (
    <WasmContextProvider>
      <Demo />
    </WasmContextProvider>
  );
}

export default App;
