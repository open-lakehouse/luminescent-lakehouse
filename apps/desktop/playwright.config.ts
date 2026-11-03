import { defineConfig, devices } from "@playwright/test";

// The UI in Playwright's WebKit (the engine behind WKWebView and WebKitGTK),
// built with `--mode e2e` so src/host/ipc-mock.ts answers the IPC with
// fixtures. What needs the real binary (IPC, CSP, the proxy) is the in-app
// smoke test instead (`just desktop-smoke`).
const PORT = 4324;

export default defineConfig({
  testDir: "e2e",
  // `*.e2e.ts`, which `bun test` doesn't match.
  testMatch: "**/*.e2e.ts",
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 1 : 0,
  reporter: process.env.CI ? [["github"], ["list"]] : "list",
  use: { baseURL: `http://localhost:${PORT}`, trace: "retain-on-failure" },
  projects: [{ name: "webkit", use: { ...devices["Desktop Safari"] } }],
  webServer: {
    command: `bunx vite build --mode e2e --outDir dist-e2e && bunx vite preview --mode e2e --outDir dist-e2e --port ${PORT} --strictPort`,
    url: `http://localhost:${PORT}`,
    reuseExistingServer: !process.env.CI,
    timeout: 120_000,
  },
});
