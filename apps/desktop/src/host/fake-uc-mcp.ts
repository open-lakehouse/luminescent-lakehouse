// `uc mcp serve` for the Playwright build (ipc-mock's `service_fetch` of `uc-mcp`), on
// fake-mcp's Streamable HTTP, with
// mangrove's real grants app (e2e/fixtures/grants-app.html) and `open_grant_form`'s A2UI.
// The tool calls it answered are on `window.__LL_UC_MCP__` for the test.

import grantsApp from "../../e2e/fixtures/grants-app.html?raw";
import grantForm from "../../e2e/fixtures/open_grant_form.json";
import {
  type FakeMcpRequest,
  type FakeResponse,
  fakeMcpResponse,
  initialized,
  type JsonRpc,
} from "./fake-mcp";

const MCP_PATH = "/uc-mcp/mcp";
const GRANTS_APP = "ui://uc/grants";
const APP_MIME = "text/html;profile=mcp-app";

export interface FakeUcMcpLog {
  calls: { name: string; arguments: Record<string, unknown> }[];
}

declare global {
  interface Window {
    __LL_UC_MCP__?: FakeUcMcpLog;
  }
}

const TOOLS = [
  {
    name: "open_grant_form",
    description: "Open a grant form",
    inputSchema: { type: "object" },
    annotations: { readOnlyHint: true },
    _meta: { ui: { resourceUri: GRANTS_APP } },
  },
  {
    name: "get_grants",
    description: "Grants on a securable",
    inputSchema: { type: "object" },
    annotations: { readOnlyHint: true },
  },
  {
    name: "search_principals",
    description: "Find principals",
    inputSchema: { type: "object" },
    annotations: { readOnlyHint: true },
  },
  {
    name: "whoami",
    description: "The caller",
    inputSchema: { type: "object" },
    annotations: { readOnlyHint: true },
    _meta: { ui: { visibility: ["app"] } },
  },
  {
    name: "apply_grants",
    description: "Apply the grant the user submitted",
    inputSchema: { type: "object" },
    annotations: { readOnlyHint: false },
    _meta: { ui: { visibility: ["app"] } },
  },
];

/** The form's A2UI, prefilled with the call's securable and principal. */
function grantFormFor(args: Record<string, unknown>) {
  const messages = structuredClone(grantForm) as Record<string, unknown>[];
  for (const m of messages) {
    const update = m.updateDataModel as { value?: Record<string, unknown> } | undefined;
    if (!update?.value) continue;
    update.value.securable = { type: args.securable_type, fullName: args.full_name };
    update.value.principal = args.principal ?? "";
    update.value.privileges = args.privileges ?? [];
  }
  return messages;
}

function callTool(log: FakeUcMcpLog, name: string, args: Record<string, unknown>) {
  log.calls.push({ name, arguments: args });
  switch (name) {
    case "open_grant_form":
      return {
        content: [
          { type: "text", text: `Opened a grant form for ${String(args.full_name)}.` },
          {
            type: "resource",
            resource: {
              uri: "a2ui://uc/grant-form",
              mimeType: "application/a2ui+json",
              text: JSON.stringify(grantFormFor(args)),
            },
          },
        ],
        structuredContent: { ...args, status: "awaiting_user" },
      };
    case "whoami":
      return {
        content: [],
        structuredContent: { principal: { kind: "user", name: "me@example.com" } },
      };
    case "search_principals":
      return { content: [], structuredContent: { principals: [] } };
    case "get_grants":
      return { content: [{ type: "text", text: "[]" }] };
    case "apply_grants": {
      const changes = (args.changes as { principal?: string; add?: string[] }[] | undefined) ?? [];
      const what = changes.map((c) => `${(c.add ?? []).join(", ")} to ${c.principal}`).join("; ");
      return {
        content: [{ type: "text", text: `Granted ${what} on ${String(args.full_name)}.` }],
        structuredContent: { ok: true },
      };
    }
    default:
      return { content: [{ type: "text", text: `unknown tool ${name}` }], isError: true };
  }
}

function answer(log: FakeUcMcpLog, m: JsonRpc): unknown {
  switch (m.method) {
    case "initialize":
      return initialized("uc", m);
    case "ping":
      return {};
    case "tools/list":
      return { tools: TOOLS };
    case "resources/read":
      return {
        contents: [{ uri: GRANTS_APP, mimeType: APP_MIME, text: grantsApp, _meta: { ui: {} } }],
      };
    case "tools/call":
      return callTool(
        log,
        String(m.params?.name),
        (m.params?.arguments as Record<string, unknown>) ?? {},
      );
    default:
      return undefined;
  }
}

export type { FakeResponse };

/** Answer one request the connection sends through the proxy. */
export function fakeUcMcp(log: FakeUcMcpLog, request: FakeMcpRequest): FakeResponse {
  return fakeMcpResponse(MCP_PATH, request, (m) => answer(log, m));
}
