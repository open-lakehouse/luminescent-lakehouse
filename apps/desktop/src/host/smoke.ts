// The in-app smoke test: `luminescent-desktop --smoke` runs the real binary in
// the real webview (WKWebView on macOS, WebKitGTK on Linux), these checks run
// once the UI is mounted, and the report goes back to Rust, which prints it and
// exits 0 or 1. It covers what a browser can't: IPC, the CSP, the proxy and
// the bundled assets.

import { getHost, hostFetch, ServiceUnavailableError } from "@open-lakehouse/host";
import { commands } from "./bindings";

/** The module `just desktop-smoke` binds to its local streaming server. */
export const SMOKE_ECHO = "smoke-echo";
/** What that server streams, in this many chunks. */
export const SMOKE_ECHO_CHUNKS = 5;

const TIMEOUT_MS = 20_000;

async function waitFor(selector: string): Promise<Element> {
  const deadline = Date.now() + TIMEOUT_MS;
  for (;;) {
    const found = document.querySelector(selector);
    if (found) return found;
    if (Date.now() > deadline) throw new Error(`no ${selector} within ${TIMEOUT_MS / 1000}s`);
    await new Promise((r) => setTimeout(r, 50));
  }
}

function click(selector: string) {
  const el = document.querySelector<HTMLElement>(selector);
  if (!el) throw new Error(`no ${selector}`);
  el.click();
}

// A check may return a detail for the report line.
const checks: [string, () => Promise<unknown>][] = [
  [
    "the shell mounts",
    async () => {
      await waitFor('[data-testid="shell"]');
    },
  ],
  [
    "the workspace shell renders the starter workspace",
    async () => {
      await waitFor('[data-testid="workspace-view"] [aria-label="Views"]');
    },
  ],
  [
    "the doctor command answers",
    async () => {
      const result = await commands.doctor();
      if (result.status === "error") throw new Error(result.error);
      const report = result.data;
      if (!Array.isArray(report.path)) throw new Error("report has no PATH");
      // For the log: what this machine (or runner) has.
      const where = (t: typeof report.docker) => (t.status === "missing" ? "missing" : t.path);
      return `docker ${where(report.docker)}, daemon ${report.daemon.status}, neon ${where(report.neon)}; PATH ${report.path.slice(0, 3).join(":")}…`;
    },
  ],
  [
    "the doctor view renders",
    async () => {
      click('[data-testid="nav-doctor"]');
      await waitFor('[data-testid="doctor-daemon"]');
      click('[data-testid="nav-workspace"]');
    },
  ],
  [
    "an unbound service is ServiceUnavailableError",
    async () => {
      const error = await hostFetch("unity-catalog")("/api/2.1/unity-catalog/catalogs").then(
        () => null,
        (e: unknown) => e,
      );
      if (!(error instanceof ServiceUnavailableError)) {
        throw new Error(`expected ServiceUnavailableError, got ${String(error)}`);
      }
    },
  ],
  [
    "Rust refuses an unbound module",
    async () => {
      const { Channel } = await import("@tauri-apps/api/core");
      const result = await commands.serviceFetch(
        0,
        { moduleId: "unity-catalog", method: "GET", path: "/", headers: [], body: null },
        new Channel(),
      );
      if (result.status !== "error" || result.error.kind !== "unbound") {
        throw new Error(`expected unbound, got ${JSON.stringify(result)}`);
      }
    },
  ],
  [
    "a bound service streams through the proxy",
    async () => {
      const binding = getHost()?.services.get(SMOKE_ECHO);
      if (!binding) throw new Error(`${SMOKE_ECHO} is not bound (run through just desktop-smoke)`);
      const response = await binding.fetch("/stream?chunks=5");
      if (response.status !== 200) throw new Error(`status ${response.status}`);
      const reader = response.body?.getReader();
      if (!reader) throw new Error("no body");
      let chunks = 0;
      let text = "";
      const decoder = new TextDecoder();
      for (;;) {
        const { done, value } = await reader.read();
        if (done) break;
        chunks++;
        text += decoder.decode(value, { stream: true });
      }
      const expected = Array.from({ length: SMOKE_ECHO_CHUNKS }, (_, i) => `chunk ${i}\n`).join("");
      if (text !== expected) throw new Error(`body ${JSON.stringify(text)}`);
      if (chunks < 2) throw new Error(`body arrived in ${chunks} chunk(s), not streamed`);
      return `${chunks} chunks`;
    },
  ],
];

export async function runSmoke(violations: string[]) {
  const passed: string[] = [];
  const failures: string[] = [];
  for (const [name, check] of checks) {
    try {
      const detail = await check();
      passed.push(typeof detail === "string" ? `${name} (${detail})` : name);
    } catch (e) {
      failures.push(`${name}: ${e instanceof Error ? e.message : String(e)}`);
    }
  }
  if (violations.length === 0) passed.push("no CSP violations");
  else failures.push(...violations.map((v) => `CSP: ${v}`));
  await commands.smokeReport({ passed, failures });
}
