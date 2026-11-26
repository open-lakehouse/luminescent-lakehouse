# Hosts the factory provisions

Status: design, not built.

[execution.md](execution.md#environments-chosen-at-submission) places each
session in an environment. Today an environment is either a connected host
that someone else started (`placement.host`) or a sandbox Omnigent provisions
itself (`placement.managed`). This doc adds a third kind: a **pool** of hosts
the factory provisions, keeps and retires itself. The pool is sized by how
many runs may work at once, and each host takes one run at a time.

The first provider is Databricks Sandboxes. Their facts below come from
lakehouse-data's factory hosts (its `omnigent/docs/factory/sandbox.md`): a
sandbox runs as whoever created it, so its host registers as the factory's own
identity.

## Why

- **Nobody else can start the hosts.** A hosted factory works as one
  identity: a Databricks App works as its own service principal. A host
  belongs to whoever registered it, and a service principal sees only its own
  hosts. That principal's OAuth client exists only inside the App, so no
  operator or job can start hosts for it. The factory has to.
- **Parallelism.** The number of runs working at once should be a setting.
  Today nothing limits it: dispatch starts a session for every ready task.
- **Isolation.** One run per host keeps one task's checkout, build output and
  shell away from another's.
- **Lifecycle.** A sandbox stops after an hour idle and runs at most 24 hours
  per start, which is also when its credential stops refreshing. Someone has
  to start it before a session needs it and restart it every day.

## The registry

```yaml
environments:
  - name: rust
    capabilities: [gh, toolchain:rust]
    placement:
      pool:
        provider: databricks-sandbox
        max_hosts: 4          # the parallelism: runs that work at once
        warm_hosts: 1         # kept started while idle (default 0)
        setup: sandbox/       # the composition's setup directory
        workspace_root: /home/sandbox-agent/work
```

- **`max_hosts`** is the pool's size and its parallelism. Its hosts are fixed
  slots, `<environment>-1` to `<environment>-N` (`factory-rust-1`, with the
  composition's prefix), not ephemeral machines. A slot keeps its persistent
  home across runs, so clones, toolchains and build caches stay warm, and the
  host's Omnigent id stays stable.
- **`warm_hosts`** are kept started while nothing uses them, so a run
  doesn't wait for a start. The rest stop when idle.
- **`setup`** is a directory the composition ships, with an entry script the
  provider uploads and runs before a host's first lease, and after each
  restart. It installs tools and starts `omnigent host` as the factory's
  identity. lakehouse-data's is `scripts/sandbox/` (`bootstrap.sh`,
  `start-host.sh`). The framework doesn't know what's in it.

Placement doesn't change: [`placement::resolve`](../../crates/factory-core/src/placement.rs)
picks environments by name and capabilities and never reads the placement
kind. A pool environment is chosen exactly like any other. What changes is
what the reconciler does with the choice.

## Leases: one run per host

A **lease** binds one host to one run.

- **Taken** when the run's first session starts in a pool environment
  (`start_session`). The reconciler leases a ready host and builds the same
  `Target::Host { host_id, workspace }` as for a named host, so the session
  path after that is unchanged.
- **Held for the run, not the session.** A task run's implementing session
  goes idle when its PR opens, and comes back with each failing review's
  findings ([`task.v1`](execution.md#taskv1-implementation-review-and-merge)).
  A run has at most one active session at a time, so the reviewer's sessions
  use the same host, after the implementer's. They read the PR, and seeing
  the same checkout does no harm.
- **Released** when the run finishes, fails, is canceled or escalates
  (`runs::finish`), and when it reaches `awaiting_merge`. Waiting for a
  person to merge can take days, and nothing runs on the host meanwhile.
- **Persisted** as a record of its own (a `HostLease` label in the store),
  written in the same pass as the run: pool, slot, host id, run, and when it
  was taken. A restarted server reads the leases back before it dispatches,
  so a host is never given to two runs. Like run leases, this is only atomic
  within one process ([execution.md](execution.md#out-of-scope-here)), which
  is what an App runs.

A planner session follows its plan for the plan's whole life
([`plan.v1`](execution.md#planv1-planning)). In a pool, it would hold a host
just as long. So planner profiles should default to an environment of their
own: a pool of one, a named host, or `managed`.

## Waiting for a host

A run that would need a host while every slot is leased waits instead of
failing. Today `start_session` fails the run with a stall when it can't build
a target.

- The run stays `QUEUED`, with a wait step whose reason is the pool, and the
  control room shows it as queued for that environment with its place in
  line.
- Runs get hosts in the order they started waiting.
- A wait isn't an attention item: it's normal, and it ends by itself.
- A run that's already working never waits: its lease stays until it ends.

## The pool keeper

Provisioning is slow: a cold sandbox takes about 2½ minutes to set up, and a
start about 30 seconds. The reconciler's pass walks every graph in sequence,
so a create inside it would stall every plan. So a background task per pool,
the **keeper**, does the slow work, and the reconciler only ever leases a host
that's already ready.

Each slot has a state the keeper drives and the reconciler reads:

| State | Meaning |
|---|---|
| `absent` | No sandbox yet. |
| `starting` | Being created or started, and set up. |
| `ready` | Started, set up, its host online in Omnigent, and not leased. |
| `leased` | A run holds it. |
| `stopping` | Being stopped (idle) or restarted (daily). |
| `stopped` | Stopped; its home is kept. |
| `broken` | Setup or start failed; retried with backoff. |

What the keeper does each round:

- **Keep `warm_hosts` ready**, plus one for each run waiting, up to
  `max_hosts`. It starts the lowest stopped slots first, then creates absent
  ones.
- **Restart before the credential runs out.** A ready host started more than
  a threshold ago (default 12 hours) is restarted before it's leased again,
  so a lease always starts with at least half the 24 hours left. A leased host
  is never restarted. A run that outlives its host's credential fails its
  session, and the run's retry gets a fresh host.
- **Stop idle hosts** above `warm_hosts`, or let the provider's own idle stop
  do it, which a sandbox does after its inactivity timeout.
- **Check that leased hosts are alive.** A host gone offline under a run is
  the session's failure; the existing retry takes a new lease. The keeper
  marks the slot `broken` and starts it again.
- **Raise attention** when a slot keeps failing setup (`HOST_UNAVAILABLE`), or
  when a pool can't get a single host ready.

The keeper's state lives in the slot records, beside the leases, so the
control room can show each pool's hosts, and a restarted server picks up where
the keeper left off.

## The provider seam

The pool logic (slots, leases, waiting, the keeper's rules) is generic and
lives in the framework. A provider only does the cloud operations:

```rust
#[async_trait]
pub trait HostProvider: Send + Sync + 'static {
    /// The `provider` name a registry's pool uses.
    fn name(&self) -> &str;
    /// Where `slot` is: absent, starting, running (and since when), stopping or stopped.
    async fn status(&self, slot: &Slot) -> Result<SlotStatus>;
    /// Create `slot` if it's absent, start it if it's stopped, run the pool's
    /// setup, and return its Omnigent host id once the host is online. Idempotent.
    async fn ready(&self, pool: &PoolSpec, slot: &Slot) -> Result<HostId>;
    /// Make sure `repo` is checked out under the pool's workspace root.
    async fn checkout(&self, pool: &PoolSpec, slot: &Slot, repo: &RepoRef) -> Result<()>;
    /// Stop `slot`, keeping its home.
    async fn stop(&self, slot: &Slot) -> Result<()>;
    /// Remove `slot` and its home (pool shrunk, or `broken` for good).
    async fn delete(&self, slot: &Slot) -> Result<()>;
}
```

- The executors stay pure, and `Action` keeps its shape. Leasing and release
  happen in the reconciler's `start_session` and `runs::finish`, where it
  already builds targets and ends runs.
- `cli::run` builds the providers the registry's pools name: built-in ones
  behind cargo features, and ones a composition passes in through
  `FactoryBuilder`. A pool whose provider isn't there is refused at startup.
- A `FakeProvider`, beside `FakeDriver`, lets the reconciler tests cover
  leases, waiting, release and restart without a cloud.

### Databricks Sandboxes

The first provider, behind a `databricks-sandbox` feature in factory-core,
like `lakebase`. It uses the Databricks REST API with the factory's own
credential, the one it already uses for Omnigent:

| Operation | API |
|---|---|
| status | `GET /api/2.0/sandboxes/{id}`: state `PENDING`, `RUNNING`, `STOPPING` or `STOPPED`, or 404 |
| create | `POST /api/2.0/sandboxes?sandbox_id={id}`, with `spec.compute.inactivity_timeout` |
| start, stop | `POST /api/2.0/sandboxes/{id}/start`, `/stop` |
| delete | `DELETE /api/2.0/sandboxes/{id}` |
| setup | upload the setup directory (`PUT /api/2.0/sandbox-files/{id}/content`), then run its entry script detached (`POST /api/2.0/sandbox-exec/{id}/exec-sync`) and poll for its exit |
| online | `GET {omnigent}/v1/hosts`: the host named `{id}`, `online` |

- The sandbox's id and its host's name are the slot's name. The setup's
  entry script gets the Omnigent URL, the host name and the setup's own
  settings as environment variables.
- A long setup can't run inside one exec call, whose HTTP read gives up long
  before a cold bootstrap finishes. So it runs detached and is polled, as
  lakehouse-data's `factory_sandbox.py` does today.

## What this covers and what it doesn't

- **Omnigent-managed sandboxes** (`placement.managed`) stay as they are. If
  hosted Omnigent offers sandbox profiles with our images and setup, a pool
  could become a managed environment, and the factory would stop owning hosts.
- **Concurrency outside pools.** A named host still takes as many sessions as
  dispatch sends it. A `max_sessions` on any environment would be a small,
  separate change.
- **Cost.** A ready host costs while it waits. `warm_hosts: 0` trades a 30
  second start for nothing running while idle.

## Work

1. **The pool in the framework:** the `pool` placement kind and its proto,
   slot and lease records, leasing and release in the reconciler, waiting
   runs, the keeper, the `HostProvider` trait, and `FakeProvider` tests.
2. **The Databricks Sandbox provider**, behind its feature, tested against a
   fake of the Sandbox API, then once against a real workspace.
3. **The control room:** each pool's slots and their state, and waiting runs
   with their place in line.
4. **lakehouse-data:** a `pool` environment in its registry, its setup
   directory shipped with the App, and the pin moved.

## Open questions

- Should a reviewer get a host of its own, so it never sees the implementer's
  checkout? That costs a second slot per run under review.
- Twelve hours as the restart threshold assumes no session runs longer than
  that. Is a per-pool setting enough?
- Should waiting runs carry a priority (a plan's, or an operator's
  override), or is first come, first served enough?
