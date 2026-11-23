# Changelog

Releases of the deployed environments: the Neon Functions, their migrations,
`neon.ts` and the web app's pages. Each release is the commit deployed to
production (docs/deployment.md). The desktop app (`apps/desktop`) is released
on its own.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.0.6](https://github.com/open-lakehouse/luminescent-lakehouse/compare/v0.0.5...v0.0.6) - 2026-11-23

### Added

- *(desktop)* agents reach the work graph through egress routes ([#230](https://github.com/open-lakehouse/luminescent-lakehouse/pull/230))
- *(workgraph-mcp)* named roles, group principals and a role catalog ([#228](https://github.com/open-lakehouse/luminescent-lakehouse/pull/228))
- *(omnigent-client)* select MCP registry services at session create ([#226](https://github.com/open-lakehouse/luminescent-lakehouse/pull/226))
- *(workgraph-mcp)* authenticate callers as principals, map them to grants ([#224](https://github.com/open-lakehouse/luminescent-lakehouse/pull/224))
- *(desktop)* Start from the tree, the tab and Commit; an environment card ([#221](https://github.com/open-lakehouse/luminescent-lakehouse/pull/221))
- *(workgraph-neon)* workgraph-mcp as a Neon Function ([#222](https://github.com/open-lakehouse/luminescent-lakehouse/pull/222))
- *(desktop)* a Configure tab for the Neon branch, models and variables ([#217](https://github.com/open-lakehouse/luminescent-lakehouse/pull/217))
- *(desktop)* adopt shared-state stores; minted keys stay on this machine ([#206](https://github.com/open-lakehouse/luminescent-lakehouse/pull/206))
- *(workgraph-mcp)* plan review as an MCP App ([#191](https://github.com/open-lakehouse/luminescent-lakehouse/pull/191))
- *(desktop)* factory control room ([#188](https://github.com/open-lakehouse/luminescent-lakehouse/pull/188))
- *(workgraph-mcp)* the agents' MCP server ([#182](https://github.com/open-lakehouse/luminescent-lakehouse/pull/182))
- *(factory-server)* the reconciler, leases and the run-driving RPCs ([#186](https://github.com/open-lakehouse/luminescent-lakehouse/pull/186))
- *(factory-core)* the CodeHost and its GitHub adapter ([#185](https://github.com/open-lakehouse/luminescent-lakehouse/pull/185))
- *(factory-core)* the OmnigentDriver, a fake, and the omnigent-client adapter ([#183](https://github.com/open-lakehouse/luminescent-lakehouse/pull/183))
- *(desktop)* [**breaking**] trestle 0.0.22 and minted declared secrets ([#177](https://github.com/open-lakehouse/luminescent-lakehouse/pull/177))
- *(factory)* the agentic factory framework ([#176](https://github.com/open-lakehouse/luminescent-lakehouse/pull/176))
- *(omnigent-client)* session fields the factory needs ([#173](https://github.com/open-lakehouse/luminescent-lakehouse/pull/173))
- *(desktop)* the Agent app ([#163](https://github.com/open-lakehouse/luminescent-lakehouse/pull/163))
- *(agents)* lakehouse agent bundle ([#162](https://github.com/open-lakehouse/luminescent-lakehouse/pull/162))
- *(desktop)* MCP App sandbox and chatcn's McpAppFrame ([#160](https://github.com/open-lakehouse/luminescent-lakehouse/pull/160))
- *(desktop)* MCP client over the service proxy ([#159](https://github.com/open-lakehouse/luminescent-lakehouse/pull/159))
- *(desktop)* omnigent-client crate ([#156](https://github.com/open-lakehouse/luminescent-lakehouse/pull/156))
- *(desktop)* run SQL against a running instance on the host ([#146](https://github.com/open-lakehouse/luminescent-lakehouse/pull/146))
- trusted compute waives EXTERNAL USE on vends ([#144](https://github.com/open-lakehouse/luminescent-lakehouse/pull/144))

### Build

- *(desktop)* move the hydrofoil pin to main and gate the query engine ([#154](https://github.com/open-lakehouse/luminescent-lakehouse/pull/154))

### Changed

- *(authz-admin)* the authz operations as a crate the desktop can link ([#229](https://github.com/open-lakehouse/luminescent-lakehouse/pull/229))
- *(authz-neon)* host breakwater's olai-pdp ([#205](https://github.com/open-lakehouse/luminescent-lakehouse/pull/205))
- *(omnigent-client)* move to the root workspace, pin Omnigent 0.22 ([#172](https://github.com/open-lakehouse/luminescent-lakehouse/pull/172))

### Fixed

- run `just admin` against the lakehouse's Neon project ([#142](https://github.com/open-lakehouse/luminescent-lakehouse/pull/142))

## [0.0.5](https://github.com/open-lakehouse/luminescent-lakehouse/compare/v0.0.4...v0.0.5) - 2026-11-15

### Added

- *(authz-neon)* [**breaking**] Unity Catalog policies over operations and grants ([#141](https://github.com/open-lakehouse/luminescent-lakehouse/pull/141))
- *(unitycatalog-neon)* trusted Neon credential vending per branch ([#137](https://github.com/open-lakehouse/luminescent-lakehouse/pull/137))

## [0.0.4](https://github.com/open-lakehouse/luminescent-lakehouse/compare/v0.0.3...v0.0.4) - 2026-11-13

### Added

- Cedar policies and grants for Unity Catalog ([#133](https://github.com/open-lakehouse/luminescent-lakehouse/pull/133))
- *(unitycatalog-neon)* authenticate Unity Catalog callers ([#131](https://github.com/open-lakehouse/luminescent-lakehouse/pull/131))
- *(authz-neon)* add the Cedar policy decision point function ([#132](https://github.com/open-lakehouse/luminescent-lakehouse/pull/132))

### Changed

- *(web)* drop the retired /b/<endpoint-id>/ pages ([#118](https://github.com/open-lakehouse/luminescent-lakehouse/pull/118))

### Fixed

- *(unitycatalog-neon)* pin olai-uc-server 0.0.15 for the Delta API fixes ([#123](https://github.com/open-lakehouse/luminescent-lakehouse/pull/123))

## [0.0.3](https://github.com/open-lakehouse/luminescent-lakehouse/compare/v0.0.2...v0.0.3) - 2026-11-11

### Added

- per-lakehouse tenant config, OAuth servers on the /a/ pages ([#115](https://github.com/open-lakehouse/luminescent-lakehouse/pull/115))
- *(web)* serve any Neon branch's auth pages from /a/<auth-host>/<db>/ ([#114](https://github.com/open-lakehouse/luminescent-lakehouse/pull/114))
- *(desktop)* Logs in the bottom panel, Neon target and gateway in the header ([#104](https://github.com/open-lakehouse/luminescent-lakehouse/pull/104))
- *(desktop)* an Apps view for running instances, apart from the editor ([#99](https://github.com/open-lakehouse/luminescent-lakehouse/pull/99))

### Changed

- *(desktop)* consume the service UIs as release tarballs ([#103](https://github.com/open-lakehouse/luminescent-lakehouse/pull/103))

### Fixed

- *(desktop)* adopt trestle's latest releases, for MLflow's embed ([#101](https://github.com/open-lakehouse/luminescent-lakehouse/pull/101))

## [0.0.2](https://github.com/open-lakehouse/luminescent-lakehouse/compare/v0.0.1...v0.0.2) - 2026-11-09

### Added

- *(desktop)* find Unity Catalog by the API it serves ([#84](https://github.com/open-lakehouse/luminescent-lakehouse/pull/84))
- *(desktop)* settings behind a gear, over the workspace ([#79](https://github.com/open-lakehouse/luminescent-lakehouse/pull/79))
- *(desktop)* one frame with environment-scoped views ([#75](https://github.com/open-lakehouse/luminescent-lakehouse/pull/75))

## [0.0.1] - 2026-11-07

### Added

- *(unitycatalog-neon)* deploy Unity Catalog as a Neon Function (#42)
- *(web)* OAuth login and consent pages (#38)
- *(web)* scaffold the web app and document its Vercel deployment (#37)
- *(migrate)* authz schema and admin CLI for accounts, keys and grants (#24)
- host the headwaters and OpenSharing Neon Functions (#1)
