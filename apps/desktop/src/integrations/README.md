# Service integrations

Per-service mounts (Unity Catalog, lineage, MLflow/Jaeger frames) arrive with
open-lakehouse/luminescent-lakehouse#23. Each one binds through
`@open-lakehouse/host` (`useService(moduleId)`, `hostFetch`, `hostTransport`)
and never imports `@tauri-apps/api` (Biome enforces it outside `src/host/`).
