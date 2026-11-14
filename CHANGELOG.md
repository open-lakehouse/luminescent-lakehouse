# Changelog

Releases of the deployed environments: the Neon Functions, their migrations,
`neon.ts` and the web app's pages. Each release is the commit deployed to
production (docs/deployment.md). The desktop app (`apps/desktop`) is released
on its own.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
