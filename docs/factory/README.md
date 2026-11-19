# The agentic factory

The agentic factory's control plane: a work graph of plans and their tasks,
Merkle revisions of it, the execution line that runs them (runs, steps,
artifacts, gates, attention) and the `factory.v1` API over it, with an
extension seam through which a composition adds its own kinds, gates and
task intent. This repository holds the framework, and a composition's binary
adds its extensions on top.

| Crate | What it is |
|---|---|
| [`plan-tree`](../../crates/plan-tree) | Structural Merkle trees and a stable-key tree diff, byte-compatible with docs-factory. No factory dependencies. |
| [`factory-proto`](../../crates/factory-proto) | Generated types for `factory.workgraph.v1` (the work graph) and `factory.v1` (the API), from [`proto/`](../../crates/factory-proto/proto). |
| [`factory-core`](../../crates/factory-core) | `Factory`: schema validation, derived conditions, Merkle revisions and change notifications over the work-graph seam, with the olai-store backend and its conformance suite. The extension seam: `Extension` and `FactoryBuilder`, through which a composition adds kinds, gate conditions, task extensions and descriptors. The built-in extension is the reference kind set, with the reference executors (`task.v1`, `gate.v1`, `plan.v1`), gate evaluators and amendment policy. The `OmnigentDriver` and `CodeHost` traits, their fakes, and their Omnigent and GitHub adapters. |
| [`factory-server`](../../crates/factory-server) | `FactoryService` over Connect: a `router()` to embed (the desktop app), the `reconciler` that executes plans, `cli::run` for a composition's binary, and the reference `factory-server` binary (built-in extension only), which also serves workgraph-mcp at `/mcp`. |
| [`workgraph-mcp`](../../crates/workgraph-mcp) | The agents' MCP server over Streamable HTTP: the work graph's tools with per-session bearer tokens and tool policies, and the tools extensions add. [workgraph-mcp.md](workgraph-mcp.md) covers it. |

| Package | What it is |
|---|---|
| [`@open-lakehouse/factory-client`](../../packages/factory-client) | The generated factory.v1 client (protobuf-es + connect-es), bound through `@open-lakehouse/host`, and an in-memory fake service (`/testing`). |
| [`@open-lakehouse/factory-ui`](../../packages/factory-ui) | React components: `FactoryControlRoom`, and its parts `PlanCanvas`, `NodeInspector`, `RevisionTimeline`, `PlanDiffView` and `NodeDiffPanel`. |
| [`@open-lakehouse/factory-app`](../../apps/factory) | The control room as a standalone Vite app against factory-server, and the Storybook. |

The graph's shape (one node envelope, typed payloads, registered edge kinds)
is specified in [work-graph.md](work-graph.md), and how a plan runs (profiles,
environments, runs, gates, the extension seam) in [execution.md](execution.md).
The reconciler (`factory_server::reconciler`) runs plans on Omnigent through
`factory_core::driver::OmnigentDriver`, whose adapter is over
[`omnigent-client`](../desktop.md#omnigent-client), and follows their PRs on
GitHub through `factory_core::codehost`.

## Working on it

```bash
just factory-test        # the crates' tests (fmt and clippy run workspace-wide)
just factory-check       # protos (buf), Biome, typecheck, bun test, the app's build
just factory-gen         # regenerate factory-proto and factory-client after a proto change
just factory-server      # the reference server on :7410 with the demo plan, in memory
```

`just factory-gen` needs `buf` and the protoc plugins named in
`crates/factory-proto/buf.gen.yaml` and `packages/factory-client/buf.gen.yaml`.
The generated code is committed, so building doesn't.

`just factory-server db='sqlite://factory.db?mode=rwc'` keeps the graph in
SQLite. The API is at
`http://127.0.0.1:7410/factory.v1.FactoryService/<Method>` and speaks Connect
(JSON or binary), for example:

```bash
curl -s -X POST -H 'content-type: application/json' -d '{}' \
  http://127.0.0.1:7410/factory.v1.FactoryService/ListGraphs
```

There's no authorization yet; it binds to localhost. Writes are attributed to
the `x-factory-actor` header (default `operator`).

The agents' MCP server is at `http://127.0.0.1:7410/mcp`. It refuses
requests without a token, so give it a development one:
`FACTORY_MCP_TOKENS='dev=*' just factory-server`, then connect an MCP client
with `Authorization: Bearer dev` ([workgraph-mcp.md](workgraph-mcp.md)). The server loads its
execution registry (repos, environments, profiles) from `--registry`;
[`registry.example.yaml`](../../crates/factory-core/registry.example.yaml) is
an example.

## The control room

```bash
just factory-dev         # the reference server with the demo plan + the app on http://localhost:4330
just factory-serve-app   # the built app served by the reference server on :7410
just factory-storybook   # the components on an in-memory fake
```

The UI never builds a transport or knows an origin. `factory-ui` gets a
client from `FactoryClientProvider`, which by default resolves the host's
`factory` binding (`@open-lakehouse/host`) on every call:

- **The web app** binds Connect over `fetch` to factory-server
  (`apps/factory/src/web-host.ts`).
- **Storybook and tests** bind an in-memory transport:
  `createFakeHost({ services: { factory: { transport } } })` with
  `createFakeFactory()` from `@open-lakehouse/factory-client/testing`.
- **The desktop app** binds the factory under the `factory` key in its
  service directory: `factory_server::router()` in-process by default (the
  reference demo), or forwarded through its Rust `service_fetch` proxy to a
  factory-server bound by hand. Its Factory app opens a run's sessions in its
  Agent view (`FactoryControlRoom`'s `onOpenSession`).
  [docs/desktop.md](../desktop.md#factory) covers it.

Messages use Connect's binary format, so `Any` payloads of kinds the client
doesn't know pass through intact. The client decodes them from `GetSchema`'s
descriptors and renders them generically, so an extension's payloads need no
UI code (`factory-ui`'s `kinds/`). The text and node diffs use Monaco through
mangrove's `@open-lakehouse/editor`, loaded lazily.
