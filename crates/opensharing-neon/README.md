# opensharing-neon

The OpenSharing server ([`opensharing-server`](https://github.com/open-lakehouse/distributary/blob/main/crates/server)) as a
Node-API addon, deployed as a [Neon Function](https://neon.com/docs). This
follows the headwaters design ([`headwaters-neon`](../headwaters-neon), [ADR 0015](https://github.com/open-lakehouse/headwaters/blob/main/docs/adr/0015-neon-functions-deployment-via-napi.md)):

- The Rust router is compiled into `native/sharing.node`.
- A dependency-free TypeScript fetch handler (`function/index.ts`) forwards
  every request into it in-process.
- There is no socket and no container.
- Sharing is pure request/response, so nothing runs between requests.

## Configuration

Everything comes from the function's environment:

| Variable | |
|---|---|
| `DATABASE_URL` / `DATABASE_URL_UNPOOLED` | Set by Neon for the branch. The pooled URL serves requests; the direct one is used for the startup schema check. |
| `SHARING_SERVER_CONFIG_YAML` | The server configuration as a YAML document (sources, `public_url`, `encryption`, …; see the server README). |
| `SHARING_PROVIDER_TOKEN` | Adds a provider (admin) token. |
| KEKs referenced as `{ env: … }` in the config | For example `SHARING_KEK_V1`. |

## Build, test, deploy

```sh
just test opensharing        # host-native addon, node --test smoke suite
just build opensharing       # linux-arm64 (glibc 2.28) bundle + zip, size-gated
just test-arm64 opensharing  # the shipped bundle in node:24-bookworm-slim
just migrate opensharing <branch>   # migrate a Neon branch by hand
```

It isn't deployed yet. Deploying it means adding it to the root `neon.ts`, after
its configuration and secrets are set on each Neon branch (see
[docs/deployment.md](../../docs/deployment.md), "Secrets").

`build` needs `zig` and cargo-zigbuild ≥ 0.23. It uses the workspace
`release-neon-small` profile: fat LTO, one codegen unit, stripped, `opt-level = "z"`.
That profile is what keeps the bundle under Neon's 10 MiB zipped limit; see
the size notes in distributary's [`ARCHITECTURE.md`](https://github.com/open-lakehouse/distributary/blob/main/ARCHITECTURE.md#hosts).
