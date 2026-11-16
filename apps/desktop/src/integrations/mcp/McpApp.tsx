// chatcn's McpAppFrame on the desktop's MCP Apps sandbox proxy: the shell host
// says where the proxy is (`mcp-app://localhost/sandbox.html` in the app, a second
// origin of the Playwright build's server on the fakes). chatcn's tooltips need
// their own provider (its radix copy, not ui-kit's); a chat view that has one above
// this costs nothing extra.

import type { McpAppFrameProps } from "@/components/ui/chatcn/mcp-app-frame";
import { McpAppFrame } from "@/components/ui/chatcn/mcp-app-frame";
import { TooltipProvider } from "@/components/ui/chatcn/tooltip";
import { useShellHost } from "../../shell/shell-host";

export type McpAppProps = Omit<McpAppFrameProps, "sandboxUrl">;

export function McpApp(props: McpAppProps) {
  const shell = useShellHost();
  return (
    <TooltipProvider>
      <McpAppFrame sandboxUrl={shell.mcpAppSandboxUrl()} {...props} />
    </TooltipProvider>
  );
}
