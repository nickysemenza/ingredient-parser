import { defineConfig, devices } from "@playwright/test";

// Both suites run the production build. `web` is the public site, every
// portable command answered by the real Rust parser compiled to WASM.
// `desktop` is the same bundle in the desktop shell: native-only commands
// are answered from a recorded fixture (create_run_fixture), and portable
// commands still run for real in WASM against the fixture's files.
export default defineConfig({
  testDir: "./e2e",
  fullyParallel: true,
  timeout: 60_000,
  expect: { timeout: 15_000 },
  use: { baseURL: "http://127.0.0.1:4173", trace: "retain-on-failure" },
  projects: [
    { name: "web", testMatch: "web.spec.ts", use: { ...devices["Desktop Chrome"] } },
    // Tauri renders with WKWebView on macOS.
    { name: "desktop", testMatch: "desktop.spec.ts", use: { ...devices["Desktop Safari"], viewport: { width: 1280, height: 820 } } },
  ],
  webServer: {
    command: "pnpm build && pnpm preview",
    url: "http://127.0.0.1:4173",
    reuseExistingServer: !process.env.CI,
    timeout: 180_000,
  },
  reporter: [["list"], ["json", { outputFile: "test-results/results.json" }]],
});
