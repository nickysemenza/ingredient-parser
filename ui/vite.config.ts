import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

// One build serves both hosts: Tauri loads `dist/` and the web deploys it.
// The platform is detected at runtime (see src/api/transport.ts).
export default defineConfig({
  plugins: [react(), tailwindcss()],
  clearScreen: false,
  build: { target: "es2022" },
  worker: { format: "es" },
  test: {
    environment: "jsdom",
    setupFiles: ["./src/test-setup.ts"],
    exclude: ["e2e/**", "node_modules/**"],
  },
});
