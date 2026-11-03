// The end-to-end test (e2e/), against a preview branch's Neon Auth and the
// production web app; `just web-e2e preview/pr-<n>`. Kept out of `bun test`:
// its files are `*.e2e.ts`, which bun's test runner doesn't match.
import { defineConfig, devices } from "@playwright/test";

export default defineConfig({
  testDir: "e2e",
  testMatch: "**/*.e2e.ts",
  // The signed authorization query expires after about 10 minutes.
  timeout: 180_000,
  expect: { timeout: 15_000 },
  retries: 0,
  workers: 1,
  forbidOnly: !!process.env.CI,
  reporter: process.env.CI ? [["list"], ["html", { open: "never" }]] : "list",
  use: {
    trace: "retain-on-failure",
    screenshot: "only-on-failure",
  },
  projects: [{ name: "chromium", use: { ...devices["Desktop Chrome"] } }],
});
