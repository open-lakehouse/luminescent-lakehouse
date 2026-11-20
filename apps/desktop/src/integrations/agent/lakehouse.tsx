// The Agent app's agent: `lakehouse` (agents/lakehouse), whose `uc` MCP server
// (mangrove's `uc mcp serve`) has the grant form as an MCP App. The factory's
// `workgraph` is there too, so a session that plans (submit, then plan review)
// renders its review the same way.

import { UC_APPS, WORKGRAPH_APPS } from "../mcp/connection";
import { AgentView, type AgentViewProps } from "./AgentView";

/** The agent's name, as `agents/lakehouse/config.yaml` has it. */
export const LAKEHOUSE_AGENT = "lakehouse";

/** What the agent's YAML must be installed with. */
const INSTALL = "just agent-install";

/** The Agent app's settings of `AgentView`. */
export const LAKEHOUSE: Pick<AgentViewProps, "agent" | "servers" | "notInstalled" | "placeholder"> =
  {
    agent: LAKEHOUSE_AGENT,
    servers: [UC_APPS, WORKGRAPH_APPS],
    notInstalled: (
      <>
        Install it from the repository with <code>{INSTALL}</code> (it takes the Omnigent URL, e.g.{" "}
        <code>{INSTALL} http://localhost:9080/omnigent</code>).
      </>
    ),
    placeholder:
      "Ask about grants on the catalog, e.g. “grant SELECT on main.default.t to group x”.",
  };

/** The Agent app: a chat with the lakehouse agent on `instance`. */
export function LakehouseAgentView({ instance, label }: { instance: string; label: string }) {
  return <AgentView instance={instance} label={label} {...LAKEHOUSE} />;
}
