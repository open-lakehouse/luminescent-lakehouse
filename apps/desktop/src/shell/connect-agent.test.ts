import { describe, expect, test } from "bun:test";
import { authNote, claudeCodeSnippet, codexSnippet, PLACEHOLDER_KEY } from "./connect-agent";
import type { AgentConnect } from "./shell-host";

const connect = (over: Partial<AgentConnect> = {}): AgentConnect => ({
  gatewayUrl: "http://localhost:9080/ai-gateway",
  models: ["claude-opus-5-5", "claude-sonnet-5-5", "claude-haiku-5-5"],
  upstream: "keys",
  profile: null,
  branch: null,
  ...over,
});

/** Anything that looks like a credential: Neon, Databricks, OpenAI, Anthropic. */
const TOKEN = /nt_live|nak_live|dapi[0-9a-f]|sk-|sk-ant-/;

describe("connect an agent", () => {
  test("Claude Code reaches the gateway's Anthropic route with its models", () => {
    const sh = claudeCodeSnippet(connect());
    expect(sh).toContain('export ANTHROPIC_BASE_URL="http://localhost:9080/ai-gateway/anthropic"');
    expect(sh).toContain(`export ANTHROPIC_AUTH_TOKEN=${PLACEHOLDER_KEY}`);
    expect(sh).toContain("export ANTHROPIC_MODEL=claude-opus-5-5");
    expect(sh).toContain("export ANTHROPIC_DEFAULT_HAIKU_MODEL=claude-haiku-5-5");
    expect(sh.trimEnd().endsWith("claude")).toBe(true);
  });

  test("Codex gets a provider on the OpenAI route, and says when no OpenAI model is served", () => {
    const toml = codexSnippet(connect({ models: ["claude-opus-5-5", "gpt-5-5"] }));
    expect(toml).toContain('base_url = "http://localhost:9080/ai-gateway/openai/v1"');
    expect(toml).toContain('wire_api = "responses"');
    expect(toml).toContain('model = "gpt-5-5"');
    const none = codexSnippet(connect());
    expect(none).toContain("serves no OpenAI model");
    expect(none).not.toMatch(/^model = /m);
  });

  test("each upstream says whose credential the gateway calls with", () => {
    expect(authNote(connect()).join(" ")).toContain("provider keys");
    const dbx = connect({ upstream: "databricks", profile: "work" });
    expect(authNote(dbx).join(" ")).toContain("Databricks profile work");
    expect(authNote(dbx).join(" ")).toContain("while the desktop runs");
    const neon = connect({ upstream: "neon", branch: "dev" });
    expect(authNote(neon).join(" ")).toContain("Neon branch dev");
    expect(authNote(neon).join(" ")).toContain("credits");
    for (const c of [connect(), dbx, neon]) {
      expect(claudeCodeSnippet(c)).toContain(authNote(c)[0]);
      expect(codexSnippet(c)).toContain(authNote(c)[0]);
    }
  });

  test("no snippet ever carries a token", () => {
    for (const upstream of ["keys", "databricks", "neon"]) {
      const c = connect({ upstream, profile: "work", branch: "dev" });
      for (const text of [claudeCodeSnippet(c), codexSnippet(c)]) {
        expect(text).not.toMatch(TOKEN);
        expect(text).not.toContain("NEON_AI_GATEWAY_TOKEN");
        expect(text).not.toContain("DATABRICKS_TOKEN");
      }
    }
  });
});
