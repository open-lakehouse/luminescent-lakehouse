// The desktop's hosts: the `LakehouseHost` every service UI binds through
// (`@open-lakehouse/host`), and the `ShellHost` with the shell's own commands.
// This directory is the only place that imports `@tauri-apps/api`; the UI
// under src/shell and src/integrations only sees these interfaces, so it runs
// on fakes in tests.

import type {
  EnvironmentRef,
  EnvironmentScope,
  LakehouseHost,
  ServiceBinding,
  ServiceDirectory,
} from "@open-lakehouse/host";
import { Channel } from "@tauri-apps/api/core";
import { runFailure, type ShellHost } from "../shell/shell-host";
import {
  type CurrentEnvironment,
  commands,
  events,
  type LogMessage,
  type RunError,
  type StateError,
} from "./bindings";
import { createNativePlanner } from "./native-planner";
import { serviceFetch } from "./service-fetch";
import {
  createStateWorkspaceStore,
  listenStateChanged,
  stateErrorMessage,
  subscribeWith,
} from "./workspace-store";

function unwrapRun<T>(result: { status: "ok"; data: T } | { status: "error"; error: RunError }): T {
  if (result.status === "error") throw runFailure(result.error);
  return result.data;
}

/** Ids for log streams, so a stop names its stream. */
let nextLogStream = 1;

function unwrapString<T>(
  result: { status: "ok"; data: T } | { status: "error"; error: string },
): T {
  if (result.status === "error") throw new Error(result.error);
  return result.data;
}

function unwrap<T>(result: { status: "ok"; data: T } | { status: "error"; error: StateError }): T {
  if (result.status === "error") throw new Error(stateErrorMessage(result.error));
  return result.data;
}

/** The services Rust binds now (without their URLs), re-read when the current
 *  environment changes. */
function serviceDirectory(): ServiceDirectory & { refresh(): Promise<void> } {
  let bindings = new Map<string, ServiceBinding>();
  return {
    get: (moduleId) => bindings.get(moduleId),
    list: () => [...bindings.keys()],
    async refresh() {
      const services = await commands.listServices();
      bindings = new Map(
        services.map((s) => [
          s.moduleId,
          { fetch: serviceFetch(s.moduleId), placement: s.placement },
        ]),
      );
    },
  };
}

/** The id is the instance's. */
export function toRef(current: CurrentEnvironment | null): EnvironmentRef | null {
  if (!current) return null;
  return {
    id: current.instanceId,
    name: current.environment,
    workspace: current.workspace,
    revision: current.revision,
  };
}

/**
 * The current environment: the running instance started (or chosen) last, from
 * `environment-changed`. The services are rebound before subscribers hear of it, so
 * the views that remount on the new key find their bindings.
 */
async function liveEnvironment(directory: { refresh(): Promise<void> }): Promise<EnvironmentScope> {
  // The key comes from Rust (`CurrentEnvironment::key`), here and in every event.
  const initial = await commands.runCurrent();
  let current = toRef(initial.current ?? null);
  let key = initial.key;
  const listeners = new Set<() => void>();
  await events.environmentChanged.listen(async (event) => {
    await directory.refresh();
    current = toRef(event.payload.current ?? null);
    key = event.payload.key;
    for (const l of [...listeners]) l();
  });
  return {
    current: () => current,
    get key() {
      return key;
    },
    subscribe(onChange) {
      listeners.add(onChange);
      return () => listeners.delete(onChange);
    },
  };
}

export interface DesktopHosts {
  lakehouse: LakehouseHost;
  shell: ShellHost;
}

export async function createDesktopHosts(): Promise<DesktopHosts> {
  const services = serviceDirectory();
  await services.refresh();
  return {
    lakehouse: {
      kind: "desktop",
      environment: await liveEnvironment(services),
      services,
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
        environments: async (id) =>
          unwrap(await commands.stateWorkspace(id)).environments.map(({ id, name, head }) => ({
            id,
            name,
            head,
          })),
        pickDirectory: (title) => commands.pickDirectory(title),
        import: async (dir, workspace) => unwrap(await commands.stateImport(dir, workspace, null)),
        exportWorkspace: async (id, parent) =>
          unwrap(await commands.stateExportWorkspace(id, parent)),
        exportRevision: async (environment, revision, parent) =>
          unwrap(await commands.stateExportRevision(environment, revision, parent)),
      },
      run: {
        workspace: async (id) => unwrapRun(await commands.runWorkspace(id)),
        start: async (environment) => unwrapRun(await commands.runStart(environment)),
        neonNeeds: async (environment) => unwrapRun(await commands.neonNeeds(environment)),
        stop: async (environment) => {
          unwrapRun(await commands.runStop(environment));
        },
        stopInstance: async (instance) => {
          unwrapRun(await commands.runStopInstance(instance));
        },
        use: async (instance) => {
          unwrapRun(await commands.runUse(instance));
        },
        subscribe: (onChange) =>
          subscribeWith(
            (cb) => events.instanceChanged.listen((e) => cb(e.payload.change)),
            onChange,
          ),
        logs: (instance, { tail, follow }, onMessage) => {
          const id = nextLogStream++;
          let stopped = false;
          const channel = new Channel<LogMessage>();
          channel.onmessage = (message) => {
            if (!stopped) onMessage(message);
          };
          void commands.instanceLogs(id, instance, tail, follow, channel).then((result) => {
            if (result.status === "error")
              onMessage({ kind: "error", message: runFailure(result.error).message });
          });
          return () => {
            stopped = true;
            void commands.instanceLogsStop(id);
          };
        },
        serviceUis: async (instance) => unwrapRun(await commands.runServiceUis(instance)),
        openServiceWindow: async (instance, module) => {
          unwrapRun(await commands.openServiceWindow(instance, module));
        },
        embedServiceUi: async (instance, module, bounds) => {
          unwrapRun(await commands.embedServiceUi(instance, module, bounds));
        },
        reconciled: () => commands.runReconciled(),
        onReconciled: (onDone) =>
          subscribeWith((cb) => events.reconciled.listen(() => cb()), onDone),
        settings: () => commands.settingsGet(),
        setQuit: async (quit) => unwrapString(await commands.settingsSetQuit(quit)),
        setOneAtATime: async (workspace, on) =>
          unwrapString(await commands.settingsSetOneAtATime(workspace, on)),
      },
      neon: {
        branches: async (workspace, project) =>
          unwrapRun(await commands.neonBranches(workspace, project ?? null)),
        projects: async () => unwrapRun(await commands.neonProjects()),
        setTarget: async (environment, target) =>
          unwrapString(await commands.neonTargetSet(environment, target)),
        createBranch: async (workspace, name, options) =>
          unwrapRun(
            await commands.neonCreateBranch(
              workspace,
              options?.project ?? null,
              name,
              options?.ttlHours ?? null,
            ),
          ),
        deleteBranch: async (workspace, branch) =>
          unwrapRun(await commands.neonDeleteBranch(workspace, branch)),
        resetBranch: async (workspace, branch) => {
          unwrapRun(await commands.neonResetBranch(workspace, branch));
        },
      },
    },
  };
}
