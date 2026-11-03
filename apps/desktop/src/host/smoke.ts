// The in-app smoke test: `luminescent-desktop --smoke` runs the real binary in
// the real webview (WKWebView on macOS, WebKitGTK on Linux), these checks run
// once the UI is mounted, and the report goes back to Rust, which prints it and
// exits 0 or 1. It covers what a browser can't: IPC, the CSP, the proxy and
// the bundled assets.

import { getHost, hostFetch, ServiceUnavailableError } from "@open-lakehouse/host";
import {
  HEADWATERS,
  HEADWATERS_BASE_PATH,
  UC_API_ROOT,
  UNITY_CATALOG,
} from "../integrations/services";
import { commands, events } from "./bindings";

/** The module `just desktop-smoke` binds to its local streaming server. */
export const SMOKE_ECHO = "smoke-echo";
/** What that server streams, in this many chunks. */
export const SMOKE_ECHO_CHUNKS = 5;
/** A module nothing binds. */
const UNBOUND = "smoke-unbound";

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

/** Waits for `text` inside `selector`. */
async function waitForText(selector: string, text: string): Promise<void> {
  const deadline = Date.now() + TIMEOUT_MS;
  while (!document.querySelector(selector)?.textContent?.includes(text)) {
    if (Date.now() > deadline) {
      throw new Error(`no "${text}" in ${selector} within ${TIMEOUT_MS / 1000}s`);
    }
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
    "the shared state holds the workspace",
    async () => {
      const result = await commands.stateWorkspaces();
      if (result.status === "error") throw new Error(result.error.kind);
      const names = result.data.map((w) => w.name);
      if (!names.includes("local")) throw new Error(`workspaces: ${names.join(", ") || "none"}`);
      // For the log: what the app reads, e.g. after a CLI write.
      const members = await Promise.all(
        result.data.map(async (w) => {
          const ws = await commands.stateWorkspace(w.id);
          const envs = ws.status === "ok" ? ws.data.environments.map((e) => e.name) : [];
          return `${w.name}: ${envs.join(", ")}`;
        }),
      );
      return members.join("; ");
    },
  ],
  [
    "a write reaches the UI as state-changed",
    async () => {
      const list = await commands.stateWorkspaces();
      const ws = list.status === "ok" ? list.data[0] : undefined;
      if (!ws) throw new Error("no workspace");
      const started = performance.now();
      let unlisten: (() => void) | undefined;
      const changed = new Promise<number>((resolve) => {
        void events.stateChanged
          .listen(() => resolve(performance.now() - started))
          .then((fn) => {
            unlisten = fn;
          });
      });
      await new Promise((r) => setTimeout(r, 100)); // let the listener register
      const created = await commands.stateCreateEnvironment(ws.id, "smoke-watch", null);
      if (created.status === "error") throw new Error(created.error.kind);
      const timeout = new Promise<never>((_, reject) =>
        setTimeout(() => reject(new Error("no state-changed within 2s")), 2000),
      );
      try {
        const ms = await Promise.race([changed, timeout]);
        return `after ${Math.round(ms)} ms`;
      } finally {
        unlisten?.();
        await commands.stateDeleteEnvironment(created.data.id);
      }
    },
  ],
  [
    "the run status answers",
    async () => {
      const list = await commands.stateWorkspaces();
      const ws = list.status === "ok" ? list.data[0] : undefined;
      if (!ws) throw new Error("no workspace");
      const run = await commands.runWorkspace(ws.id);
      if (run.status === "error") throw new Error(run.error.kind);
      return run.data.engineProblem
        ? `Docker unavailable: ${run.data.engineProblem}`
        : "Docker answers";
    },
  ],
  [
    "the native planner plans the default selection",
    async () => {
      const catalog = (await commands.plannerCatalog()) as {
        modules: unknown[];
        default_selection: unknown;
      };
      const plan = await commands.plannerPlan(catalog.default_selection, null, null);
      if (plan.status === "error") throw new Error(plan.error.message);
      const files = (plan.data as { files: { path: string }[] }).files.map((f) => f.path);
      if (!files.includes("compose.yaml")) throw new Error(`files: ${files.join(", ")}`);
      return `${catalog.modules.length} modules, ${files.length} files`;
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
    // With `unity-catalog` bound (`SMOKE_SERVICES`, see scripts/smoke.sh), the
    // catalog browser lists what the service lists, through the proxy.
    "the catalog view renders",
    async () => {
      const binding = getHost()?.services.get(UNITY_CATALOG);
      click('[data-testid="nav-catalog"]');
      if (!binding) {
        await waitFor(`[data-testid="unbound-${UNITY_CATALOG}"]`);
        return "unbound";
      }
      const response = await binding.fetch(`${UC_API_ROOT}/catalogs`);
      const { catalogs = [] } = (await response.json()) as { catalogs?: { name: string }[] };
      const first = catalogs[0]?.name;
      if (first) await waitForText('[data-testid="catalog-view"]', first);
      else await waitFor('[data-testid="catalog-view"]');
      return `${catalogs.length} catalog(s)`;
    },
  ],
  [
    "the lineage view renders",
    async () => {
      const binding = getHost()?.services.get(HEADWATERS);
      click('[data-testid="nav-lineage"]');
      if (!binding) {
        await waitFor(`[data-testid="unbound-${HEADWATERS}"]`);
        click('[data-testid="nav-workspace"]');
        return "unbound";
      }
      const response = await binding.fetch(
        `${HEADWATERS_BASE_PATH}/headwaters.read.v1.ReadService/ListDatasets`,
        { method: "POST", headers: { "content-type": "application/json" }, body: "{}" },
      );
      const { datasets = [] } = (await response.json()) as { datasets?: { name: string }[] };
      await waitFor('[data-testid="lineage-view"]');
      const first = datasets[0]?.name;
      if (first) {
        click('[data-testid="lineage-page-datasets"]');
        await waitForText('[data-testid="lineage-view"]', first);
      }
      click('[data-testid="nav-workspace"]');
      return `${datasets.length} dataset(s)`;
    },
  ],
  [
    "an unbound service is ServiceUnavailableError",
    async () => {
      const error = await hostFetch(UNBOUND)("/anything").then(
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
        { moduleId: UNBOUND, method: "GET", path: "/", headers: [], body: null },
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

/** Wait for `selector`'s text to be `text`, up to `ms`; `fail` aborts early. */
async function waitForText(
  selector: string,
  text: string,
  ms: number,
  fail?: () => string | null,
): Promise<void> {
  const deadline = Date.now() + ms;
  for (;;) {
    const el = document.querySelector(selector);
    if (el?.textContent === text) return;
    const problem = fail?.();
    if (problem) throw new Error(problem);
    if (Date.now() > deadline) {
      throw new Error(`${selector} is "${el?.textContent}", not "${text}", after ${ms / 1000}s`);
    }
    await new Promise((r) => setTimeout(r, 250));
  }
}

const runAlert = () =>
  document.querySelector('[data-testid="run-dev"] [role="alert"]')?.textContent ?? null;

/** Through the Run view, as a user would: commit dev, Start, wait for healthy, Stop. */
const dockerChecks: [string, () => Promise<unknown>][] = [
  [
    "Start runs dev on Docker until every container is healthy",
    async () => {
      const list = await commands.stateWorkspaces();
      const ws = list.status === "ok" ? list.data.find((w) => w.name === "local") : undefined;
      if (!ws) throw new Error("no local workspace");
      const snapshot = await commands.stateWorkspace(ws.id);
      const dev =
        snapshot.status === "ok"
          ? snapshot.data.environments.find((e) => e.name === "dev")
          : undefined;
      if (!dev) throw new Error("no dev");
      if (dev.head == null) {
        const committed = await commands.stateCommit(dev.id);
        if (committed.status === "error") throw new Error(committed.error.kind);
      }
      click('[data-testid="nav-run"]');
      await waitFor('[data-testid="run-dev-start"]:not([disabled])');
      const started = Date.now();
      click('[data-testid="run-dev-start"]');
      click('[aria-label="dev containers"]');
      try {
        await waitForText('[data-testid="run-dev-state"]', "running", 840_000, runAlert);
      } catch (e) {
        const table = document.querySelector('[data-testid="run-services"]')?.textContent;
        throw new Error(`${e instanceof Error ? e.message : e}; containers: ${table ?? "none"}`);
      }
      const url = document.querySelector('[data-testid="gateway-url"]')?.textContent ?? "";
      if (!url.startsWith("http://localhost:")) throw new Error(`gateway: "${url}"`);
      return `${Math.round((Date.now() - started) / 1000)} s, ${url}`;
    },
  ],
  [
    "Stop takes it down",
    async () => {
      // A failed Start leaves its alert up; only a new one (Stop's own) counts.
      const before = runAlert();
      click('[data-testid="run-dev-stop"]');
      await waitForText('[data-testid="run-dev-state"]', "stopped", 120_000, () => {
        const now = runAlert();
        return now !== before ? now : null;
      });
    },
  ],
];

export async function runSmoke(violations: string[]) {
  const passed: string[] = [];
  const failures: string[] = [];
  const all = window.__LL_SMOKE_DOCKER__ ? [...checks, ...dockerChecks] : checks;
  for (const [name, check] of all) {
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
