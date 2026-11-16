// The smoke test's MCP App sandbox checks (src-tauri/src/mcp_app.rs), in the real
// webview: the proxy loads on its scheme and answers, an inline-script View runs
// inside it and talks through it, and a frame on the scheme has neither IPC nor a
// way around the scheme's CSP header. docs/desktop.md, "MCP App sandbox", records
// what they found per webview.

import { convertFileSrc } from "@tauri-apps/api/core";

const TIMEOUT_MS = 10_000;

/** What a frame on the scheme could reach (smoke-probe.html, and the probe View). */
interface Probe {
  tauriInternals: string;
  ipc: string;
  ipcSchemeBlocked: boolean;
  ipcHttpBlocked: boolean;
  violations?: { directive: string; policy: string }[];
}

function frame(src: string, sandbox: string): HTMLIFrameElement {
  const iframe = document.createElement("iframe");
  iframe.setAttribute("sandbox", sandbox);
  iframe.style.cssText = "position:fixed;width:1px;height:1px;opacity:0;pointer-events:none";
  iframe.src = src;
  document.body.appendChild(iframe);
  return iframe;
}

/** The next JSON-RPC message from `source` that `match` accepts. */
function message(
  source: () => Window | null,
  match: (data: Record<string, unknown>) => boolean,
): Promise<{ data: Record<string, unknown>; origin: string }> {
  return new Promise((resolve, reject) => {
    const timer = window.setTimeout(() => {
      window.removeEventListener("message", on);
      reject(new Error(`no message within ${TIMEOUT_MS} ms`));
    }, TIMEOUT_MS);
    function on(event: MessageEvent) {
      const data = event.data as Record<string, unknown> | null;
      if (event.source !== source() || !data || data.jsonrpc !== "2.0" || !match(data)) return;
      window.clearTimeout(timer);
      window.removeEventListener("message", on);
      resolve({ data, origin: event.origin });
    }
    window.addEventListener("message", on);
  });
}

function assertNoIpc(where: string, probe: Probe) {
  if (probe.tauriInternals !== "undefined") {
    throw new Error(`${where} has __TAURI_INTERNALS__ (${probe.tauriInternals})`);
  }
  if (probe.ipc !== "undefined") throw new Error(`${where} has window.ipc (${probe.ipc})`);
  if (!probe.ipcSchemeBlocked) throw new Error(`${where} reached ipc://localhost`);
  if (!probe.ipcHttpBlocked) throw new Error(`${where} reached http://ipc.localhost`);
}

/** A View with an inline script: it reports what it can reach in `ui/initialize`,
 *  then sends `initialized` once the host answers. */
const PROBE_VIEW = `<!doctype html><html><head><script>
(async () => {
  const blocked = async (url) => { try { await fetch(url, { method: "POST", body: "{}" }); return false } catch { return true } };
  const probe = {
    tauriInternals: typeof window.__TAURI_INTERNALS__,
    ipc: typeof window.ipc,
    ipcSchemeBlocked: await blocked("ipc://localhost/plugin%3Aevent%7Clisten"),
    ipcHttpBlocked: await blocked("http://ipc.localhost/plugin%3Aevent%7Clisten"),
  };
  window.addEventListener("message", (e) => {
    if (e.data && e.data.id === 1 && e.data.result) {
      window.parent.postMessage({ jsonrpc: "2.0", method: "ui/notifications/initialized", params: {} }, "*");
    }
  });
  window.parent.postMessage({ jsonrpc: "2.0", id: 1, method: "ui/initialize", params: { probe } }, "*");
})();
</script></head><body></body></html>`;

export const mcpAppChecks: [string, () => Promise<unknown>][] = [
  [
    "the MCP App sandbox loads on its scheme and announces itself",
    async () => {
      const proxy = frame(
        convertFileSrc("sandbox.html", "mcp-app"),
        "allow-scripts allow-same-origin allow-forms",
      );
      try {
        const { origin } = await message(
          () => proxy.contentWindow,
          (d) => d.method === "ui/notifications/sandbox-proxy-ready",
        );
        if (origin === window.location.origin)
          throw new Error(`the proxy shares the shell's origin`);
        return origin;
      } finally {
        proxy.remove();
      }
    },
  ],
  [
    "an inline-script View completes a handshake through the sandbox, with no IPC",
    async () => {
      const proxy = frame(
        convertFileSrc("sandbox.html", "mcp-app"),
        "allow-scripts allow-same-origin allow-forms",
      );
      const source = () => proxy.contentWindow;
      try {
        await message(source, (d) => d.method === "ui/notifications/sandbox-proxy-ready");
        const initialize = message(source, (d) => d.method === "ui/initialize");
        proxy.contentWindow?.postMessage(
          {
            jsonrpc: "2.0",
            method: "ui/notifications/sandbox-resource-ready",
            params: { html: PROBE_VIEW, csp: {} },
          },
          "*",
        );
        const { data } = await initialize;
        assertNoIpc("the View", (data.params as { probe: Probe }).probe);
        const initialized = message(source, (d) => d.method === "ui/notifications/initialized");
        proxy.contentWindow?.postMessage({ jsonrpc: "2.0", id: 1, result: {} }, "*");
        await initialized;
      } finally {
        proxy.remove();
      }
    },
  ],
  [
    "a frame on the sandbox's scheme has no IPC and its CSP header holds",
    async () => {
      const probe = frame(
        convertFileSrc("smoke-probe.html", "mcp-app"),
        "allow-scripts allow-same-origin",
      );
      try {
        const { data } = await message(
          () => probe.contentWindow,
          (d) => d.method === "smoke/probe",
        );
        const report = data.params as Probe;
        assertNoIpc("the probe", report);
        // `frame-ancestors` only comes as a header, so a violation reporting it is
        // the header's policy at work (the page has no meta CSP).
        const header = (report.violations ?? []).find((v) => v.policy.includes("frame-ancestors"));
        if (!header) {
          throw new Error(
            `no violation under the header's policy: ${JSON.stringify(report.violations)}`,
          );
        }
        return `blocked ${[...new Set(report.violations?.map((v) => v.directive))].join(", ")}`;
      } finally {
        probe.remove();
      }
    },
  ],
];
