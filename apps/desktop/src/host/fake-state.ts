// An in-memory stand-in for the shared state commands (shell-core's `state`), with
// the same rules: names unique per workspace, a commit refused when the draft equals
// its head, revisions newest first, one change notification per write. The unit
// tests and the Playwright IPC mock run on it.

import type {
  EnvironmentDraft_Serialize as EnvironmentDraft,
  Json,
  RevisionInfo,
  StateError,
  WorkspaceInfo,
  WorkspaceSnapshot_Serialize as WorkspaceSnapshot,
} from "./bindings";

type Result<T> = { status: "ok"; data: T } | { status: "error"; error: StateError };

const ok = <T>(data: T): Result<T> => ({ status: "ok", data });
const err = <T>(error: StateError): Result<T> => ({ status: "error", error });

interface Env {
  id: string;
  workspace: string;
  name: string;
  selection: Json;
  revisions: RevisionInfo[];
}

const NAME = /^[a-z][a-z0-9-]{0,30}$/;

export interface FakeState {
  stateLocation(): Promise<Result<string>>;
  stateWorkspaces(): Promise<Result<WorkspaceInfo[]>>;
  stateCreateWorkspace(
    name: string,
    mesh: boolean,
    firstEnv: string,
  ): Promise<Result<WorkspaceInfo>>;
  stateWorkspace(id: string): Promise<Result<WorkspaceSnapshot>>;
  stateCreateEnvironment(
    workspace: string,
    name: string,
    selection: Json | null,
  ): Promise<Result<EnvironmentDraft>>;
  stateDeleteEnvironment(id: string): Promise<Result<null>>;
  stateSetSelection(id: string, selection: Json): Promise<Result<null>>;
  stateRevisions(id: string): Promise<Result<RevisionInfo[]>>;
  stateRevision(id: string, number: number): Promise<Result<RevisionInfo | null>>;
  stateCommit(id: string): Promise<Result<RevisionInfo>>;
  /** Called after every write, like the store's watcher. */
  listen(onChange: () => void): Promise<() => void>;
}

export function createFakeState(baseline: Json = { modules: [] }): FakeState {
  const workspaces = new Map<string, { id: string; name: string; mesh: boolean }>();
  const envs = new Map<string, Env>();
  const listeners = new Set<() => void>();
  let nextId = 1;
  const id = (prefix: string) => `${prefix}-${nextId++}`;
  const changed = () => {
    for (const l of [...listeners]) setTimeout(l, 0);
  };
  const notFound = <T>(what: string, key: string) =>
    err<T>({ kind: "notFound", message: `no ${what} \`${key}\`` });

  const draft = (env: Env): EnvironmentDraft => ({
    id: env.id,
    name: env.name,
    head: env.revisions.at(-1)?.number ?? null,
    selection: env.selection,
  });

  const createEnv = (workspace: string, name: string, selection: Json | null) => {
    if (!NAME.test(name))
      return err<EnvironmentDraft>({ kind: "invalid", message: `bad name \`${name}\`` });
    if ([...envs.values()].some((e) => e.workspace === workspace && e.name === name)) {
      return err<EnvironmentDraft>({
        kind: "duplicate",
        message: `environment \`${name}\` already exists`,
      });
    }
    const env: Env = {
      id: id("env"),
      workspace,
      name,
      selection: structuredClone(selection ?? baseline),
      revisions: [],
    };
    envs.set(env.id, env);
    return ok(draft(env));
  };

  return {
    async stateLocation() {
      return ok("/fake/trestle/state.db");
    },
    async stateWorkspaces() {
      return ok(
        [...workspaces.values()]
          .sort((a, b) => a.name.localeCompare(b.name))
          .map((ws) => ({
            ...ws,
            environments: [...envs.values()].filter((e) => e.workspace === ws.id).length,
          })),
      );
    },
    async stateCreateWorkspace(name, mesh, firstEnv) {
      if (!NAME.test(name)) return err({ kind: "invalid", message: `bad name \`${name}\`` });
      if ([...workspaces.values()].some((w) => w.name === name)) {
        return err({ kind: "duplicate", message: `workspace \`${name}\` already exists` });
      }
      const ws = { id: id("ws"), name, mesh };
      workspaces.set(ws.id, ws);
      createEnv(ws.id, firstEnv, null);
      changed();
      return ok({ ...ws, environments: 1 });
    },
    async stateWorkspace(wsId) {
      const ws = workspaces.get(wsId);
      if (!ws) return notFound("workspace", wsId);
      const members = [...envs.values()]
        .filter((e) => e.workspace === wsId)
        .sort((a, b) => a.name.localeCompare(b.name))
        .map(draft);
      return ok({ ...ws, environments: members });
    },
    async stateCreateEnvironment(workspace, name, selection) {
      if (!workspaces.has(workspace)) return notFound("workspace", workspace);
      const result = createEnv(workspace, name, selection);
      if (result.status === "ok") changed();
      return result;
    },
    async stateDeleteEnvironment(envId) {
      if (!envs.delete(envId)) return notFound("environment", envId);
      changed();
      return ok(null);
    },
    async stateSetSelection(envId, selection) {
      const env = envs.get(envId);
      if (!env) return notFound("environment", envId);
      env.selection = structuredClone(selection);
      changed();
      return ok(null);
    },
    async stateRevisions(envId) {
      const env = envs.get(envId);
      if (!env) return notFound("environment", envId);
      return ok([...env.revisions].reverse());
    },
    async stateRevision(envId, number) {
      const env = envs.get(envId);
      if (!env) return notFound("environment", envId);
      return ok(env.revisions.find((r) => r.number === number) ?? null);
    },
    async stateCommit(envId) {
      const env = envs.get(envId);
      if (!env) return notFound("environment", envId);
      const head = env.revisions.at(-1);
      if (head && JSON.stringify(head.selection) === JSON.stringify(env.selection)) {
        return err({ kind: "noChanges", head: head.number });
      }
      const revision: RevisionInfo = {
        number: (head?.number ?? 0) + 1,
        digest: `fake-${nextId++}`,
        selection: structuredClone(env.selection),
        manifestToml: `[selection]\n# ${env.name}\n`,
      };
      env.revisions.push(revision);
      changed();
      return ok(revision);
    },
    async listen(onChange) {
      listeners.add(onChange);
      return () => listeners.delete(onChange);
    },
  };
}
