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

The store's tables and its ledger are unqualified, so they land in the first
schema on the role's `search_path`: `public` by default. A role that can't
create objects in `public` needs a schema of its own, `--db-schema`
(`FACTORY_DB_SCHEMA`), for example `factory`. Since Postgres 15 only the
database's owner can create in `public`, so this covers any role that
connects to someone else's database. With `--migrate`, the server
creates the schema when it's missing, sets the role's `search_path` in that
database to it, and migrates in it. Later sessions, pooled ones included, get
the role's `search_path` when they start, so they find the store without the
flag.

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

A token only has to be valid when a connection opens. The token comes from
trestle's olai-http (`factory_core::databricks::DatabricksOAuth`), which
renews it five minutes before it expires. A task asks for it every minute and
hands the pool each renewed one; a failure is tried again on the next tick,
and open connections keep their session. The migrations and the pool share
the token.

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

### Watching other servers' writes

On Postgres, `WatchGraph` also shows writes made by other processes on the
same store: another factory-server, or the Neon `workgraph` function.
`--watch` (`FACTORY_WATCH`) sets how:

| Value | Watchers see |
|---|---|
| `notify` (the default) | Every server's writes, over one `LISTEN` connection on `--watch-db`. |
| `poll` | Every server's writes, read from the store every `--watch-interval` seconds (`FACTORY_WATCH_INTERVAL`, default 2). |
| `off` | This server's writes only. |

`--watch-db` (`FACTORY_WATCH_DB`) defaults to `--db` and uses
`--db-credentials`' password. It has to be a direct host. A pooled one
(`ep-…-pooler.…`) is refused, with the direct host to use instead: `LISTEN`
through a pooler never delivers. Keep `--db` pooled and point `--watch-db` at
the direct host:

```bash
FACTORY_DB=postgres://…@ep-x-pooler.region.aws.neon.tech/workgraph \
FACTORY_WATCH_DB=postgres://…@ep-x.region.aws.neon.tech/workgraph \
  factory-server
```

SQLite and memory stores only show this server's writes.
[execution.md](../../docs/factory/execution.md#watching-across-processes)
covers the design.

## Running as a Databricks App

The server runs as a Databricks App against hosted Omnigent, with its store
on Lakebase. Everything the App needs is a flag or a variable:

```bash
FACTORY_DB=postgres FACTORY_DB_CREDENTIALS=lakebase FACTORY_MIGRATE=1 FACTORY_DB_SCHEMA=factory \
FACTORY_APPS_PROXY=1 \
FACTORY_MCP_ROLES='operator=*;planner=@read,add_item,update_item,add_edge,annotate,submit' \
FACTORY_MCP_PRINCIPALS='group:factory-operators=operator;service-account:<factory app id>=planner' \
FACTORY_OMNIGENT_URL="https://$DATABRICKS_HOST/api/2.0/omnigent" \
FACTORY_OMNIGENT_CLIENT_ID=… FACTORY_OMNIGENT_CLIENT_SECRET=… \
FACTORY_OMNIGENT_SHARE='alice@example.com=read,bob@example.com=read' \
FACTORY_GITHUB=app FACTORY_GITHUB_APP_ID=… FACTORY_GITHUB_APP_KEY="$(cat app.pem)" \
FACTORY_GIT_CREDENTIAL_REFRESH=1 \
  factory-server --static control-room/
```

- **The store's schema.** The App's service principal connects through its
  `lakebase` resource (`CAN_CONNECT_AND_CREATE`). It can create schemas in
  the database but not objects in `public`, since the database belongs to
  whoever deployed it. So the App sets `FACTORY_DB_SCHEMA`, and the
  migrations make the schema, its role's `search_path` and the store.
- **The port.** With `DATABRICKS_APP_PORT` set, the server listens on
  `0.0.0.0:$DATABRICKS_APP_PORT`, where the proxy reaches it. `--listen`
  (`FACTORY_LISTEN`) still wins when it's given.
- **Callers** (`--apps-proxy`, `FACTORY_APPS_PROXY`, off by default). The
  Apps proxy accepts a Databricks credential from anyone with `CAN_USE` on
  the App and forwards who the caller is (`X-Forwarded-User`,
  `X-Forwarded-Email`). In Apps mode the server authenticates factory.v1 and
  `/mcp` from that identity, maps it to a grant with `FACTORY_MCP_ROLES`,
  `FACTORY_MCP_PRINCIPALS` and `FACTORY_MCP_APP_PRINCIPALS` (`group:` entries
  through the workspace's SCIM API), and refuses a request without one (401,
  or 403 without a grant). A workspace shows a non-admin service principal,
  such as the App's own, nobody else's groups. So give the App a user
  authorization scope (`user_api_scopes`, any one, such as `sql:read`): the
  proxy then forwards each person's token (`X-Forwarded-Access-Token`), and
  the server reads their groups from SCIM `/Me` as them, which
  `iam.current-user:read` (added to every App's scopes) allows. Without it,
  a person's groups are read as the App's service principal and come back
  empty. factory.v1
  writes are attributed to the caller (`user:<email>`, or
  `service-account:<application id>`), whatever `x-factory-actor` says.
  `/mcp` keeps no sessions in this mode (rmcp's stateless protocol): an
  agent's session would otherwise be lost on every App restart and after
  five idle minutes, and Omnigent's MCP client doesn't open a new one on
  `404 Session not found`. Outside Apps mode, sessions never time out.
  Development tokens are refused in this mode, and the API allows no other
  origin: a browser carries the App's session, so the control room is served
  from the App (`--static`). Outside Apps mode nothing reads the forwarded
  headers. [workgraph-mcp.md](../../docs/factory/workgraph-mcp.md#a-databricks-apps-proxy-identity)
  has the mapping.
- **Omnigent.** Hosted Omnigent's API is `<workspace>/api/2.0/omnigent`
  (`/omnigent` is its web UI). `--omnigent-client-id` and
  `--omnigent-client-secret` (`FACTORY_OMNIGENT_CLIENT_ID`,
  `FACTORY_OMNIGENT_CLIENT_SECRET`) are the OAuth client of the service
  principal the factory works as. Its token comes from
  `$DATABRICKS_HOST/oidc/v1/token` (client credentials, scope `all-apis`) and
  is renewed before it expires, by the same code as the Lakebase password. A
  static `--omnigent-token` (`FACTORY_OMNIGENT_TOKEN`) still works; it's one
  or the other.
- **Who sees the sessions.** The sessions belong to that service principal,
  so the people signed in to Omnigent's UI don't see them until they're
  shared. `--omnigent-share` (`FACTORY_OMNIGENT_SHARE`) grants its principals
  their levels on every session the factory starts (see below). Name the
  users; a group works only if hosted Omnigent resolves group membership.
- **Slice keys.** Hosted Omnigent routes a connected host's traffic to the
  replica its tunnel is on. Calls for a session on a connected host carry
  `X-Databricks-Omnigent-Slice-Key: <host_id>`; without it the server
  answers "host is on another replica". A managed sandbox's session is the
  other way round: hosted routes it by itself and answers `wrong_replica`
  to a keyed call, so its calls carry no key. A session whose server
  answers `wrong_replica` (one this process didn't create, say) is sent
  once more without the key, and never keyed again. `--omnigent-slice-keys false`
  (`FACTORY_OMNIGENT_SLICE_KEYS`) turns them off. An unsharded server ignores
  the header.
- **GitHub.** `--github app` mints a GitHub App's installation tokens: the
  App's id in `--github-app-id` (`FACTORY_GITHUB_APP_ID`), its private key in
  `FACTORY_GITHUB_APP_KEY` or a file at `--github-app-key-file`
  (`FACTORY_GITHUB_APP_KEY_FILE`). A token is minted per repo owner, cached,
  and minted again five minutes before it expires or when GitHub refuses it.
- **The sessions' GitHub** (`--git-credential-refresh`,
  `FACTORY_GIT_CREDENTIAL_REFRESH`, off by default; needs `--github app`).
  A managed sandbox on hosted Omnigent gets GitHub from its creator's
  Databricks Git credential: the sandbox's git helper reads it from the
  workspace, and `gh` is logged in with it. The factory's sessions are the
  Omnigent principal's, so a background task keeps that principal's default
  `gitHub` credential holding an installation token of the App (user
  `x-access-token`, named `factory-github-app:<owner>`). It writes through
  the Git credentials API (`/api/2.0/git-credentials`) as the
  `--omnigent-client-id` principal, with the same OAuth token as Omnigent's
  calls. Without one, it writes as the App's own principal
  (`DATABRICKS_CLIENT_ID`), which then has to be the principal the sessions
  run as.
  - **Finding the credential.** It's found by name, or else the principal's
    default `gitHub` credential is adopted (one set up by hand, say). It's
    created when there's neither, and again if it's deleted.
  - **Timing.** A new token is written ten minutes before the current one
    expires (they last an hour). A failed write is retried after 5 s,
    doubling up to a minute. A token Databricks refuses is minted again,
    after looking the installation up again.
  - **Failures.** Each failure is logged as a warning, and as an error once
    the written token is within five minutes of expiring. A stale credential
    fails the next managed session's clone. There's no attention item for it
    yet.
  - **One owner.** A token covers one installation, and the sandbox reads the
    principal's default credential, so the sessions reach one owner's repos.
    `--git-credential-owner` (`FACTORY_GIT_CREDENTIAL_OWNER`) names it, and
    can be left out when the App is installed on only one.
  - **Open:** `gh`'s token is written into the sandbox at launch, so a
    session past its first hour may lose `gh` even though git keeps working
    ([hosts.md](../../docs/factory/hosts.md#open-questions)).

`just factory-app-build` builds the App's binary: a static
`x86_64-unknown-linux-musl` build with `cargo zigbuild`, size-optimized
(`--profile release-app`), and gzipped into `factory-server.gz`. The Apps
source upload takes files up to 10 MiB, and the binary is about 18 MB
unpacked (7.8 MB gzipped), so the App ships the `.gz` and unpacks it at
start. The recipe fails when the `.gz` is over 10 MiB, and CI runs it. The
App's `app.yaml`:

```yaml
command:
  - sh
  - -c
  - gunzip -c factory-server.gz > /tmp/factory-server && chmod +x /tmp/factory-server && exec /tmp/factory-server --static control-room/
```

## Other flags

- `--registry` (`FACTORY_REGISTRY`): the profiles and repos plans execute
  with (`crates/factory-core/registry.example.yaml`).
- `--omnigent-url` (`FACTORY_OMNIGENT_URL`): runs the reconciler against
  that Omnigent. Without it, the run RPCs only record.
- `--omnigent-share` (`FACTORY_OMNIGENT_SHARE`): who the dispatched
  sessions are shared with, as `principal=level` pairs separated by commas
  (`data-eng=read,alice@example.com=manage`). The levels are `read` (watch),
  `edit` (also message and steer the session) and `manage` (also share it).
  The sessions belong to the factory's identity, so on a server with accounts
  nobody else sees them without this. A group reaches its members only if the
  server resolves group membership; prefer `read`, since an edit grant lets
  people steer a session the factory orchestrates
  ([execution.md](../../docs/factory/execution.md#what-omnigent-gives-us)).
- `--github gh|token|app|off` (`FACTORY_GITHUB`): where PR status comes from.
- `--mcp-token`, `--mcp-app-token` (`FACTORY_MCP_TOKENS`,
  `FACTORY_MCP_APP_TOKENS`): development tokens for `/mcp`
  ([workgraph-mcp.md](../../docs/factory/workgraph-mcp.md)).
- `--static <dir>`: serves a built control room as well.
