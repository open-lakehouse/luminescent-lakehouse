// Which of an agent's tool calls the Agent view renders as an MCP App: a call to a tool
// of one of the session's MCP servers with Apps (the `uc` server for the lakehouse
// agent; the factory's `workgraph` for a planner's plan review) that, on the desktop's
// own connection to that
// server, has an app (`_meta.ui.resourceUri`) and is safe to run again for its full
// result (`readOnlyHint`). Omnigent flattens results to text, so the app needs the
// re-run. Every other call is a plain tool card.

import type { Tool } from "@modelcontextprotocol/client";
import type { McpConnection } from "../mcp/connection";

export interface ToolName {
  /** The MCP server, as the agent's YAML names it; `undefined` for a built-in tool. */
  server?: string;
  tool: string;
}

/**
 * Omnigent's name for a tool, split: `<server>__<tool>` for an MCP server's tool, seen
 * by the Claude Code harness behind Omnigent's own MCP server as
 * `mcp__omnigent__<server>__<tool>`. The rules of omnigent-client's `ToolName::parse`
 * (crates/omnigent-client/src/fold.rs), with the same cases in the tests.
 */
export function parseToolName(name: string): ToolName {
  const bare = name.startsWith("mcp__omnigent__") ? name.slice("mcp__omnigent__".length) : name;
  const at = bare.indexOf("__");
  if (at > 0 && at + 2 < bare.length) {
    return { server: bare.slice(0, at), tool: bare.slice(at + 2) };
  }
  return { tool: bare };
}

type Inspect = Pick<McpConnection, "uiResourceUri" | "isReadOnly">;

/** A server the view is connected to: the connection, and its tools. */
export interface ConnectedServer<C extends Inspect = McpConnection> {
  conn: C;
  tools: readonly Tool[];
}

/** A call the view renders: the MCP tool, the app's `ui://` resource, and the
 *  connection to run it on. */
export interface AppCall<C extends Inspect = McpConnection> {
  tool: Tool;
  resourceUri: string;
  conn: C;
}

/** Whether the call to `toolName` renders as an app, given the connected servers, by the
 *  name the agent knows each by. A server that isn't connected has no apps. */
export function matchApp<C extends Inspect>(
  toolName: string,
  servers: ReadonlyMap<string, ConnectedServer<C>>,
): AppCall<C> | undefined {
  const name = parseToolName(toolName);
  const server = name.server === undefined ? undefined : servers.get(name.server);
  if (!server) return undefined;
  const tool = server.tools.find((t) => t.name === name.tool);
  if (!tool || !server.conn.isReadOnly(tool)) return undefined;
  const resourceUri = server.conn.uiResourceUri(tool);
  return resourceUri ? { tool, resourceUri, conn: server.conn } : undefined;
}
