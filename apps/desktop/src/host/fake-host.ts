// A `ShellHost` for tests and the Playwright build: the real workspace-store
// adapter on the in-memory fake state, the fixture planner, a fixed doctor.

import { FIXTURE_CATALOG } from "@open-lakehouse/stack-wasm/fixtures";
import { createWasmPlanner } from "@open-lakehouse/stack-wasm/stub";
import { READY_REPORT } from "../shell/fixtures";
import {
  type BranchChoice,
  type DoctorReport,
  type NeonBranches,
  runFailure,
  type ShellHost,
} from "../shell/shell-host";
import type {
  Json,
  ReconcileReport,
  RunError,
  ServiceUi,
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

/** The project the fake lists: production and staging, as `neon.ts` deploys them, and
 *  `scratch`, a branch trestle made. */
export const FAKE_NEON: NeonBranches = {
  projectId: "cool-poetry-28343550",
  source: "default",
  parent: "staging",
  branches: [
    { id: "br-production", name: "production", default: true, owned: false, expiresAt: null },
    { id: "br-staging", name: "staging", default: false, owned: false, expiresAt: null },
    {
      id: "br-scratch",
      name: "scratch",
      default: false,
      owned: true,
      expiresAt: "2026-11-14T00:00:00Z",
    },
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
    /** The service UIs an instance offers. */
    serviceUis?: (instance: string) => ServiceUi[];
    /** Called by `run.openServiceWindow`. */
    onOpenWindow?: (instance: string, module: string) => void;
    /** What the folder picker answers (`null`, cancelled, by default). */
    pickDirectory?: (title: string) => string | null;
    /** Called by `run.start` with the branch it was asked to run on. */
    onStartOn?: (environment: string, branch: BranchChoice) => void;
  } = {},
): FakeShellHost {
  const state = options.state ?? createFakeState(FIXTURE_CATALOG.default_selection as Json);
  let settings: Settings = {
    quit: "keep",
    oneAtATime: [],
    lastInstance: null,
    neonBranches: {},
    ...options.settings,
  };
  // The branches, as deleting one changes them.
  let neon =
    options.neon instanceof Error ? options.neon : structuredClone(options.neon ?? FAKE_NEON);
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
      start: async (environment, branch) => {
        // As in the app: what starts becomes the current environment, and the branch
        // it ran on is remembered.
        const ws = await workspaceOf(environment);
        const oneAtATime = ws !== null && settings.oneAtATime.includes(ws);
        if (branch) options.onStartOn?.(environment, branch);
        const started = unwrapRun(await state.runStart(environment, oneAtATime, branch));
        if (started.branch) {
          settings = {
            ...settings,
            neonBranches: { ...settings.neonBranches, [environment]: started.branch.name },
          };
        }
        options.onUse?.(started.id);
        return started;
      },
      neonNeeds: async (environment) => unwrapRun(await state.neonNeeds(environment)),
      stop: async (environment) => {
        unwrapRun(await state.runStop(environment));
      },
      stopInstance: async (instance) => {
        unwrapRun(await state.runStopInstance(instance));
      },
      use: async (instance) => {
        options.onUse?.(instance);
      },
      subscribe: (onChange) => subscribeWith(state.listenInstances, onChange),
      logs: (instance, { follow }, onMessage) => state.runLogs(instance, follow, onMessage),
      serviceUis: async (instance) => options.serviceUis?.(instance) ?? [],
      openServiceWindow: async (instance, module) => options.onOpenWindow?.(instance, module),
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
        if (neon instanceof Error) throw neon;
        return structuredClone(neon);
      },
      deleteBranch: async (_workspace, name) => {
        if (neon instanceof Error) throw neon;
        const branch = neon.branches.find((b) => b.name === name || b.id === name);
        if (!branch) throw runFailure({ kind: "neon", message: `no branch ${name}` });
        if (!branch.owned) {
          throw runFailure({
            kind: "neon",
            message: `\`${branch.name}\` wasn't created by trestle`,
          });
        }
        neon = { ...neon, branches: neon.branches.filter((b) => b.id !== branch.id) };
        return branch;
      },
      resetBranch: async (_workspace, name) => {
        if (neon instanceof Error) throw neon;
        const branch = neon.branches.find((b) => b.name === name || b.id === name);
        if (!branch?.owned) throw runFailure({ kind: "neon", message: `${name} isn't owned` });
      },
    },
  };
}
