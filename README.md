# luminescent-lakehouse

Deploys open-lakehouse services to [Neon](https://neon.com) as
[Neon Functions](https://neon.com/docs/compute/functions/overview). The services
themselves live in their own repositories. This repository holds what it takes
to run them on Neon, so their configuration and clients can be kept in step:

| Crate | Function | Service |
|---|---|---|
| [`crates/headwaters-neon`](crates/headwaters-neon) | `headwaters` | [headwaters](https://github.com/open-lakehouse/headwaters): OpenLineage ingest + Marquez-compatible reads |
| [`crates/opensharing-neon`](crates/opensharing-neon) | `opensharing` | [distributary](https://github.com/open-lakehouse/distributary): the OpenSharing / Delta Sharing server |
| [`crates/unitycatalog-neon`](crates/unitycatalog-neon) | `unitycatalog` | [mangrove](https://github.com/open-lakehouse/mangrove): the Unity Catalog server |
| [`crates/migrate`](crates/migrate) | | `luminescent-migrate`, applies a service's migrations to a Neon branch |

Each function is a Node-API addon that wraps the service's in-process axum
router, plus a dependency-free TypeScript fetch handler. The design is in
headwaters
[ADR 0015](https://github.com/open-lakehouse/headwaters/blob/main/docs/adr/0015-neon-functions-deployment-via-napi.md).

## Usage

Prerequisites: Rust (see `rust-toolchain.toml`), Node 22.13 or newer, `just`,
`zig` + `cargo-zigbuild` 0.23 or newer for the deployable build, Docker for the
Postgres-backed tests and the arm64 test, and Node 24 + Bun for deploying (`bun install`).

```bash
just test <fn>         # host-native addon behind the fetch handler, node --test
just build <fn>        # linux-arm64 bundle + zip, gated on Neon's size limits
just test-arm64 <fn>   # the deployable bundle in node:24-bookworm-slim
just deploy <branch>   # migrate a Neon branch and apply neon.ts to it
```

`<fn>` is `headwaters`, `opensharing` or `unitycatalog`. Each crate's README covers its runtime
configuration.

## Deployment

[`neon.ts`](neon.ts) declares what runs on Neon. GitHub Actions deploys it to
three kinds of Neon branch: a `preview/pr-<n>` branch per pull request, `staging`
on every merge to `main`, and production (the project's default branch) by an
approved manual run. See [docs/deployment.md](docs/deployment.md).

`headwaters` and `unitycatalog` are deployed; `opensharing` builds and tests
but isn't in `neon.ts` yet.

## License

Apache-2.0
