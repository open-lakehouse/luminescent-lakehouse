# factory-server

The `factory.v1` Connect API over `factory-core`. It comes in three forms:

- `router()`, to embed: the desktop serves it in-process.
- `cli::run`, for a composition's binary: it adds its extensions and seeds.
- The reference `factory-server` binary: the built-in extension, with the
  agents' MCP server (`workgraph-mcp`) at `/mcp`.

[docs/factory](../../docs/factory/README.md) covers the factory, and
[execution.md](../../docs/factory/execution.md) covers the reconciler and the
store.

```bash
just factory-server                       # in memory, with the demo plan, on :7410
cargo run -p factory-server -- --help     # every flag and its environment variable
```

## The store

`--db` (`FACTORY_DB`) takes one of these forms:

| Form | Store |
|---|---|
| `memory` (the default) | In memory. |
| `sqlite://factory.db?mode=rwc` | SQLite, migrated on connect. |
| `postgres://user@host:5432/factory?sslmode=require` | Postgres at that URL. |
| `postgres` | Postgres from libpq's variables: `PGHOST`, `PGPORT`, `PGDATABASE`, `PGUSER`, `PGPASSWORD`, `PGSSLMODE` (and `.pgpass`). |

On Postgres the server checks the schema at startup and refuses a missing,
partial or newer one. Apply it in one of two ways:

- `--migrate` (`FACTORY_MIGRATE=1`) migrates at startup, before the pool opens.
- `luminescent-migrate factory` migrates as a deploy step. It reads the same
  `FACTORY_DB`, falling back to `DATABASE_URL`.

Either way, the migrations run over a connection of their own. The migrator
holds a session advisory lock, so point it at a direct connection, not a
transaction pooler.

```bash
PGPASSWORD=… FACTORY_DB=postgres://postgres@localhost:5432/factory \
  cargo run -p factory-server -- --migrate --seed demo

PGHOST=localhost PGUSER=postgres PGDATABASE=factory FACTORY_DB=postgres \
  cargo run -p factory-server
```

The server logs the database as `user@host:port/database`, without the
password.

### Credentials

`--db-credentials` (`FACTORY_DB_CREDENTIALS`) says where a Postgres store's
password comes from:

| Value | Password |
|---|---|
| `static` (the default) | The URL's, `PGPASSWORD`, or `.pgpass`. |
| `lakebase` | A Databricks service principal's OAuth token, renewed before it expires. Needs a Postgres `--db`. |

On Databricks Apps, a `postgres` resource sets the `PG*` variables (`PGUSER`
is the app's service principal), and the platform injects `DATABRICKS_HOST`,
`DATABRICKS_CLIENT_ID` and `DATABRICKS_CLIENT_SECRET`. `lakebase` gets a
token from `$DATABRICKS_HOST/oidc/v1/token` (client credentials, scope
`all-apis`) and uses it as the password:

```bash
FACTORY_DB=postgres FACTORY_DB_CREDENTIALS=lakebase factory-server --migrate
```

A token only has to be valid when a connection opens. A task hands the pool a
new token five minutes before the current one expires, and retries a minute
after a failure; open connections keep their session. The migrations and the
pool share the token.

Locally, a person's token works with `static`. It lasts an hour and nobody
renews it:

```bash
PGPASSWORD="$(databricks postgres generate-database-credential \
  projects/<project>/branches/<branch>/endpoints/<endpoint> -o json | jq -r .token)" \
  FACTORY_DB=postgres factory-server
```

Both are `factory_core::pg::Credentials` (`StaticCredentials`, and
`LakebaseCredentials` with factory-core's `lakebase` feature). A composition
with another credential implements `Credentials` and opens the store with
`cli::open_backend`.

## Other flags

- `--registry` (`FACTORY_REGISTRY`): the profiles and repos plans execute
  with (`crates/factory-core/registry.example.yaml`).
- `--omnigent-url` (`FACTORY_OMNIGENT_URL`): runs the reconciler against
  that Omnigent. Without it, the run RPCs only record.
- `--github gh|token|off` (`FACTORY_GITHUB`): where PR status comes from.
- `--mcp-token`, `--mcp-app-token` (`FACTORY_MCP_TOKENS`,
  `FACTORY_MCP_APP_TOKENS`): development tokens for `/mcp`
  ([workgraph-mcp.md](../../docs/factory/workgraph-mcp.md)).
- `--static <dir>`: serves a built control room as well.
