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

The password comes from a `factory_core::pg::Credentials`. The binary uses
`StaticCredentials`, which keeps the URL's password or `PGPASSWORD`. For a
rotating one (a Lakebase OAuth token), a composition implements `Credentials`
and opens the store with `cli::open_backend`.

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
