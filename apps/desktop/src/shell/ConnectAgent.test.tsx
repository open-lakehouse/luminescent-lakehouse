// The MLflow tab's "Connect an agent": its snippets for Claude Code and Codex, from
// `run.connectAgent`, copied as shown.

import { afterEach, expect, test } from "bun:test";
import { createFakeHost } from "@open-lakehouse/host/testing";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { Providers } from "../app";
import { createFakeShellHost } from "../host/fake-host";
import { ConnectAgentDialog } from "./ConnectAgent";
import type { AgentConnect } from "./shell-host";

afterEach(cleanup);

const CONNECT: AgentConnect = {
  gatewayUrl: "http://localhost:9080/ai-gateway",
  models: ["claude-opus-5-5", "claude-haiku-5-5", "gpt-5-5"],
  upstream: "databricks",
  profile: "work",
  branch: null,
};

function show(connect: AgentConnect | null) {
  const host = createFakeHost({ kind: "desktop", environment: null });
  const shell = createFakeShellHost({ connectAgent: () => connect });
  render(
    <Providers host={host} shell={shell}>
      <ConnectAgentDialog instance="i-1" onClose={() => {}} />
    </Providers>,
  );
}

test("it shows Claude Code's snippet, then Codex's, and copies the one shown", async () => {
  const copied: string[] = [];
  Object.defineProperty(navigator, "clipboard", {
    value: { writeText: async (text: string) => void copied.push(text) },
    configurable: true,
  });
  show(CONNECT);
  const snippet = await screen.findByTestId("connect-agent-snippet");
  expect(snippet.textContent).toContain(
    'export ANTHROPIC_BASE_URL="http://localhost:9080/ai-gateway/anthropic"',
  );
  expect(snippet.textContent).toContain("Databricks profile work");
  fireEvent.click(screen.getByTestId("connect-agent-codex"));
  await waitFor(() =>
    expect(screen.getByTestId("connect-agent-snippet").textContent).toContain(
      'base_url = "http://localhost:9080/ai-gateway/openai/v1"',
    ),
  );
  fireEvent.click(screen.getByTestId("connect-agent-copy"));
  await waitFor(() => expect(copied).toHaveLength(1));
  expect(copied[0]).toContain('model = "gpt-5-5"');
  expect(copied[0]).toContain("managed settings");
});

test("an instance without MLflow says so and copies nothing", async () => {
  show(null);
  await screen.findByTestId("connect-agent-none");
  expect((screen.getByTestId("connect-agent-copy") as HTMLButtonElement).disabled).toBe(true);
});
