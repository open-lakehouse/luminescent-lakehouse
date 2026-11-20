// The factory's `workgraph-mcp` for the Playwright build (ipc-mock's `service_fetch` of
// `factory` at `/mcp`), on fake-mcp's Streamable HTTP: the real plan review view
// (crates/workgraph-mcp/assets/plan-review.html, the committed build) and
// `open_plan_review`'s real result on the demo plan (e2e/fixtures/open_plan_review.json,
// written by workgraph-mcp's tests). The tool calls it answered are on
// `window.__LL_WORKGRAPH_MCP__` for the test.

import planReview from "../../../../crates/workgraph-mcp/assets/plan-review.html?raw";
import openPlanReview from "../../e2e/fixtures/open_plan_review.json";
import {
  type FakeMcpRequest,
  type FakeResponse,
  fakeMcpResponse,
  initialized,
  type JsonRpc,
} from "./fake-mcp";

/** The in-process factory's MCP endpoint (shell-core's `factory::MCP_PATH`). */
export const WORKGRAPH_MCP_PATH = "/mcp";
const PLAN_VIEW = "ui://factory/plan";
const APP_MIME = "text/html;profile=mcp-app";

export interface FakeWorkgraphMcpLog {
  calls: { name: string; arguments: Record<string, unknown> }[];
}

declare global {
  interface Window {
    __LL_WORKGRAPH_MCP__?: FakeWorkgraphMcpLog;
  }
}

const object = { type: "object" };

// As workgraph-mcp lists them to the desktop's apps grant (a few of its tools).
const TOOLS = [
  {
    name: "plan_read",
    description: "Read a plan",
    inputSchema: object,
    annotations: { readOnlyHint: true },
  },
  {
    name: "submit",
    description: "Submit a plan",
    inputSchema: object,
    annotations: { readOnlyHint: false },
  },
  {
    name: "open_plan_review",
    description: "Show the plan to the person for review",
    inputSchema: object,
    annotations: { readOnlyHint: true },
    _meta: { ui: { resourceUri: PLAN_VIEW } },
  },
  {
    name: "approve_revision",
    description: "Approve the plan at the reviewed revision",
    inputSchema: object,
    annotations: { readOnlyHint: false },
    _meta: { ui: { visibility: ["app"] } },
  },
  {
    name: "return_plan",
    description: "Return the plan with notes",
    inputSchema: object,
    annotations: { readOnlyHint: false },
    _meta: { ui: { visibility: ["app"] } },
  },
];

const review = openPlanReview.structuredContent;

const text = (t: string) => [{ type: "text", text: t }];

function callTool(log: FakeWorkgraphMcpLog, name: string, args: Record<string, unknown>) {
  log.calls.push({ name, arguments: args });
  switch (name) {
    case "open_plan_review":
      return openPlanReview;
    case "submit":
      return {
        content: text(`{"status":"awaiting_approval","revision":"${review.revision}"}`),
        structuredContent: { status: "awaiting_approval", root: review.root },
      };
    case "approve_revision":
      // As `Factory::approve`: only the current revision.
      if (args.revision !== review.revision) {
        return {
          content: text(`${String(args.revision)} isn't the current revision (${review.revision})`),
          isError: true,
        };
      }
      return {
        content: text(`Approved the plan ${review.root} at revision ${review.revision}.`),
        structuredContent: { status: "approved", root: review.root, revision: review.revision },
      };
    case "return_plan":
      return {
        content: text(`Returned the plan ${review.root} with notes: ${String(args.notes)}`),
        structuredContent: { status: "returned", root: review.root, notes: args.notes },
      };
    default:
      return { content: text(`unknown tool ${name}`), isError: true };
  }
}

function answer(log: FakeWorkgraphMcpLog, m: JsonRpc): unknown {
  switch (m.method) {
    case "initialize":
      return initialized("workgraph", m);
    case "ping":
      return {};
    case "tools/list":
      return { tools: TOOLS };
    case "resources/list":
      return { resources: [{ uri: PLAN_VIEW, name: "plan-review", mimeType: APP_MIME }] };
    case "resources/read":
      return { contents: [{ uri: PLAN_VIEW, mimeType: APP_MIME, text: planReview }] };
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

/** Answer one request the connection sends to the factory's `/mcp` through the proxy. */
export function fakeWorkgraphMcp(log: FakeWorkgraphMcpLog, request: FakeMcpRequest): FakeResponse {
  return fakeMcpResponse(WORKGRAPH_MCP_PATH, request, (m) => answer(log, m));
}

/** Whether a `factory` request is for its MCP endpoint rather than its Connect API. */
export const isWorkgraphMcp = (path: string) =>
  new URL(path, "http://factory.invalid").pathname === WORKGRAPH_MCP_PATH;
