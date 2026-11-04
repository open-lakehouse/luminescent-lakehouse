// Entry point. Order matters: the host is registered before the UI is imported,
// so service clients that resolve it at import time (`hostFetch`, the
// packages' `register*` shims) bind to the desktop's.

import { registerHost } from "@open-lakehouse/host";

declare global {
  interface Window {
    /** Set by src-tauri when started with `--smoke` (see src/host/smoke.ts). */
    __LL_SMOKE__?: boolean;
    /** And with `LUMINESCENT_SMOKE_DOCKER=start|resume`: that phase on Docker too. */
    __LL_SMOKE_DOCKER__?: "start" | "resume" | null;
    /** Knob overrides for the Docker phases, `module.knob=value,…`. */
    __LL_SMOKE_KNOBS__?: string;
    /** The Docker phases add headwaters and check the Lineage view. */
    __LL_SMOKE_LINEAGE__?: boolean;
    /** A Neon branch the smoke test makes the current environment (`SMOKE_NEON_BRANCH`). */
    __LL_SMOKE_NEON__?: string | null;
  }
}

async function start() {
  const smoke = window.__LL_SMOKE__ === true;
  // Record CSP violations from the very start, for the smoke check.
  const violations: string[] = [];
  if (smoke) {
    document.addEventListener("securitypolicyviolation", (e) => {
      violations.push(`${e.violatedDirective} blocked ${e.blockedURI || "inline"}`);
    });
  }
  // Playwright runs the UI in a plain browser, against a mocked IPC.
  if (import.meta.env.MODE === "e2e") {
    const { installIpcMock } = await import("./host/ipc-mock");
    installIpcMock();
  }

  const { createDesktopHosts } = await import("./host/desktop-host");
  const { lakehouse, shell } = await createDesktopHosts();
  registerHost(lakehouse);
  // The service UIs' clients, onto the host's bindings.
  const { bindServiceClients } = await import("./integrations/bind");
  bindServiceClients();

  const root = document.getElementById("root");
  if (!root) throw new Error("missing #root element");
  const { mount } = await import("./app");
  mount(root, lakehouse, shell);

  if (smoke) {
    const { runSmoke } = await import("./host/smoke");
    await runSmoke(violations);
  }
}

void start();
