// An MCP App through the sandbox, in WebKit on the mocked IPC: mangrove's real
// `ui://uc/grants` (e2e/fixtures/grants-app.html, one inlined HTML with inline
// module scripts) in chatcn's McpAppFrame on the sandbox proxy, served from a
// second origin with the scheme's CSP header (vite.config.ts). The harness page is
// src/integrations/mcp/harness.tsx.
import { expect, test } from "@playwright/test";

test("mangrove's grants app completes ui/initialize through the sandbox and renders", async ({
  page,
}) => {
  const violations: string[] = [];
  page.on("console", (message) => {
    // The page's own (the shell's side); the View's are its CSP doing its job.
    const own = message.location().url.startsWith("http://localhost:4324/");
    if (own && /Content Security Policy|Refused to/i.test(message.text())) {
      violations.push(message.text());
    }
  });
  await page.goto("/?harness=mcp-app");

  const frame = page.locator('[data-slot="mcp-app-frame"]');
  // `ready` is the host's AppBridge after the View's ui/initialize and initialized.
  await expect(frame).toHaveAttribute("data-status", "ready", { timeout: 20_000 });

  // The proxy is another origin than the page, and the View is inside it.
  const proxy = page.locator('iframe[title$="interactive app"]');
  const src = await proxy.getAttribute("src");
  expect(new URL(src ?? "").origin).not.toBe(new URL(page.url()).origin);
  expect(await proxy.getAttribute("sandbox")).toBe("allow-scripts allow-same-origin allow-forms");
  const view = page.frameLocator('iframe[title$="interactive app"]').frameLocator("iframe");
  await expect(view.locator("iframe")).toHaveCount(0);
  await expect(
    page.frameLocator('iframe[title$="interactive app"]').locator("iframe"),
  ).toHaveAttribute("sandbox", "allow-scripts allow-forms");

  // The tool result reached it: the form renders the securable it carries.
  await expect(view.getByText("main.sales.orders").first()).toBeVisible({ timeout: 20_000 });
  // The app's own tool calls come back through the bridge (it looks up the
  // prefilled principal).
  await expect
    .poll(() => page.evaluate(() => window.__LL_MCP_HARNESS__?.calls.map((c) => c.name) ?? []))
    .toContain("search_principals");

  // The page itself (the shell's side) saw no CSP violation.
  expect(violations).toEqual([]);
});
