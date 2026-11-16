// Which of the agent's tool calls the Agent app renders as an MCP App: a call to the
// `uc` server's tool that, on the desktop's own connection to that server, has an app
// (`_meta.ui.resourceUri`) and is safe to run again for its full result
// (`readOnlyHint`). Omnigent flattens results to text, so the app needs the re-run.
// Every other call is a plain tool card.

import type { Tool } from "@modelcontextprotocol/client";
import type { McpConnection } from "../mcp/connection";

/** The agent YAML's name for mangrove's `uc mcp serve` (agents/lakehouse/config.yaml). */
export const UC_SERVER = "uc";

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

/** A call the app renders: the MCP tool, and the app's `ui://` resource. */
export interface AppCall {
  tool: Tool;
  resourceUri: string;
}

type Inspect = Pick<McpConnection, "uiResourceUri" | "isReadOnly">;

/** Whether the call to `toolName` renders as an app, given the `uc` server's tools. */
export function matchApp(
  toolName: string,
  tools: readonly Tool[],
  inspect: Inspect,
): AppCall | undefined {
  const name = parseToolName(toolName);
  if (name.server !== UC_SERVER) return undefined;
  const tool = tools.find((t) => t.name === name.tool);
  if (!tool || !inspect.isReadOnly(tool)) return undefined;
  const resourceUri = inspect.uiResourceUri(tool);
  return resourceUri ? { tool, resourceUri } : undefined;
}
