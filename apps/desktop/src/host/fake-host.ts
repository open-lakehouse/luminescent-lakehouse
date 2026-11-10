// A `ShellHost` for tests and the Playwright build: the real workspace-store
// adapter on the in-memory fake state, the fixture planner, a fixed doctor.

import { FIXTURE_CATALOG } from "@open-lakehouse/stack-wasm/fixtures";
import { createWasmPlanner } from "@open-lakehouse/stack-wasm/stub";
import { READY_REPORT } from "../shell/fixtures";
import {
  type DoctorReport,
  type NeonBranch,
  type NeonBranches,
  type NeonProject,
  type NeonTarget,
  runFailure,
  type ShellHost,
} from "../shell/shell-host";
import type {
  EmbedBounds,
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

/** The account's projects, as `neon orgs list` / `neon projects list` show them. */
export const FAKE_PROJECTS: NeonProject[] = [
  { id: "cool-poetry-28343550", name: "luminescent-eu", org: "open-lakehouse" },
  { id: "quiet-lake-11", name: "luminescent-us", org: "open-lakehouse" },
];

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
    /** Whether service UIs embed (`true` by default; `false` as on Linux). */
    embeds?: boolean;
    /** Called by `run.embedServiceUi`. */
    onEmbed?: (instance: string, module: string, bounds: EmbedBounds | null) => void;
    /** What the folder picker answers (`null`, cancelled, by default). */
    pickDirectory?: (title: string) => string | null;
    /** Called by `run.start` with the Neon target it binds. */
    onStartOn?: (environment: string, target: NeonTarget) => void;
  } = {},
): FakeShellHost {
  const state = options.state ?? createFakeState(FIXTURE_CATALOG.default_selection as Json);
  let settings: Settings = {
    quit: "keep",
    oneAtATime: [],
    lastInstance: null,
    neonTargets: {},
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
      start: async (environment) => {
        // As in the app: on the environment's Neon target, a new branch then becoming
        // the one made; what starts becomes the current environment.
        const ws = await workspaceOf(environment);
        const oneAtATime = ws !== null && settings.oneAtATime.includes(ws);
        const target = settings.neonTargets[environment];
        if (target) options.onStartOn?.(environment, target);
        const started = unwrapRun(
          await state.runStart(environment, oneAtATime, target?.branch ?? null),
        );
        if (target?.branch.kind === "new" && started.branch) {
          settings = {
            ...settings,
            neonTargets: {
              ...settings.neonTargets,
              [environment]: {
                project: target.project,
                branch: { kind: "existing", branch: started.branch.name },
              },
            },
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
      embedsSupported: async () => options.embeds ?? true,
      embedServiceUi: async (instance, module, bounds) =>
        options.onEmbed?.(instance, module, bounds),
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
      branches: async (_workspace, project) => {
        if (neon instanceof Error) throw neon;
        if (project && project !== neon.projectId) {
          const other = FAKE_PROJECTS.find((p) => p.id === project);
          if (!other) throw runFailure({ kind: "neon", message: `no project ${project}` });
          return {
            projectId: other.id,
            source: "environment",
            parent: null,
            branches: [
              { id: "br-main", name: "main", default: true, owned: false, expiresAt: null },
            ],
          };
        }
        return structuredClone(neon);
      },
      projects: async () => {
        if (neon instanceof Error) throw neon;
        return FAKE_PROJECTS;
      },
      setTarget: async (environment, target) => {
        const { [environment]: _, ...rest } = settings.neonTargets;
        settings = { ...settings, neonTargets: target ? { ...rest, [environment]: target } : rest };
        return settings;
      },
      createBranch: async (_workspace, name, options) => {
        if (neon instanceof Error) throw neon;
        const trimmed = name.trim();
        if (!trimmed) throw runFailure({ kind: "neon", message: "a branch needs a name" });
        if (neon.branches.some((b) => b.name === trimmed)) {
          throw runFailure({
            kind: "neon",
            message: `the project already has a branch \`${trimmed}\``,
          });
        }
        const hours = options?.ttlHours ?? 7 * 24;
        const branch: NeonBranch = {
          id: `br-fake-${neon.branches.length + 1}`,
          name: trimmed,
          default: false,
          owned: true,
          expiresAt: new Date(Date.now() + hours * 3_600_000).toISOString(),
        };
        neon = { ...neon, branches: [...neon.branches, branch] };
        return branch;
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
