// The fakes' MCP Apps sandbox: the same page as the desktop's `mcp-app://` scheme,
// on a second origin of the page's own server.

/** The sandbox page on a second origin, as the desktop's scheme is: the page's
 *  server's port + 1, which the Playwright build serves it on (vite.config.ts). */
export function fakeMcpAppSandboxUrl(): string {
  const url = new URL("/mcp-app/sandbox.html", window.location.href);
  url.port = String(Number(url.port || (url.protocol === "https:" ? 443 : 80)) + 1);
  return url.href;
}
