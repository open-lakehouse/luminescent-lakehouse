# workgraph-neon

The factory's work-graph MCP server for agents
([`workgraph-mcp`](../workgraph-mcp)) as a Node-API addon, deployed as the
Neon Function `workgraph`. `function/index.ts` forwards each fetch `Request`
into the in-process router (`workgraph_mcp::http::hosted_router`) and streams
tool calls' SSE responses back. Nothing lives in an isolate: MCP is served
without sessions, the work graph is in the branch's Postgres, and the token set
comes from function secrets.
[docs/factory/workgraph-mcp.md](../../docs/factory/workgraph-mcp.md#hosted-the-workgraph-function)
covers the design, and the gateway auth model with it.

## Build and test

```bash
just test workgraph        # host-native debug addon, two isolates on the local Postgres
just test-arm64 workgraph  # the linux-arm64 bundle in node:24-bookworm-slim
just build workgraph       # the deployable zip, size-gated
```

The tests migrate the `workgraph` database first (`luminescent-migrate
workgraph`), as a deploy does. `function/fixtures/isolate.mjs` runs the built
handler in a process of its own behind a loopback HTTP server, so two of them
share nothing but the database.

The zip is 5.0 MiB of Neon's 10 MiB (2026-11-22, `release-neon`; 11.5 MiB
unzipped), plan review's view (`assets/plan-review.html`, 2.4 MB) included.

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
| `DATABASE_URL`, `DATABASE_URL_UNPOOLED` | Set by Neon for the branch |
| `FACTORY_MCP_TOKENS` | Secret. The gateway routes' role tokens, `[ROLE:]SECRET=POLICY;…` |
| `FACTORY_MCP_APP_TOKENS` | Secret. The desktop's apps token, in the same form |
| `FACTORY_CONTROL_ROOM_URL` | Optional. The control room that results link a plan to |
| `FACTORY_REGISTRY_YAML` | Optional. The registry `list_repos` and `list_profiles` read (`factory-server --registry`'s YAML) |
| `WORKGRAPH_NEON_POOL_SIZE` | Optional. Connections per isolate (default 5) |

### Secrets

Set the token secrets once per branch, never in `neon.ts`
([docs/deployment.md, "Secrets"](../../docs/deployment.md#secrets)). Store
hashes (`sha256:<hex>`, `printf %s "$TOKEN" | shasum -a 256`), so the secret
holds nothing a caller could present:

```bash
bunx neon functions deploy workgraph --branch <branch> \
  --src crates/workgraph-neon/function/dist --no-bundle \
  --env FACTORY_MCP_TOKENS="planner:sha256:…=@read,add_item,add_edge,annotate,submit;reviewer:sha256:…=@read,annotate"
```

The plain role tokens go on the gateway routes, one per route. The apps
token belongs to the desktop's direct connection only. Never put it on a
gateway or Envoy route: everything behind the route would get approval.
Without any token set, the function answers every request with a 401.
