# luminescent-lakehouse

Deploys open-lakehouse services to [Neon](https://neon.com) as
[Neon Functions](https://neon.com/docs/compute/functions/overview). The services
themselves live in their own repositories. This repository holds what it takes
to run them on Neon, so their configuration and clients can be kept in step:

| Crate | Function | Service |
|---|---|---|
| [`crates/headwaters-neon`](crates/headwaters-neon) | `headwaters` | [headwaters](https://github.com/open-lakehouse/headwaters): OpenLineage ingest + Marquez-compatible reads |
| [`crates/opensharing-neon`](crates/opensharing-neon) | `opensharing` | [distributary](https://github.com/open-lakehouse/distributary): the OpenSharing / Delta Sharing server |
| [`crates/migrate`](crates/migrate) | | `luminescent-migrate`, applies a service's migrations to a Neon branch |

Each function is a Node-API addon that wraps the service's in-process axum
router, plus a dependency-free TypeScript fetch handler. The design is in
headwaters
[ADR 0015](https://github.com/open-lakehouse/headwaters/blob/main/docs/adr/0015-neon-functions-deployment-via-napi.md).

## Usage

Prerequisites: Rust (see `rust-toolchain.toml`), Node 22.13 or newer, `just`,
`zig` + `cargo-zigbuild` 0.23 or newer for the deployable build, Docker for the
Postgres-backed tests and the arm64 test, and the `neon` CLI for deploying.

```bash
just test <fn>         # host-native addon behind the fetch handler, node --test
just build <fn>        # linux-arm64 bundle + zip, gated on Neon's size limits
just test-arm64 <fn>   # the deployable bundle in node:24-bookworm-slim
just migrate <fn>      # once per schema change, over DATABASE_URL_UNPOOLED
just deploy <fn>       # neon functions deploy
```

`<fn>` is `headwaters` or `opensharing`. Each crate's README covers its runtime
configuration.

## License

Apache-2.0
