// connection.ts against an in-process `uc mcp serve` stand-in: an McpServer behind
// the SDK's Streamable HTTP server transport, reached through the `fetch` the
// connection would otherwise get from the host. The router keeps a transport per
// session and answers an unknown session with 404, as a multi-session server does
// (mangrove's rmcp one included), so a restart is `server.restart()`.
//
// The preloaded happy-dom replaces the web stream globals with Node stream
// polyfills, which break the SDK's SSE parsing (its parser extends
// TransformStream when it loads). This file puts the native ones back while it
// runs and loads the SDK after that; the webview has them natively anyway.

import { afterAll, afterEach, beforeAll, describe, expect, test } from "bun:test";
import type {
  McpServer as McpServerType,
  WebStandardStreamableHTTPServerTransport as ServerTransport,
} from "@modelcontextprotocol/server";
import * as z from "zod";
import {
  CLIENT_CAPABILITIES_META,
  closeMcpConnections,
  connectMcp,
  MCP_APP_MIME_TYPE,
  MCP_UI_EXTENSION,
  McpToolNotCallableError,
  mcpConnection,
  UC_APPS,
  UC_MCP_URL,
  withClientCapabilities,
} from "./connection";

const GRANTS_APP = "ui://uc/grants";
const GRANTS_HTML = "<!doctype html><html><head></head><body><script>1</script></body></html>";
const GRANTS_CSP = { connectDomains: [], resourceDomains: ["https://cdn.example.com"] };

interface FakeServer {
  fetch: (input: string | URL | Request, init?: RequestInit) => Promise<Response>;
  /** Every session's client capabilities, in order. */
  capabilities: unknown[];
  /** Tool calls, in order. */
  calls: { name: string; args: unknown }[];
  /** Requests by JSON-RPC method, in order. */
  methods: string[];
  /** Every request's path, in order. */
  paths: string[];
  /** Drops every session, as a restarted server would. */
  restart(): void;
  /** Adds a tool to the live sessions (they notify `tools/list_changed`). */
  addTool(name: string): void;
  mimeType: string;
}

function fakeUcMcp(): FakeServer {
  const sessions = new Map<string, { transport: ServerTransport; server: McpServerType }>();
  const fake: FakeServer = {
    capabilities: [],
    calls: [],
    methods: [],
    paths: [],
    mimeType: MCP_APP_MIME_TYPE,
    restart: () => sessions.clear(),
    addTool: (name) => {
      for (const { server } of sessions.values()) {
        server.registerTool(name, { description: name }, () => ({ content: [] }));
      }
    },
    fetch: async (input, init) => {
      const request = new Request(input, init);
      fake.paths.push(new URL(request.url).pathname);
      if (request.method === "POST") {
        const body = await request.clone().json();
        for (const m of Array.isArray(body) ? body : [body])
          if (m.method) fake.methods.push(m.method);
      }
      const id = request.headers.get("mcp-session-id");
      if (id) {
        const session = sessions.get(id);
        if (!session) return new Response("session not found", { status: 404 });
        return session.transport.handleRequest(request);
      }
      if (request.method !== "POST") return new Response(null, { status: 405 });
      const server = ucServer(fake);
      const transport = new WebStandardStreamableHTTPServerTransport({
        sessionIdGenerator: () => crypto.randomUUID(),
        enableJsonResponse: true,
        onsessioninitialized: (sid) => {
          sessions.set(sid, { transport, server });
        },
      });
      await server.connect(transport);
      return transport.handleRequest(request);
    },
  };
  return fake;
}

/** The tools and resource mangrove's server registers, the way it registers them. */
function ucServer(fake: FakeServer): McpServerType {
  const server = new McpServer({ name: "uc", version: "0.0.0" });
  server.registerTool(
    "open_grant_form",
    {
      description: "Show the user a form to grant privileges",
      inputSchema: z.object({ securable_type: z.string(), full_name: z.string() }),
      annotations: { readOnlyHint: true, destructiveHint: false },
      _meta: { ui: { resourceUri: GRANTS_APP, visibility: ["model", "app"] } },
    },
    (args) => {
      fake.calls.push({ name: "open_grant_form", args });
      return {
        content: [{ type: "text", text: "Opened the grant form" }],
        structuredContent: { securable: args.full_name },
        _meta: { ui: { resourceUri: GRANTS_APP } },
      };
    },
  );
  server.registerTool(
    "get_grants",
    {
      description: "Grants on a securable",
      annotations: { readOnlyHint: true },
      _meta: { ui: { visibility: ["model"] } },
    },
    () => ({ content: [{ type: "text", text: "[]" }] }),
  );
  // Only for clients that render apps, as mangrove gates it.
  const apply = server.registerTool(
    "apply_grants",
    {
      description: "Apply grant changes",
      inputSchema: z.object({ full_name: z.string(), changes: z.array(z.unknown()) }),
      annotations: { readOnlyHint: false, destructiveHint: false },
      _meta: { ui: { visibility: ["app"] } },
    },
    (args) => {
      fake.calls.push({ name: "apply_grants", args });
      return { content: [{ type: "text", text: "applied" }] };
    },
  );
  apply.disable();
  server.server.oninitialized = () => {
    const capabilities = server.server.getClientCapabilities();
    fake.capabilities.push(capabilities);
    if ((capabilities?.extensions as Record<string, unknown> | undefined)?.[MCP_UI_EXTENSION]) {
      apply.enable();
    }
  };
  server.registerResource("grants", GRANTS_APP, { mimeType: MCP_APP_MIME_TYPE }, (uri) => ({
    contents: [
      {
        uri: uri.href,
        mimeType: fake.mimeType,
        text: GRANTS_HTML,
        _meta: { ui: { csp: GRANTS_CSP, prefersBorder: false } },
      },
    ],
  }));
  return server;
}

const STREAMS = [
  "ReadableStream",
  "WritableStream",
  "TransformStream",
  "TextDecoderStream",
] as const;
const polyfilled = new Map<string, unknown>();
let McpServer: typeof McpServerType;
let WebStandardStreamableHTTPServerTransport: typeof ServerTransport;

beforeAll(async () => {
  const web = (await import("node:stream/web")) as unknown as Record<string, unknown>;
  const global = globalThis as unknown as Record<string, unknown>;
  for (const name of STREAMS) {
    polyfilled.set(name, global[name]);
    global[name] = web[name];
  }
  ({ McpServer, WebStandardStreamableHTTPServerTransport } = await import(
    "@modelcontextprotocol/server"
  ));
});
afterAll(() => {
  const global = globalThis as unknown as Record<string, unknown>;
  for (const [name, value] of polyfilled) global[name] = value;
});
afterEach(() => closeMcpConnections());

describe("the MCP connection", () => {
  test("declares the MCP Apps extension, so the server lists its app-only tools", async () => {
    const server = fakeUcMcp();
    const mcp = await connectMcp("local", { fetch: server.fetch });
    const names = (await mcp.tools()).map((t) => t.name).sort();
    expect(server.capabilities).toEqual([
      { extensions: { [MCP_UI_EXTENSION]: { mimeTypes: [MCP_APP_MIME_TYPE] } } },
    ]);
    expect(names).toEqual(["apply_grants", "get_grants", "open_grant_form"]);
    await mcp.close();
  });

  test("declares it in every request's _meta too, for a stateless server", async () => {
    const server = fakeUcMcp();
    const sent: { method?: string; id?: unknown; params?: { _meta?: Record<string, unknown> } }[] =
      [];
    const fetch = async (input: string | URL | Request, init?: RequestInit) => {
      if (init?.method === "POST" && typeof init.body === "string") {
        const body = JSON.parse(init.body);
        sent.push(...(Array.isArray(body) ? body : [body]));
      }
      return server.fetch(input, init);
    };
    const mcp = await connectMcp("local", { fetch });
    await mcp.tools();
    await mcp.callTool("get_grants", {});
    const declared = { extensions: { [MCP_UI_EXTENSION]: { mimeTypes: [MCP_APP_MIME_TYPE] } } };
    const meta = (method: string) => sent.find((m) => m.method === method)?.params?._meta;
    expect(meta("tools/list")?.[CLIENT_CAPABILITIES_META]).toEqual(declared);
    expect(meta("tools/call")?.[CLIENT_CAPABILITIES_META]).toEqual(declared);
    // initialize declares them itself, and notifications have no capabilities.
    expect(meta("initialize")).toBeUndefined();
    expect(meta("notifications/initialized")).toBeUndefined();
    await mcp.close();
  });

  test("keeps a request's own _meta and anything that isn't JSON-RPC", async () => {
    const seen: (string | undefined)[] = [];
    const fetch = withClientCapabilities(
      async (_url, init) => {
        seen.push(init?.body as string | undefined);
        return new Response(null, { status: 202 });
      },
      { extensions: { x: {} } },
    );
    const call = {
      jsonrpc: "2.0",
      id: 7,
      method: "tools/call",
      params: { name: "t", _meta: { progressToken: 1 } },
    };
    await fetch("http://x.invalid/mcp", { method: "POST", body: JSON.stringify([call]) });
    await fetch("http://x.invalid/mcp", { method: "POST", body: "not json" });
    await fetch("http://x.invalid/mcp", { method: "GET" });
    expect(JSON.parse(seen[0] ?? "[]")).toEqual([
      {
        ...call,
        params: {
          name: "t",
          _meta: { progressToken: 1, [CLIENT_CAPABILITIES_META]: { extensions: { x: {} } } },
        },
      },
    ]);
    expect(seen.slice(1)).toEqual(["not json", undefined]);
  });

  test("speaks to trestle's uc-mcp endpoint under the binding root", async () => {
    expect(UC_MCP_URL).toBe("http://uc-mcp.invalid/uc-mcp/mcp");
    const server = fakeUcMcp();
    const mcp = await connectMcp("local", { fetch: server.fetch });
    await mcp.tools();
    expect(new Set(server.paths)).toEqual(new Set(["/uc-mcp/mcp"]));
    await mcp.close();
  });

  test("reads a tool's app, read-only hint and visibility", async () => {
    const mcp = await connectMcp("local", { fetch: fakeUcMcp().fetch });
    const form = await mcp.tool("open_grant_form");
    const apply = await mcp.tool("apply_grants");
    const grants = await mcp.tool("get_grants");
    if (!form || !apply || !grants) throw new Error("missing tools");
    expect(mcp.uiResourceUri(form)).toBe(GRANTS_APP);
    expect(mcp.uiResourceUri(apply)).toBeUndefined();
    expect([mcp.isReadOnly(form), mcp.isReadOnly(apply)]).toEqual([true, false]);
    expect([mcp.isAppCallable(form), mcp.isModelVisible(form)]).toEqual([true, true]);
    expect([mcp.isAppCallable(apply), mcp.isModelVisible(apply)]).toEqual([true, false]);
    expect([mcp.isAppCallable(grants), mcp.isModelVisible(grants)]).toEqual([false, true]);
    await mcp.close();
  });

  test("caches the tool list until the server says it changed", async () => {
    const server = fakeUcMcp();
    const mcp = await connectMcp("local", { fetch: server.fetch });
    await mcp.tools();
    await mcp.tools();
    expect(server.methods.filter((m) => m === "tools/list")).toHaveLength(1);
    server.addTool("whoami");
    // The notification comes on the session's standalone SSE stream.
    const deadline = Date.now() + 2000;
    let names: string[] = [];
    while (!names.includes("whoami") && Date.now() < deadline) {
      await Bun.sleep(10);
      names = (await mcp.tools()).map((t) => t.name);
    }
    expect(names).toContain("whoami");
    expect(server.methods.filter((m) => m === "tools/list")).toHaveLength(2);
    await mcp.close();
  });

  test("reads an app's HTML and CSP once", async () => {
    const server = fakeUcMcp();
    const mcp = await connectMcp("local", { fetch: server.fetch });
    const app = await mcp.readApp(GRANTS_APP);
    expect(app).toEqual({
      uri: GRANTS_APP,
      html: GRANTS_HTML,
      meta: { csp: GRANTS_CSP, permissions: undefined, prefersBorder: false },
    });
    expect(await mcp.readApp(GRANTS_APP)).toBe(app);
    expect(server.methods.filter((m) => m === "resources/read")).toHaveLength(1);
    await mcp.close();
  });

  test("refuses a resource that isn't an MCP App, and doesn't cache the failure", async () => {
    const server = fakeUcMcp();
    server.mimeType = "text/html";
    const mcp = await connectMcp("local", { fetch: server.fetch });
    await expect(mcp.readApp(GRANTS_APP)).rejects.toThrow("not an MCP App");
    server.mimeType = MCP_APP_MIME_TYPE;
    expect((await mcp.readApp(GRANTS_APP)).html).toBe(GRANTS_HTML);
    await mcp.close();
  });

  test("re-runs a tool for its full result", async () => {
    const server = fakeUcMcp();
    const mcp = await connectMcp("local", { fetch: server.fetch });
    const result = await mcp.callTool("open_grant_form", {
      securable_type: "table",
      full_name: "main.default.t",
    });
    expect(result.structuredContent).toEqual({ securable: "main.default.t" });
    expect(result._meta).toEqual({ ui: { resourceUri: GRANTS_APP } });
    await mcp.close();
  });

  test("lets an app call app-visible tools only", async () => {
    const server = fakeUcMcp();
    const mcp = await connectMcp("local", { fetch: server.fetch });
    const args = { full_name: "main.default.t", changes: [] };
    expect((await mcp.callTool("apply_grants", args, { from: "app" })).isError).toBeFalsy();
    await expect(mcp.callTool("get_grants", {}, { from: "app" })).rejects.toBeInstanceOf(
      McpToolNotCallableError,
    );
    await expect(mcp.callTool("drop_table", {}, { from: "app" })).rejects.toBeInstanceOf(
      McpToolNotCallableError,
    );
    expect(server.calls.map((c) => c.name)).toEqual(["apply_grants"]);
    await mcp.close();
  });

  test("starts a new session when the server lost it, and retries once", async () => {
    const server = fakeUcMcp();
    const mcp = await connectMcp("local", { fetch: server.fetch });
    await mcp.tools();
    server.restart();
    const result = await mcp.callTool("get_grants");
    expect(result.content).toEqual([{ type: "text", text: "[]" }]);
    expect(server.methods.filter((m) => m === "initialize")).toHaveLength(2);
    // The new session lists its own tools.
    await mcp.tools();
    expect(server.methods.filter((m) => m === "tools/list")).toHaveLength(2);
    await mcp.close();
  });

  test("keeps one connection per environment and server", async () => {
    const server = fakeUcMcp();
    const a = await mcpConnection("a", UC_APPS, { fetch: server.fetch });
    expect(a.server).toBe("uc");
    expect(await mcpConnection("a")).toBe(a);
    const other = { name: "workgraph", module: "factory", url: "http://factory.invalid/mcp" };
    const w = await mcpConnection("a", other, { fetch: server.fetch });
    expect(w).not.toBe(a);
    expect(w.server).toBe("workgraph");
    const b = await mcpConnection("b", UC_APPS, { fetch: server.fetch });
    expect(b).not.toBe(a);
    await closeMcpConnections("b");
    expect(await mcpConnection("b")).toBe(b);
    expect(await mcpConnection("a", UC_APPS, { fetch: server.fetch })).not.toBe(a);
    await expect(a.tools()).rejects.toThrow("closed");
    await expect(w.tools()).rejects.toThrow("closed");
  });
});
