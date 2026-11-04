// A `ShellHost` for tests and the Playwright build: the real workspace-store
// adapter on the in-memory fake state, the fixture planner, a fixed doctor.

import { FIXTURE_CATALOG } from "@open-lakehouse/stack-wasm/fixtures";
import { createWasmPlanner } from "@open-lakehouse/stack-wasm/stub";
import { READY_REPORT } from "../shell/fixtures";
import {
  type DoctorReport,
  type NeonBranches,
  runFailure,
  type ShellHost,
} from "../shell/shell-host";
import type {
  Json,
  ReconcileReport,
  RunError,
  Settings_Serialize as Settings,
  StateError,
} from "./bindings";
import { createFakeState, type FakeState } from "./fake-state";
import { createStateWorkspaceStore, stateErrorMessage, subscribeWith } from "./workspace-store";

function unwrap<T>(result: { status: "ok"; data: T } | { status: "error"; error: StateError }): T {
  if (result.status === "error") throw new Error(stateErrorMessage(result.error));
  return result.data;
}

function unwrapRun<T>(result: { status: "ok"; data: T } | { status: "error"; error: RunError }): T {
  if (result.status === "error") throw runFailure(result.error);
  return result.data;
}

/** The project the fake lists: production and staging, as `neon.ts` deploys them. */
export const FAKE_NEON: NeonBranches = {
  projectId: "cool-poetry-28343550",
  source: "default",
  branches: [
    { id: "br-production", name: "production", default: true },
    { id: "br-staging", name: "staging", default: false },
  ],
};

export interface FakeShellHost extends ShellHost {
  /** The state behind it, to write to as the CLI would. */
  state: FakeState;
}

export function createFakeShellHost(
  options: {
    doctor?: () => Promise<DoctorReport>;
    state?: FakeState;
    /** Called by `run.use`; a test switches its fake LakehouseHost's environment here. */
    onUse?: (instance: string) => void;
    /** What `neon.branches` answers (an Error is thrown). */
    neon?: NeonBranches | Error;
    /** What the launch's reconcile found (`null`: it hasn't run). */
    reconciled?: ReconcileReport | null;
    /** The app's settings to start from. */
    settings?: Partial<Settings>;
    /** What the folder picker answers (`null`, cancelled, by default). */
    pickDirectory?: (title: string) => string | null;
    /** Called by `neon.use` with the branch's id and name. */
    onUseNeon?: (branch: { id: string; name: string }) => void;
  } = {},
): FakeShellHost {
  const state = options.state ?? createFakeState(FIXTURE_CATALOG.default_selection as Json);
  let settings: Settings = {
    quit: "keep",
    oneAtATime: [],
    lastInstance: null,
    ...options.settings,
  };
  const workspaceOf = async (environment: string) => {
    const list = unwrap(await state.stateWorkspaces());
    for (const ws of list) {
      const snapshot = unwrap(await state.stateWorkspace(ws.id));
      if (snapshot.environments.some((e) => e.id === environment)) return ws.id;
    }
    return null;
  };
  return {
    state,
    doctor: options.doctor ?? (async () => READY_REPORT),
    planner: createWasmPlanner(),
    workspaces: {
      location: async () => unwrap(await state.stateLocation()),
      list: async () => unwrap(await state.stateWorkspaces()),
      create: async (name, firstEnv) =>
        unwrap(await state.stateCreateWorkspace(name, false, firstEnv)),
      store: (id) => createStateWorkspaceStore(id, state, state.listen),
      subscribe: (onChange) => subscribeWith(state.listen, onChange),
      environments: async (id) =>
        unwrap(await state.stateWorkspace(id)).environments.map(({ id, name, head }) => ({
          id,
          name,
          head,
        })),
      pickDirectory: async (title) => (options.pickDirectory ?? (() => null))(title),
      import: async (dir, workspace) => unwrap(await state.stateImport(dir, workspace, null)),
      exportWorkspace: async (id, parent) => unwrap(await state.stateExportWorkspace(id, parent)),
      exportRevision: async (environment, revision, parent) =>
        unwrap(await state.stateExportRevision(environment, revision, parent)),
    },
    run: {
      workspace: async (id) => unwrapRun(await state.runWorkspace(id)),
      start: async (environment) => {
        // As in the app: what starts becomes the current environment.
        const ws = await workspaceOf(environment);
        const oneAtATime = ws !== null && settings.oneAtATime.includes(ws);
        const started = unwrapRun(await state.runStart(environment, oneAtATime));
        options.onUse?.(started.id);
        return started;
      },
      stop: async (environment) => {
        unwrapRun(await state.runStop(environment));
      },
      use: async (instance) => {
        options.onUse?.(instance);
      },
      subscribe: (onChange) => subscribeWith(state.listenInstances, onChange),
      reconciled: async () => options.reconciled ?? null,
      onReconciled: () => () => {},
      settings: async () => settings,
      setQuit: async (quit) => {
        settings = { ...settings, quit };
        return settings;
      },
      setOneAtATime: async (workspace, on) => {
        const rest = settings.oneAtATime.filter((w) => w !== workspace);
        settings = { ...settings, oneAtATime: on ? [...rest, workspace] : rest };
        return settings;
      },
    },
    neon: {
      branches: async () => {
        const neon = options.neon ?? FAKE_NEON;
        if (neon instanceof Error) throw neon;
        return neon;
      },
      use: async (_workspace, name) => {
        const neon = options.neon instanceof Error ? FAKE_NEON : (options.neon ?? FAKE_NEON);
        const branch = neon.branches.find((b) => b.name === name || b.id === name);
        if (!branch) throw runFailure({ kind: "neon", message: `no branch ${name}` });
        options.onUseNeon?.(branch);
      },
    },
  };
}
