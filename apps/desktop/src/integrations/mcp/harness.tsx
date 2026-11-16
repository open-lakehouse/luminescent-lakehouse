// The Playwright build's MCP App page (`/?harness=mcp-app`, e2e mode only): mangrove's
// real grants app (e2e/fixtures/grants-app.html) in McpApp, through the sandbox proxy,
// with the `open_grant_form` result `uc mcp serve` emits and a fake for the tools the
// app calls. What the app sent is on `window.__LL_MCP_HARNESS__` for the test.

import type {
  McpCallToolParams,
  McpCallToolResult,
} from "@/components/ui/chatcn/hooks/use-chatcn-genui-mcp-app";
import "../../globals.css";
import { ThemeProvider } from "@open-lakehouse/ui-kit";
import { createRoot } from "react-dom/client";
import grantsApp from "../../../e2e/fixtures/grants-app.html?raw";
import grantForm from "../../../e2e/fixtures/open_grant_form.json";
import { type ShellHost, ShellHostProvider } from "../../shell/shell-host";
import { McpApp } from "./McpApp";

export interface McpHarnessLog {
  calls: McpCallToolParams[];
  modelContext: unknown[];
}

declare global {
  interface Window {
    __LL_MCP_HARNESS__?: McpHarnessLog;
  }
}

const INPUT = { securable_type: "table", full_name: "main.sales.orders", principal: "bob" };

const RESULT = {
  content: [
    { type: "text" as const, text: "Showed the grant form for main.sales.orders." },
    {
      type: "resource" as const,
      resource: {
        uri: "a2ui://uc/grant-form",
        mimeType: "application/a2ui+json",
        text: JSON.stringify(grantForm),
      },
    },
  ],
};

async function callTool(log: McpHarnessLog, params: McpCallToolParams): Promise<McpCallToolResult> {
  log.calls.push(params);
  switch (params.name) {
    case "whoami":
      return {
        content: [],
        structuredContent: { principal: { kind: "user", name: "me@example.com" } },
      };
    case "search_principals":
      return { content: [], structuredContent: { principals: [] } };
    case "apply_grants":
      return { content: [{ type: "text", text: "Granted." }], structuredContent: { ok: true } };
    default:
      return { content: [{ type: "text", text: `unknown tool ${params.name}` }], isError: true };
  }
}

export function mountMcpAppHarness(root: HTMLElement, shell: ShellHost): void {
  const log: McpHarnessLog = { calls: [], modelContext: [] };
  window.__LL_MCP_HARNESS__ = log;
  createRoot(root).render(
    <ThemeProvider>
      <ShellHostProvider host={shell}>
        <main className="p-4">
          <McpApp
            resource={{ uri: "ui://uc/grants", html: grantsApp }}
            toolName="open_grant_form"
            toolInput={INPUT}
            toolResult={RESULT}
            onCallTool={(params) => callTool(log, params)}
            onUpdateModelContext={(params) => {
              log.modelContext.push(params);
            }}
          />
        </main>
      </ShellHostProvider>
    </ThemeProvider>,
  );
}
