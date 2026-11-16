import { describe, expect, test } from "bun:test";
import type { Tool } from "@modelcontextprotocol/client";
import { matchApp, parseToolName } from "./correlate";

describe("parseToolName", () => {
  // The cases of omnigent-client's `ToolName::parse` test (tests/fold.rs).
  test.each([
    ["uc__open_grant_form", { server: "uc", tool: "open_grant_form" }],
    ["mcp__omnigent__uc__open_grant_form", { server: "uc", tool: "open_grant_form" }],
    ["uc-mcp__get_grants", { server: "uc-mcp", tool: "get_grants" }],
    ["sys_os_read", { tool: "sys_os_read" }],
    ["__odd", { tool: "__odd" }],
    ["uc__", { tool: "uc__" }],
    ["mcp__omnigent__sys_session_list", { tool: "sys_session_list" }],
    ["uc__a__b", { server: "uc", tool: "a__b" }],
  ])("%s", (name, expected) => {
    expect(parseToolName(name)).toEqual(expected);
  });
});

describe("matchApp", () => {
  const tool = (name: string, readOnly: boolean, resourceUri?: string): Tool =>
    ({
      name,
      inputSchema: { type: "object" },
      annotations: { readOnlyHint: readOnly },
      _meta: resourceUri ? { ui: { resourceUri } } : undefined,
    }) as Tool;
  const tools = [
    tool("open_grant_form", true, "ui://uc/grants"),
    tool("get_grants", true),
    tool("apply_grants", false, "ui://uc/grants"),
  ];
  const inspect = {
    isReadOnly: (t: Tool) => t.annotations?.readOnlyHint === true,
    uiResourceUri: (t: Tool) => (t._meta?.ui as { resourceUri?: string } | undefined)?.resourceUri,
  };

  test.each([
    ["uc__open_grant_form", "ui://uc/grants"],
    ["mcp__omnigent__uc__open_grant_form", "ui://uc/grants"],
    // No app.
    ["uc__get_grants", undefined],
    // An app, but not safe to run again.
    ["uc__apply_grants", undefined],
    // Another server's tool of the same name.
    ["github__open_grant_form", undefined],
    // A built-in tool.
    ["open_grant_form", undefined],
    // Not on the server.
    ["uc__drop_table", undefined],
  ])("%s", (name, uri) => {
    expect(matchApp(name, tools, inspect)?.resourceUri).toBe(uri);
  });
});
