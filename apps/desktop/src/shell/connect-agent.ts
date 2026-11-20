// "Connect an agent" (docs/ai-gateway.md): the snippets that point Claude Code or Codex
// on the laptop at a running instance's MLflow AI Gateway, so their calls use the
// environment's upstream. They never carry a token: the
// gateway's route drops whatever a client sends and calls the upstream with the
// environment's own credential, so each snippet says whose that is.

import type { AgentConnect } from "./shell-host";

/** The placeholder a client must send as its key: the gateway drops it. */
export const PLACEHOLDER_KEY = "via-mlflow-ai-gateway";

/** Whose credential the gateway calls the upstream with, in a line or two. */
export function authNote(connect: AgentConnect): string[] {
  switch (connect.upstream) {
    case "databricks":
      return [
        `Calls run as Databricks profile ${connect.profile ?? "(none)"}, on its workspace's AI Gateway,`,
        "with tokens the desktop mints: they work while the desktop runs.",
      ];
    case "neon":
      return [
        `Calls go to the AI Gateway of Neon branch ${connect.branch ?? "(unbound)"}, with the instance's`,
        "own credential, and draw on the organization's AI Gateway credits.",
      ];
    default:
      return [
        "Calls use the environment's own provider keys (ANTHROPIC_API_KEY, OPENAI_API_KEY, …),",
        "set as its variables or in the desktop's environment.",
      ];
  }
}

const pick = (models: string[], test: (m: string) => boolean) => models.find(test) ?? null;

/** The model Claude Code asks for, and its fast one. */
function claudeModels(models: string[]) {
  const claude = models.filter((m) => m.startsWith("claude"));
  return {
    main: pick(claude, (m) => m.includes("opus")) ?? claude[0] ?? models[0] ?? null,
    fast: pick(claude, (m) => m.includes("haiku")),
  };
}

/** A model Codex can ask for: an OpenAI one. */
function codexModel(models: string[]) {
  return pick(models, (m) => /^(gpt|o\d|codex)/.test(m));
}

const comment = (lines: string[]) => lines.map((l) => `# ${l}`);

/** Managed settings an organization installs (a gateway of its own) win over these. */
const MANAGED = "# An organization's managed settings for the agent, if any, take precedence.";

/** The `sh` lines for Claude Code. */
export function claudeCodeSnippet(connect: AgentConnect): string {
  const { main, fast } = claudeModels(connect.models);
  return [
    "# Claude Code on this environment's MLflow AI Gateway.",
    ...comment(authNote(connect)),
    "# No token here: the gateway drops whatever the client sends.",
    MANAGED,
    `export ANTHROPIC_BASE_URL="${connect.gatewayUrl}/anthropic"`,
    `export ANTHROPIC_AUTH_TOKEN=${PLACEHOLDER_KEY}`,
    ...(main ? [`export ANTHROPIC_MODEL=${main}`] : ["# The gateway serves no Claude model yet."]),
    ...(fast ? [`export ANTHROPIC_DEFAULT_HAIKU_MODEL=${fast}`] : []),
    "claude",
  ].join("\n");
}

/** The file to save Codex's profile as (its profiles are files next to config.toml). */
export const CODEX_PROFILE_FILE = "~/.codex/lakehouse.config.toml";

/** A Codex profile (`~/.codex/lakehouse.config.toml`) on the gateway, and how to run it. */
export function codexSnippet(connect: AgentConnect): string {
  const model = codexModel(connect.models);
  return [
    `# Codex on this environment's MLflow AI Gateway: save as ${CODEX_PROFILE_FILE},`,
    `# then run \`LAKEHOUSE_GATEWAY_KEY=${PLACEHOLDER_KEY} codex --profile lakehouse\`.`,
    ...comment(authNote(connect)),
    "# No token here: the gateway drops whatever the client sends.",
    MANAGED,
    ...(model
      ? [`model = "${model}"`]
      : [
          "# The gateway serves no OpenAI model: add one to MLflow's",
          "# `ai_gateway_models` knob (e.g. openai:gpt-5-5), then name it here.",
          '# model = "gpt-5-5"',
        ]),
    'model_provider = "lakehouse"',
    "",
    "[model_providers.lakehouse]",
    'name = "Lakehouse MLflow AI Gateway"',
    `base_url = "${connect.gatewayUrl}/openai/v1"`,
    'wire_api = "responses"',
    'env_key = "LAKEHOUSE_GATEWAY_KEY"',
  ].join("\n");
}
