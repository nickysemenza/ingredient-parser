import { WasmContextProvider } from "./wasmContext";
import { Demo } from "./Demo";
function App() {
  return (
    <WasmContextProvider>
      <div className="App">
        <Demo />
      </div>
    </WasmContextProvider>
  );
}

export default App;
