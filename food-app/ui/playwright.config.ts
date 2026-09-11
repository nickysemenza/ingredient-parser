import { defineConfig } from "@playwright/test";
export default defineConfig({
  testDir: "./e2e",
  fullyParallel: false,
  // The fixture bundle is large; webkit needs room to install it per navigation.
  timeout: 60_000,
  use: { baseURL: "http://127.0.0.1:1420", browserName: "webkit" },
  webServer: {
    command: "pnpm dev",
    url: "http://127.0.0.1:1420",
    reuseExistingServer: !process.env.CI,
  },
  reporter: "list",
});
