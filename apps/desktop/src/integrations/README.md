# Service integrations

The service UIs the shell mounts as an environment's sub-tabs (Catalog,
Lineage; `src/shell/environments.tsx`), each bound through `@open-lakehouse/host`
(`hostFetch`, `useService`) and never importing `@tauri-apps/api` (Biome
enforces it outside `src/host/`):

- `unity-catalog/`: mangrove's Unity Catalog browser, on its own memory-history
  router (the explorer keeps its selection in the URL)
- `lineage/`: headwaters' lineage browsers, search and graph
- `bind.ts`: points the clients at the host's bindings; `services.ts` holds the
  module ids and API roots; `ServiceGate.tsx` renders a view only when its
  module is bound

The packages build from source at pinned commits for now (docs/desktop.md,
"Building the service UIs from pinned commits"). MLflow, Jaeger and other UIs with
their own session open in a top-level webview instead (#23).
