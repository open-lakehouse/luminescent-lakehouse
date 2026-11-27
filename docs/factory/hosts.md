# Hosts for factory sessions

Status: design, not built. Revised on 2026-11-27 after a spike against hosted
Omnigent. That spike showed the factory can use Omnigent's managed sandboxes,
so they are now the first path. Factory-provisioned pools come later.

[execution.md](execution.md#environments-chosen-at-submission) places each
session in an environment. An environment is either a connected host that
someone else started (`placement.host`) or a sandbox Omnigent provisions for
the session (`placement.managed`). This doc covers what the factory needs to
run its sessions in sandboxes on hosted Omnigent:

- a cap on how many runs work in an environment at once, with a queue;
- a GitHub credential for the factory's own identity;
- the toolchain setup each session needs.

A third kind of environment, a **pool** of hosts the factory provisions and
keeps itself, stays designed [below](#later-pools-of-factory-hosts) for when
managed sandboxes stop being enough.

## Why

- **The factory has one identity.** A hosted factory works as its Databricks
  App's service principal. Whatever runs its sessions has to work as that
  principal, or with credentials it hands out.
- **Parallelism.** How many runs work at once should be a setting. Today
  nothing limits it: dispatch starts a session for every ready task.
- **GitHub.** Sessions clone private repos, push branches and open PRs as the
  factory's GitHub App, not as a person.
- **Isolation.** One run's checkout, build output and shell stay away from
  another's.

## What hosted Omnigent's managed sandboxes do

Checked on 2026-11-27 against our hosted Omnigent, with a throwaway service
principal and the operator, all cleaned up afterwards:

- **A service principal gets one.** `POST /v1/sessions` with
  `host_type: managed`, `sandbox_provider: databricks-lakebox`, and the
  repo as `workspace` (`https://github.com/<repo>.git#<branch>`), made with
  the principal's M2M token, provisions a sandbox in about 20 seconds. The
  first turn answered within a minute. Inside the sandbox,
  `databricks current-user me` is the service principal, and the harness
  reaches models with that principal's own credential.
- **The repo is cloned before the session starts**, into
  `~/workspace/<repo>`. That's the URL the reconciler already sends for a
  managed target.
- **GitHub comes from the creator's Databricks Git credential.** The
  sandbox's git credential helper fetches it from the workspace
  (`/api/2.0/git-credentials`), and `gh` is logged in with it. A service
  principal can hold one. With a `gitHub` credential holding an installation
  token of the factory's GitHub App (`open-lakehouse-factory`), the sandbox
  cloned the private `lakehouse-data` repo. `gh api`, `gh pr list` and a
  `git push --dry-run` all worked. Without a credential that reaches the repo,
  the launch fails with a clone error.
- **One sandbox per session.** Each managed session gets a new sandbox and a
  new host. Deleting the session removes its host. There's no cap, no queue
  and no warm pool.
- **The default image only.** `GET /v1/sandbox-profiles` lists no profiles,
  so there's no custom image and no setup step. The image has git, `gh`,
  `uv`, Node, the Databricks CLI and the harnesses, but Rust is Ubuntu's 1.75.
  That's too old for our repos.

The open-source Omnigent's `databricks` provider differs. It creates the
sandbox with the creator's browser-linked Databricks connection, which a
service principal can't have. Hosted runs its own provider, `databricks-lakebox`.

## Managed sandboxes for the factory

```yaml
environments:
  - name: sandbox
    capabilities: [gh, "toolchain:rust", isolated]
    max_sessions: 4            # runs that work here at once (default: unlimited)
    placement:
      managed: { provider: databricks-lakebox }
```

Placement doesn't change:
[`placement::resolve`](../../crates/factory-core/src/placement.rs) picks
environments by name and capabilities, and never reads the placement kind.
The managed target already maps onto Omnigent's create fields
([execution.md](execution.md#environments-chosen-at-submission)).

### A cap per environment

`max_sessions` applies to any environment, not just managed ones. It bounds a
named host as well.

- **A run takes a place** when its first session starts in the environment
  (`start_session`), and **holds it for the run**, not the session. A task
  run has at most one active session at a time. The implementer's session
  goes idle when its PR opens, and the reviewer's sessions follow it in the
  same environment, so a review doesn't need a second place.
- **It gives the place back** when the run finishes, fails, is canceled or
  escalates (`runs::finish`), and when it reaches `awaiting_merge`. Waiting
  for a person to merge can take days, and nothing runs meanwhile.
- **The count comes from the runs.** A place is a run in the environment
  that has started a session and hasn't given its place back. That's already
  in the run records, so a restarted server counts the same places without a
  record of its own.

### Waiting for a place

A run that would start a session while its environment is full waits instead
of failing. Today `start_session` fails the run with a stall when it can't
build a target.

- The run stays `QUEUED`, and its `waiting` field (a `PlaceWait`) names the
  environment and when it started waiting. That's a field of the run rather
  than a step, because the executors number a run's next session by its
  steps. The control room shows the run as queued there, with its place in
  line, which `ListPlaces` reports across every graph
  ([packages.md](packages.md#queued-runs)).
- Runs get places first come, first served, in the order they started
  waiting, across every graph. A free place goes to a newcomer only once
  every run that waited longer has one, so the order holds however the pass
  walks the graphs.
- The actions after the session that has to wait (the stage it would set)
  wait with it, so the run's stage doesn't run ahead of a session that
  hasn't started.
- A wait isn't an attention item: it's normal, and it ends by itself.
- A run that's already working never waits: its place stays until it ends.
  A run that was retried is a new attempt, and it waits behind the runs
  already in line.

A planner session follows its plan for the plan's whole life
([`plan.v1`](execution.md#planv1-planning)), so in a capped environment it
holds a place that long. Planner profiles should default to an environment of
their own.

### The factory's GitHub credential

The sandbox gets GitHub from its creator's Databricks Git credential, so the
factory's service principal holds one. The factory sets it up itself, with
its own Databricks credential:

- The credential is a `gitHub` credential (user `x-access-token`) holding an
  installation token of the factory's GitHub App, scoped to the repos the App
  is installed on. The factory already holds the App's key for `CodeHost`
  (`--github app`).
- An installation token lasts an hour. So a background task in the factory
  (`--git-credential-refresh`) mints a new one ten minutes before the old one
  expires and updates the credential (`PATCH /api/2.0/git-credentials/{id}`).
  It creates the credential if it's missing. The task runs outside the
  reconciler's pass. A failed refresh is retried and logged, as an error once
  the current token is close to expiring. An attention item for it waits for
  attention that isn't tied to a graph, since every attention item belongs to
  a graph's root today.
- **One owner.** A token covers one installation, the App's on one owner. The
  API allows a principal several Git credentials, but only one is the default
  for GitHub, and that's the one the sandbox gets. So the sessions reach one
  owner's repos (`--git-credential-owner`), and the credential is that
  owner's: `factory-github-app:<owner>`, the principal's default.
- Any process in the sandbox can use the credential through the helper,
  including the agent's shell. That's the same exposure the hand-run sandbox
  hosts have, and the App's permissions bound it. Approval gates on pushes and
  GitHub writes stay in the agent specs.

A long-lived token of a bot account would avoid the refresh, but it can't be
scoped to an installation, and it would need rotating by hand.

**The App's installation is the factory's boundary.** The factory can only
clone, push to and open PRs on repos the factory App is installed on, so
setting it up includes picking those repos. That limit is intended: the
installation decides where the factory may work, and its permissions decide
what it may do there. The same goes for a person's own managed sessions.
Their credential is a token from the Databricks GitHub App, so a private repo
works only if that App's installation in the org includes it.

### Setup

Hosted Omnigent offers no image or setup step, so each session sets up what
the default image lacks, mostly a current Rust toolchain. That lives with the
agents the composition ships: a skill the executor and reviewer follow first
(`rustup` with the repo's pinned toolchain, then `cargo-nextest`, `just` and
`gh-stack`). A session also starts with a cold build. Both costs come with
every session and haven't been measured yet.

When hosted offers sandbox profiles with a custom image and a setup step, the
environment names a profile (`sandbox_profile`) and the skill shrinks to a
check.

## Later: pools of factory hosts

If per-session setup and cold builds cost too much, or a session needs
something a managed sandbox can't give it, the factory can provision hosts
itself. It would run them as fixed slots that keep their home across runs, so
clones, toolchains and build caches stay warm. lakehouse-data's hand-run
factory hosts (its `omnigent/docs/factory/sandbox.md`) are this, operated by
hand. The design is kept here so it can be picked up later.

```yaml
environments:
  - name: rust
    capabilities: [gh, toolchain:rust]
    placement:
      pool:
        provider: databricks-sandbox
        max_hosts: 4          # also the environment's max_sessions
        warm_hosts: 1         # kept started while idle (default 0)
        restart_after: 12h    # restart a ready host started longer ago
        setup: sandbox/       # the composition's setup directory
        workspace_root: /home/sandbox-agent/work
```

- **Slots and leases.** Hosts are fixed slots, `<environment>-1` to
  `<environment>-N`. A run's place in the environment becomes a **lease** on
  one ready slot, taken and given back exactly like a place above. The
  reviewer uses the run's host after the implementer. A lease is a record of
  its own (pool, slot, host id, run, when it was taken), written in the same
  pass as the run, so a restarted server never gives a host to two runs.
- **The keeper.** A cold sandbox takes about 2½ minutes to set up, and a start
  about 30 seconds, so a background task per pool does the slow work. The
  reconciler only leases a host that's already ready. Each round, the keeper:
  - keeps `warm_hosts` ready, plus one per waiting run, up to `max_hosts`;
  - restarts a ready host started more than `restart_after` ago, before it's
    leased again, so a lease starts with at least half of the platform's 24
    hours left (a leased host is never restarted);
  - stops idle hosts above `warm_hosts`;
  - marks a leased host that went offline `broken` and starts it again (the
    session's failure is the run's, and its retry takes a new lease);
  - raises `HOST_UNAVAILABLE` when a slot keeps failing setup.
  Slot states (`absent`, `starting`, `ready`, `leased`, `stopping`,
  `stopped`, `broken`) live in the slot records beside the leases.
- **The provider seam.** The pool logic is generic. A provider only does the
  cloud operations:

  ```rust
  #[async_trait]
  pub trait HostProvider: Send + Sync + 'static {
      fn name(&self) -> &str;
      async fn status(&self, slot: &Slot) -> Result<SlotStatus>;
      /// Create or start `slot`, run the pool's setup, and return its
      /// Omnigent host id once the host is online. Idempotent.
      async fn ready(&self, pool: &PoolSpec, slot: &Slot) -> Result<HostId>;
      async fn checkout(&self, pool: &PoolSpec, slot: &Slot, repo: &RepoRef) -> Result<()>;
      async fn stop(&self, slot: &Slot) -> Result<()>;
      async fn delete(&self, slot: &Slot) -> Result<()>;
  }
  ```

  `cli::run` builds the providers the registry's pools name, through
  `FactoryBuilder`, and refuses a pool whose provider isn't there. A
  `FakeProvider` beside `FakeDriver` covers leases, waiting, release and
  restart in the reconciler tests. The first provider would be Databricks
  Sandboxes (`/api/2.0/sandboxes`, `sandbox-files` and `sandbox-exec`),
  created by the factory's service principal so the hosts register as it.

## Work

1. **`max_sessions` and the queue** in the framework: the registry field and
   its proto, taking and giving back places in `start_session` and
   `runs::finish` (and at `awaiting_merge`), waiting runs, and reconciler
   tests with `FakeDriver`.
2. **The GitHub credential refresher** in factory-server, beside
   `--github app`, tested against a fake Git credentials API. Built:
   `--git-credential-refresh` ([factory-server's README](../../crates/factory-server/README.md#running-as-a-databricks-app)).
3. **lakehouse-data:** a managed `sandbox` environment in its registry, the
   factory service principal's grants, and the setup skill for the executor
   and reviewer. Then one task end to end on it.
4. **The control room:** queued runs with their place in line: `ListPlaces`
   in factory.v1, and the run's card and inspector
   ([packages.md](packages.md#queued-runs)).
5. **Later, if needed:** pools, as above.

## Open questions

- **Does a running sandbox see a refreshed token?** The git helper fetches
  the credential per operation, so it should. `gh`'s token is written at
  launch (under `/dev/shm`), so a session past its first hour may lose `gh`.
  The setup skill could point `gh` at `git credential fill` instead. The
  refresher keeps the workspace's credential current either way. This needs a
  run longer than an hour.
- **Idle sandboxes.** What does hosted do with the sandbox of a session that's
  been idle for days, such as an implementer waiting on review or merge? Is it
  stopped and resumed on the next message, and does it cost while it waits?
  The factory may need to archive or delete a run's sessions when the run
  gives its place back.
- **Branches.** Answered on 2026-11-27, on the first factory task on
  managed sandboxes: hosted refuses `git {branch_name, base_branch}` on a
  managed session (HTTP 422, "git worktree creation requires host_id"). So
  the driver sends the session's branch only to a connected host, and on a
  managed sandbox, cloned at the base branch, the agent creates its branch
  itself. The run still records the branch it would have had.
- **Setup cost.** How long the Rust setup and a cold build take per session.
  That decides whether pools are needed.
