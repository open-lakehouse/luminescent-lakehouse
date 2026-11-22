# workgraph-mcp: the agents' MCP server

Agents read and write the work graph through `workgraph-mcp`, never through a
tracker's CLI. The factory then stays the one place that checks an agent's
writes and records them in a plan's revisions. The server works over the same
`Factory` as `factory.v1`, holds no tracker or code-host credentials, and gives
each agent session a token of its own.

The crate is [`crates/workgraph-mcp`](../../crates/workgraph-mcp).
factory-server mounts it at `/mcp` through its `mcp` feature, which the
binary turns on. An embedder that only wants `factory_server::router()` (the
desktop, in-process) doesn't build it.

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

## Tokens and grants

Every HTTP request needs `Authorization: Bearer <token>`. Without a live
token the answer is `401` with a `WWW-Authenticate: Bearer` challenge. The
check runs on every request, so revoking a token cuts off its session at once.

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

let planner = ToolPolicy::from_str("@read,add_item,add_edge,annotate,submit")?;
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
tools as usual. While the plan executes, the reconciler's amendment policies
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
tool elicits: an elicitation isn't a gate (omnigent-agents'
`docs/control-plane.md`, "Human gates stay in our UI"). A tool sees which
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

Plan review's view has its own: `bun test` in `packages/factory-plan-review`
(the snapshot's decoding and the client over it, the decision's tool calls
and model context, the view on the fixture), in `just factory-check`. The
desktop runs the committed build through its real sandbox
(`apps/desktop/e2e/mcp-app.e2e.ts`) and the whole flow, submit to
`<mcp-app-context>`, in the Agent view (`e2e/agent.e2e.ts`), on mocked IPC.
