// The Streamable HTTP side of the Playwright build's stand-in MCP servers (fake-uc-mcp,
// fake-workgraph-mcp): just enough for the desktop's MCP connection. JSON responses,
// no SSE stream, one fixed session.

export interface JsonRpc {
  jsonrpc: "2.0";
  id?: number | string;
  method?: string;
  params?: Record<string, unknown>;
}

export interface FakeResponse {
  status: number;
  headers: [string, string][];
  body?: string;
}

export interface FakeMcpRequest {
  method: string;
  path: string;
  body?: number[] | null;
}

/** What a server answers a request with: the result, or `undefined` for an unknown
 *  method. */
export type Answer = (message: JsonRpc) => unknown;

/** Answer one request the connection sends through the proxy, if it is for `mcpPath`. */
export function fakeMcpResponse(
  mcpPath: string,
  request: FakeMcpRequest,
  answer: Answer,
): FakeResponse {
  if (new URL(request.path, "http://fake-mcp.invalid").pathname !== mcpPath) {
    return { status: 404, headers: [], body: "not found" };
  }
  // No standalone SSE stream: the connection does without one.
  if (request.method === "GET") return { status: 405, headers: [] };
  if (request.method === "DELETE") return { status: 200, headers: [] };
  const message = JSON.parse(
    new TextDecoder().decode(new Uint8Array(request.body ?? [])),
  ) as JsonRpc;
  const headers: [string, string][] = [["mcp-session-id", "fake-session"]];
  // A notification (no id) is accepted with no body.
  if (message.id === undefined) return { status: 202, headers };
  const result = answer(message);
  const reply =
    result === undefined
      ? { jsonrpc: "2.0", id: message.id, error: { code: -32601, message: "method not found" } }
      : { jsonrpc: "2.0", id: message.id, result };
  return {
    status: 200,
    headers: [...headers, ["content-type", "application/json"]],
    body: JSON.stringify(reply),
  };
}

/** `initialize`'s answer for a server named `name`. */
export function initialized(name: string, message: JsonRpc) {
  return {
    protocolVersion: message.params?.protocolVersion ?? "2025-06-18",
    capabilities: { tools: {}, resources: {} },
    serverInfo: { name, version: "0.0.0" },
  };
}
