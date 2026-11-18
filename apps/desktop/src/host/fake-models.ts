// The Models choice on a fake state, for the bun tests and the Playwright IPC mock:
// shell-core's `models` and `Runner::missing_secrets` in miniature. An environment
// wants models when its draft runs MLflow or Omnigent; a profile choice is "applied"
// at once (the fake has no knobs to commit), and a Databricks upstream with no host
// is the one missing secret it reports.

import type {
  DatabricksProfile,
  ModelsApplied,
  ModelsChoice,
  ModelsInfo,
  SecretNeeds,
} from "../shell/shell-host";
import type { FakeState } from "./fake-state";

export const FAKE_PROFILES: DatabricksProfile[] = [
  { name: "acct", host: "https://accounts.cloud.databricks.com", authType: "oauth-m2m" },
  { name: "work", host: "https://dbc-1.cloud.databricks.com", authType: "databricks-cli" },
];

export interface FakeModelsOptions {
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
  /** The choices made so far, by environment. */
  choices: ReadonlyMap<string, ModelsChoice>;
}

const MODEL_MODULES = ["mlflow", "omnigent"];

export function createFakeModels(state: FakeState, options: FakeModelsOptions = {}): FakeModels {
  const choices = new Map<string, ModelsChoice>();
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
                value: choice.kind === "databricksProfile" ? "databricks" : "keys",
              }
            : null,
      };
    },
    set: async (environment, choice) => {
      if (choice.kind === "neonAiGateway") {
        throw new Error("the Neon AI Gateway isn't offered yet (#169)");
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
