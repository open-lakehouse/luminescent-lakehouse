# workgraph-neon

The factory's work-graph MCP server for agents
([`workgraph-mcp`](../workgraph-mcp)) as a Node-API addon, deployed as the
Neon Function `workgraph`. `function/index.ts` forwards each fetch `Request`
into the in-process router (`workgraph_mcp::http::hosted_router`) and streams
tool calls' SSE responses back. Nothing lives in an isolate: MCP is served
without sessions, the work graph is in the branch's Postgres, callers
authenticate with the branch's Neon Auth or a service account's API key, and
what each may do comes from function env.
[docs/factory/workgraph-mcp.md](../../docs/factory/workgraph-mcp.md#hosted-the-workgraph-function)
covers the design, and the gateway auth model with it.

## Build and test

```bash
just test workgraph        # host-native debug addon, two isolates on the local Postgres
just test-arm64 workgraph  # the linux-arm64 bundle in node:24-bookworm-slim
just build workgraph       # the deployable zip, size-gated
```

The tests migrate the `workgraph` database first (`luminescent-migrate
workgraph`), as a deploy does, and seed the callers in the test database
(`function/fixtures/identity.sql`: Neon Auth users, service accounts and their
keys, one revoked and one expired). `function/fixtures/auth.mjs` stands in for
the branch's Neon Auth and signs the users' tokens. `function/fixtures/isolate.mjs` runs the built
handler in a process of its own behind a loopback HTTP server, so two of them
share nothing but the database.

The zip is 6.4 MiB of Neon's 10 MiB (2026-11-22, `release-neon`; 14.5 MiB
unzipped), with olai-auth (its JWKS client) and plan review's view
(`assets/plan-review.html`, 2.4 MB) included.

## Its own database

olai-store's Postgres tables (`objects`, `associations`) and its migration
ledger are unqualified, so the work graph gets a `workgraph` database on the
branch, as Unity Catalog gets `unitycatalog`. `scripts/migrate.sh` creates it
when it's missing and migrates it before each deploy. The addon points
`DATABASE_URL` (pooled, for requests) and `DATABASE_URL_UNPOOLED` (direct, for
the startup schema check) at it. `WORKGRAPH_NEON_DATABASE` names another one.

## Runtime configuration

| Variable | |
|---|---|
| `DATABASE_URL`, `DATABASE_URL_UNPOOLED` | Set by Neon for the branch. The users and keys are read from `DATABASE_URL`'s own database (`neondb`) |
| `NEON_AUTH_BASE_URL`, `NEON_AUTH_JWKS_URL` | Set by Neon for a branch with Neon Auth: the issuer of the tokens accepted |
| `WORKGRAPH_RESOURCE` | `neon.ts` (`TENANT.resources.workgraph`): the OAuth resource, the access tokens' audience |
| `FACTORY_MCP_ROLES` | Per branch. The named roles, `NAME=POLICY;…`, which `GET /roles` publishes |
| `FACTORY_MCP_PRINCIPALS` | Per branch. The role of each principal or group, `SUBJECT=ROLE;…` (or `SUBJECT=ROLE:POLICY`) |
| `FACTORY_MCP_APP_PRINCIPALS` | Per branch. The people (`user:<id>;…`) who get MCP Apps |
| `FACTORY_MCP_APP_TOKENS` | Secret, interim. The desktop's apps token, `[ROLE:]SECRET=POLICY`, until the desktop signs in to Neon Auth |
| `FACTORY_CONTROL_ROOM_URL` | Optional. The control room that results link a plan to |
| `FACTORY_REGISTRY_YAML` | Optional. The registry `list_repos` and `list_profiles` read (`factory-server --registry`'s YAML) |
| `WORKGRAPH_NEON_POOL_SIZE` | Optional. Connections per isolate (default 5) |

### Callers and their grants

A caller presents one of these as its bearer token
([docs/factory/workgraph-mcp.md, "Principals"](../../docs/factory/workgraph-mcp.md#principals-from-configuration)):

- a Neon Auth OAuth access token for the `workgraph` resource (an MCP client
  a person signed in with), or the web app's session token: `user:<id>`;
- a service account's `olk_` API key (an agent, the key on its gateway
  route): `service-account:<name>`. A revoked or expired key is a 401.

Then `FACTORY_MCP_PRINCIPALS` decides what the principal may do: its own
entry, or else the entries of the `authz` groups it is in, directly or
through other groups (they must agree). A principal with neither is a 403,
as is a banned user. `FACTORY_MCP_TOKENS` is no longer read (the function
logs that it ignores it).

`GET /roles` answers without a token: each role of `FACTORY_MCP_ROLES`, its
policy, and the tools it reaches, expanded against the server's tool set
(app-only tools never). It names no principal and no credential. The
desktop builds an environment's egress routes from it, one per role, so a
route's tool allowlist is what the function enforces.

Agents get their roles through groups (one per role), and each environment's
agents are accounts of their own, which the desktop creates in those groups
([docs/deployment.md, "Secrets"](../../docs/deployment.md#secrets)):

```bash
just admin <branch> group create factory-planners   # and the executors', the reviewers'
bunx neon functions deploy workgraph --branch <branch> \
  --src crates/workgraph-neon/function/dist --no-bundle \
  --env FACTORY_MCP_ROLES="planner=@read,add_item,add_edge,annotate,submit;reviewer=@read,annotate" \
  --env FACTORY_MCP_PRINCIPALS="group:factory-planners=planner;group:factory-reviewers=reviewer;user:<your user id>=operator:*" \
  --env FACTORY_MCP_APP_PRINCIPALS="user:<your user id>"
```

The apps token, while it lasts, belongs to the desktop's direct connection
only. Never put it, or an app principal's credentials, on a gateway route:
everything behind the route would get approval.
