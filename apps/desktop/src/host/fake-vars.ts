// The variable editors and the effective-env view on a fake state, for the bun tests and
// the Playwright IPC mock: shell-core's `vars` in miniature. Secret values go to a keychain
// map under the CLI's keys; the var sets hold only `keychain:<key>` references. A commit
// freezes the draft's vars into the revision. The effective view layers the var sets as
// the runtime does (catalog defaults … instance overrides, a bound branch as `source:neon`),
// a key any layer holds as a secret staying masked.

import type {
  EffectiveEntry_Serialize as EffectiveEntry,
  EffectiveView_Serialize as EffectiveView,
  LayerRef,
  VarEntry,
  VarInput,
  VarsView_Serialize as VarsView,
  VarTarget,
} from "./bindings";
import type { FakeState } from "./fake-state";

/** The runtime's mask for a secret. */
export const MASK = "••••••";

type Value = { secret: false; value: string } | { secret: true; reference: string };
type VarSet = Map<string, Value>;

export interface FakeVars {
  get(environment: string, instance: string | null): Promise<VarsView>;
  set(target: VarTarget, key: string, input: VarInput): Promise<VarTarget>;
  unset(target: VarTarget, key: string): Promise<void>;
  effective(instance: string): Promise<EffectiveView>;
  /** Whether any var set the environment's head resolves sets `name`. */
  isSet(environment: string, name: string): Promise<boolean>;
  /** The keychain: key → value. */
  keychain: ReadonlyMap<string, string>;
  /** As on a team's store (`true`): secrets stay on this machine. */
  setShared(shared: boolean): void;
  /** Make the next write fail with `message`. */
  failNextWrite(message: string): void;
}

interface EnvInfo {
  id: string;
  name: string;
  workspace: string;
  workspaceName: string;
  head: number | null;
}

interface InstanceInfo {
  id: string;
  environment: string;
  revision: number;
  branch: string | null;
}

const NAME = /^[A-Za-z_][A-Za-z0-9_]*$/;

function invalid(message: string): Error {
  return Object.assign(new Error(message), { kind: "invalid" });
}

function entries(vars: VarSet | undefined): VarEntry[] {
  return [...(vars ?? new Map<string, Value>()).entries()]
    .sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0))
    .map(([key, v]) =>
      v.secret ? { key, secret: true, value: v.reference } : { key, secret: false, value: v.value },
    );
}

/** A dotenv value, double-quoted when it needs to be (as shell-core quotes it). */
function quote(value: string): string {
  if (value === "" || !/[\s"'#\\$`]/.test(value)) return value;
  return `"${value.replace(/[\\"$`]/g, (c) => `\\${c}`).replace(/\n/g, "\\n")}"`;
}

export function dotenv(header: string, list: EffectiveEntry[]): string {
  return `# ${header}\n${list.map((e) => `# ${e.layer.label}\n${e.key}=${quote(e.value)}\n`).join("")}`;
}

export function createFakeVars(state: FakeState): FakeVars {
  const workspace = new Map<string, VarSet>();
  const environment = new Map<string, VarSet>();
  const draft = new Map<string, VarSet>();
  const local = new Map<string, VarSet>();
  const revisions = new Map<string, VarSet>();
  const overrides = new Map<string, VarSet>();
  const keychain = new Map<string, string>();
  let shared = false;
  let failure: string | null = null;

  const set = (sets: Map<string, VarSet>, id: string) => {
    let s = sets.get(id);
    if (!s) {
      s = new Map();
      sets.set(id, s);
    }
    return s;
  };

  const envs = async (): Promise<EnvInfo[]> => {
    const list = await state.stateWorkspaces();
    if (list.status === "error") return [];
    const out: EnvInfo[] = [];
    for (const ws of list.data) {
      const snapshot = await state.stateWorkspace(ws.id);
      if (snapshot.status === "error") continue;
      for (const e of snapshot.data.environments) {
        out.push({
          id: e.id,
          name: e.name,
          workspace: ws.id,
          workspaceName: ws.name,
          head: e.head,
        });
      }
    }
    return out;
  };

  /** A commit freezes the draft's vars into the new head (what the store does). */
  const freeze = async () => {
    for (const e of await envs()) {
      const at = `${e.id}@${e.head}`;
      if (e.head !== null && !revisions.has(at)) {
        revisions.set(at, new Map(draft.get(e.id) ?? []));
      }
    }
  };
  void state.listen(() => void freeze());

  const env = async (id: string) => {
    await freeze();
    const found = (await envs()).find((e) => e.id === id);
    if (!found) throw Object.assign(new Error(`no environment ${id}`), { kind: "notFound" });
    return found;
  };

  const instances = async (): Promise<InstanceInfo[]> => {
    const out: InstanceInfo[] = [];
    const workspaces = new Set((await envs()).map((e) => e.workspace));
    for (const ws of workspaces) {
      const run = await state.runWorkspace(ws);
      if (run.status === "error") continue;
      for (const e of run.data.environments) {
        for (const i of e.instances) {
          out.push({
            id: i.id,
            environment: e.id,
            revision: i.revision,
            branch: i.branch?.name ?? null,
          });
        }
      }
    }
    return out;
  };

  const instance = async (id: string) => {
    const found = (await instances()).find((i) => i.id === id);
    if (!found) throw Object.assign(new Error(`no instance ${id}`), { kind: "notFound" });
    return found;
  };

  /** The var set and the CLI's keychain key prefix of `target`. */
  const resolve = async (target: VarTarget): Promise<[VarSet, string, string]> => {
    switch (target.scope) {
      case "workspace":
        return [set(workspace, target.workspace), target.workspace, target.workspace];
      case "environment": {
        const e = await env(target.environment);
        return [set(environment, e.id), e.id, e.workspace];
      }
      case "draft": {
        const e = await env(target.environment);
        return [set(draft, e.id), `${e.id}@r${(e.head ?? 0) + 1}`, e.workspace];
      }
      case "local": {
        const e = await env(target.environment);
        return [set(local, e.id), `${e.id}@local`, e.workspace];
      }
      case "instance": {
        const i = await instance(target.instance);
        const e = await env(i.environment);
        return [set(overrides, i.id), `${i.id}@instance`, e.workspace];
      }
    }
  };

  const allRefs = () => {
    const refs = new Set<string>();
    for (const sets of [workspace, environment, draft, local, revisions, overrides]) {
      for (const vars of sets.values()) {
        for (const v of vars.values()) if (v.secret) refs.add(v.reference);
      }
    }
    return refs;
  };

  const forget = (dropped: Value | undefined) => {
    if (dropped?.secret && !allRefs().has(dropped.reference)) {
      keychain.delete(dropped.reference.replace(/^keychain:/, ""));
    }
  };

  const takeFailure = () => {
    if (failure !== null) {
      const message = failure;
      failure = null;
      throw Object.assign(new Error(message), { kind: "store" });
    }
  };

  const layer = (name: string, label = name): LayerRef => ({ name, label });

  return {
    keychain,
    setShared: (value) => {
      shared = value;
    },
    failNextWrite: (message) => {
      failure = message;
    },
    get: async (environmentId, instanceId) => {
      const e = await env(environmentId);
      const i = instanceId ? await instance(instanceId) : null;
      return {
        workspaceId: e.workspace,
        environmentId: e.id,
        workspace: entries(workspace.get(e.workspace)),
        environment: entries(environment.get(e.id)),
        draft: entries(draft.get(e.id)),
        local: entries(local.get(e.id)),
        revision:
          e.head === null
            ? null
            : { number: e.head, vars: entries(revisions.get(`${e.id}@${e.head}`)) },
        instance: i ? { id: i.id, revision: i.revision, vars: entries(overrides.get(i.id)) } : null,
        shared,
      };
    },
    set: async (target, key, input) => {
      takeFailure();
      if (!NAME.test(key)) {
        throw invalid(
          `\`${key}\` is not a variable name (letters, digits and \`_\`, not starting with a digit)`,
        );
      }
      let where = target;
      if (input.kind === "secret") {
        if (input.value === "") throw invalid(`no value for the secret ${key}`);
        if (shared) {
          if (target.scope === "environment") {
            where = { scope: "local", environment: target.environment };
          } else if (target.scope === "workspace" || target.scope === "draft") {
            throw invalid(
              "a secret here would be shared with the team, whose keychains don't hold it: set it on the environment (kept on this machine)",
            );
          }
        }
      }
      const [vars, prefix] = await resolve(where);
      const old = vars.get(key);
      let next: Value;
      if (input.kind === "secret") {
        const k = `${prefix}/${key}`;
        keychain.set(k, input.value);
        next = { secret: true, reference: `keychain:${k}` };
      } else {
        next = { secret: false, value: input.value };
      }
      vars.set(key, next);
      if (old?.secret && (!next.secret || next.reference !== old.reference)) forget(old);
      return where;
    },
    unset: async (target, key) => {
      takeFailure();
      const [vars] = await resolve(target);
      const old = vars.get(key);
      vars.delete(key);
      forget(old);
    },
    isSet: async (environmentId, name) => {
      const e = await env(environmentId);
      const head = e.head === null ? undefined : revisions.get(`${e.id}@${e.head}`);
      return [workspace.get(e.workspace), environment.get(e.id), head, local.get(e.id)].some(
        (vars) => vars?.has(name) ?? false,
      );
    },
    effective: async (instanceId) => {
      const i = await instance(instanceId);
      const e = await env(i.environment);
      const resolved = new Map<
        string,
        {
          value: string;
          secret: boolean;
          reference: string | null;
          layer: LayerRef;
          shadowed: LayerRef[];
        }
      >();
      const apply = (l: LayerRef, vars: Iterable<[string, Value]>) => {
        for (const [key, v] of vars) {
          const before = resolved.get(key);
          const secret = v.secret || (before?.secret ?? false);
          resolved.set(key, {
            value: v.secret ? "" : v.value,
            secret,
            reference: v.secret ? v.reference : null,
            layer: l,
            shadowed: before ? [...before.shadowed, before.layer] : [],
          });
        }
      };
      const plain = (o: Record<string, string>): [string, Value][] =>
        Object.entries(o).map(([k, value]) => [k, { secret: false, value }]);
      apply(layer("placement"), plain({ TRESTLE_INSTANCE: i.id }));
      apply(
        layer("client env"),
        plain({ UNITY_CATALOG_URL: "http://localhost:8080/api/2.1/unity-catalog" }),
      );
      apply(layer("workspace vars"), workspace.get(e.workspace) ?? []);
      apply(layer("environment vars"), environment.get(e.id) ?? []);
      apply(layer("revision vars"), revisions.get(`${e.id}@${i.revision}`) ?? []);
      apply(layer("local vars"), local.get(e.id) ?? []);
      if (i.branch !== null) {
        const neon = layer("source:neon", `Neon branch ${i.branch}`);
        apply(neon, plain({ PGHOST: `ep-fake-${i.branch}.eu-central-1.aws.neon.tech` }));
        apply(neon, [
          ["DATABASE_URL", { secret: true, reference: "" }],
          ["PGPASSWORD", { secret: true, reference: "" }],
        ]);
      }
      apply(layer("instance overrides"), overrides.get(i.id) ?? []);
      const list: EffectiveEntry[] = [...resolved.entries()]
        .sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0))
        .map(([key, r]) => ({
          key,
          value: r.secret ? MASK : r.value,
          secret: r.secret,
          reference: r.secret && r.reference ? r.reference : null,
          layer: r.layer,
          shadowed: r.shadowed,
        }));
      return {
        instance: i.id,
        revision: i.revision,
        entries: list,
        dotenv: dotenv(
          `${e.workspaceName}/${e.name}@${i.revision} (${i.id}), from the host; secrets masked`,
          list,
        ),
      };
    },
  };
}
