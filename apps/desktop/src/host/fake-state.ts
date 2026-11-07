// An in-memory stand-in for the shared state commands (shell-core's `state`), with
// the same rules: names unique per workspace, a commit refused when the draft equals
// its head, revisions newest first, one change notification per write. The unit
// tests and the Playwright IPC mock run on it.

import type {
  EnvironmentDraft_Serialize as EnvironmentDraft,
  Imported_Serialize as Imported,
  InstanceChange,
  InstanceStatus_Serialize as InstanceStatus,
  Json,
  LogEntry,
  LogMessage,
  RevisionInfo,
  RunError,
  StateError,
  WorkspaceInfo,
  WorkspaceRun_Serialize as WorkspaceRun,
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
  /** Import an exported directory (only ones this fake exported exist). */
  stateImport(
    dir: string,
    workspace: string | null,
    name: string | null,
  ): Promise<Result<Imported>>;
  stateExportWorkspace(id: string, parent: string): Promise<Result<string>>;
  stateExportRevision(id: string, number: number, parent: string): Promise<Result<string>>;
  /** The directories exported so far, by path. */
  exported: ReadonlyMap<string, ExportedDir>;
  /** Called after every write, like the store's watcher. */
  listen(onChange: () => void): Promise<() => void>;
  /** Instances, as shell-core's `instances` runs them; every container healthy. */
  runWorkspace(workspace: string): Promise<RunResult<WorkspaceRun>>;
  /** With `oneAtATime`, the workspace's other instances stop first. */
  runStart(environment: string, oneAtATime?: boolean): Promise<RunResult<InstanceStatus>>;
  runStop(environment: string): Promise<RunResult<null>>;
  runStopInstance(instance: string): Promise<RunResult<null>>;
  /** Docker's events for the instances (what `setServices` raises). */
  listenInstances(onChange: (change: InstanceChange) => void): Promise<() => void>;
  /** As Docker would report the instance's containers from now on; raises a change. */
  setServices(instance: string, services: InstanceStatus["services"]): void;
  /** Docker unavailable (`null`: back). */
  setEngineProblem(problem: string | null): void;
  /** Make the next start fail with `error`. */
  failNextStart(error: RunError): void;
  /** The instance's logs: a `<service> started` line per container, then what
   *  `pushLogs` adds while followed. Returns a stop function. */
  runLogs(instance: string, follow: boolean, onMessage: (message: LogMessage) => void): () => void;
  /** Lines the instance's containers log now. */
  pushLogs(instance: string, lines: LogEntry[]): void;
}

/** What an export wrote: a workspace's members, or one environment. */
export type ExportedDir =
  | { kind: "workspace"; name: string; mesh: boolean; envs: { name: string; selection: Json }[] }
  | { kind: "environment"; selection: Json };

type RunResult<T> = { status: "ok"; data: T } | { status: "error"; error: RunError };

export function createFakeState(baseline: Json = { modules: [] }): FakeState {
  const workspaces = new Map<string, { id: string; name: string; mesh: boolean }>();
  const envs = new Map<string, Env>();
  const listeners = new Set<() => void>();
  /** By environment: its instances, newest revision first. */
  const instances = new Map<string, InstanceStatus[]>();
  let nextSlot = 0;
  let engineProblem: string | null = null;
  let nextStartError: RunError | null = null;
  const instanceListeners = new Set<(change: InstanceChange) => void>();
  const logFollowers = new Map<string, Set<(message: LogMessage) => void>>();
  const findInstance = (instanceId: string) => {
    for (const list of instances.values()) {
      const found = list.find((i) => i.id === instanceId);
      if (found) return found;
    }
    return undefined;
  };
  /** Docker's event for one of the instance's containers. */
  const raise = (envId: string, instance: InstanceStatus, action: string) => {
    const ws = workspaces.get(envs.get(envId)?.workspace ?? "");
    const change: InstanceChange = {
      project: instance.project,
      workspace: ws?.name ?? "",
      service: instance.services[0]?.service ?? "",
      action,
    };
    for (const l of [...instanceListeners]) setTimeout(() => l(change), 0);
  };
  const exported = new Map<string, ExportedDir>();
  const fresh = (parent: string, name: string): Result<string> => {
    const dir = `${parent.replace(/\/$/, "")}/${name}`;
    return exported.has(dir)
      ? err({ kind: "duplicate", message: `${dir} already exists` })
      : ok(dir);
  };
  /** A new environment committed at revision 1, as an import makes one. */
  const commitFirst = (env: Env) => {
    env.revisions.push({
      number: 1,
      digest: `fake-${nextId++}`,
      selection: structuredClone(env.selection),
      manifestToml: `[selection]\n# ${env.name}\n`,
    });
  };
  const runErr = <T>(error: RunError): RunResult<T> => ({ status: "error", error });
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
    exported,
    async stateImport(dir, workspace, name) {
      const found = exported.get(dir);
      if (!found) {
        return err({
          kind: "invalid",
          message: `${dir} holds neither a trestle-workspace.toml nor an env.toml`,
        });
      }
      if (found.kind === "workspace") {
        if ([...workspaces.values()].some((w) => w.name === found.name)) {
          return err({ kind: "duplicate", message: `workspace \`${found.name}\` already exists` });
        }
        const ws = { id: id("ws"), name: found.name, mesh: found.mesh };
        workspaces.set(ws.id, ws);
        for (const member of found.envs) {
          const created = createEnv(ws.id, member.name, member.selection);
          if (created.status === "ok") commitFirst(envs.get(created.data.id) as Env);
        }
        changed();
        return ok({ kind: "workspace", workspace: { ...ws, environments: found.envs.length } });
      }
      if (!workspace)
        return err({ kind: "invalid", message: "an environment is imported into a workspace" });
      if (!workspaces.has(workspace)) return notFound("workspace", workspace);
      const created = createEnv(workspace, name ?? dir.split("/").at(-1) ?? "", found.selection);
      if (created.status === "error") return created;
      const env = envs.get(created.data.id) as Env;
      commitFirst(env);
      changed();
      return ok({ kind: "environment", workspaceId: workspace, environment: draft(env) });
    },
    async stateExportWorkspace(wsId, parent) {
      const ws = workspaces.get(wsId);
      if (!ws) return notFound("workspace", wsId);
      const members = [...envs.values()].filter((e) => e.workspace === wsId);
      const unrevised = members.find((e) => e.revisions.length === 0);
      if (unrevised) {
        return err({
          kind: "invalid",
          message: `can't export: environment \`${unrevised.name}\` has no revision; commit it first`,
        });
      }
      const dir = fresh(parent, ws.name);
      if (dir.status === "error") return dir;
      exported.set(dir.data, {
        kind: "workspace",
        name: ws.name,
        mesh: ws.mesh,
        envs: members.map((e) => ({
          name: e.name,
          selection: structuredClone((e.revisions.at(-1) as RevisionInfo).selection),
        })),
      });
      return dir;
    },
    async stateExportRevision(envId, number, parent) {
      const env = envs.get(envId);
      if (!env) return notFound("environment", envId);
      const revision = env.revisions.find((r) => r.number === number);
      if (!revision) return notFound("revision", `${env.name}#${number}`);
      const dir = fresh(parent, `${env.name}-r${number}`);
      if (dir.status === "error") return dir;
      exported.set(dir.data, {
        kind: "environment",
        selection: structuredClone(revision.selection),
      });
      return dir;
    },
    async runWorkspace(workspace) {
      const members = [...envs.values()]
        .filter((e) => e.workspace === workspace)
        .sort((a, b) => a.name.localeCompare(b.name));
      return {
        status: "ok",
        data: {
          engineProblem,
          environments: members.map((env) => ({
            id: env.id,
            name: env.name,
            head: env.revisions.at(-1)?.number ?? null,
            instances: structuredClone(instances.get(env.id) ?? []),
          })),
        },
      };
    },
    async runStart(environment, oneAtATime = false) {
      const env = envs.get(environment);
      if (!env) return runErr({ kind: "other", message: `no environment \`${environment}\`` });
      const head = env.revisions.at(-1)?.number;
      if (head === undefined) return runErr({ kind: "noRevision", env: env.name });
      if (engineProblem) return runErr({ kind: "unavailable", message: engineProblem });
      if (nextStartError) {
        const error = nextStartError;
        nextStartError = null;
        return runErr(error);
      }
      if (oneAtATime) {
        for (const other of envs.values()) {
          if (other.workspace !== env.workspace) continue;
          for (const i of instances.get(other.id) ?? []) {
            if (other.id === env.id && i.revision === head) continue;
            i.desired = "stopped";
            i.services = [];
          }
        }
      }
      const ws = workspaces.get(env.workspace);
      const list = instances.get(env.id) ?? [];
      let instance = list.find((i) => i.revision === head);
      if (!instance) {
        const slot = nextSlot++;
        instance = {
          id: id("inst"),
          revision: head,
          desired: "stopped",
          slot,
          project: `${ws?.name}_${env.name}_r${head}`,
          gatewayUrl: `http://localhost:${9080 + slot * 1000}`,
          services: [],
        };
        list.unshift(instance);
        instances.set(env.id, list);
      }
      instance.desired = "running";
      // As compose names them: after the service spec, not the module.
      instance.services = [
        ["postgres", "postgres"],
        ["rustfs", "rustfs"],
        ["unitycatalog", "unity-catalog"],
      ].map(([service, module]) => ({
        service: service as string,
        module: module as string,
        state: "running",
        health: "healthy",
        status: "Up 1 second (healthy)",
        exitCode: 0,
        ports: [],
        ready: true,
      }));
      raise(env.id, instance, "start");
      return { status: "ok", data: structuredClone(instance) };
    },
    async runStop(environment) {
      for (const instance of instances.get(environment) ?? []) {
        instance.desired = "stopped";
        instance.services = [];
        raise(environment, instance, "destroy");
      }
      return { status: "ok", data: null };
    },
    async listen(onChange) {
      listeners.add(onChange);
      return () => listeners.delete(onChange);
    },
    async runStopInstance(instanceId) {
      for (const [envId, list] of instances) {
        const found = list.find((i) => i.id === instanceId);
        if (!found) continue;
        found.desired = "stopped";
        found.services = [];
        raise(envId, found, "destroy");
        return { status: "ok", data: null };
      }
      return runErr({ kind: "other", message: `no instance ${instanceId}` });
    },
    async listenInstances(onChange) {
      instanceListeners.add(onChange);
      return () => instanceListeners.delete(onChange);
    },
    setServices(instanceId, services) {
      for (const [envId, list] of instances) {
        const found = list.find((i) => i.id === instanceId);
        if (!found) continue;
        found.services = structuredClone(services);
        raise(envId, found, "health:changed");
      }
    },
    runLogs(instanceId, follow, onMessage) {
      const found = findInstance(instanceId);
      if (!found) {
        setTimeout(() => onMessage({ kind: "error", message: `no instance ${instanceId}` }), 0);
        return () => {};
      }
      const lines = found.services.map((s) => ({
        service: s.service,
        container: `${s.service}-1`,
        timestamp: null,
        text: `${s.service} started`,
      }));
      setTimeout(() => {
        onMessage({ kind: "lines", lines });
        if (!follow) onMessage({ kind: "end" });
      }, 0);
      if (!follow) return () => {};
      const set = logFollowers.get(instanceId) ?? new Set();
      set.add(onMessage);
      logFollowers.set(instanceId, set);
      return () => set.delete(onMessage);
    },
    pushLogs(instanceId, lines) {
      for (const l of logFollowers.get(instanceId) ?? []) {
        setTimeout(() => l({ kind: "lines", lines }), 0);
      }
    },
    setEngineProblem(problem) {
      engineProblem = problem;
    },
    failNextStart(error) {
      nextStartError = error;
    },
  };
}
