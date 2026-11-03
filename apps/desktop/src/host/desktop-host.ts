// The desktop's hosts: the `LakehouseHost` every service UI binds through
// (`@open-lakehouse/host`), and the `ShellHost` with the shell's own commands.
// This directory is the only place that imports `@tauri-apps/api`; the UI
// under src/shell and src/integrations only sees these interfaces, so it runs
// on fakes in tests.

import type {
  EnvironmentScope,
  LakehouseHost,
  ServiceBinding,
  ServiceDirectory,
} from "@open-lakehouse/host";
import type { ShellHost } from "../shell/shell-host";
import { commands, type StateError } from "./bindings";
import { createNativePlanner } from "./native-planner";
import { serviceFetch } from "./service-fetch";
import {
  createStateWorkspaceStore,
  listenStateChanged,
  stateErrorMessage,
  subscribeWith,
} from "./workspace-store";

function unwrap<T>(result: { status: "ok"; data: T } | { status: "error"; error: StateError }): T {
  if (result.status === "error") throw new Error(stateErrorMessage(result.error));
  return result.data;
}

/**
 * No environments yet (#17 brings them and the `environment-changed` event),
 * so the scope is fixed.
 */
const noEnvironment: EnvironmentScope = {
  current: () => null,
  key: "none",
  subscribe: () => () => {},
};

/** The services Rust binds, without their URLs. */
async function serviceDirectory(): Promise<ServiceDirectory> {
  const services = await commands.listServices();
  const bindings = new Map<string, ServiceBinding>(
    services.map((s) => [s.moduleId, { fetch: serviceFetch(s.moduleId), placement: s.placement }]),
  );
  return {
    get: (moduleId) => bindings.get(moduleId),
    list: () => [...bindings.keys()],
  };
}

export interface DesktopHosts {
  lakehouse: LakehouseHost;
  shell: ShellHost;
}

export async function createDesktopHosts(): Promise<DesktopHosts> {
  return {
    lakehouse: {
      kind: "desktop",
      environment: noEnvironment,
      services: await serviceDirectory(),
    },
    shell: {
      async doctor() {
        const result = await commands.doctor();
        if (result.status === "error") throw new Error(result.error);
        return result.data;
      },
      planner: createNativePlanner(),
      workspaces: {
        location: async () => unwrap(await commands.stateLocation()),
        list: async () => unwrap(await commands.stateWorkspaces()),
        create: async (name, firstEnv) =>
          unwrap(await commands.stateCreateWorkspace(name, false, firstEnv)),
        store: (id) => createStateWorkspaceStore(id),
        subscribe: (onChange) => subscribeWith(listenStateChanged, onChange),
      },
    },
  };
}
