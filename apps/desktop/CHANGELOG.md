# Changelog

Releases of the desktop app (`apps/desktop`), tagged `desktop-v<version>`
(docs/desktop-release.md). The deployed environments are released on their own,
in the root CHANGELOG.md.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.0.2](https://github.com/open-lakehouse/luminescent-lakehouse/compare/desktop-v0.0.1...desktop-v0.0.2) - 2026-11-23

### Added

- *(factory-server)* postgres store with migrations ([#232](https://github.com/open-lakehouse/luminescent-lakehouse/pull/232))
- *(desktop)* agents reach the work graph through egress routes ([#230](https://github.com/open-lakehouse/luminescent-lakehouse/pull/230))
- *(workgraph-mcp)* named roles, group principals and a role catalog ([#228](https://github.com/open-lakehouse/luminescent-lakehouse/pull/228))
- *(factory)* profiles name the MCP registry services their sessions call ([#227](https://github.com/open-lakehouse/luminescent-lakehouse/pull/227))
- *(omnigent-client)* select MCP registry services at session create ([#226](https://github.com/open-lakehouse/luminescent-lakehouse/pull/226))
- *(workgraph-mcp)* authenticate callers as principals, map them to grants ([#224](https://github.com/open-lakehouse/luminescent-lakehouse/pull/224))
- *(desktop)* Start from the tree, the tab and Commit; an environment card ([#221](https://github.com/open-lakehouse/luminescent-lakehouse/pull/221))
- *(workgraph-neon)* workgraph-mcp as a Neon Function ([#222](https://github.com/open-lakehouse/luminescent-lakehouse/pull/222))
- *(workgraph-mcp)* markdown results for plan reads and writes ([#220](https://github.com/open-lakehouse/luminescent-lakehouse/pull/220))
- *(desktop)* a Configure tab for the Neon branch, models and variables ([#217](https://github.com/open-lakehouse/luminescent-lakehouse/pull/217))
- *(desktop)* the effective-env view ([#212](https://github.com/open-lakehouse/luminescent-lakehouse/pull/212))
- *(desktop)* variable editors ([#211](https://github.com/open-lakehouse/luminescent-lakehouse/pull/211))
- *(desktop)* variable scopes and the effective env in shell-core ([#210](https://github.com/open-lakehouse/luminescent-lakehouse/pull/210))
- *(desktop)* adopt shared-state stores; minted keys stay on this machine ([#206](https://github.com/open-lakehouse/luminescent-lakehouse/pull/206))

### Changed

- *(desktop)* split the environments host into a model and its views ([#215](https://github.com/open-lakehouse/luminescent-lakehouse/pull/215))

### Fixed

- *(desktop)* keep long variable values inside their tables ([#214](https://github.com/open-lakehouse/luminescent-lakehouse/pull/214))

## [0.0.1](https://github.com/open-lakehouse/luminescent-lakehouse/releases/tag/desktop-v0.0.1) - 2026-11-20

The first published release of the desktop app.
