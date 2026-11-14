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

Built with `default-features = false, features = ["embed", "postgres",
"olai-auth", "neon-trusted-vending"]`: no UI, Swagger, storage proxy or Files
API. At `release-neon` (opt-level 3) the zip is 7.55 MiB of Neon's 10 MiB,
measured 2026-11-14 (`olai-auth` adds 0.33 MiB, `neon-trusted-vending` about
50 KB); `release-neon-small` (opt-level "z") made it 5.1 MiB without either,
measured 2026-11-03.

The tests authenticate as on a branch: `smoke.test.mjs` serves the JWKS of a
fresh key itself and signs the tokens, and `just test` / `just test-arm64`
apply `luminescent-migrate authz` and `function/fixtures/identity.sql` (Neon
Auth users and an API key) to the test database first. `vending.test.mjs`
runs the catalog with vending on, in a process of its own, against a fake Neon
control plane it serves; `smoke.test.mjs` checks that with it off, a vend is a
400 `FAILED_PRECONDITION`.

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

- **Authentication: `olai-auth`.** Every request but `/health`, `/version` and
  `/.well-known/oauth-protected-resource` needs `Authorization: Bearer` with
  one of:
  - an OAuth access token of the branch's Neon Auth (`iss` =
    `NEON_AUTH_BASE_URL`) for the `unitycatalog` resource (`TENANT.resources`
    in `neon.ts`);
  - a Neon Auth session token of the web app;
  - a service account's `olk_` API key from `authz.api_keys`.

  Admin and ban status come from `neon_auth."user"` on every request. Those
  tables and `authz` are in `neondb`, not in the catalog's database, so the
  config reads them over `DATABASE_URL` itself: the addon rewrites only the
  backend's copy of it. See [docs/auth.md](../../docs/auth.md).
- **Authorization: the `authz` function.** `policy.kind: authorizer` asks the
  branch's Cedar policy decision point
  ([crates/authz-neon](../authz-neon/README.md)) about every request, and
  denies whatever it can't decide (an error, a timeout, a missing secret).
  The policies are `crates/authz-neon/function/policies/unitycatalog.cedar`;
  [docs/auth.md](../../docs/auth.md#unity-catalog) has the model. Decisions
  are cached for 5 s. The function still connects as `neondb_owner`, not the
  least-privilege role
  ([#26](https://github.com/open-lakehouse/luminescent-lakehouse/issues/26)).
- **Where the authz function is.** Neon injects no other function's URL, and
  one is named after its branch and slug (`https://<branch id>-<slug>.compute.…`),
  so on the first request `function/index.ts` sets `AUTHZ_URL` to its own host
  with `unitycatalog` swapped for `authz`. Set `AUTHZ_URL` to point it
  elsewhere.
- **CA certificates.** The token verifier fetches the JWKS over rustls, which
  trusts the system's CA certificates only and refuses to start without any.
  Unless `SSL_CERT_FILE` or `SSL_CERT_DIR` is set, `function/index.ts` writes
  the roots Node ships with to a temporary file and points `SSL_CERT_FILE` at
  it before the addon starts.
- **Storage.** The managed storage root is `s3://lakehouse/managed`, the
  branch's own bucket (`buckets` in `neon.ts`), on the `neon` credential. Where
  vending is on, the deploy registers that credential and the `lakehouse`
  external location through the API ([docs/deployment.md](../../docs/deployment.md#storage)).
- **Credential vending.** On staging and previews (`vending` in `neon.ts`;
  off on production), `temporary-table-credentials`,
  `temporary-path-credentials` and the Delta API's `credentials` hand out
  expiring Neon storage keys for the branch, minted with `NEON_API_KEY`. A key
  reaches every bucket on the branch, so a caller needs `read` or `write` *and*
  an explicit `external_use` grant. Each vend is logged at target
  `unitycatalog::audit`. See [docs/auth.md](../../docs/auth.md#credential-vending-on-neon).
  The server refuses to start with vending on unless `olai-auth` and the
  `authorizer` policy are both configured.

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

Where vending is on, it needs `NEON_API_KEY`, a project-scoped Neon API key,
or it refuses to start (503); the key can change everything in the project
([docs/deployment.md](../../docs/deployment.md#the-vending-key)).

It also needs `AUTHZ_TOKEN_UNITYCATALOG`, its secret for the `authz` function:
the same value on both functions of the branch
([docs/deployment.md](../../docs/deployment.md#secrets)). Without it the start
fails (503); with a value `authz` doesn't hold, every request is denied (403).
