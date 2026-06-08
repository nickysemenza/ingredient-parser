import { StrictMode } from "react";
import ReactDOM from "react-dom/client";
import "./index.css";
import { LoadingScreen } from "./Spinner";

// Paint the spinner first, then resolve the WASM-dependent module tree (App ->
// Demo -> wasm.ts's top-level await) via a dynamic import. By the time App
// renders, WASM is loaded, so every component uses it synchronously.
const root = ReactDOM.createRoot(document.getElementById("root") as HTMLElement);
root.render(<LoadingScreen />);
const { default: App } = await import("./App");
root.render(
  <StrictMode>
    <App />
  </StrictMode>
);
