// The work-graph agents on fakes, for the bun tests and the Playwright IPC mock:
// shell-core's `workgraph_agents` in miniature. The branch's function grants the
// catalog below; enabling makes an identity and a route per role and marks its key set;
// a pasted key is refused unless it is a service account's `olk_` key, as shell-core
// refuses it. No call ever returns a key: the fake keeps only whether one is set.

import type {
  AgentIdentity,
  AgentsApplied,
  AgentsView,
  CatalogRole,
  RouteView,
  WorkgraphAgentsHost,
} from "../shell/shell-host";
import { runFailure } from "../shell/shell-host";

/** The roles the fake branch's function grants. */
export const FAKE_CATALOG: CatalogRole[] = [
  {
    name: "planner",
    policy: "@read,add_item,add_edge,annotate,submit",
    tools: ["plan_read", "item_read", "ready", "add_item", "add_edge", "annotate", "submit"],
  },
  {
    name: "executor",
    policy: "@read,add_item,add_edge,annotate,propose,report_change",
    tools: ["plan_read", "item_read", "add_item", "annotate", "propose", "report_change"],
  },
  { name: "reviewer", policy: "@read,annotate", tools: ["plan_read", "item_read", "annotate"] },
];

/** shell-core's `check_key_value`: `olk_<16 hex>_<64 hex>`. */
export function isServiceAccountKey(value: string): boolean {
  return /^olk_[0-9a-f]{16}_[0-9a-f]{64}$/i.test(value);
}

export interface FakeWorkgraphAgentsOptions {
  /** Whether the environment's draft runs Omnigent (yes by default). */
  omnigent?: (environment: string) => boolean;
  /** The branch the agents live on (`dev` by default; `null`: none picked). */
  branchOf?: (environment: string) => string | null;
  /** Routes the draft has that this section didn't make. */
  otherRoutes?: RouteView[];
}

export interface FakeWorkgraphAgents extends WorkgraphAgentsHost {
  /** What the fake was asked, never a key's value. */
  calls: string[];
}

export function createFakeWorkgraphAgents(
  options: FakeWorkgraphAgentsOptions = {},
): FakeWorkgraphAgents {
  const omnigent = options.omnigent ?? (() => true);
  const branchOf = options.branchOf ?? (() => "dev");
  const byEnv = new Map<string, { identities: AgentIdentity[]; keySet: Set<string> }>();
  const calls: string[] = [];
  let keys = 0;
  const entry = (environment: string) => {
    let e = byEnv.get(environment);
    if (!e) {
      e = { identities: [], keySet: new Set() };
      byEnv.set(environment, e);
    }
    return e;
  };
  const route = (role: CatalogRole, keySet: boolean): RouteView => ({
    name: `workgraph-${role.name}`,
    function: "workgraph",
    upstream: null,
    path: "/mcp",
    consumers: ["omnigent"],
    secret: `WORKGRAPH_${role.name.toUpperCase()}_API_KEY`,
    keySet,
    tools: role.tools,
    role: role.name,
  });
  const view = (environment: string): AgentsView => {
    const e = entry(environment);
    const branch = branchOf(environment);
    const routes = e.identities.map((i) => {
      const role = FAKE_CATALOG.find((r) => r.name === i.role) ?? FAKE_CATALOG[0];
      return route(role, e.keySet.has(i.role));
    });
    return {
      routes: [...(options.otherRoutes ?? []), ...routes],
      identities: e.identities,
      omnigent: omnigent(environment),
      branch,
      branchError: branch ? null : "pick a Neon branch that runs the workgraph function first",
      catalog: branch ? { roles: FAKE_CATALOG } : null,
      catalogError: null,
      drift: [],
    };
  };
  const applied = (environment: string, recreated: string[] = []): AgentsApplied => ({
    view: view(environment),
    routes: null,
    recreated,
  });
  const fail = (message: string) => runFailure({ kind: "other", message });
  return {
    calls,
    get: async (environment) => view(environment),
    enable: async (environment, roles) => {
      calls.push(`enable ${environment} ${roles.join(",")}`);
      if (!omnigent(environment)) throw fail("the agents reach the work graph through Omnigent");
      const branch = branchOf(environment);
      if (!branch) throw runFailure({ kind: "neon", message: "pick a Neon branch first" });
      const e = entry(environment);
      e.identities = roles.map((role) => {
        const known = e.identities.find((i) => i.role === role);
        if (known) return known;
        keys += 1;
        e.keySet.add(role);
        return {
          role,
          account: `factory-${role}-${environment}`,
          group: `factory-${role}s`,
          keyId: `key-${keys}`,
          branch,
        };
      });
      for (const role of [...e.keySet]) if (!roles.includes(role)) e.keySet.delete(role);
      return { ...applied(environment), routes: { committed: null, needsCommit: false } };
    },
    disable: async (environment) => {
      calls.push(`disable ${environment}`);
      byEnv.delete(environment);
      return { ...applied(environment), routes: { committed: null, needsCommit: false } };
    },
    rotate: async (environment, role) => {
      calls.push(`rotate ${environment} ${role}`);
      const e = entry(environment);
      const identity = e.identities.find((i) => i.role === role);
      if (!identity) throw fail(`the environment has no ${role} agent`);
      keys += 1;
      identity.keyId = `key-${keys}`;
      return applied(environment, [`${environment}-instance`]);
    },
    setKey: async (environment, role, value) => {
      calls.push(`setKey ${environment} ${role}`);
      if (!isServiceAccountKey(value.trim())) {
        throw fail(
          "that isn't a service account's API key (olk_…): a route takes only an agent's key, never the apps token",
        );
      }
      const e = entry(environment);
      e.keySet.add(role);
      const identity = e.identities.find((i) => i.role === role);
      if (identity) identity.keyId = null;
      return applied(environment, [`${environment}-instance`]);
    },
  };
}
