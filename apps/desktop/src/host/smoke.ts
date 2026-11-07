// The in-app smoke test: `luminescent-desktop --smoke` runs the real binary in
// the real webview (WKWebView on macOS, WebKitGTK on Linux), these checks run
// once the UI is mounted, and the report goes back to Rust, which prints it and
// exits 0 or 1. It covers what a browser can't: IPC, the CSP, the proxy and
// the bundled assets.

import { getHost, hostFetch, ServiceUnavailableError } from "@open-lakehouse/host";
import {
  HEADWATERS,
  HEADWATERS_READ_SERVICE,
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
async function waitForContent(selector: string, text: string): Promise<void> {
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

/** Open a view of the editor's activity bar (Environments, Doctor, Neon, …). */
function openView(label: string) {
  click(`nav[aria-label="Views"] button[aria-label="${label}"]`);
}

const SUB_TABS = '[role="tablist"][aria-label="Environment views"]';

/** The open environment's header: its state, revision, placement and actions. */
function envHeader(): HTMLElement | null {
  return (document.querySelector(SUB_TABS)?.previousElementSibling as HTMLElement | null) ?? null;
}

/** The open environment's state, as its header's dot says (`running`, `failed`, …). */
function envState(): string | null {
  return envHeader()?.querySelector("[data-run-state]")?.getAttribute("data-run-state") ?? null;
}

/** The header's button labelled `label`, once there and enabled. */
async function headerButton(label: string): Promise<HTMLButtonElement> {
  const deadline = Date.now() + TIMEOUT_MS;
  for (;;) {
    const button = [...(envHeader()?.querySelectorAll<HTMLButtonElement>("button") ?? [])].find(
      (b) => b.textContent?.trim() === label,
    );
    if (button && !button.disabled) return button;
    if (Date.now() > deadline) {
      throw new Error(`no enabled ${label} in the header ("${envHeader()?.textContent}")`);
    }
    await new Promise((r) => setTimeout(r, 50));
  }
}

/** The open environment's sub-tab `name` (Design, Catalog, Lineage, Logs). */
function subTab(name: string): HTMLElement | undefined {
  return [...document.querySelectorAll<HTMLElement>(`${SUB_TABS} [role="tab"]`)].find(
    (t) => t.textContent?.trim() === name,
  );
}

async function openSubTab(name: string) {
  const deadline = Date.now() + TIMEOUT_MS;
  for (;;) {
    const tab = subTab(name);
    if (tab && tab.getAttribute("aria-disabled") !== "true") {
      tab.click();
      return;
    }
    if (Date.now() > deadline) {
      throw new Error(`${name} isn't there to open (${tab?.getAttribute("title") ?? "no tab"})`);
    }
    await new Promise((r) => setTimeout(r, 50));
  }
}

/** Show the bottom panel's Run view (the open environment's revisions and containers). */
function openRunPanel() {
  const run = [...document.querySelectorAll<HTMLElement>("button")].find(
    (b) => b.textContent?.trim() === "Run",
  );
  if (!run) throw new Error("no Run panel");
  run.click();
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
    "the explorer removes an environment from the shared state",
    async () => {
      const list = await commands.stateWorkspaces();
      const ws = list.status === "ok" ? list.data[0] : undefined;
      if (!ws) throw new Error("no workspace");
      const created = await commands.stateCreateEnvironment(ws.id, "smoke-remove", null);
      if (created.status === "error") throw new Error(created.error.kind);
      const row = () =>
        [...document.querySelectorAll<HTMLElement>('[role="tree"] [role="treeitem"]')].find((el) =>
          el.textContent?.includes("smoke-remove"),
        );
      const until = async (what: string, ok: () => boolean | Promise<boolean>) => {
        const deadline = Date.now() + TIMEOUT_MS;
        while (!(await ok())) {
          if (Date.now() > deadline) throw new Error(`${what} within ${TIMEOUT_MS / 1000}s`);
          await new Promise((r) => setTimeout(r, 50));
        }
      };
      await until("no smoke-remove row", () => !!row());
      const remove = row()?.querySelector<HTMLElement>('button[aria-label="Remove"]');
      if (!remove) throw new Error("no Remove button on the row");
      remove.click();
      await until(
        "no removal confirmation",
        () => !!document.querySelector('[role="alertdialog"]'),
      );
      const confirm = [
        ...document.querySelectorAll<HTMLElement>('[role="alertdialog"] button'),
      ].find((b) => b.textContent === "Remove");
      if (!confirm) throw new Error("no Remove button in the confirmation");
      confirm.click();
      await until("the row stays", () => !row());
      await until("the store keeps it", async () => {
        const now = await commands.stateWorkspace(ws.id);
        return now.status === "ok" && !now.data.environments.some((e) => e.name === "smoke-remove");
      });
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
      openView("Doctor");
      await waitFor('[data-testid="doctor-daemon"]');
      openView("Environments");
    },
  ],
  [
    "a stopped environment's Catalog and Lineage say why they can't open",
    async () => {
      await waitFor(SUB_TABS);
      const state = envState();
      if (state !== "stopped") return `skipped (dev is ${state})`;
      for (const name of ["Catalog", "Lineage"]) {
        const tab = subTab(name);
        if (tab?.getAttribute("aria-disabled") !== "true") throw new Error(`${name} is enabled`);
        const reason = tab.getAttribute("title");
        if (reason !== "dev isn't running: Start it") throw new Error(`${name}: "${reason}"`);
      }
    },
  ],
  [
    // With `unity-catalog` bound (by hand with `SMOKE_SERVICES`, see scripts/smoke.sh,
    // or by a running instance), the proxy reaches it; the tabs are the Docker phases'.
    "a bound Unity Catalog answers through the proxy",
    async () => {
      const binding = getHost()?.services.get(UNITY_CATALOG);
      if (!binding) return "unbound";
      const response = await binding.fetch(`${UC_API_ROOT}/catalogs`);
      const { catalogs = [] } = (await response.json()) as { catalogs?: { name: string }[] };
      return `${catalogs.length} catalog(s)`;
    },
  ],
  [
    "a bound headwaters answers through the proxy",
    async () => {
      const binding = getHost()?.services.get(HEADWATERS);
      if (!binding) return "unbound";
      const response = await binding.fetch(`${HEADWATERS_READ_SERVICE}/ListDatasets`, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: "{}",
      });
      const { datasets = [] } = (await response.json()) as { datasets?: { name: string }[] };
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

/** What the last Start or Stop failed with, from the Run panel. */
const runAlert = () =>
  document.querySelector('[data-testid="run-panel-dev"] [role="alert"]')?.textContent ?? null;

/** Catalogs the Docker phases create, one per revision's Unity Catalog. */
const R1_CATALOG = "smoke_r1";
const R2_CATALOG = "smoke_r2";
/** The dataset the start phase posts to revision 2's headwaters. */
const LINEAGE_NAMESPACE = "smoke";
const LINEAGE_DATASET = "smoke.lineage_check";

/** `module.knob=value,…` as a selection's knob overrides. */
function knobOverrides(spec: string): Record<string, Record<string, string>> {
  const out: Record<string, Record<string, string>> = {};
  for (const part of spec
    .split(",")
    .map((p) => p.trim())
    .filter(Boolean)) {
    const eq = part.indexOf("=");
    const dot = part.indexOf(".");
    if (eq < 0 || dot < 0 || dot > eq) throw new Error(`bad knob \`${part}\``);
    const module = part.slice(0, dot);
    out[module] = { ...out[module], [part.slice(dot + 1, eq)]: part.slice(eq + 1) };
  }
  return out;
}

async function localDev() {
  const list = await commands.stateWorkspaces();
  const ws = list.status === "ok" ? list.data.find((w) => w.name === "local") : undefined;
  if (!ws) throw new Error("no local workspace");
  const snapshot = await commands.stateWorkspace(ws.id);
  const dev =
    snapshot.status === "ok" ? snapshot.data.environments.find((e) => e.name === "dev") : undefined;
  if (!dev) throw new Error("no dev");
  return dev;
}

/** Wait until `revision`'s row in the Run panel says running; its containers on
 *  failure. Resolves to its gateway URL. */
async function waitRunning(revision: number) {
  const row = `[data-testid="run-dev-r${revision}"]`;
  try {
    await waitFor(row);
    await waitForText(
      `${row} [data-testid="run-dev-r${revision}-state"]`,
      "running",
      840_000,
      () => (envState() === "failed" ? (runAlert() ?? "the start failed") : null),
    );
  } catch (e) {
    const table = document.querySelector(`${row} [data-testid="run-services"]`)?.textContent;
    throw new Error(`${e instanceof Error ? e.message : e}; containers: ${table ?? "none"}`);
  }
  return document.querySelector(`${row} [data-testid="gateway-url"]`)?.textContent ?? "";
}

/** Create `name` in the current environment's Unity Catalog, unless it's there (a
 *  data volume kept from an earlier run). */
async function ensureCatalog(name: string) {
  const uc = getHost()?.services.get(UNITY_CATALOG);
  if (!uc) throw new Error("unity-catalog isn't bound");
  const got = await uc.fetch(`${UC_API_ROOT}/catalogs/${name}`);
  if (got.ok) return;
  const created = await uc.fetch(`${UC_API_ROOT}/catalogs`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ name }),
  });
  if (!created.ok) throw new Error(`creating ${name}: ${created.status} ${await created.text()}`);
}

/** The environment's Catalog tab's text once it lists `name`. */
async function catalogViewWith(name: string): Promise<string> {
  await openSubTab("Catalog");
  await waitForContent('[data-testid="catalog-view"]', name);
  const text = document.querySelector('[data-testid="catalog-view"]')?.textContent ?? "";
  await openSubTab("Design");
  return text;
}

async function waitCurrentRevision(revision: number, ms = 10_000) {
  const deadline = Date.now() + ms;
  for (;;) {
    const current = getHost()?.environment.current();
    if (current?.name === "dev" && current.revision === revision) return current;
    if (Date.now() > deadline) {
      throw new Error(`dev #${revision} isn't current (${current?.name} #${current?.revision})`);
    }
    await new Promise((r) => setTimeout(r, 100));
  }
}

/**
 * The first launch: through the Run view, revision 1 of dev, then revision 2 (with
 * headwaters, under LUMINESCENT_SMOKE_LINEAGE) next to it. Each gets a catalog of its
 * own, so the views show that no data crosses between them. They stay running for the
 * second launch.
 */
const dockerStartChecks: [string, () => Promise<unknown>][] = [
  [
    "Start runs dev #1 on Docker until every container is healthy",
    async () => {
      const dev = await localDev();
      if (dev.head == null) {
        const selection = dev.selection as { knob_overrides?: Record<string, unknown> };
        const knobs = knobOverrides(window.__LL_SMOKE_KNOBS__ ?? "");
        const set = await commands.stateSetSelection(dev.id, {
          ...selection,
          knob_overrides: { ...selection.knob_overrides, ...knobs },
        });
        if (set.status === "error") throw new Error(set.error.kind);
        const committed = await commands.stateCommit(dev.id);
        if (committed.status === "error") throw new Error(committed.error.kind);
      }
      const start = await headerButton("Start");
      openRunPanel();
      const started = Date.now();
      start.click();
      const url = await waitRunning(1);
      if (!url.startsWith("http://localhost:")) throw new Error(`gateway: "${url}"`);
      return `${Math.round((Date.now() - started) / 1000)} s, ${url}`;
    },
  ],
  [
    "the started instance is the current environment, its gateway's services bound",
    async () => {
      await waitCurrentRevision(1, 5000);
      const bound = (await commands.listServices()).map((s) => s.moduleId);
      if (!bound.includes(UNITY_CATALOG)) throw new Error(`bound: ${bound.join(", ")}`);
      return bound.join(", ");
    },
  ],
  [
    "the Catalog view shows dev #1's Unity Catalog",
    async () => {
      await ensureCatalog(R1_CATALOG);
      await catalogViewWith(R1_CATALOG);
      return R1_CATALOG;
    },
  ],
  [
    "Logs follows dev #1's containers",
    async () => {
      await openSubTab("Logs");
      await waitForContent('[data-testid="logs-lines"]', "|");
      const lines = document.querySelectorAll('[data-testid="logs-lines"] > div').length;
      await openSubTab("Design");
      return `${lines} lines`;
    },
  ],
  [
    "dev #2 starts next to #1, on its own slot",
    async () => {
      const dev = await localDev();
      const selection = dev.selection as { modules: string[] };
      // A second revision: headwaters, or else a module dropped and back is no change,
      // so the lineage-less run adds the knob a revision must differ by.
      const next = window.__LL_SMOKE_LINEAGE__
        ? { ...selection, modules: [...new Set([...selection.modules, HEADWATERS])] }
        : {
            ...selection,
            knob_overrides: {
              ...(selection as { knob_overrides?: Record<string, Record<string, string>> })
                .knob_overrides,
              postgres: {
                ...(selection as { knob_overrides?: Record<string, Record<string, string>> })
                  .knob_overrides?.postgres,
                data: "volume",
              },
            },
          };
      const set = await commands.stateSetSelection(dev.id, next);
      if (set.status === "error") throw new Error(set.error.kind);
      const committed = await commands.stateCommit(dev.id);
      if (committed.status === "error") throw new Error(`commit: ${committed.error.kind}`);
      (await headerButton("Start #2")).click();
      const url = await waitRunning(2);
      const first = document.querySelector('[data-testid="run-dev-r1-state"]')?.textContent;
      if (first !== "running") throw new Error(`#1 is ${first}`);
      await waitCurrentRevision(2);
      return url;
    },
  ],
  [
    "the Catalog view shows dev #2's catalog, and none of #1's",
    async () => {
      await ensureCatalog(R2_CATALOG);
      const text = await catalogViewWith(R2_CATALOG);
      if (text.includes(R1_CATALOG)) throw new Error(`${R1_CATALOG} shows under #2`);
      return R2_CATALOG;
    },
  ],
  [
    "the Lineage view reads dev #2's headwaters at <gateway>/lineage/",
    async () => {
      if (!window.__LL_SMOKE_LINEAGE__) return "skipped (LUMINESCENT_SMOKE_LINEAGE unset)";
      const headwaters = getHost()?.services.get(HEADWATERS);
      if (!headwaters) throw new Error("headwaters isn't bound");
      const now = new Date().toISOString();
      const event = {
        eventType: "COMPLETE",
        eventTime: now,
        producer: "https://github.com/open-lakehouse/luminescent-lakehouse/apps/desktop/smoke",
        schemaURL: "https://openlineage.io/spec/2-0-2/OpenLineage.json#/definitions/RunEvent",
        run: { runId: crypto.randomUUID() },
        job: { namespace: LINEAGE_NAMESPACE, name: "smoke_job" },
        inputs: [],
        outputs: [{ namespace: LINEAGE_NAMESPACE, name: LINEAGE_DATASET }],
      };
      const posted = await headwaters.fetch("/api/v1/lineage/batch", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify([event]),
      });
      if (!posted.ok) throw new Error(`posting lineage: ${posted.status} ${await posted.text()}`);
      // headwaters projects events into datasets on its own time: wait on its read API.
      const posted_at = Date.now();
      for (;;) {
        const listed = await headwaters.fetch(`${HEADWATERS_READ_SERVICE}/ListDatasets`, {
          method: "POST",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({ namespace: LINEAGE_NAMESPACE }),
        });
        if (!listed.ok)
          throw new Error(`listing datasets: ${listed.status} ${await listed.text()}`);
        const { datasets = [] } = (await listed.json()) as {
          datasets?: { id?: { name?: string } }[];
        };
        if (datasets.some((d) => d.id?.name === LINEAGE_DATASET)) break;
        if (Date.now() - posted_at > 90_000) {
          throw new Error(`${LINEAGE_DATASET} isn't listed 90 s after posting it`);
        }
        await new Promise((r) => setTimeout(r, 1000));
      }
      const projected = Date.now() - posted_at;
      await openSubTab("Lineage");
      await waitFor('[data-testid="lineage-view"]');
      click('[data-testid="lineage-page-datasets"]');
      await waitForContent('[data-testid="lineage-view"]', LINEAGE_DATASET);
      await openSubTab("Design");
      return `${LINEAGE_DATASET}, listed after ${projected} ms`;
    },
  ],
];

/**
 * The second launch, on the same state: the app reconciles with what still runs,
 * makes the last current instance current again, switches back to #1 with its data,
 * and stops both.
 */
const dockerResumeChecks: [string, () => Promise<unknown>][] = [
  [
    "the launch adopted both running revisions",
    async () => {
      const deadline = Date.now() + 60_000;
      for (;;) {
        const report = await commands.runReconciled();
        if (report) {
          const adopted = report.adopted.map((i) => i.label);
          for (const want of ["local/dev #1", "local/dev #2"]) {
            if (!adopted.includes(want)) {
              throw new Error(
                `adopted: ${adopted.join(", ") || "none"}; stale: ${report.stale.map((i) => i.label).join(", ")}`,
              );
            }
          }
          return adopted.join(", ");
        }
        if (Date.now() > deadline) throw new Error("no reconcile within 60 s");
        await new Promise((r) => setTimeout(r, 250));
      }
    },
  ],
  [
    "the last current instance, dev #2, is current again",
    async () => {
      const current = await waitCurrentRevision(2, 20_000);
      return getHost()?.environment.key ?? current.id;
    },
  ],
  [
    "#1 in dev's revision switch: its Catalog shows #1's catalog, and none of #2's",
    async () => {
      (await headerButton("#1")).click();
      await openSubTab("Catalog");
      await waitCurrentRevision(1);
      await openSubTab("Design");
      const text = await catalogViewWith(R1_CATALOG);
      if (text.includes(R2_CATALOG)) throw new Error(`${R2_CATALOG} shows under #1`);
      return R1_CATALOG;
    },
  ],
  [
    "Stop all takes both revisions down",
    async () => {
      const stop = await headerButton("Stop all");
      openRunPanel();
      stop.click();
      const deadline = Date.now() + 180_000;
      while (envState() !== "stopped") {
        if (envState() === "failed") throw new Error(runAlert() ?? "the stop failed");
        if (Date.now() > deadline) throw new Error(`dev is ${envState()} after 180 s`);
        await new Promise((r) => setTimeout(r, 250));
      }
      const left = document.querySelectorAll('[data-testid^="run-dev-r"][data-testid$="-state"]');
      if (left.length > 0) throw new Error(`${left.length} revisions still listed`);
    },
  ],
];

/** What `just desktop-demo-seed` creates, for the views to show. */
const DEMO_CATALOG = "demo";
const DEMO_SCHEMA = "sales";
const DEMO_DATASET = "demo.sales.daily_revenue";

const DEMO_JOB = "build_daily_revenue";
/** The environment the Neon phase runs: Unity Catalog and headwaters as Neon Functions. */
const NEON_ENV = "neon-demo";

/** Open environment `name`'s tab through the ⌘K palette's Go to. */
async function openEnvironment(name: string) {
  window.dispatchEvent(new KeyboardEvent("keydown", { key: "k", metaKey: true, bubbles: true }));
  const deadline = Date.now() + TIMEOUT_MS;
  for (;;) {
    const item = [...document.querySelectorAll<HTMLElement>('[role="dialog"] *')].find(
      (el) => el.children.length === 0 && el.textContent?.trim() === `Go to ${name}`,
    );
    if (item) {
      item.click();
      break;
    }
    if (Date.now() > deadline) throw new Error(`no "Go to ${name}" in the palette`);
    await new Promise((r) => setTimeout(r, 50));
  }
  await waitForContent("body", name);
}

/**
 * The Neon phase (needs Docker for the gateway, and the Neon CLI signed in): an
 * environment whose Unity Catalog and headwaters run as functions starts on branch
 * `branch` through Start's dialog, and its tabs show the branch's seeded data.
 */
function neonChecks(branch: string): [string, () => Promise<unknown>][] {
  return [
    [
      `${NEON_ENV} starts on Neon branch ${branch}, its functions bound`,
      async () => {
        const list = await commands.stateWorkspaces();
        const ws = list.status === "ok" ? list.data.find((w) => w.name === "local") : undefined;
        if (!ws) throw new Error("no local workspace");
        const snapshot = await commands.stateWorkspace(ws.id);
        let env =
          snapshot.status === "ok"
            ? snapshot.data.environments.find((e) => e.name === NEON_ENV)
            : undefined;
        if (!env) {
          const fn = { vendor: "neon-function", connection: { kind: "function" } };
          const created = await commands.stateCreateEnvironment(ws.id, NEON_ENV, {
            modules: [UNITY_CATALOG, HEADWATERS],
            hosted: { [UNITY_CATALOG]: fn, [HEADWATERS]: fn },
          });
          if (created.status === "error") throw new Error(stateErrorText(created.error));
          env = created.data;
        }
        if (env.head == null) {
          const committed = await commands.stateCommit(env.id);
          if (committed.status === "error") throw new Error(stateErrorText(committed.error));
        }
        openView("Environments");
        await openEnvironment(NEON_ENV);
        (await headerButton("Start")).click();
        await waitFor(`[data-testid="neon-start-${CSS.escape(branch)}"]`);
        click(`[data-testid="neon-start-${CSS.escape(branch)}"]`);
        click('[data-testid="neon-start-go"]');
        const deadline = Date.now() + 840_000;
        while (envState() !== "running") {
          if (envState() === "failed") throw new Error(envHeader()?.textContent ?? "failed");
          if (Date.now() > deadline) throw new Error(`${NEON_ENV} is ${envState()}`);
          await new Promise((r) => setTimeout(r, 250));
        }
        if (!envHeader()?.textContent?.includes(`☁ ${branch}`)) {
          throw new Error(`header: ${envHeader()?.textContent}`);
        }
        await openSubTab("Catalog");
        const bound = await (async () => {
          const until = Date.now() + TIMEOUT_MS;
          for (;;) {
            const now = (await commands.listServices()).map(
              (s) => `${s.moduleId} (${s.placement})`,
            );
            if (now.some((b) => b.startsWith(`${UNITY_CATALOG} (remote`))) return now;
            if (Date.now() > until) throw new Error(`bound: ${now.join(", ")}`);
            await new Promise((r) => setTimeout(r, 100));
          }
        })();
        return bound.join(", ");
      },
    ],
    [
      `${NEON_ENV}'s Catalog shows the branch's seeded catalog`,
      async () => {
        const started = Date.now();
        await waitForContent('[data-testid="catalog-view"]', DEMO_CATALOG);
        const uc = getHost()?.services.get(UNITY_CATALOG);
        if (!uc) throw new Error(`${UNITY_CATALOG} isn't bound`);
        const response = await uc.fetch(
          `${UC_API_ROOT}/tables?catalog_name=${DEMO_CATALOG}&schema_name=${DEMO_SCHEMA}`,
        );
        if (!response.ok) throw new Error(`listing tables: ${response.status}`);
        const { tables = [] } = (await response.json()) as { tables?: { name: string }[] };
        if (tables.length === 0) throw new Error(`no tables in ${DEMO_CATALOG}.${DEMO_SCHEMA}`);
        return `${Date.now() - started} ms; ${DEMO_CATALOG}.${DEMO_SCHEMA}: ${tables.map((t) => t.name).join(", ")}`;
      },
    ],
    [
      `${NEON_ENV}'s Lineage shows the branch's seeded datasets and jobs`,
      async () => {
        await openSubTab("Lineage");
        await waitFor('[data-testid="lineage-view"]');
        click('[data-testid="lineage-page-datasets"]');
        await waitForContent('[data-testid="lineage-view"]', DEMO_DATASET);
        click('[data-testid="lineage-page-jobs"]');
        await waitForContent('[data-testid="lineage-view"]', DEMO_JOB);
        await openSubTab("Design");
        (await headerButton("Stop")).click();
        const deadline = Date.now() + 180_000;
        while (envState() !== "stopped") {
          if (Date.now() > deadline) throw new Error(`${NEON_ENV} is ${envState()} after 180 s`);
          await new Promise((r) => setTimeout(r, 250));
        }
        return `${DEMO_DATASET}, ${DEMO_JOB}`;
      },
    ],
  ];
}

/** A state error's text, for a check's failure. */
function stateErrorText(error: { kind: string; message?: string }): string {
  return error.message ?? error.kind;
}

export async function runSmoke(violations: string[]) {
  const passed: string[] = [];
  const failures: string[] = [];
  const neon = window.__LL_SMOKE_NEON__;
  const all = [
    ...checks,
    ...(window.__LL_SMOKE_DOCKER__ === "start" ? dockerStartChecks : []),
    ...(window.__LL_SMOKE_DOCKER__ === "resume" ? dockerResumeChecks : []),
    ...(neon ? neonChecks(neon) : []),
  ];
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
