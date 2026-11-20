// The Models choice on a fake state, for the bun tests and the Playwright IPC mock:
// shell-core's `models` and `Runner::missing_secrets` in miniature. An environment
// wants models when its draft runs MLflow or Omnigent; a choice is "applied" at once
// (the fake has no knobs to commit), and a Databricks upstream with no host is the one
// missing secret it reports. The Neon AI Gateway is refused without a branch, as
// shell-core refuses it; its status is one of the scenarios below (the real shapes are
// shell-core's fixtures, tests/fixtures/neon/).

import type {
  DatabricksProfile,
  GatewayProblem,
  ModelsApplied,
  ModelsChoice,
  ModelsInfo,
  NeonGateway,
  SecretNeeds,
} from "../shell/shell-host";
import type { FakeState } from "./fake-state";

export const FAKE_PROFILES: DatabricksProfile[] = [
  { name: "acct", host: "https://accounts.cloud.databricks.com", authType: "oauth-m2m" },
  { name: "work", host: "https://dbc-1.cloud.databricks.com", authType: "databricks-cli" },
];

/** The branch gateway's states the option shows. */
export type GatewayScenario = "ok" | "credits" | "paidPlan" | "region" | "noModels" | "error";

const CONSOLE_URL = "https://console.neon.tech/app/projects/p-fake/branches/br-fake/ai-gateway";

/** The branch's gateway in `scenario`. */
export function fakeGateway(scenario: GatewayScenario, branchName = "dev"): NeonGateway {
  const base: NeonGateway = {
    projectId: "p-fake",
    branch: "br-fake",
    branchName,
    baseUrl: "https://br-fake-api.ai.c-8.eu-central-1.aws.neon.tech",
    models: [],
    locked: 0,
    missingSeeded: [],
    problem: null,
    consoleUrl: CONSOLE_URL,
  };
  const problem = (p: GatewayProblem): NeonGateway => ({ ...base, problem: p });
  switch (scenario) {
    case "ok":
      return {
        ...base,
        models: [
          { id: "claude-haiku-5-5", name: "Claude Haiku 5.5", ownedBy: "anthropic" },
          { id: "claude-opus-5-5", name: "Claude Opus 5.5", ownedBy: "anthropic" },
          { id: "gpt-5-5", name: "GPT-5.5", ownedBy: "openai" },
        ],
        locked: 2,
        missingSeeded: ["claude-sonnet-5-5"],
      };
    case "credits":
      return {
        ...problem({
          kind: "credits",
          recoveryUrl: "https://console.neon.tech/app/org-fake/billing#ai_gateway_credits",
        }),
        locked: 65,
      };
    case "paidPlan":
      return { ...problem({ kind: "paidPlan" }), baseUrl: null };
    case "region":
      return { ...problem({ kind: "region", region: "azure-eastus2" }), baseUrl: null };
    case "noModels":
      return { ...problem({ kind: "noModels", reason: "model_restricted" }), locked: 3 };
    case "error":
      return problem({ kind: "unreachable", message: "HTTP 503: upstream unavailable" });
  }
}

export interface FakeModelsOptions {
  /** The environment's Neon branch (its target's, or a bound instance's), if any. */
  branchOf?: (environment: string) => string | null;
  /** The branch gateway's state (default `ok`), or an Error `neonGateway` throws. */
  gateway?: GatewayScenario | Error;
  /** What `profiles` answers (an Error is thrown, as with no CLI). */
  profiles?: DatabricksProfile[] | Error;
  /** What `missingSecrets` answers, by environment, over the fake's own. */
  missing?: (environment: string) => SecretNeeds | undefined;
}

export interface FakeModels {
  get(environment: string): Promise<ModelsInfo>;
  set(environment: string, choice: ModelsChoice): Promise<ModelsApplied>;
  profiles(): Promise<DatabricksProfile[]>;
  missingSecrets(environment: string): Promise<SecretNeeds>;
  neonGateway(environment: string): Promise<NeonGateway>;
  /** The choices made so far, by environment. */
  choices: ReadonlyMap<string, ModelsChoice>;
  /** Switch the gateway's scenario (Playwright, through `__LL_MODELS__`). */
  setGateway(scenario: GatewayScenario | Error): void;
  /** How many times the gateway was asked (each one mints and revokes a credential). */
  gatewayCalls: () => number;
}

const UPSTREAM: Record<ModelsChoice["kind"], string> = {
  keys: "keys",
  databricksProfile: "databricks",
  neonAiGateway: "neon",
};

const MODEL_MODULES = ["mlflow", "omnigent"];

export function createFakeModels(state: FakeState, options: FakeModelsOptions = {}): FakeModels {
  const choices = new Map<string, ModelsChoice>();
  let gateway: GatewayScenario | Error = options.gateway ?? "ok";
  let gatewayCalls = 0;
  const modulesOf = async (environment: string): Promise<string[]> => {
    const list = await state.stateWorkspaces();
    if (list.status === "error") return [];
    for (const ws of list.data) {
      const snapshot = await state.stateWorkspace(ws.id);
      if (snapshot.status === "error") continue;
      const env = snapshot.data.environments.find((e) => e.id === environment);
      const modules = (env?.selection as { modules?: unknown } | undefined)?.modules;
      if (env) return Array.isArray(modules) ? modules.map(String) : [];
    }
    return [];
  };
  return {
    choices,
    setGateway: (scenario) => {
      gateway = scenario;
    },
    gatewayCalls: () => gatewayCalls,
    neonGateway: async (environment) => {
      gatewayCalls += 1;
      const branch = options.branchOf?.(environment) ?? null;
      if (branch === null) throw new Error("the environment has no Neon branch: pick one first");
      if (gateway instanceof Error) throw gateway;
      return fakeGateway(gateway, branch);
    },
    get: async (environment) => {
      const modules = await modulesOf(environment);
      const module = MODEL_MODULES.find((m) => modules.includes(m));
      const choice = choices.get(environment);
      return {
        choice: choice ?? { kind: "keys" },
        chosen: choice !== undefined,
        wantsModels: module !== undefined,
        applied:
          module && choice
            ? {
                module,
                value: UPSTREAM[choice.kind],
              }
            : null,
      };
    },
    set: async (environment, choice) => {
      const modules = await modulesOf(environment);
      if (choice.kind === "neonAiGateway") {
        if ((options.branchOf?.(environment) ?? null) === null) {
          throw new Error(
            "the Neon AI Gateway is its branch's: pick a Neon branch for the environment first",
          );
        }
        if (!MODEL_MODULES.some((m) => modules.includes(m))) {
          throw new Error("nothing in the environment takes models: add MLflow or Omnigent first");
        }
      }
      choices.set(environment, choice);
      return { committed: null, needsCommit: false };
    },
    profiles: async () => {
      const profiles = options.profiles ?? FAKE_PROFILES;
      if (profiles instanceof Error) throw profiles;
      return profiles;
    },
    missingSecrets: async (environment) => {
      const over = options.missing?.(environment);
      if (over) return over;
      const choice = choices.get(environment);
      const required =
        choice?.kind === "databricksProfile" && !choice.host
          ? [{ name: "DATABRICKS_HOST", help: "The workspace's URL." }]
          : [];
      const optional =
        !choice || choice.kind === "keys"
          ? ["ANTHROPIC_API_KEY", "OPENAI_API_KEY", "GEMINI_API_KEY"].map((name) => ({
              name,
              help: null,
            }))
          : [];
      return { required, optional };
    },
  };
}
