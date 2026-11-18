# The factory's work graph

The work graph holds a plan: the items to do, how they nest, and what blocks
what. The factory reads and writes it only through `factory.workgraph.v1` and
the `WorkGraphReader` / `WorkGraph` traits, so a backend can be swapped
without touching the rest. This doc fixes the graph's shape.

The constraint is that it should be generic without being untyped. A schema
with an `Epic`, a `Story` and a `Task`, each with a dozen bespoke fields, would
bake one workflow into the contract and need a migration for every new
workflow. An untyped graph of nodes, edges and JSON blobs is the opposite
failure: nothing can validate it, hash it reliably, or render it without
guessing. The shape below keeps a small, fixed envelope and puts everything
domain-specific into typed payloads that the protos extend.

## Prior art

We looked at how other systems type graphs that have to stay extensible
(2026-11, sources at the end).

- **The Kubernetes object model.** Every object is an envelope
  (`apiVersion`, `kind`, `metadata`) with a `spec` that a person or controller
  writes and a `status` that the system writes. The object is a "record of
  intent" that a reconciler drives toward. `ownerReferences` give a hierarchy
  without encoding it in names.
- **Backstage's catalog.** Relations are typed, come in inverse pairs
  (`parentOf`/`childOf`, `dependsOn`/`dependencyOf`), and are derived from
  spec fields, not hand-written on both sides.
- **Trackers separate three topologies.** GitHub keeps sub-issues (a tree)
  and dependencies (`blocked_by`/`blocking`) in separate APIs. Linear stores
  the parent as a field on the child and relations (`blocks`, `related`,
  `duplicate`) separately. Azure DevOps names the three link topologies
  outright: tree, dependency and network. Each stores one direction of a
  relation and derives the inverse.
- **Beads**, an issue tracker built for agents, computes `bd ready` from open
  blockers. It also encodes the hierarchy in the id (`bd-a3f8.1.1`) as well as
  in a `parent-child` edge, so the two can disagree. We take its ready
  semantics and avoid the duplication.
- **SpiceDB's schema language.** Each type is declared with the relations it
  may have and the subject types they may point to, and the server checks
  every write against the schema. That's the model for registered edge kinds,
  though topology and cardinality are ours to add.
- **Envoy's `typed_config`.** A stable envelope carries a
  `google.protobuf.Any`, and the payload's type URL must name a registered
  extension. That makes it typed and open at once.
- **Tekton and Temporal** keep the definition (a Pipeline, a Workflow) apart
  from executions (a PipelineRun, an append-only event history). Neither puts
  run state into the definition.
- **A2A tasks and MCP tasks** both use a small run state machine (working,
  input required, completed, failed, canceled) and keep outputs separate from
  the state. Neither models dependencies between tasks.

The pitfalls they show:

- Hierarchy in two places (ids and edges) drifts.
- Status mixed into spec makes every observation look like a change of
  intent.
- Hashing non-structural relations makes approvals noisy.
- `google.protobuf.Struct` payloads have no types and no stable hash.
- Storing both directions of a relation lets them disagree.

## The shape

### Nodes

Every node has the same envelope:

| Part | Fields | Hashed |
|---|---|---|
| identity | `ref`, a `WorkItemRef` (`olai:<uuid>`, `gh:owner/repo#123`) | key only |
| type | `kind`, a registered type URL | yes |
| `spec` | `title`, `body` (markdown), `payload` (`google.protobuf.Any` of type `kind`) | yes |
| `status` | `phase`, `conditions[]` | no |
| `metadata` | `labels`, `backend`, `external_url`, `etag`, `created_by`, `created_at` | no |

- **A ref is opaque.** It never encodes where the node sits in the tree. Only
  edges do.
- **`spec` is intent.** Changing it changes the plan, which is what the
  Merkle revision tracks.
- **`status` is observation.**
  - `phase` is the steering lifecycle: `proposed`, `approved`, `done`,
    `dropped`.
  - `conditions` are derived by the core and never written by a client:
    `Ready`, `Blocked`, `Running`, `Stalled`, `Drifted`, `InReview`,
    `AwaitingGate`, each with a reason and the time it last changed.
  - "Ready" is computed, not stored.

### Kinds

A kind is a protobuf message in `factory.workgraph.v1` (or any other package)
plus an entry in the schema, registered by an
[extension](execution.md#the-extension-seam). The built-in extension ships the
reference kind set:

- **`Plan`:** the root of a graph. Goal, acceptance rubric, links to the
  target repo's criteria, the target repos, and the planner profile.
- **`Task`:** acceptance text, touched paths, and what dispatch needs: the
  repo and base branch, the PR title, and the profiles to run and review it
  on. Extra intent on the same line of work rides in `extensions`.
- **`Decision`:** a question, its options, the outcome once decided.
- **`Gate`:** a condition the plan waits on. See
  [execution.md](execution.md#gates).

Adding a kind means adding a message and registering it. There's no
`oneof` over kinds in the envelope, so a new kind changes no existing
message. A backend stores the payload as canonical proto-JSON, and the
GitHub adapter (later) as a fenced block in the issue body.

Two payloads hold other payloads, each a `google.protobuf.Any` whose type an
extension registers:

- **`Gate.condition`:** one condition message (`PrMerged`, `HumanGate`,
  `NotBefore`, or an extension's own), evaluated by the extension that
  registered it. A gate with no condition yet is a draft.
- **`Task.extensions`:** extra intent that a registered task hook interprets
  (a stack layer, a release). The rule of thumb: when the line of work
  differs, add a kind. When it's the same line with extra intent, add an
  extension.

Both are plan content: hashed, and covered by approval. Extensions hash in
order, so reordering them changes the plan. A write whose condition or
extension type isn't registered is refused. A server without the extension
can still read such a node: its payload hashes by its bytes, so it shows as
drifted rather than failing.

### Edges

An edge is `{kind, from, to}`. Each edge kind is declared in the schema:

| Field | Meaning |
|---|---|
| `name`, `inverse` | e.g. `blocked_by` / `blocks`; only `name` is stored |
| `topology` | `TREE` (one parent, defines the hierarchy), `DAG` (acyclic), `NETWORK` (anything) |
| `from_kinds`, `to_kinds` | the node kinds allowed at each end |
| `hashed` | whether the edge is plan content, folded into its source's `node_hash` |

The built-in kinds:

| Kind | Inverse | Topology | Hashed | Use |
|---|---|---|---|---|
| `child_of` | `parent_of` | TREE | as the tree itself | the hierarchy, and the shape of the Merkle tree |
| `blocked_by` | `blocks` | DAG | yes | ordering; drives the ready set |
| `relates_to` | `relates_to` | NETWORK | no | a cross-reference |
| `discovered_from` | `led_to` | NETWORK | no | provenance: an executor found this item while working on that one |

`child_of` is the only TREE kind. The core rejects a second parent, a
`blocked_by` cycle, an edge whose endpoints aren't of the declared kinds, and
an edge between two graphs.

### Hierarchy over a flat store

The wire format is flat: `GetGraph` returns `nodes[]` and `edges[]`. The tree
is a projection over `child_of`. The core computes it for the Merkle tree,
and the UI for layout. olai-store holds the same flat shape: a node is an
Object named `[graph, node]`, and an edge is an Association labeled with its
kind. GitHub maps `child_of` onto sub-issues and `blocked_by` onto issue
dependencies.

### Schema

`GetSchema` returns the merged schema of every extension the server runs:

- the node kinds: type URL, display name, the kinds allowed as children, the
  extension that registered it, and display hints (`icon`, `tone`, the
  `card_fields` a card shows);
- the edge kinds;
- the registered gate conditions and task extensions, each with a display
  name and its extension (`PayloadKind`);
- `file_descriptor_set`: an encoded `google.protobuf.FileDescriptorSet` with
  every payload type and its imports.

The core validates every write against it. The control room renders from it:
kinds it knows through a renderer registry keyed by type URL, and anything
else (an extension's kind, condition or task extension) decoded from the
descriptors and shown as a property list. No extension needs code in the UI.

### Outside the graph

- **Annotations** are append-only records attached to a node: a learning, a
  reconcile note, a decision outcome. Each has a typed payload, the actor and
  the run that wrote it. They're not node fields and they're not hashed.
- **Runs** are execution records in the factory store:
  `{ref, node, attempt, state, lease_expiry, executor, stage, revision, steps, artifacts, cost}`,
  with `state` one of `queued`, `working`, `input_required`, `completed`,
  `failed`, `canceled`. Steps carry the Omnigent sessions, and artifacts the
  PRs (with their place in a PR group) and releases. The dispatch idempotency key
  is the node ref plus the attempt. The graph sees runs only through the
  `Running`, `Stalled`, `InReview` and `AwaitingGate` conditions.
  [execution.md](execution.md) has the details.
- **Revisions** are Merkle trees over the `spec`s and hashed edges
  (`plan-tree`), each with
  the provenance of the write that produced it: `{actor, via, run_ref}`.

### Steering

- Approving a plan binds its current revision.
- An item is ready when:
  - its phase is `approved`
  - every `blocked_by` target is `done`
  - it has no live run
  - no ancestor is unapproved or has drifted from the approved revision
- An executor's amendment is an ordinary write with a `run_ref` in its
  provenance. The control plane checks its scope against the subtree of that
  run's node (Cedar, later), and records it on the revision either way.

## Sources

- Kubernetes, [Objects](https://kubernetes.io/docs/concepts/overview/working-with-objects/)
  and [Owners and dependents](https://kubernetes.io/docs/concepts/overview/working-with-objects/owners-dependents/)
- Backstage, [Well-known relations](https://backstage.io/docs/features/software-catalog/well-known-relations/)
- GitHub, [Issue dependencies REST API](https://docs.github.com/en/rest/issues/issue-dependencies)
  and the sub-issues announcement (2025-04)
- Linear, [Parent and sub-issues](https://linear.app/docs/parent-and-sub-issues)
  (from search results, not read in full)
- Azure DevOps link types (from search results; the Microsoft Learn page
  wasn't reachable)
- [Beads](https://github.com/steveyegge/beads) (README)
- SpiceDB, [Schema language](https://authzed.com/docs/spicedb/concepts/schema)
- Envoy, `typed_config` in the
  [access log API](https://www.envoyproxy.io/docs/envoy/latest/api-v3/config/accesslog/v3/accesslog.proto)
- Tekton, [Pipelines](https://tekton.dev/docs/pipelines/pipelines/); Temporal,
  [Events and Event History](https://docs.temporal.io/workflow-execution/event)
- [A2A specification](https://a2a-protocol.org/latest/specification/) (Task);
  MCP, [Tasks](https://modelcontextprotocol.io/specification/draft/basic/utilities/tasks)
