# workgraph-mcp: the agents' MCP server

Agents read and write the work graph through `workgraph-mcp`, never through a
tracker's CLI. The factory then stays the one place that checks an agent's
writes and records them in a plan's revisions. The server works over the same
`Factory` as `factory.v1`, holds no tracker or code-host credentials, and gives
each agent session a token of its own.

The crate is [`crates/workgraph-mcp`](../../crates/workgraph-mcp).
factory-server mounts it at `/mcp` through its `mcp` feature, which the
binary turns on. An embedder that only wants `factory_server::router()` (the
desktop, in-process) doesn't build it. The same server also runs hosted, as
the `workgraph` Neon Function ([below](#hosted-the-workgraph-function)).

## Transport

MCP over Streamable HTTP (rmcp, the same line as mangrove's `uc mcp serve`):

```rust
let tokens = workgraph_mcp::Tokens::new();
let server = workgraph_mcp::WorkgraphMcp::new(factory.clone())?;
let app = factory_server::router(factory)
    .merge(workgraph_mcp::http::router(server, tokens.clone(), "/mcp", workgraph_mcp::http::config(&[])));
```

One endpoint serves many clients: every agent session with its own token, and
the desktop's side channel with its own. The `Host` allowlist (loopback unless
`--mcp-allowed-host` adds more) guards against DNS rebinding. The tokens guard
against callers.

With the `stdio` feature, `workgraph_mcp::stdio::serve_stdio(server, grant)`
serves one client on stdin and stdout, with a grant fixed for the process.

## Callers and grants

Every HTTP request goes through a `CallerAuth` first (`workgraph_mcp::auth`),
which turns it into the caller's `Grant` or refuses it: `401` with a
`WWW-Authenticate: Bearer` challenge for a missing or invalid credential,
`403` for a caller that may not use the server, `503` when the identity
provider can't be asked. The check runs on every request, so a revoked
credential cuts off its session at once. There are two kinds:

- **Tokens** (`Tokens`): bearer tokens that each stand for a grant, minted
  per session or taken from configuration. factory-server uses them, and
  they're described next.
- **Principals** (`PrincipalAuth`): an `Authenticate` finds out who the
  caller is in the host's identity provider, and a map from configuration
  says what that principal may do
  ([below](#principals-from-configuration)). The hosted function uses them.

`http::router` and `http::hosted_router` take either.

### Tokens

Each token is `Authorization: Bearer <token>`.

A token maps to a `Grant`:

| Field | Meaning |
|---|---|
| `actor` | Who writes are attributed to (`Provenance.actor`) |
| `role` | A label for provenance and logs. The server never branches on it |
| `root` | The plan the session works on. Tools default to it, and refuse to read or write another plan |
| `run` | The session's run: recorded as `Provenance.run_ref` on every write, and `report_change`'s target |
| `node` | The item the session works on, for the amendment-scope check (below) |
| `policy` | The tools it may use (below) |
| `apps` | The right to MCP Apps: app-only tools and `ui://` views ([below](#mcp-apps-and-the-side-channel)). Only an MCP Apps host acting for a person gets it, never an agent |

`Tokens::mint(grant)` returns a fresh random token, and `Tokens::revoke`
forgets it. The server stores only the tokens' SHA-256. Whoever starts a
session mints its token and hands it to the session.

**Not yet: delivering a token to an Omnigent session.** The reconciler
starts the sessions, but Omnigent's session create takes no environment,
secret or MCP header, so there's no channel to hand a session its token. A
token in the prompt would sit in the model's context and the transcript, so
the reconciler doesn't mint one. Until Omnigent can carry it, an agent's MCP
connection uses a token configured with the agent (one per agent rather than
per session), and the session's run comes from the tools' `run` arguments,
which the executors' prompts name. `factory-server --mcp-token TOKEN=POLICY`
(repeatable, or `FACTORY_MCP_TOKENS=a=*;b=@read`) adds such tokens,
attributed to `operator`; a composition can insert its own with
`Tokens::insert`. `--mcp-app-token TOKEN=POLICY` (or `FACTORY_MCP_APP_TOKENS`)
adds a token with an apps grant, for the desktop's side channel
([desktop.md, Factory](../desktop.md#factory)); never give one to an agent.

### Tokens from configuration

A server that can't mint, because nothing survives between its requests (the
[hosted function](#hosted-the-workgraph-function)), takes its whole token set
from configuration: `Tokens::from_config(tokens, app_tokens)`, or
`Tokens::from_env()` over `FACTORY_MCP_TOKENS` and `FACTORY_MCP_APP_TOKENS`
(`;`-separated). factory-server's `--mcp-token` and `--mcp-app-token` go
through the same parser, and its sessions' tokens are still minted into the
set. Each entry is `[ROLE:]SECRET=POLICY`:

- `SECRET` is the token, or `sha256:<64 hex>`, the SHA-256 of the token, so
  the configuration holds nothing a caller could present
  (`printf %s "$TOKEN" | shasum -a 256`).
- With a `ROLE` (`planner:…`), the grant's actor and role are the role: the
  writes are attributed to the gateway route that carries the token. Without
  one, the actor is `operator` and the role `dev` (`app` for an apps token).
  A token itself can't contain `:`.
- `POLICY` is a [tool policy](#policies-not-roles).

```bash
FACTORY_MCP_TOKENS='planner:sha256:…=@read,add_item,update_item,add_edge,annotate,submit;executor:sha256:…=@read,add_item,add_edge,annotate,propose,report_change;reviewer:sha256:…=@read,annotate'
FACTORY_MCP_APP_TOKENS='sha256:…=*'
```

### Principals from configuration

A host with an identity provider authenticates its callers as principals,
written as `luminescent-admin` names them:

- `user:<id>`: a person, by the provider's stable user id (Neon Auth's `sub`);
- `service-account:<name>`: a non-human caller, such as an agent.

A role is what the server enforces, so it is declared once, on the server,
and everything else derives from it. `Roles::from_config(roles)`, or
`Roles::from_env()` over `FACTORY_MCP_ROLES` (`;`-separated), names tool
policies: `NAME=POLICY`, with `POLICY` a [tool policy](#policies-not-roles).
The framework still has no roles of its own; the composition names them.

`Principals::with_roles(roles, principals, app_principals)`, or
`Principals::from_env()` over `FACTORY_MCP_ROLES`, `FACTORY_MCP_PRINCIPALS`
and `FACTORY_MCP_APP_PRINCIPALS`, says what each caller may do. An entry is
`SUBJECT=ROLE`, naming one of the roles, or `SUBJECT=ROLE:POLICY`, with the
policy inline (a name `FACTORY_MCP_ROLES` defines can't be redefined that
way). `SUBJECT` is a principal, or `group:<name>` for every member of an
`authz` group, directly or through other groups (`luminescent-admin group`).

- A principal's own entry comes first. Without one, its groups' entries
  decide, and they must agree: groups that map to different grants grant
  nothing, so membership never picks between two roles.
- The grant's actor is always the principal itself: writes are attributed
  to `service-account:factory-planner-<env>`, never to the group or a
  shared role.
- An authenticated principal without a matching entry is a `403`.

```bash
FACTORY_MCP_ROLES='planner=@read,add_item,update_item,add_edge,annotate,submit;executor=@read,add_item,add_edge,annotate,propose,report_change;reviewer=@read,annotate'
FACTORY_MCP_PRINCIPALS='group:factory-planners=planner;group:factory-executors=executor;group:factory-reviewers=reviewer;user:5e1f…=operator:*'
FACTORY_MCP_APP_PRINCIPALS='user:5e1f…'
```

**The role catalog.** `Roles::catalog(tools)` lists each role with the
tools its policy allows, by name. App-only tools are never listed, since a
role never carries the right to MCP Apps. The hosted function serves it
without a token at `GET /roles` (`http::roles_router`):

```json
{"roles": [{"name": "reviewer", "policy": "@read,annotate",
  "tools": ["plan_read", "item_read", "ready", "annotate", "list_repos", "list_profiles", "open_plan_review"]}]}
```

It names no principal and no credential. A host builds on it: the desktop
turns an environment's chosen roles into egress routes whose tool allowlist
is the catalog's ([below](#agents-never-hold-a-credential-the-gateway-injects-it)),
so a route can't ask for more than the server grants, and a policy change on
the server shows up as drift rather than staying hidden.

`FACTORY_MCP_APP_PRINCIPALS` names the people who get the right to MCP Apps
([below](#mcp-apps-and-the-side-channel)). Each needs an entry of its own
(a group's is never enough), and a service account is refused: no agent can
approve its own work. The map
knows nothing about the host, so the same configuration grammar serves any
`Authenticate`:

- `auth::olai::OlaiAuth` (feature `olai-auth`), the hosted function's, as
  `unitycatalog` authenticates: the branch's Neon Auth OAuth access tokens for
  the server's resource and web session tokens (EdDSA JWTs; a banned or
  unknown user is refused), and service accounts' `olk_` API keys from
  `authz.api_keys` (a revoked or expired key, or a disabled account's, is a
  `401`). A caller's groups come from `authz.memberships`, transitively,
  and are only asked for when the map has a group entry. Token scopes and
  claims never decide anything; the map does.
- `auth::AppsProxy`, factory-server's in Apps mode
  ([below](#a-databricks-apps-proxy-identity)): the caller the Databricks Apps
  proxy forwards, with its groups from the workspace's SCIM API.

### Agents never hold a credential: the gateway injects it

Hosted, no agent holds a workgraph credential. A gateway in front of the
server adds it to each request:

- **On Databricks**, a Unity Catalog HTTP connection behind an AI Gateway MCP
  service (not built yet).
- **Locally**, trestle's egress gateway
  ([egress-gateway.md](https://github.com/open-lakehouse/trestle/blob/main/docs/egress-gateway.md)):
  an environment's `[[selection.egress.routes]]`, one per role, to
  `function = "workgraph"` at `/mcp`, consumed by Omnigent, each with a bearer
  credential from `WORKGRAPH_<ROLE>_API_KEY`. The egress Envoy fetches the key
  from the environment's secret server over SDS, so the key is in that one
  container's environment and nowhere else: not in an agent spec, a prompt, a
  transcript or the instance's files. Omnigent's MCP registry lists the route
  by name, with the route's tools as its allowlist, and a session selects it
  (`mcp_registry_services`, which a factory profile names:
  [execution](execution.md#profiles-thin-pointers-into-agent-specs)).

Each route carries the API key of a service account made for that
environment and role. The account is in the role's group, and
`FACTORY_MCP_PRINCIPALS` maps the group to the role
([above](#principals-from-configuration)), so writes are attributed to the
account and revoking one environment touches no other. The routes come from
the [role catalog](#principals-from-configuration), so a route asks for the
tools the server grants that role, no more. The desktop provisions all of it
([desktop.md](../desktop.md#work-graph-agents)): the routes, the accounts, the
keys straight into the keychain. Rotating a key issues a new one, recreates
the environment's secret server with it (the egress gateway keeps running),
and revokes the old one; nothing in an agent or the function changes.

**MCP Apps are for people only.** No app principal's credentials and no apps
token may sit on a gateway route: everything behind the route would get the
app-only tools, and an agent could approve its own plan. The desktop refuses
it: a route takes only a service account's `olk_` key, and Start refuses a
route whose secret is an apps one. The desktop connects to the function itself.
Until it signs in to Neon Auth, it does so with the interim apps token
(`FACTORY_MCP_APP_TOKENS`, which the hosted function still accepts in front of
the principals); agents only ever reach the server through their routes.

## Hosted: the `workgraph` function

[`crates/workgraph-neon`](../../crates/workgraph-neon) serves the same tools
as a Neon Function (`workgraph` in `neon.ts`), built like the other hosts: a
napi addon over `workgraph_mcp::http::hosted_router` and a fetch handler
(`function/index.ts`). Neon evicts an isolate at any time and runs several
side by side, so nothing lives in one:

- **No MCP sessions.** `http::hosted_config()` turns rmcp's
  `legacy_session_mode` off: a 2026-07-28 client sends its capabilities in
  every request's `_meta`, and a 2025-11-25 client's `initialize` opens no
  session. Such a client's capabilities from `initialize` don't carry over to
  its next request; declared in that request's own `_meta`
  (`io.modelcontextprotocol/clientCapabilities`), they do. The desktop
  declares MCP Apps that way ([below](#mcp-apps-and-the-side-channel)).
- **POST only.** `GET`, `DELETE` and the rest get `405` with `Allow: POST`
  before the token check (there's no stream to open and no session to end).
  Then the token, then the transport. There's no `Host` check: the URL is
  public, so the tokens guard it, and each branch has its own hostname.
- **The work graph on the branch's Postgres**, in a `workgraph` database of
  its own (olai-store's `objects`, `associations` and migration ledger are
  unqualified). `luminescent-migrate workgraph` applies its schema before
  each deploy; the function only checks it at startup
  (`factory_core::olai::postgres_check`) and answers `503` until it matches.
  The reference kind set, like factory-server's binary.
- **Callers are principals** ([above](#principals-from-configuration)):
  `OlaiAuth` over the branch's Neon Auth (`NEON_AUTH_BASE_URL`,
  `NEON_AUTH_JWKS_URL`, the `workgraph` resource of `TENANT.resources` as
  `WORKGRAPH_RESOURCE`) and `authz`'s API keys, both read from `neondb`;
  `FACTORY_MCP_PRINCIPALS` and `FACTORY_MCP_APP_PRINCIPALS` per branch.
  Nothing is minted, and `FACTORY_MCP_TOKENS` isn't read. The interim apps
  token (`FACTORY_MCP_APP_TOKENS`) goes first. Every isolate reads the same
  configuration, so they all map a caller the same way.
- `GET /.well-known/oauth-protected-resource` (RFC 9728) names the resource
  and the branch's Neon Auth, without a token, so MCP clients can sign in.
- **Streaming.** A tool call's response is an SSE stream, and the addon hands
  it to the handler chunk by chunk, so rmcp's keep-alive (every 15 s) reaches
  the client while a slow tool runs. Hosted tools never elicit, and stay
  idempotent under re-calls.
- `GET /health` answers once the server started (the deploy's check).

Optional environment: `FACTORY_CONTROL_ROOM_URL` (the control room results
link to), `FACTORY_REGISTRY_YAML` (what `list_repos` and `list_profiles`
read), `WORKGRAPH_NEON_DATABASE`, `WORKGRAPH_NEON_POOL_SIZE`. The zip is
about 6.4 MiB of Neon's 10, olai-auth and plan review's view included
([its README](../../crates/workgraph-neon/README.md)).

The reconciler and the control room keep running where they run today. They
reach the same work graph once they're pointed at the branch's database. The
function's writes notify the change feed through the pooled URL, so a
factory-server on that database shows them live. It needs `--watch-db` on the
branch's direct host
([watching across processes](execution.md#watching-across-processes)).

## Policies, not roles

The framework doesn't know what a plan means, so it can't know what a planner
or a reviewer is: that's the composition's workflow. It has no built-in roles.
A grant carries a `ToolPolicy`: `allow` selectors, and `deny` selectors that
win over them.

| Selector | Picks |
|---|---|
| `*` | every tool |
| `plan_read` | one tool by name |
| `plan_*` | names matching a `*` pattern |
| `@read` | tools annotated read-only |
| `@write` | tools that may write |

The text form is comma-separated, with `!` for a deny:
`@read,report_change`, or `*,!submit`. An empty policy allows nothing. The
same policy applies to built-in tools and to extensions' tools.

A tool a token may not use doesn't exist for it: it isn't listed, and calling
it is an unknown tool. This mirrors an Omnigent agent's own MCP tool
allowlist, so a composition can put the agent's allowlist in the token, and
the server enforces it a second time.

What a composition's roles might look like, as an example only:

```rust
use workgraph_mcp::ToolPolicy;

let planner = ToolPolicy::from_str("@read,add_item,update_item,add_edge,annotate,submit")?;
let executor = ToolPolicy::from_str("@read,add_item,add_edge,annotate,propose,report_change")?;
let reviewer = ToolPolicy::from_str("@read,annotate")?;
let desktop = ToolPolicy::all();
```

## Tools

The built-in tools are generic operations on the graph. When an agent
proposes or submits, and what follows, is up to the composition's agents and
executors. Arguments are JSON, and payloads are proto-JSON: an `Any`
carries its `@type`, which the tools fill in from the kind when it's left
out. `root` defaults to the grant's. Results are JSON in `structuredContent`;
the reads and the plan's writes put markdown in their text
([below](#text-and-structured-results)).

| Tool | Read-only | Arguments | Does |
|---|---|---|---|
| `plan_read` | yes | `root?`, `under?` | The plan's outline: items with ref, parent, kind, title, phase and the conditions that hold; edges other than `child_of`; the revision, the approved revision, and what drifted. `under` narrows it to an item and what's below it, and the edges that touch them (and adds `under` to the result) |
| `item_read` | yes | `ref` | One item in full: spec, status, labels, annotations, its runs, and its attention items (resolved ones too, which is how a proposal's outcome comes back) |
| `ready` | yes | `root?` | The items with the `Ready` condition |
| `add_item` | | `parent`, `kind`, `title`, `body?`, `payload?`, `labels?` | `CreateNode`. `kind` is a short name (`Task`) or a type URL, the extensions' kinds included. The item lands `proposed` |
| `update_item` | | `ref`, `title?`, `body?`, `payload?`, `labels?`, `etag?` | `UpdateNode`: reads the item and changes only the fields given. A `payload` replaces the payload whole (its `@type` filled in from the item's kind, which can't change), and `labels` the labels. With `etag` (`metadata.etag`, from `item_read`), a stale read is refused. Returns the updated item as `item_read` does |
| `add_edge` | | `kind`, `from`, `to` | `AddEdge` |
| `annotate` | | `node`, `kind`, `payload?` | `Annotate`, with `kind` `Learning`, `ReconcileNote`, `DecisionOutcome`, `ReviewVerdict` (a reviewer's verdict, which `task.v1` waits on), or a type URL |
| `propose` | | `node`, `summary`, `changes?` | Raises a `proposal` attention item on the item, with the change as its detail, and returns `status: "proposed"`. It changes nothing in the plan |
| `report_change` | | `run?`, `group?`, `prs[]`, `releases[]?` | `ReportArtifact`: the PRs as one group in landing order (`position` 1..n, `of` n, named `group` or after the run), merged by URL with what's already on the run. A session bound to a run reports only for that run |
| `list_repos` | yes | | The registry's repositories |
| `list_profiles` | yes | | The registry's session profiles |
| `submit` | | `root?`, `note?` | Hands the plan in at its current revision, returning it with `status: "awaiting_approval"`. The note goes on the plan as a `ReconcileNote` |
| `open_plan_review` | yes | `root?` | Plan review as an MCP App (`ui://factory/plan`, [below](#plan-review)). Returns `status: "awaiting_user"`; approves nothing |
| `approve_revision` | app only | `root?`, `revision` | Approves the plan at `revision`, which must be the current one, and accepts its open `approval` items |
| `return_plan` | app only | `root?`, `revision?`, `notes` | Declines the plan's open `approval` items with the notes as their comment, and puts the notes on the plan as a `ReconcileNote` |

`report_change` and the `ReportArtifact` RPC share
`Factory::report_artifacts`, which merges by URL with what Omnigent detected
and the code host polled (`factory_core::artifact`): a report owns a PR's
`group`, and never overwrites the state the code host reports.

**Changing a plan while it runs.** An agent changes the plan with the write
tools as usual: a planner that follows the plan folds a stage report's
lessons into the tasks that haven't run yet with `update_item`. While the plan executes, the reconciler's amendment policies
accept a change on their own, or escalate it to an `approval` item and hold
the changed items ([execution.md](execution.md)). `propose` is for a change
an agent shouldn't make itself. It raises a `proposal` item, which a person
accepts or declines, with a comment. Accepting one approves no revision,
unlike an `approval`. Whoever accepts it makes the change, or tells the agent
to.

**Asking for approval.** In a planning run (`plan.v1`), the executor raises
the `approval` item once the planner's session goes idle, so `submit` doesn't
raise one. Outside a planning run, `submit` raises it, unless the plan
already has an open one. Accepting it approves the plan's current revision.

### Text and structured results

An agent host shows a tool's result to the model only as its text: Omnigent
0.22 flattens a result to its text content, and drops `structuredContent` and
`_meta`. Its tool cards show that text raw, and the chat renders markdown,
tables and Mermaid only when the model relays them into its reply. So the
text of `plan_read`, `item_read`, `ready`, `propose`, `submit` and
`open_plan_review` is compact markdown the model can reason over and relay
(`workgraph_mcp::render`), and `structuredContent` keeps the JSON in the table
above, unchanged, for clients that read it. The other tools' text is their
JSON.

- **`plan_read`**: a header (title, goal, rubric, budget; the revision, the
  approved one, and the items that drifted), at most one Mermaid graph, and a
  table with a row per item (alias, ref, kind, title, phase, conditions,
  `blocked_by`, and the parent when items nest), then the other edges. Tasks
  are drawn as `T12: title` with titles cut to 24 characters, and styled by
  phase, or by `Stalled`, `Running` (`InReview`, `AwaitingGate`), `Blocked` or
  `Ready` while open. An item with children is a subgraph.
- **`item_read`**: the item with its conditions' messages, body, payload
  fields, labels, edges both ways, annotations, runs (state, cost, PRs) and
  attention items.
- **`ready`**: a table of what can start.
- **`submit`** and **`open_plan_review`**: the revision, and a table of the
  changes since the approved revision (the compact diff plan review shows).
- **`propose`**: the attention item and the revision it was raised against.

Aliases (`T3`, `D8`) number the items in outline order for one result; the
tables pair each with its ref, and tools take refs.

**The size rule** is by rank width, not node count: the chat scales a diagram
to its column (about 748 px), so a graph three or four items wide stays
readable however tall, and one six wide doesn't. Ranks are the longest
`blocked_by` chain to an item, as Mermaid's layout ranks them, with a
subgraph standing for the items in it:

| Widest rank | Items | Drawn |
|---|---|---|
| ≤ 4 | ≤ 40 | `flowchart TD` |
| 5–6, at most 4 ranks deep | ≤ 40 | `flowchart LR` |
| 5–6, deeper | ≤ 40 | one node per phase, with the edges between phases |
| > 6 | any | table only |
| any | > 40 | table only |

When the graph is left out or collapsed, the text says why and suggests
`plan_read` with `under`. An edge between a subgraph and an item inside it is
in the table only, since Mermaid's layout can't draw it.

**Where a person decides.** A client that can't show plan review (no MCP
Apps extension, or no apps grant) gets, in `submit`, `open_plan_review` and
`propose`, a link to the control room, `<url>?plan=<root>`, where a person
approves the plan or decides the proposal. The URL is factory-server's
`--control-room-url` (`FACTORY_CONTROL_ROOM_URL`;
`WorkgraphMcp::with_control_room`). Without one the text says to approve in
the control room. An MCP Apps client is pointed at plan review instead. No
tool elicits: an elicitation isn't a gate (lakehouse-data's
`omnigent/docs/control-plane.md`, "Human gates stay in our UI"). A tool sees which
client it serves in `ToolCall::client` (`factory_core::tool::Client`).

## An extension's tools

An extension adds tools as `ToolSpec`s: a name, a JSON Schema, hints, and a
handler that gets the `Factory`, the caller (from its grant) and the
arguments.

```rust
use factory_core::tool::{ToolCall, ToolHandler, ToolOutput, ToolSpec};

struct StackStatus;

#[async_trait::async_trait]
impl ToolHandler for StackStatus {
    async fn call(&self, call: ToolCall<'_>) -> factory_core::Result<ToolOutput> {
        let run = call.caller.run.as_deref().unwrap_or_default();
        // … read the graph through call.factory …
        Ok(ToolOutput::json(serde_json::json!({ "run": run })))
    }
}

impl Extension for Ours {
    fn mcp_tools(&self) -> Vec<ToolSpec> {
        vec![
            ToolSpec::new("stack_status", "Where the run's stack stands.",
                serde_json::json!({ "type": "object", "properties": {} }), StackStatus)
                .read_only(),
        ]
    }
}
```

Its tools are listed after the built-in ones. A name a built-in tool already
uses is refused when the server is built, and two extensions can't register
the same name (`FactoryBuilder`).

## MCP Apps and the side channel

Omnigent flattens a tool's result to text (markdown, [above](#text-and-structured-results))
and can't read `ui://` resources, so an MCP App never reaches the desktop
through the agent. The desktop holds
its own connection to the same server instead ([desktop.md, "Why a side
channel"](../desktop.md#agent)).

- A tool marked `.app_only()` (`Visibility::App`) is listed, and callable,
  only for a client that declared the MCP Apps extension
  (`io.modelcontextprotocol/ui`) in its `initialize` or in the request's
  `_meta`. It carries `_meta.ui.visibility: ["app"]`, so the host keeps it
  from the model. Approval belongs here, so no agent can approve its own plan.
  It also needs an **apps grant** (`Grant::apps`, `--mcp-app-token`): a client
  that declares the extension with an agent's token still sees no app-only
  tool, so no agent's token reaches approval, whatever its client declares.
- `.ui("ui://…")` sets `_meta.ui.resourceUri`, the view the host renders for a
  read-only tool. The server serves the views as resources (`resources/list`,
  `resources/read`, `text/html;profile=mcp-app`) to the same clients only:
  a declared extension and an apps grant. Anyone else lists none and reads
  none.
- The desktop's token can allow everything. Its app-only tools still stay out
  of every agent's listing.
- Against a stateless server (the hosted function), a 2025-11-25 client's
  `initialize` declaration is gone by its next request. The desktop's SDK
  (`@modelcontextprotocol/client` 2.4) speaks 2025-11-25 at most, so its
  connection also puts the extension in every request's `_meta`
  (`withClientCapabilities` in `apps/desktop/src/integrations/mcp/connection.ts`).

## Plan review

A planner shows its plan to the person with `open_plan_review`, and the person
approves it, or returns it with notes, in a view the desktop renders inline
next to the call: in the Agent view, and in the Factory app's session view.

```
planner ── submit ──▶ workgraph-mcp ◀── open_plan_review (re-run) ── desktop's side channel
   │                       ▲                                              │
   │                       └── approve_revision / return_plan ◀── ui://factory/plan (sandbox)
   └──── <mcp-app-context tool="open_plan_review" …>Approved …</mcp-app-context> ◀┘
```

- **The call.** `open_plan_review` is read-only, so the desktop can run it
  again on its own connection for the full result (Omnigent flattens it to
  text). The model gets a short text saying nothing is approved until the
  person decides. `structuredContent` has `status: "awaiting_user"`, the
  plan's `root` and `title`, its `revision` and `approved_revision`, the
  compact change counts (`changes`), and a `snapshot` of what the view shows:
  factory.v1 messages in the binary format, base64 (`GetSchemaResponse`,
  `GetGraphResponse`, `GetRevisionResponse` of the revision and of the
  approved one, `DiffRevisionsResponse` compact and full). Binary, so an
  extension's payloads pass through intact without a registry entry.
- **The view** (`ui://factory/plan`) is
  [`packages/factory-plan-review`](../../packages/factory-plan-review):
  factory-ui's `PlanCanvas`, tinted by change, and `PlanDiffView` in tree mode
  against the approved revision. It's one HTML document (a single-file Vite
  build, Monaco stubbed out), committed as
  `crates/workgraph-mcp/assets/plan-review.html`, and `just factory-check`
  fails when it's stale. It has no network: an in-memory Connect router
  answers factory-ui's RPCs from the snapshot.
- **The decision.** Approve calls `approve_revision` at the reviewed
  revision, so what's approved is what was reviewed (a plan that changed
  since is refused, and the view says so). Return calls `return_plan` with
  the person's notes. Both are app-only, through the host, as the person the
  desktop's grant names. Then the view tells the model with
  `ui/update-model-context` ("Approved revision … of the plan …", or
  "Returned … The reviewer's notes: …"), which the desktop posts into the
  session as `<mcp-app-context>` once it's idle.
- Accepting the `approval` items means the reconciler finds nothing left to
  approve. A returned plan isn't planned again by the reconciler yet: in a
  chat, the planner reads the notes from `<mcp-app-context>`.

## An extension's views

An extension contributes its own MCP App the same way, with no code in the
desktop: a view, a read-only tool that opens it, and app-only tools for what
the person decides.

```rust
use factory_core::tool::{ToolSpec, UiResource};

impl Extension for Ours {
    fn ui_resources(&self) -> Vec<UiResource> {
        // One inline HTML document: scripts and styles inlined, no network.
        vec![UiResource::new("ui://ours/stack", "stack-review", include_str!("../assets/stack.html"))
            .title("Review the stack")]
    }

    fn mcp_tools(&self) -> Vec<ToolSpec> {
        vec![
            // What the agent calls: read-only (the host re-runs it), it names the view,
            // and its structuredContent carries what the view shows, with
            // `status: "awaiting_user"`.
            ToolSpec::new("open_stack_review", "…", schema.clone(), OpenStackReview)
                .read_only()
                .ui("ui://ours/stack"),
            // What the view calls: listed only to an MCP Apps host with an apps grant.
            ToolSpec::new("merge_stack", "…", schema, MergeStack).app_only(),
        ]
    }
}
```

- **The view** is built like plan review's: `vite-plugin-singlefile`, and
  ext-apps' `App` for the host. It reads the tool's result from
  `ui/notifications/tool-result`, calls its app-only tools with `tools/call`,
  and reports the outcome to the model with `ui/update-model-context`. It may
  use factory-ui's components over a client answering from its result, as
  plan review's does. The sandbox has no network, no storage and no IPC
  ([desktop.md, MCP App sandbox](../desktop.md#mcp-app-sandbox)): no
  `localStorage` (ui-kit's `ThemeProvider` uses it; take the host's theme
  from its context instead), no fetch, no workers from URLs.
- **The URI** is `ui://<extension>/<view>`. Two extensions can't register the
  same one (`FactoryBuilder`), and none can take a built-in's
  (`ui://factory/plan`).
- **The desktop** renders a call as an app when the tool, on its own
  connection to the server, has `_meta.ui.resourceUri` and `readOnlyHint`. It
  already connects to the factory's `/mcp` as `workgraph`, so an extension's
  tools on the same server render as soon as an agent calls them (by
  `workgraph__<tool>`).
- **Never embed** the view, or anything else, in a tool's result: Omnigent
  can't serialize an embedded resource. The host reads the view as a resource.

## No embedded resources

A tool returns text and `structuredContent`, and nothing else. `ToolOutput`
has no way to hold a resource, so it can't return one. Omnigent 0.21 and 0.22
serialize a non-text content block with `json.dumps(block.model_dump())`,
which fails on an embedded resource's URL, and the agent gets an error
instead of the result. An MCP App's view is read as a `ui://` resource on the
side channel, never embedded in a result.

## A Databricks Apps proxy identity

factory-server's other home is a Databricks App (`--apps-proxy`; the server
README's "Running as a Databricks App"). The App is only reachable through
the workspace's Apps proxy, which accepts a Databricks credential from any
identity with `CAN_USE` on the App and forwards who the caller is:
`X-Forwarded-User` (the identity provider's id), `X-Forwarded-Email` and
`X-Forwarded-Preferred-Username`. `auth::AppsProxy` is the `Authenticate` over
those headers, feeding the same `Principals` map as the others:

- A caller whose forwarded email has an `@` is a person: `user:<email>`, in
  lower case. Any other is a service principal, by the application id it is
  forwarded under (the email header, or the user name without one):
  `service-account:<application id>`.
- A request without `X-Forwarded-User`, or with neither of the others,
  didn't come through the proxy and is a `401`.
- The proxy forwards no groups. When the map has a `group:` entry, a
  `GroupSource` looks the caller's up, and factory-server caches them for
  five minutes. A failed lookup is a `503`.
  - A person whose token the proxy forwards (`X-Forwarded-Access-Token`,
    when the App has a user authorization scope) is read from SCIM `/Me`
    with that token, and `/Me` has to name the same person. A workspace
    shows a non-admin service principal, such as the App's own, nobody
    else's groups, but it shows everyone theirs.
  - Otherwise the server reads the workspace's SCIM API as the App's own
    service principal, by user name or application id. That finds groups
    only when the App's principal may read them.

So the App's permissions are the allowlist (who may reach it at all), and the
map decides what each caller may do:

```bash
FACTORY_MCP_ROLES='operator=*;planner=@read,add_item,update_item,add_edge,annotate,submit'
FACTORY_MCP_PRINCIPALS='group:factory-operators=operator;service-account:6f0e5b9c-…=planner'
FACTORY_MCP_APP_PRINCIPALS='user:ada@example.com'
```

The headers are only trustworthy behind the proxy: anyone who reaches the
port directly can send them. So nothing reads them unless the server runs
with `--apps-proxy`, and then no development token is accepted (none could
pass the proxy anyway). Agent specs reach `/mcp` with `auth: { type:
databricks }`, as the service principal their sessions run as.

Which header carries a service principal's application id is what S1's
spike didn't pin down: the Apps docs only name the headers. The App's first
deploy (C6) confirms it. Until then a service principal's entry may need its
forwarded name, which the server logs at debug level.

Still not built: the desktop's Neon Auth sign-in, which retires the hosted
function's apps token, and moving `AppsProxy` into trestle's olai-auth, for
other services that run as Apps. That waits for olai-auth's next release,
which the mangrove and breakwater pins have to take with it.

## Not yet: amendment scope

Today a grant's `root` is the only boundary: a session bound to a plan can't
read or write another one. The amendment scope comes with Cedar: an
executor's session may write only inside its run node's subtree, and
`propose` is the way out of it ([work-graph.md, Steering](work-graph.md#steering)).
The control plane will ask the PDP with the caller, the action and the target
at each write site (`TODO(cedar)` in `tools.rs`). Every write already records
its actor and run in the revision's provenance, so the boundary can be checked
after the fact too.

## Tests

`just factory-test` runs `crates/workgraph-mcp/tests/http.rs`: the demo plan
on the in-memory backend, served over Streamable HTTP on a loopback port, and
driven by rmcp's client with bearer tokens. It covers:

- `401` without a token, and a revoked token mid-session
- listing and calling under policies
- MCP Apps gating and its `_meta`, the apps grant, and the views served to
  apps hosts only
- plan review: the snapshot of the demo plan's drift, approval at the reviewed
  revision only (and the approval items it accepts), a return with notes, and
  `apps/desktop/e2e/fixtures/open_plan_review.json`, the result the desktop's
  e2e fakes serve (`UPDATE_FIXTURES=1` rewrites it)
- every built-in tool
- a PR group's numbering and merge
- a proposal's attention item and its outcome, and `submit`'s approval (once, and none in a planning run)
- the plan boundary
- an extension that shadows a built-in name, or a built-in view's URI
- a fixed grant over an in-memory pipe (the stdio path)

Every result is checked for content other than text.

The hosted function has its own, `just test workgraph`
(`crates/workgraph-neon/function/smoke.test.mjs`, on the local Postgres): two
isolates, a process each, serve `plan_read`, `propose`, `item_read` and
`add_item` on one database with no shared memory. It also covers `405` and
`401` before the transport, role tokens (one configured as its hash) that
reach only their policy's tools, app-only tools and views that need the apps
token, and a 2025-11-25 and a 2026-07-28 client. `just factory-test` runs the
conformance suite on Postgres too.

Plan review's view has its own: `bun test` in `packages/factory-plan-review`
(the snapshot's decoding and the client over it, the decision's tool calls
and model context, the view on the fixture), in `just factory-check`. The
desktop runs the committed build through its real sandbox
(`apps/desktop/e2e/mcp-app.e2e.ts`) and the whole flow, submit to
`<mcp-app-context>`, in the Agent view (`e2e/agent.e2e.ts`), on mocked IPC.
