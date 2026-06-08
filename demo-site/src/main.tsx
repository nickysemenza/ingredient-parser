import { StrictMode } from "react";
import ReactDOM from "react-dom/client";
import "./index.css";
import { LoadingScreen } from "./Spinner";

// Paint the spinner first, then resolve the WASM-dependent module tree (App ->
// Demo -> wasm.ts's top-level await) via a dynamic import. By the time App
// renders, WASM is loaded, so every component uses it synchronously.
const root = ReactDOM.createRoot(document.getElementById("root") as HTMLElement);
root.render(<LoadingScreen />);
try {
  const { default: App } = await import("./App");
  root.render(
    <StrictMode>
      <App />
    </StrictMode>
  );
} catch (e) {
  // A failed WASM/module load would otherwise leave the spinner up forever as an
  // unhandled rejection. Surface it instead.
  console.error("Failed to load the app (WASM parser?)", e);
  root.render(
    <div role="alert" style={{ padding: "1rem" }}>
      Failed to load the parser. Try reloading the page.
    </div>
  );
}
