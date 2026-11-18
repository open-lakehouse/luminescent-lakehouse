# lakehouse: the desktop's Agent app agent

An [Omnigent](https://github.com/omnigent-ai/omnigent) agent for the desktop's
Agent app (`apps/desktop`, [docs/desktop.md](../../docs/desktop.md#agent)). It
answers questions about Unity Catalog privileges and proposes changes to them as
mangrove's grant form, an MCP App the desktop renders inline.

- **Tools:** the `uc` MCP server, mangrove's `uc mcp serve` as trestle's `uc-mcp`
  module (`http://uc-mcp:8090/uc-mcp/mcp` on the stack's network), limited to
  `get_grants`, `open_grant_form` and `search_principals`.
- **No grants of its own.** The server lists `apply_grants` only to clients
  that declare the MCP Apps extension, which the desktop's own connection does
  and the agent's doesn't. The form calls `apply_grants` through the desktop,
  and the desktop posts the outcome back into the session as an
  `<mcp-app-context>` message ([prompt.md](prompt.md)).
- **Model:** none pinned. The environment's Omnigent host points the claude-sdk
  harness at MLflow's AI Gateway, or, without MLflow, at the upstream the
  environment's Models choice names (a Databricks profile, provider keys;
  [docs/desktop.md](../../docs/desktop.md#running-it)).

The desktop looks for an agent named `lakehouse` (`AGENT_NAME` in
`apps/desktop/crates/shell-core/src/agent.rs`), so keep the name.

## Install

In a running environment with the `omnigent` and `uc-mcp` modules (trestle's
`agents-rs` scenario has both):

```bash
just agent-install                                  # http://localhost:9080/omnigent
just agent-install http://localhost:<port>/omnigent # another gateway port
```

It runs `omnigent agent add agents/lakehouse --server <url>`. A reinstall
updates the installed agent in place; running sessions pick it up from their
next turn.
