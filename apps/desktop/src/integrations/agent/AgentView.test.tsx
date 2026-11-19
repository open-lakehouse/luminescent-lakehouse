// The Agent app on fakes: the scripted Omnigent of src/host/fake-agent.ts behind the
// session commands, a fake `uc-mcp` connection, and a stand-in for the sandboxed
// frame that records what the app was given and acts for it.

import { afterEach, beforeEach, describe, expect, test } from "bun:test";
import type { CallToolResult, Tool } from "@modelcontextprotocol/client";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import {
  createFakeAgent,
  type FakeAgent,
  GRANT_INPUT,
  GRANT_TOOL_NAME,
  READY_AGENT,
} from "../../host/fake-agent";
import { createFakeShellHost, type FakeShellHost } from "../../host/fake-host";
import { AgentFailure, type ChatMessage, ShellHostProvider } from "../../shell/shell-host";
import type { McpConnection } from "../mcp/connection";
import type { McpAppProps } from "../mcp/McpApp";
import { AgentView, type AgentViewProps } from "./AgentView";
import { LAKEHOUSE } from "./lakehouse";

beforeEach(() => sessionStorage.clear());
afterEach(cleanup);

const GRANTS_APP = "ui://uc/grants";
const AWAITING: CallToolResult = {
  content: [{ type: "text", text: "Opened a grant form" }],
  structuredContent: { status: "awaiting_user", full_name: "main.default.t" },
};

interface FakeConnection extends McpConnection {
  calls: { name: string; args: unknown; from?: string }[];
  reads: string[];
}

function fakeConnection(): FakeConnection {
  const tools: Tool[] = [
    {
      name: "open_grant_form",
      inputSchema: { type: "object" },
      annotations: { readOnlyHint: true },
      _meta: { ui: { resourceUri: GRANTS_APP } },
    },
    { name: "get_grants", inputSchema: { type: "object" }, annotations: { readOnlyHint: true } },
    {
      name: "apply_grants",
      inputSchema: { type: "object" },
      _meta: { ui: { visibility: ["app"] } },
    },
  ];
  const conn: FakeConnection = {
    environment: "i1",
    server: "uc",
    calls: [],
    reads: [],
    tools: async () => tools,
    tool: async (name) => tools.find((t) => t.name === name),
    uiResourceUri: (t) => (t._meta?.ui as { resourceUri?: string } | undefined)?.resourceUri,
    isReadOnly: (t) => t.annotations?.readOnlyHint === true,
    isAppCallable: () => true,
    isModelVisible: (t) => t.name !== "apply_grants",
    readApp: async (uri) => {
      conn.reads.push(uri);
      return { uri, html: "<!doctype html><p>grants</p>", meta: {} };
    },
    callTool: async (name, args, options) => {
      conn.calls.push({ name, args, from: options?.from });
      if (name === "open_grant_form") return AWAITING;
      return { content: [{ type: "text", text: "Granted SELECT on main.default.t to x." }] };
    },
    close: async () => {},
  };
  return conn;
}

/** The frame's stand-in: what the app got, and buttons that act as the app would. */
let shown: McpAppProps[] = [];
function StubApp(props: McpAppProps) {
  shown.push(props);
  return (
    <div data-testid="stub-app" data-uri={props.resource?.uri ?? ""}>
      <button
        type="button"
        onClick={async () => {
          const result = await props.onCallTool?.({
            name: "apply_grants",
            arguments: { full_name: "main.default.t" },
          });
          const text = (result?.content?.[0] as { text?: string } | undefined)?.text ?? "";
          props.onUpdateModelContext?.({ content: [{ type: "text", text }] });
        }}
      >
        Submit form
      </button>
      <button type="button" onClick={() => props.onOpenLink?.("https://docs.example.com/")}>
        Docs
      </button>
    </div>
  );
}

function renderAgent(
  options: { agent?: FakeAgent; conn?: FakeConnection; props?: Partial<AgentViewProps> } = {},
) {
  shown = [];
  const agent = options.agent ?? createFakeAgent();
  const conn = options.conn ?? fakeConnection();
  const shell: FakeShellHost = createFakeShellHost({ agent });
  const view = render(
    <ShellHostProvider host={shell}>
      <AgentView
        instance="i1"
        label="dev #1"
        {...LAKEHOUSE}
        mcp={async () => conn}
        App={StubApp}
        {...options.props}
      />
    </ShellHostProvider>,
  );
  return { agent, conn, shell, view };
}

async function ask(text: string) {
  const box = await screen.findByRole("textbox", { name: "Message the agent" });
  fireEvent.change(box, { target: { value: text } });
  fireEvent.submit(box.closest("form") as HTMLFormElement);
}

const lastSent = (agent: FakeAgent) => agent.sent.at(-1)?.text;

describe("the Agent app", () => {
  test("says when the agent isn't installed, and checks again", async () => {
    const agent = createFakeAgent({ status: { ...READY_AGENT, agentId: null } });
    renderAgent({ agent });
    const notice = await screen.findByTestId("agent-not-installed");
    expect(notice.textContent).toContain("just agent-install");
    agent.setStatus(READY_AGENT);
    fireEvent.click(within(notice).getByRole("button", { name: /Check again/ }));
    await screen.findByTestId("agent-view");
  });

  test("says when no host is online, or Omnigent doesn't answer", async () => {
    const agent = createFakeAgent({ status: { ...READY_AGENT, hostOnline: false } });
    renderAgent({ agent });
    expect((await screen.findByTestId("agent-no-host")).textContent).toContain("omnigent-host");
    cleanup();
    renderAgent({ agent: createFakeAgent({ status: new AgentFailure({ kind: "noOmnigent" }) }) });
    expect((await screen.findByTestId("agent-unreachable")).textContent).toContain(
      "doesn't run Omnigent",
    );
  });

  test("a grant request opens the form, with the full result of a re-run", async () => {
    const { agent, conn } = renderAgent();
    await ask("grant SELECT on main.default.t to group x");
    await screen.findByTestId("stub-app");
    expect(agent.sent.map((s) => s.text)).toEqual(["grant SELECT on main.default.t to group x"]);
    expect(screen.getByText("I'll open the grant form.")).toBeTruthy();
    await waitFor(() => expect(shown.at(-1)?.toolResult).toEqual(AWAITING));
    const props = shown.at(-1) as McpAppProps;
    expect(props.toolInput).toEqual(GRANT_INPUT);
    expect(props.resource?.uri).toBe(GRANTS_APP);
    // Re-run once, with the agent's arguments, however often the part renders.
    expect(conn.calls).toEqual([{ name: "open_grant_form", args: GRANT_INPUT, from: undefined }]);
    expect(conn.reads).toEqual([GRANTS_APP]);
  });

  test("the form's submit goes through the desktop's connection, and its outcome back to the agent", async () => {
    const { agent, conn, shell } = renderAgent();
    await ask("grant SELECT on main.default.t to group x");
    await waitFor(() => expect(shown.at(-1)?.toolResult).toEqual(AWAITING));
    await waitFor(() => expect(agent.sessions.get("sess_1")?.status).toBe("idle"));
    fireEvent.click(screen.getByRole("button", { name: "Submit form" }));
    await waitFor(() => expect(agent.sent).toHaveLength(2));
    expect(conn.calls.at(-1)).toEqual({
      name: "apply_grants",
      args: { full_name: "main.default.t" },
      from: "app",
    });
    expect(lastSent(agent)).toBe(
      '<mcp-app-context tool="open_grant_form" call_id="call_1">Granted SELECT on main.default.t to x.</mcp-app-context>',
    );
    // It shows as the app's chip, not as something the user wrote; the agent confirms.
    const chip = await screen.findByTestId("agent-context-chip");
    expect(chip.textContent).toContain("open_grant_form: Granted SELECT");
    await screen.findByText("Done: x now holds SELECT on main.default.t.");
    // The app's link opens in the browser.
    fireEvent.click(screen.getByRole("button", { name: "Docs" }));
    await waitFor(() => expect(shell.opened).toEqual(["https://docs.example.com/"]));
  });

  test("other tool calls are plain cards", async () => {
    const agent = createFakeAgent({
      reply: (_text, turn) => [
        {
          id: `a${turn}`,
          role: "assistant",
          parts: [
            {
              type: "dynamic-tool",
              toolCallId: "c1",
              toolName: "uc__get_grants",
              state: "output-available",
              input: { full_name: "main.default.t" },
              output: "[]",
              errorText: null,
              approval: null,
            },
          ],
        },
      ],
    });
    const { conn } = renderAgent({ agent });
    await ask("who can read main.default.t?");
    await waitFor(() => expect(document.querySelector('[data-slot="tool-call"]')).not.toBeNull());
    expect(screen.queryByTestId("stub-app")).toBeNull();
    expect(conn.calls).toEqual([]);
  });

  test("a form from the history stays closed until asked, and runs nothing before", async () => {
    const agent = createFakeAgent();
    const created = await agent.create("i1", "lakehouse", null);
    const history: ChatMessage[] = [
      {
        id: "u0",
        role: "user",
        parts: [{ type: "text", text: "grant SELECT on main.default.t to x", state: "done" }],
      },
      {
        id: "a0",
        role: "assistant",
        parts: [
          {
            type: "dynamic-tool",
            toolCallId: "call_0",
            toolName: GRANT_TOOL_NAME,
            state: "output-available",
            input: GRANT_INPUT,
            output: "Opened a grant form",
            errorText: null,
            approval: null,
          },
        ],
      },
    ];
    agent.emit(created.id, { kind: "upsert", message: history[0] as ChatMessage });
    agent.emit(created.id, { kind: "upsert", message: history[1] as ChatMessage });
    sessionStorage.setItem("agent-session:i1", created.id);
    const { conn } = renderAgent({ agent });
    const reopen = await screen.findByRole("button", { name: "Open form again" });
    expect(screen.queryByTestId("stub-app")).toBeNull();
    expect(conn.calls).toEqual([]);
    fireEvent.click(reopen);
    await screen.findByTestId("stub-app");
    await waitFor(() => expect(conn.calls.map((c) => c.name)).toEqual(["open_grant_form"]));
  });

  test("stops a running turn, and starts a new chat", async () => {
    const agent = createFakeAgent({ delay: 200 });
    renderAgent({ agent });
    await ask("hello");
    const stop = await screen.findByRole("button", { name: /stop/i });
    fireEvent.click(stop);
    await waitFor(() => expect(agent.interrupts).toEqual(["sess_1"]));
    fireEvent.click(screen.getByRole("button", { name: /New chat/ }));
    await waitFor(() => expect(agent.sessions.size).toBe(2));
    await waitFor(() => expect(screen.queryByText("hello")).toBeNull());
  });

  test("asks for the lakehouse agent and starts its sessions", async () => {
    const { agent } = renderAgent();
    await ask("hello");
    await waitFor(() => expect(agent.sent).toHaveLength(1));
    expect(agent.agents).toEqual(["lakehouse", "lakehouse"]);
  });
});

describe("one given session (the factory's Open session)", () => {
  const history: ChatMessage[] = [
    {
      id: "u0",
      role: "user",
      parts: [{ type: "text", text: "Implement the backend", state: "done" }],
    },
    {
      id: "a0",
      role: "assistant",
      parts: [
        { type: "text", text: "Opened the PR.", state: "done" },
        {
          type: "dynamic-tool",
          toolCallId: "call_0",
          toolName: GRANT_TOOL_NAME,
          state: "output-available",
          input: GRANT_INPUT,
          output: "Opened a grant form",
          errorText: null,
          approval: null,
        },
      ],
    },
  ];

  test("follows it without the agent installed or a host, and starts no other", async () => {
    const agent = createFakeAgent({
      status: { ...READY_AGENT, agentId: null, hostOnline: false },
      sessions: { "sess-demo-2": history },
    });
    renderAgent({
      agent,
      props: { agent: "factory-reviewer", session: "sess-demo-2", heading: "review" },
    });
    expect(await screen.findByText("Opened the PR.")).toBeDefined();
    expect(screen.getByTestId("agent-view").textContent).toContain("review");
    expect(screen.queryByRole("button", { name: /New chat/ })).toBeNull();
    // Its uc call is an app of the uc server, from the history: collapsed.
    expect(await screen.findByRole("button", { name: "Open form again" })).toBeDefined();
    await ask("and the tests?");
    await waitFor(() =>
      expect(agent.sent).toEqual([{ sessionId: "sess-demo-2", text: "and the tests?" }]),
    );
    expect(agent.agents).toEqual(["factory-reviewer"]);
    expect(sessionStorage.length).toBe(0);
  });

  test("a session Omnigent doesn't have says so, and stays", async () => {
    const agent = createFakeAgent();
    renderAgent({ agent, props: { agent: "factory-executor", session: "sess-gone" } });
    expect((await screen.findByRole("alert")).textContent).toContain("sess-gone");
    expect(screen.queryByRole("button", { name: /New chat/ })).toBeNull();
  });

  test("without servers, every call is a plain card", async () => {
    const agent = createFakeAgent({ sessions: { "sess-demo-2": history } });
    const { conn } = renderAgent({
      agent,
      props: { agent: "factory-reviewer", session: "sess-demo-2", servers: [] },
    });
    await screen.findByText("Opened the PR.");
    expect(screen.queryByRole("button", { name: "Open form again" })).toBeNull();
    expect(conn.calls).toEqual([]);
  });
});
