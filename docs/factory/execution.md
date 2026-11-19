# Executing a work graph on Omnigent

[work-graph.md](work-graph.md) fixes what a plan is. This doc fixes how a plan
runs: what the control plane needs to know to start an Omnigent session for a
node, which records it keeps while the session works, how PRs, reviews and
releases come back into the graph, and what the operator sees.

Two workflows come first:

- **Planning.** A planner agent turns a goal into a graph through the
  work-graph MCP server, split by target repo and release unit.
- **Execution.** The control plane walks the approved graph. A Task becomes an
  implementing session, then one or more PRs (possibly a stack), then review
  sessions, then a wait for the merge. Gates hold the rest of the plan until a
  PR merges or a release is out.

## Generic line, specific messages

The factory is generic in its *production line* and specific in the
*messages* that travel along it.

- **The line** is the same for every kind of work: a node gets a `Run` (one
  attempt) in an environment chosen at submission, the run is a sequence of
  `Step`s, steps produce `Artifact`s, gates
  hold nodes back, and anything a person must act on becomes an `Attention`
  item. A per-kind executor drives the line.
- **The messages** are typed and allowed to be specific. A Task names its
  repo, and an extension can add intent to it (a gh-stack layer). A step that
  ran a session carries the Omnigent session id, agent version, host, harness
  and model. A PR artifact carries its place in a PR group.

The split between the two is a code boundary: see
[The extension seam](#the-extension-seam).

Omnigent is the execution substrate, and we lean on it instead of abstracting
it away. Sessions, agent specs and their versions, hosts and managed
sandboxes, harness and model overrides, PR detection, ASKs, cost tracking and
sub-agents all come from Omnigent. The factory records them in typed fields and
adds only what Omnigent doesn't have: the plan, its ordering, PR groups, gates,
and the operator's single inbox.

## What Omnigent gives us

Checked against omnigent 0.22.0, the installed package, on 2026-11-16.

| Need | Omnigent | Consequence |
|---|---|---|
| Start a session | `POST /v1/sessions`: `agent_id` (an `ag_…` id), `host_type` (`external` or `managed`), `host_id` + absolute `workspace`, or a managed `workspace` as a git URL with `#branch`; `sandbox_provider`, `sandbox_profile_id`; `git {branch_name, base_branch}`; `model_override`, `harness_override`, `reasoning_effort`; `labels`; `parent_session_id`; `project_id` | Everything a profile needs is a create field |
| Idempotent create | none | Dedupe by label (`factory.run`, `factory.step`) and look up before creating |
| Observe | status `launching`, `running`, `waiting`, `idle`, `failed`; an SSE stream without replay; a snapshot with `total_cost_usd` and `last_task_error` | SSE for liveness, the snapshot to reconcile after a restart |
| PRs | detected from `gh pr create` on the runner, many per session, with `state` and `relationship` | Detection is free; stacks aren't modeled |
| ASKs | `ElicitationRequestEvent` on the stream, resolved by `POST /v1/sessions/{id}/events` | No list-pending route, so the factory keeps its own list from the stream |
| Budgets | the agent policy's `cost_budget` | Per-session limits stay in agent specs |
| Push to us | none (no webhooks) | SSE plus polling |

The factory talks to Omnigent through one trait,
`factory_core::driver::OmnigentDriver`, so tests use a deterministic fake
(`driver::fake::FakeDriver`) and production the adapter over
`crates/omnigent-client` (`driver::omnigent::OmnigentAdapter`, feature
`omnigent`). How it models 0.22:

- **Create-or-find.** Before creating a step's session, the driver looks it up
  by its `factory.run` and `factory.step` labels with `find_sessions`, which
  pages through the caller's sessions and filters on the client (0.22 has no
  label filter). The prompt goes with the create, as its initial item, so a
  found session never needs it again.
- **The stream.** A turn starts with `response.in_progress`. The stream never
  ends and has no replay, so a step's session is done when its status is
  `idle` after its turn (or its follow-up's turn) started, and after a restart
  or a dropped connection the state comes from the snapshot, which counts the
  turns so far. `last_task_error` stays a string map.
- **PRs** aren't on the stream or the snapshot: the adapter reads
  `pull_requests()` whenever a turn ends.
- **Agents** resolve by name, the user's own (`scope=user`) first. **Hosts**
  report `configured_harnesses`, and a session is only placed on a host that
  has its harness ready.

## What a node says (hashed intent)

A node must say enough to be dispatched without guessing. Anything that changes
what gets built is in the payload, so it's hashed into the revision and
covered by approval.

**`Plan`** adds:

- `repos`: the target repositories. Each has its base branch, criteria file and
  release tool (`release-plz`, `release-please`).
- `planner_profile`: the profile its planning runs use.
- `budget_usd`: the plan's spend cap.

**`Task`** adds:

- `repo` and `base_branch`.
- `change_title`: the title of the PR.
- `extensions`: extra intent, each a `google.protobuf.Any` of a type a
  registered task hook interprets, hashed in order. An extension might add a
  stack layer (the tasks of one stack share its name) or a release intent
  (the package the change releases and a bump hint).
- `profile` and `review_profile`: profile names, see below.
- `prompt`: a `PromptRef`, either inline markdown (by default, the node body)
  or a prompt-registry name and alias.
- `requires`: capability labels the task needs from wherever it runs (`gpu`),
  beyond what its repo and profile need. They narrow where it can run, so
  they're intent. *Which* environment it runs in isn't: that's chosen at
  submission (see [Environments](#environments-chosen-at-submission)).

**`Gate`** is a new kind, described [below](#gates).

The resolved values (which agent version ran, in which environment, with which
prompt version) aren't intent. They go on the step.

## Profiles: thin pointers into agent specs

Agent behavior lives in Omnigent agent specs. They're maintained with the
composition that deploys the factory and versioned on the server: tools, policies, budgets,
skills, MCP servers, sub-agents. The factory doesn't re-model any of that.

A **profile** names an agent and the per-session overrides Omnigent accepts at
create time, and nothing else. Profiles live in the execution registry, a YAML
file next to the target repos and the environments (see
[`crates/factory-core/registry.example.yaml`](../../crates/factory-core/registry.example.yaml)). It's
loaded at start (`factory-server --registry`), checked (unique names, a known
reasoning effort, defaults and environments that exist), and served read-only
by `ListProfiles`, `ListRepos` and `ListEnvironments`:

```yaml
defaults: { planner: planner, executor: executor, reviewer: reviewer, environment: local }
profiles:
  executor:
    agent: factory-executor        # spec name; resolved to its ag_ id and version
    model: opus                    # model_override, optional
    reasoning_effort: high
    requires: [gh-stack]           # what the agent's tools need
  reviewer:
    agent: factory-reviewer
    environment: decide            # a default only
```

A node that names no profile uses the matching default.

- `agent` is the spec `name`, which is the agent's identity on the server.
  The factory
  resolves it to the current `ag_…` id and version at dispatch.
- `harness`, `model` and `reasoning_effort` are set only to override the spec.
- `environment` is the profile's *default* environment, or `decide`. Where a
  session actually runs is chosen when the work is submitted (below).
- `requires` lists what the agent needs from any environment it runs in.
- **`decide`.** A choice can be left to the `Decider`, a quick
  classification of typed questions rather than a full agent turn. It answers from the node,
  the repo and the run so far, with a deterministic fallback, and only ever
  among the environments that meet the requirements. The question, the
  answer, its confidence and whether the fallback was used are recorded on the
  step.

A workflow that is a fixed sequence of agent steps belongs in an Omnigent
agent: one orchestrating spec with sub-agents. The factory sees its top
session. The reviewer is the example: the factory starts one review session,
and that agent may fan out to sub-agents on other harnesses and models. The
factory reads only its verdict.

At dispatch the factory snapshots a **`ResolvedSessionSpec`** onto the step:
agent id and version, harness, model, effort, the environment and the rule
that chose it, host or sandbox, workspace and branch, prompt name and version,
and the labels it stamped. That's what traceability points at. It isn't
hashed.

## Environments, chosen at submission

We want to run work locally first and on sandboxes later, without editing
agents or plans. Some work can only run in some places, though: it needs a
custom image with the right toolchain, or a network that reaches a private
codebase. So where a session runs is a choice made when the work is
submitted, constrained by what the work needs.

**An environment** is a named place sessions run, with the capabilities it
offers:

```yaml
environments:
  - name: local
    placement: { host: local, workspace_root: /workspace }       # host_type: external
    capabilities: [gh, gh-stack, "toolchain:rust", "toolchain:node"]
  - name: sandbox
    placement: { managed: { provider: example, sandbox_profile: factory-host } }
    capabilities: [gh, gh-stack, "toolchain:rust", "toolchain:node", isolated]
```

- `placement` maps straight onto Omnigent's create fields: a registered host
  and a workspace root, or a managed sandbox provider and sandbox profile. A
  custom image is a sandbox profile, so "the image with our tools" is an
  environment.
- Capabilities are plain labels. The factory only matches them; it doesn't
  interpret them. Conventions: `toolchain:<lang>`, `net:<network>`,
  `repo:<owner/name>` for access to a codebase only some places have, and a
  bare name for a tool (`gh-stack`, `gpu`).

**Requirements** come from three places, and a session needs all of them:

| Source | Example | Why it lives there |
|---|---|---|
| the repo (registry) | `net:corp` for a private codebase | true of every task on that repo |
| the profile (registry) | `gh-stack` for the executor's tools | true of every session of that agent |
| the Task (`requires`, hashed) | `gpu` | true of this piece of work |

**The choice** is a `PlacementChoice`: an environment for the whole
submission, and optional per-node choices. `StartPlanning` takes an
environment for the planner, `StartExecution` takes a choice for an
approved revision, and `StartRun` can override one node's run. Each session
is placed by the first rule that names an environment:

| # | Rule | If it misses a requirement |
|---|---|---|
| 1 | the run's environment (`StartRun`) | a problem: refused |
| 2 | the submission's choice for the node | a problem: refused |
| 3 | the submission's environment | fall through, with a note (`local lacks gpu`) |
| 4 | the profile's default environment | fall through |
| 5 | the registry's default environment | fall through |
| 6 | the first eligible environment, in registry order | — |

`decide` at any rule hands the pick to the Decider, among the eligible
environments. A session with nothing eligible is a problem.

A whole-plan choice is a preference so that "run this plan in the sandbox"
works even when one task has to run elsewhere. The note makes that visible.
A per-node or per-run choice says exactly where, so a miss is an error rather
than a silent move.

**Checking before starting.** `ResolvePlacements` is a dry run: for a graph
and a choice, it returns every session the graph would start (the planner on
the root, the executor and reviewer of each open Task) with the environment,
the rule that chose it, the requirements, the eligible environments, and any
note or problem. `StartExecution` runs the same check and refuses the
submission if anything can't be placed, so a plan never stalls halfway on a
task no environment can run. The control room uses the dry run to show where
each node will run before the operator starts it.

**Execution is its own step.** Approving a revision says *what* to build.
`StartExecution` says *that it should start, and where*. Nothing is
dispatched until a graph has an execution, and the choice is recorded with it,
not hashed into the plan. The same approved plan can run locally today and
on sandboxes tomorrow.


## Runs, steps and artifacts

All three are records next to the graph (see
[Outside the graph](work-graph.md#outside-the-graph)), never node fields.

- **`Run`**: one attempt at a node. It keeps its A2A-style `state` and adds:
  - `executor` (`task.v1`)
  - `stage`, an executor-defined label for the canvas (`implementing`,
    `in_review`, `fixing`, `awaiting_merge`)
  - the `revision` it was dispatched against
  - `steps` and `artifacts`, inline (runs are small)
  - `cost_usd`, the sum over its steps
- **`Step`**: one unit inside a run. Its kind is a session, a wait on a gate,
  or a decision. Its payload is typed:
  - **`OmnigentSession`**: the session id, the agent id and version, harness,
    model, host type and id, sandbox provider, workspace, branch, parent
    session, its last status and error, cost, a UI link, and the resolved
    spec.
  - **`GateWait`**: which gate, and the last observation.
  - **`DecisionRecord`**: a `Decider` answer.
- **`Artifact`**: what a run produced.
  - **`PullRequest`**: url, repo, number, head and base, state, draft, review
    decision, checks, its place in a PR group (`group`: `name`, `position`,
    `of`; a gh stack is one use), and its source: Omnigent's detection, a
    report through the work-graph MCP, or `CodeHost` polling.
  - **`Release`**: repo, package, version, url, and when it becomes
    resolvable downstream (a registry mirror may lag the release).

**How PRs come back.** Omnigent detects the PRs a session opens, which covers
the single-PR case for free. It doesn't know about stacks, so the executor
calls [`report_change`](workgraph-mcp.md#tools) on the work-graph MCP after `gh stack submit`, with the
layers in order as one PR group. `CodeHost` polling then keeps state, checks, review decision
and merge current. The control plane merges the three sources by PR URL.

**Labels on every session.** `factory.graph`, `factory.node`, `factory.run`,
`factory.step`, `factory.revision`. Omnigent has no idempotency key on create,
so before creating a step's session the driver looks one up by its run and
step labels. A crash between create and record then can't start a second
session.

The work-graph MCP server is [`workgraph-mcp`](workgraph-mcp.md): a bearer
token per client, with the tool policy its starter chose. Per session once
Omnigent can hand a session its token.

## Executors per node kind

An executor is a pure state machine: given the node, its run and what's been
observed (session status, PRs, gates), it returns the next actions. The
reconciler applies them through the `OmnigentDriver` and `CodeHost`, and
records the result. An LLM makes no scheduling decision; the `Decider` only
fills in choices a profile leaves open.

```rust
trait NodeExecutor {
    fn kind(&self) -> &str;
    fn name(&self) -> &str;                          // what runs record: `task.v1`
    fn next(&self, cx: &ExecContext) -> Vec<Action>; // node, run, graph, runs, Observations
}

enum Action {
    StartSession { step: String, role: String, profile: String, prompt: String },
    SendMessage { step: u32, text: String },          // a follow-up turn
    SetStep { step: u32, state: StepState },
    Wait { observed: String },                        // a gate's GateWait step
    SetStage(String),
    RaiseAttention { kind: AttentionKind, summary: String, step: Option<u32> },
    Complete { node_done: bool },
    Fail { reason: String },
    Retry { reason: String },                         // fail this attempt, start the next
}
```

`Observations` hold each active step's session as last seen (status, turns,
error, cost, whether its PRs are synced), the node's `ReviewVerdict`
annotations and its attention items. The reconciler merges the run's PRs
itself (see [How PRs come back](#runs-steps-and-artifacts)), so there's no
action for it.

### `plan.v1`: planning

```
StartPlanning(goal, repos, planner_profile)
  └─► Plan root (proposed) + run ──► planner session
        tools: list_repos, list_profiles, plan_read, add_item, add_edge, submit
        items land as proposed; the planner sets repo, change_title, extensions,
        profile per Task, and Gates for cross-repo release order
  └─► submit ──► Attention(approval) ──► operator approves revision R
```

- The planner sees the repos and profiles through the MCP, so it assigns
  them from a known list instead of inventing names.
- Partitioning follows the release units. One PR per `type(scope)`, a stack
  when one coherent change spans several, and a Gate where a downstream repo
  needs an upstream release.
- Re-planning is a new run on the same root. The revision diff shows what it
  changed. Changes while the plan executes are assessed rather than paused:
  see [Amendments](#amendments-a-plan-that-changes-while-it-runs).

### `task.v1`: implementation, review and merge

| Stage | Step | Leaves when |
|---|---|---|
| `implementing` | a session on `profile`, a new worktree branch from `base_branch` | the session is idle and has at least one PR |
| `in_review` | a session on `review_profile`, given the PRs | it posts a verdict |
| `fixing` | a message into the implementing session with the findings, up to K rounds | the session is idle again, then back to `in_review` |
| `awaiting_merge` | a wait on the PRs | every PR is merged: the node is `done` |

- A session that goes idle without a PR, fails, or loses its lease stalls the
  run. It retries once, then raises attention.
- Review escalation after K rounds is an attention item, not a failure.
- For a PR group, the run waits until every member is merged. For a gh stack
  (an extension's task hook), the lower layers merge first, as `gh stack`
  does.

### `gate.v1`

No session. It watches its condition and marks the node `done` when it holds.

## Gates

A `Gate` is a node like any other: a child of the plan, `blocked_by` its
inputs, blocking what depends on it, and hashed. Its payload is one condition,
a `google.protobuf.Any` evaluated by the `GateEvaluator` registered for its
type URL. The built-in extension registers:

- `PrMerged`: a Task's PRs are merged.
- `HumanGate`: someone has to confirm, with a prompt.
- `NotBefore`: a time.

An extension adds its own, for example a gate that holds once a package
version matching a requirement is published, so a downstream task in another
repo waits for the release it builds on.

Gates make cross-repo ordering visible in the plan, and the canvas shows what
each one is waiting on. A `human` gate, and a gate that has waited longer than
expected, raise attention.

## The extension seam

The framework knows the line and the reference kinds. Everything specific to
a workflow comes in through an `Extension`, and a composition root (a
binary) chooses the extensions:

```rust
pub trait Extension {
    fn name(&self) -> &'static str;
    fn kinds(&self) -> Vec<KindSpec>;                    // node kinds, each with its payload check
    fn edge_kinds(&self) -> Vec<EdgeKind>;
    fn gates(&self) -> Vec<Box<dyn GateEvaluator>>;      // keyed by condition type URL
    fn task_hooks(&self) -> Vec<Box<dyn TaskHook>>;      // keyed by task extension type URL
    fn executors(&self) -> Vec<Box<dyn NodeExecutor>>;   // keyed by node kind (with the reconciler)
    fn mcp_tools(&self) -> Vec<ToolSpec>;                // extra workgraph-mcp tools (schema + handler)
    fn types(&self) -> Vec<AddTypes>;                    // proto-JSON for its Any payloads
    fn descriptors(&self) -> &'static [u8];              // a FileDescriptorSet, for clients
}
```

Every method but `name` has an empty default. `FactoryBuilder::new()` holds
the built-in extension (`factory_core::builtin`, the reference kind set), and
`.extension(e)` adds one. The builder merges them and refuses a kind, edge
kind, condition, extension type or tool registered twice. It installs their
proto-JSON types, which payloads need in order to hash, and merges their
descriptors for `GetSchema`. The result is a `Factory` whose writes are
checked against the merged set.

```rust
let builder = FactoryBuilder::new().extension(MyExtension);
let backend = olai::in_memory(&builder.schema()?);
let factory = builder.build(Arc::new(backend))?;
```

`factory_server::cli::run(builder, seeds)` is the standalone server's body.
The reference `factory-server` binary runs it with the built-in extension
alone, and a composition's binary runs it with its own extensions.

The reconciler drives what's registered, and every driving method has a
default, so an extension overrides only what it needs:

- `GateEvaluator::evaluate(condition, &GateContext) -> GateObservation`:
  open, pending (and on what), or needs a person.
- `TaskHook::prompt` (add to a session's prompt or a fix message),
  `TaskHook::group` (group and order the run's PRs: a stack's layers) and
  `TaskHook::react` (amend the executor's actions: hold `Complete` until a
  release is out), in the order of the task's `extensions`.
- `NodeExecutor::next`, above.
- `Extension::amendment_policies`, below.

`Declared<M>` still declares a payload with no behavior: it checks the
payload by decoding `M`, a gate on it holds ("has no evaluator yet"), and a
task hook on it does nothing. A `ToolSpec` is a tool for `workgraph-mcp`:
a JSON Schema, hints and a handler.

### Amendments: a plan that changes while it runs

Agents discover work, split tasks and reshape what's left while a plan runs,
and most of it needs no person. When an executing plan's revision moves past
the approved one, the reconciler assesses each changed node of the compact
diff with the extensions' `AmendmentPolicy`s in registration order, then the
built-in default; the first answer that isn't `Abstain` decides, so a
composition can tighten or loosen the default. The default accepts every
change except a Task on a repo that isn't one of the plan's, a change to the
plan root's own content (its repos, its budget), and removing a node that has
a run.

If every change is accepted, the reconciler approves the new revision itself
(recorded as an `amendment` event) and the plan carries on: discovered work
is dispatched like any other. If one is escalated, one `approval` item asks a
person; the changed nodes stay `Drifted` until it's accepted, and the rest of
the plan keeps running.

The UI needs no code for an extension. It decodes payloads from `GetSchema`'s
descriptors and shows them as property lists, labelled by the display names
the extension registers. A custom view ships as an MCP App from the
extension's tools.

## Attention: one inbox

Everything a person has to act on is an **`Attention`** item:

| Kind | From | Resolved by |
|---|---|---|
| `ask` | an Omnigent elicitation, read from the session stream | accept, decline or cancel, forwarded to the session |
| `approval` | a submitted plan, or a change an amendment policy escalates | `ApproveRevision`, or accepting it |
| `gate` | a `human` gate, or a long wait | confirming the gate |
| `stalled` | a run that stalled twice | retry, or drop the node |
| `review_escalation` | K review rounds without a pass | take over the PR, or retry |
| `budget` | spend reaching the plan's cap | raise the cap, or stop |
| `proposal` | an agent's `propose` ([workgraph-mcp](workgraph-mcp.md#tools)) | accepting or declining it, with a comment; it approves nothing |

`ListAttention` and `ResolveAttention` serve the inbox. An item carries its
node, run and step, so the canvas can badge the node too.

## The live view

- **`WatchGraph`** sends typed changes: the revision and sequence as now, plus
  a changed run (with its steps and artifacts), or a raised or resolved
  attention item. The canvas patches its cache and doesn't refetch the graph.
- **Conditions** add `AwaitingGate`, `InReview` and `NeedsAttention`, derived
  like the others.
- **A node card** shows the run's stage, the session status, PR chips (with
  their place in a PR group), chips for a gate's condition and a task's
  extensions, and the cost. It links out to the Omnigent session and the
  PRs.

## API

`factory.v1` adds:

| RPC | Purpose |
|---|---|
| `ListProfiles`, `ListRepos`, `ListEnvironments` | the registries, for the UI and the planner |
| `ResolvePlacements` | where each session would run under a choice, and what can't be placed |
| `StartPlanning` | create a Plan root and its planning run, with the planner's environment |
| `StartExecution` | start executing an approved revision, with a placement choice |
| `StartRun`, `CancelRun`, `RetryRun` | manual control over a node's run, optionally in another environment |
| `GetRun` | one run, with its steps and artifacts |
| `ReportArtifact` | a PR or PR group from the executor (via the work-graph MCP), or a release |
| `ListAttention`, `ResolveAttention` | the inbox |

`GetRun` takes the graph's root as well as the run ref, as `ListRuns` does.
The run-driving RPCs only write records: `StartExecution` records the
execution (after refusing a revision that isn't the approved one, or a plan it
can't place), `StartPlanning` and `StartRun` queue a run, `CancelRun` marks
one canceled, `RetryRun` queues the next attempt, `ReportArtifact` merges a
PR group or release into a run, and `ResolveAttention` records a resolution.
The reconciler acts on them, so the API works without one, and nothing is
dispatched until one runs (`factory-server --omnigent-url …`).

## What exists now

- The protos: `crates/factory-proto/proto/factory/workgraph/v1/execution.proto` (steps,
  sessions, artifacts, profiles, attention and how it was resolved), the
  `Task`, `Plan` and `Gate` fields in `kinds.proto` (`Gate.condition` and
  `Task.extensions` as `Any`), the `Run` fields in `records.proto` (with its
  lease holder, `StartRun`'s overrides and its error) and `ReviewVerdict`, and
  the RPCs above in `service.proto`.
- The extension seam above, with the built-in extension: the reference kinds,
  the `task.v1`, `gate.v1` and `plan.v1` executors (`factory_core::executor`),
  the `PrMerged`, `HumanGate` and `NotBefore` evaluators
  (`factory_core::gates`), and the default amendment policy
  (`factory_core::amendment`). Each is table-tested over its transitions.
- The registry has environments with capabilities, and requirements on
  repos and profiles. `factory_core::placement` resolves where each session
  runs, by the rules above; at dispatch, a host must have the session's
  harness ready.
- `factory_core::driver`: the `OmnigentDriver`, its fake, and the adapter
  over `omnigent-client`. `factory_core::codehost`: the `CodeHost`, its fake,
  and the GitHub adapter (GraphQL, polled; the token from `GitHubAuth`, given
  or the gh CLI's). `factory_core::artifact` merges a run's PRs from the three
  sources by URL.
- The record store keeps attention items, and `Factory::lease_run` leases a
  run to one reconciler.
- `factory_server::reconciler`: amendments, dispatch of the ready set, leases,
  placement, create-or-find, observing sessions (streams, snapshots after a
  restart), the code host and verdicts, applying actions, attention from
  elicitations, and acting on resolutions. `factory-server --omnigent-url`
  runs it (`--github gh|token|off`). Its tests drive passes by hand on the
  fakes, against the demo plan too, including a crash between a session's
  create and its record.
- The core registers `Gate` (under a plan or a task, no children, blocking like
  any item), derives `InReview` and `AwaitingGate` from a live run's stage, and
  sends a changed run or attention item along with its graph change.
- The reference demo plan has a `PrMerged` gate and a run in review with a
  group of two PRs. The control room shows each node's latest run:
  stage, cost and PR chips on the card, and the sessions and PRs in the
  inspector. Payloads it has no code for (an extension's) render from the descriptors.

## Out of scope here

- Cedar on the new actions (`start_run`, `resolve_attention`), and on
  amendments (an agent's amendment scope).
- Webhooks instead of polling, for Omnigent and the code host.
- Leases across processes: `lease_run` is atomic within one process, so run
  one reconciler per store until the store can compare-and-swap a record.
- A cache for `find_sessions`, whose lookup is a scan of the caller's sessions.
- `parent_session_id` on create: `omnigent-client` doesn't send it yet, so a
  step's session never names a parent.
- The Decider: `decide` falls back to the first eligible environment, and no
  `DecisionRecord` step is written yet.
- Registry prompts (`PromptRef.registry`): a task's prompt is its inline text
  or its body.
- The `budget` attention item: spend is summed per run, not checked against
  the plan's cap.
