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
`Tokens::insert`.

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
executors. Arguments and results are JSON, and payloads are proto-JSON: an
`Any` carries its `@type`, which the tools fill in from the kind when it's
left out. `root` defaults to the grant's.

| Tool | Read-only | Arguments | Does |
|---|---|---|---|
| `plan_read` | yes | `root?` | The plan's outline: items with ref, parent, kind, title, phase and the conditions that hold; edges other than `child_of`; the revision, the approved revision, and what drifted |
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

Omnigent flattens a tool's result to text and can't read `ui://` resources,
so an MCP App never reaches the desktop through the agent. The desktop holds
its own connection to the same server instead ([desktop.md, "Why a side
channel"](../desktop.md#agent)).

- A tool marked `.app_only()` (`Visibility::App`) is listed, and callable,
  only for a client that declared the MCP Apps extension
  (`io.modelcontextprotocol/ui`) in its `initialize` or in the request's
  `_meta`. It carries `_meta.ui.visibility: ["app"]`, so the host keeps it
  from the model. Approval belongs here, so no agent can approve its own plan.
- `.ui("ui://…")` sets `_meta.ui.resourceUri`, the view the host renders for a
  read-only tool. Serving the view and plan review itself come next.
- The desktop's token can allow everything. Its app-only tools still stay out
  of every agent's listing.

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
- MCP Apps gating and its `_meta`
- every built-in tool
- a PR group's numbering and merge
- a proposal's attention item and its outcome, and `submit`'s approval (once, and none in a planning run)
- the plan boundary
- an extension that shadows a built-in name
- a fixed grant over an in-memory pipe (the stdio path)

Every result is checked for content other than text.
