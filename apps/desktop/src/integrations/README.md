# Service integrations

The service UIs the shell mounts as apps of a running instance (Catalog,
Lineage; the Apps view, `src/shell/apps.tsx`), each bound through `@open-lakehouse/host`
(`hostFetch`, `useService`) and never importing `@tauri-apps/api` (Biome
enforces it outside `src/host/`):

- `unity-catalog/`: mangrove's Unity Catalog browser, on its own memory-history
  router (the explorer keeps its selection in the URL)
- `lineage/`: headwaters' lineage browsers, search and graph
- `agent/`: `AgentView`, an agent's session on an instance's Omnigent, with its
  MCP Apps (`correlate.ts`), and the Agent app's settings of it (`lakehouse.tsx`)
- `factory/`: the Factory app, the factory's control room on the `factory`
  binding, which opens a run's sessions in `AgentView` (not an instance's: the
  Apps view lists it above them)
- `mcp/`: the desktop's own MCP connections to an environment's MCP servers
  with Apps (`McpAppServer`; `UC_APPS` is `uc mcp serve`, `uc-mcp`;
  `WORKGRAPH_APPS` the factory's `workgraph-mcp`, at the `factory` binding's
  `/mcp`), next to the
  agent's: Streamable HTTP over `hostFetch(<module>)`, declaring the MCP Apps
  extension (`io.modelcontextprotocol/ui`), so the server lists app-only tools
  to it. `connection.ts` keeps one per environment and server
  (`mcpConnection`), caches the tool list and app resources, refuses an app's
  calls to tools not visible to apps, and starts a new session when the server
  lost it (404). The SDK and ext-apps load on the first connect.
- `bind.ts`: points the clients at the host's bindings; `services.ts` holds the
  service keys and API roots; `ServiceGate.tsx` renders a view only when its
  service is bound

A service is keyed by the API it serves when other implementations serve it too
(`unity-catalog`: the OSS server, `unity-catalog-oss`, or mangrove's,
`unity-catalog-rs`), else by its trestle module id (`headwaters`). So a view
doesn't care which implementation an environment runs, nor whether the service
runs in a container or as a Neon Function (shell-core's `ServiceKeys`).

The packages are pinned release tarballs (docs/desktop.md, "The service UIs'
packages"). MLflow, Jaeger and other UIs with
their own session show in a child webview or a window of their own instead
(`src/shell/EmbeddedServiceUi.tsx`).
