// The IPC a Playwright run (`vite build --mode e2e`) answers in place of Rust:
// the UI is exercised in a plain browser, the commands return fixtures. main.ts
// imports this only in that mode, so production builds don't contain it.

import { mockIPC } from "@tauri-apps/api/mocks";
import { READY_REPORT } from "../shell/fixtures";
import type { ProxyError } from "./bindings";

export function installIpcMock() {
  mockIPC(async (cmd, payload) => {
    switch (cmd) {
      case "doctor":
        return READY_REPORT;
      case "list_services":
        return [];
      case "service_fetch": {
        const { request } = payload as { request: { moduleId: string } };
        const error: ProxyError = { kind: "unbound", moduleId: request.moduleId };
        throw error;
      }
      case "service_fetch_cancel":
        return null;
      default:
        throw new Error(`ipc-mock: unexpected command ${cmd}`);
    }
  });
}
