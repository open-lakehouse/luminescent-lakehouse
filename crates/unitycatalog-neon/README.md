# unitycatalog-neon

Packages mangrove's Unity Catalog server (`olai-uc-server`) as a
[Neon Function](https://neon.com/docs/compute/functions/overview), slug
`unitycatalog`. It has two parts:

- a Node-API addon, `src/lib.rs`, that wraps the in-process axum router from
  `unitycatalog_server::embed`
- a short fetch handler, `function/index.ts`

It follows headwaters
[ADR 0015](https://github.com/open-lakehouse/headwaters/blob/main/docs/adr/0015-neon-functions-deployment-via-napi.md).
The catalog is pure request/response. It has no background work, so an isolate
can be evicted at any time.

## Build and test

```bash
just build unitycatalog        # linux-arm64 bundle → crates/unitycatalog-neon/unitycatalog-neon.zip (size-gated)
just test unitycatalog         # host-native addon, end-to-end against a local Postgres
just test-arm64 unitycatalog   # the deployable bundle, in node:24-bookworm-slim
```

Built with `default-features = false, features = ["embed", "postgres"]`: no UI,
Swagger, storage proxy or Files API. At `release-neon` (opt-level 3) the zip is
7.1 MiB of Neon's 10 MiB; `release-neon-small` (opt-level "z") would make it 5.1
MiB, measured 2026-11-03.

## Its own database

UC keeps its migration ledger in the unqualified `_sqlx_migrations`, as
headwaters does, and each refuses to start on migrations it doesn't know. So
UC doesn't share Neon's `neondb`. It lives in a `unitycatalog` database on the
same branch:

- the addon rewrites the database of `DATABASE_URL` (requests, pooled) and
  `DATABASE_URL_UNPOOLED` (the startup schema check) to `unitycatalog`, or to
  `UNITYCATALOG_NEON_DATABASE` when that is set;
- `scripts/migrate.sh` creates the database when it's missing, and
  `luminescent-migrate unitycatalog` makes the same rewrite before migrating.

Child branches inherit the database with the rest of the branch.

## Runtime configuration

`Config::from_env`: the YAML in `UC_SERVER_CONFIG_YAML` (set in
[`neon.ts`](../../neon.ts)), then Neon's `DATABASE_URL` /
`DATABASE_URL_UNPOOLED` as the Postgres backend. Logs follow `RUST_LOG`
(default `unitycatalog=info`).

- **Authentication: anonymous.** Function URLs are public, so anyone who has
  the URL can read and change the catalog. This matches headwaters until the
  services verify Neon Auth tokens ([docs/auth.md](../../docs/auth.md)).
  mangrove's `auth.mode: bearer` (static tokens) is the stopgap if it has to
  be closed sooner.
- **Storage.** No `managed_storage_root` is set, so a managed catalog needs a
  `storage_root` inside a registered external location (a credential plus an
  external location, created through the API).

### Secrets

A durable backend needs a key-encryption key to seal the credentials it
stores. The config reads it from `UC_SERVER_KEK`, 32 random bytes in base64,
which is set once per branch on Neon and never in `neon.ts` or GitHub:

```bash
bunx neon functions deploy unitycatalog --branch <branch> \
  --src crates/unitycatalog-neon/function/dist --no-bundle \
  --env UC_SERVER_KEK="$(openssl rand -base64 32)"
```

Without it the start fails, and the function answers 503 (logged). Don't
replace the key on a branch that holds data: secrets sealed with the old key
can't be opened without it. Rotating the key means listing the old one under
`encryption.retired`.
