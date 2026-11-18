# The desktop shell

`apps/desktop` is a Tauri 2 app for running lakehouse environments locally: it
manages trestle workspaces, environments and revisions, runs them as instances
(compose projects), and binds the service UIs to them. The design is in
open-lakehouse/luminescent-lakehouse#14, the work in #15. This document covers
the scaffold (#16): the layout, the seams, and the decisions the later work
builds on.

macOS first. Linux compiles, lints and passes the smoke test in CI; Windows
comes later.

## Layout

```
apps/desktop/
  Cargo.toml, Cargo.lock   its own Cargo workspace (members below), apart from the
                           root's Neon Function workspace and its size gate
  package.json             a member of the root Bun workspace (root bun.lock)
  src/
    main.ts                registers the host, then imports the UI
    app.tsx                providers + mount
    host/                  the only importer of @tauri-apps/api (Biome enforces it)
      bindings.ts          generated from the Rust commands (tauri-specta)
      desktop-host.ts      the LakehouseHost and ShellHost the UI sees
      service-fetch.ts     ServiceBinding.fetch over the Rust proxy
      query.ts             the QueryRunner over the host's query engine
      workspace-store.ts   the editor's WorkspaceStore on the shared state
      native-planner.ts    the editor's Planner on the native planner
      smoke.ts             the in-app smoke checks (`--smoke`; the MCP App
                           sandbox's in smoke-mcp-app.ts)
      ipc-mock.ts          fixtures for Playwright (`--mode e2e` builds only)
      fake-state.ts        in-memory state commands (tests, Playwright)
    shell/                 the frame (env-editor's workspace shell) and what the desktop
                           contributes to it: environments' run state and tabs, the
                           Apps view, settings
    integrations/          the service UIs (Unity Catalog, lineage), the Query app,
                           and the MCP client and MCP App host (mcp/)
    components/ui/chatcn/  chatcn's components, copied by the shadcn CLI
                           (components.json): not under this repository's Apache-2.0
                           license (NOTICE.md there)
  src-tauri/               thin bin: window, commands, events, CSP, capabilities,
                           the query engine (hydrofoil, `src/query.rs`), and the
                           `mcp-app` scheme (`src/mcp_app.rs`, `assets/mcp-app/`)
  crates/shell-core/       the logic, free of Tauri: `cargo test` covers it
                           (state, planner, toolchain, services)
  e2e/                     Playwright (WebKit) specs
  scripts/                 smoke.sh and its streaming test service
```

## Seams

**The service host.** Service UIs bind through `@open-lakehouse/host`: a
`LakehouseHost` with an `EnvironmentScope` and a `ServiceDirectory` keyed by
service: the API a module serves when other implementations serve it too
(`unity-catalog`, whichever of `unity-catalog-oss` and `unity-catalog-rs` runs),
else the trestle module id (`headwaters`, …). `main.ts` calls
`registerHost()` before it imports the UI, so clients that resolve the host at
import time bind to the desktop's. The UI never sees a base URL or a token:
`ServiceBinding.fetch` sends Rust a service key and a path, and Rust resolves the
upstream, adds its credentials and streams the response back.

**The shell host.** The shell's own needs (the doctor, the planner, the
workspaces in shared state and running them; secrets in #19) go through
`ShellHost`
(`src/shell/shell-host.tsx`), implemented in `src/host/`. Views take both hosts
from context, so tests and Storybook run them on fakes
(`createFakeHost()` from `@open-lakehouse/host/testing`, a plain object for
`ShellHost`).

**The frame.** The window is env-editor's `WorkspaceShell`, with one activity
bar. The desktop fills its host seams (`ContributionsProvider`, env-editor
0.0.4) from `src/shell/`, and the editor learns nothing about Docker or Neon:

- the sidebar keeps the editor's own views (Environments, History, Workspace
  settings); the workspace and its diagram always keep the rest of the window
- `settings.tsx`: a gear at the bottom of the activity bar (an
  `activityActions` entry) opens **Settings** in a dialog over the workspace:
  General (one at a time, stop on quit, theme, what the launch's reconcile
  found), Workspaces (picker, new, import and export, where the state lives;
  also opened from the workspace's name in the status bar), Neon (the
  workspace's project and its branches) and Doctor
- `environments.tsx`: per environment, its run state and placement (tree row
  and tab header), the header's details (its **Neon** branch and **Gateway**
  URL, each copyable), Start and Stop, a revision switch while several run, the
  state of each service on the canvas, and an **Apps** header action (the shown
  instance's apps, below). The bottom panel (env-editor's `panels`) gets two
  tabs next to Problems: **Run** (`RunPanel.tsx`: its revisions, each gateway as
  a link to its apps, containers and the last failure) and **Logs**
  (`LogsPanel.tsx`; ⌘K "Show logs of `<env>`")
- `integrations/editor/`: Files and Changes in Monaco, through env-editor's
  `fileViewer` / `diffViewer` and mangrove's `ReadOnlyFile` / `DiffHost`
  (`@open-lakehouse/editor`): read-only, the language from the extension, the
  app's light or dark theme. Both load lazily, so Monaco stays out of the first
  chunk.
- `apps.tsx`: the **Apps** view, an env-editor workbench view
  (`workbenchViews`, 0.0.7) next to the editor in the activity bar. Its
  sidebar lists the workspace's running instances, by environment, and the
  selected one's apps; the main area shows the app picked. The editor is where
  an environment is configured and started, Apps where what runs is used, so
  the two don't share tabs. See Services.
- ⌘K is env-editor's command palette. The editor brings Go to, Edit (new
  environment, commit, diff, export) and View. The desktop adds:
  - **Environment**: each environment's header actions, from the same
    `HostAction`s so the two can't disagree. That covers Start (or Start #head
    next to a running revision), Stop, Stop #n, Restart and Pick the Neon
    branch. Each row's hint is the `trestle`
    CLI equivalent (`trestle up local/dev@3 --neon-branch demo`).
  - **Apps**: `Apps: <app> on <env #rev>` for each app of a running instance.
  - **CLI**: **Copy CLI** for each environment's `up`, `down`, a restart,
    `logs --follow` and `health`, offered whatever the header shows right now.
    `src/shell/cli.ts` builds them.
  - **App**: Doctor, Neon branches (reset and delete; new branches come from an
    environment's Neon picker), Workspaces (switch, import, export) and the theme.

  Type a section and a colon to narrow (`env: start`, `cli:`). Nothing shells
  out to `trestle`: the commands run through the same `ShellHost` calls as the
  header, and the CLI lines are for a terminal.

The editor knows environments by its own ids and the run data by the store's;
names are unique in a workspace, so the two meet on the name.

**Environment switches.** A service view (an instance's Catalog or Lineage)
sits under `EnvironmentBoundary` (`src/shell/EnvironmentData.tsx`): a query
cache of its own per environment key, and a remount on the key. A response
still in flight from the previous environment lands in that environment's
cache, which nothing renders any more.

**Bindings.** `src/host/bindings.ts` is generated by `src-tauri`'s
`export_bindings` test from the `#[specta::specta]` commands and the event
types; `just desktop-check` (and CI) fails if it is stale. The body channel of
`service_fetch` is typed by hand (`BodyMessage` in `service-fetch.ts`): raw
chunks arrive as `ArrayBuffer`s, which specta can't express.

## Workspaces and shared state

The workspace editor (env-editor's `WorkspaceShell`) works on trestle's shared
state store, `$TRESTLE_HOME/state.db` (without `TRESTLE_HOME`, the platform
data dir: `~/Library/Application Support/trestle/` on macOS), the same store the
`trestle` CLI uses. What the app writes, `trestle ws list` / `ws show` /
`env show` show, and a CLI write reaches the app within about 0.3 s:
shell-core's `state` polls the store's change counter and the app emits
`state-changed`, on which the editor and the workspace picker reload. Planning
runs natively (shell-core's `planner`, the `olai-stack-topology` projection the
in-browser wasm planner wraps), so the editor previews exactly what the runtime
runs. A first launch with no workspace creates `local` with one environment,
`dev`, on the baseline selection, as `trestle env create` would.

**History** in the editor commits the draft as the next revision, compares the
draft with the head (**Diff**) and any revision with the one before it (the
compare button on a revision), all over the store's revisions.

**Import** and **Export** in the workspace bar move workspaces in and out of the
store in the CLI's exchange formats (olai-stack-state's `transfer`):

- **Import** takes a directory. A workspace directory (`trestle-workspace.toml`
  with one environment directory per member) becomes a new workspace, each member
  an environment at revision 1. An environment directory (`env.toml` and the
  module manifests it lists) becomes a new environment of the current workspace,
  named after the directory, at revision 1, provided it plans. Slots and
  `secrets.env` aren't imported: the store leases slots per instance and holds
  secret references only. A failed import leaves nothing behind.
- **Export** writes the workspace (each environment's head revision) to
  `<chosen>/<workspace>`, or one revision to `<chosen>/<env>-r<n>`. It never
  writes over an existing directory.

The folder picker is the native one, opened from Rust (`rfd`), so the webview
needs no dialog capability.

 the adapter
(`src/host/workspace-store.ts`) bridges them:

- An environment's slot is leased when an instance runs (#18); the editor only
  previews host ports with one, so the app assigns preview slots per session.
- Revision digests: the store's covers manifest, module manifests and vars; the
  editor compares by selection, so revisions are reported with the editor's.
- Not in the store yet, so refused with an error in the editor: renaming a
  workspace or an environment and changing the gateway mesh (trestle#313). A commit's message
  and time aren't stored either (the message is dropped).

The CLI must be one that has the shared state (`trestle ws …`, olai-trestle
0.0.22 or later); older ones have only the file-based `trestle workspace`.
`just desktop-smoke` (and CI) run the app on a throwaway `TRESTLE_HOME`.

## Running environments

An environment's tab header starts and stops it through trestle's runtime
(`olai-stack-runtime`, shell-core's `instances`), as `trestle up` and `trestle
down` do, on the same instances: what the app starts, `trestle ps` shows and
`trestle down` stops, and the other way round. The header and the tree row show
its state (a dot), the revision that runs and where (`local slot 0`); the
canvas shows each module's state, from its services' containers (the
revision's plan says which container is whose: Unity Catalog runs as
`unitycatalog`).

- **Start** runs the environment's head revision (commit one first) and makes it
  the current environment (see Services). It reuses
  that revision's instance or creates one, leases a slot (host ports shift by
  1000 per slot: the gateway is `http://localhost:9080` on slot 0, `:10080` on
  slot 1), renders the compose tree under `$TRESTLE_HOME/instances/<id>/`, checks
  the host ports and runs `docker compose up --wait` (up to 5 minutes). Secrets
  resolve as the CLI resolves them: the process environment and the `trestle`
  keychain service.
- **Minted secrets.** A module can declare a secret as minted: one value per
  environment that the launcher generates once and keeps. Two do:
  mangrove's server (`unity-catalog-rs`) encrypts the credentials it stores
  with a key from `UC_SERVER_KEK`, and MLflow seals its AI Gateway's provider
  keys with a key derived from `MLFLOW_CRYPTO_KEK_PASSPHRASE`. Both refuse to
  start without theirs. When the revision's plan declares one and neither the
  environment nor the workspace has a variable for it, Start mints it: 32
  random bytes from the OS, base64, in the keychain under `<environment
  id>/<VAR>` (as `trestle var set --secret` would). The environment's vars
  keep only a reference to it, every revision of the environment uses the
  same value, and `trestle up` from the CLI finds it too. If the keychain
  refuses, Start fails; there is no fallback. The OSS server
  (`unity-catalog-oss`) needs no key, and a Unity Catalog on a Neon Function
  has its key on the deployment.
  - **MLflow from before trestle 0.0.22** ran under a passphrase fixed in its
    template, `lakehouse-dev-kek`. An environment whose MLflow already ran
    (one of its instances has a rendered tree from a revision with MLflow)
    gets that value as its minted passphrase, so the keys MLflow stored still
    decrypt. To rotate it: delete the gateway's secrets in the MLflow UI, stop
    the environment, remove the reference (`trestle var unset <ws>/<env>
    MLFLOW_CRYPTO_KEK_PASSPHRASE`), and start again. That Start mints a random
    passphrase, and the gateway's init job stores the keys again under it.
- **Required secrets.** A plan also declares the secrets its modules can't run
  without (`DATABRICKS_HOST` for `mlflow.upstream=databricks`, say). The runtime
  resolves each one from the variables, else from the process environment by
  name. When one is set nowhere, Start fails before Compose runs and names it.
- A failed start stays wanted running, as `trestle up` leaves it: whatever came
  up stays to inspect, and **Stop** cleans up. The header says it failed, a
  toast says why, and the bottom panel's **Run** view (⌘J) has compose's output.
- **Stop** is `docker compose down` with the data volumes kept; the slot stays
  with the instance.
- The status (each container's state, health and ports) comes from Docker
  (`compose ps`), refreshed on Docker's events: shell-core follows `docker
  events` for every instance project (the CLI's too) and the app emits
  `instance-changed`. It also refreshes at once after a commit, and every 30 s
  as a fallback. A one-shot job that exited 0 (the gateway's certificates)
  counts as done.
- The bottom panel's **Logs** tab follows the shown revision's containers (`docker compose logs
  --follow`, the last 500 lines per service first), streamed over a `Channel`
  in batches into a ring buffer of the newest 5,000 lines. The service chips
  filter it, and Follow keeps the newest line in view (scrolling up turns it
  off). Leaving the tab stops the stream.
- **Side by side** is the default: each revision runs as its own instance on
  its own slot. While an older revision runs, the header offers **Start #<head>**
  next to it. With several running, a revision switch (`#1`, `#2`) picks the one
  the header, the canvas, the Logs and the header's Apps show; **Stop #<n>** stops that one and
  **Stop all** every revision. The Run panel lists each with its gateway (a link to its apps) and
  containers. **One at a time** (Settings → General, per workspace) makes Start stop
  the workspace's other instances first (data volumes kept).
- **At launch** the app reconciles with Docker (olai-stack-runtime's
  `reconcile`). A running instance stays wanted running (adopted). One wanted
  running whose project is gone is marked stopped. An instance-shaped project
  the store doesn't know is reported and left alone. The app shows what was
  found (Settings → General, until dismissed; a notice at launch points there). If the
  last current instance still runs, it is current again.
- **On quit** instances keep running, as the CLI's do. With **Stop on quit**,
  the app first stops the instances it started in that session.
- These settings live in the app's own `settings.json` (in its data directory,
  or `LUMINESCENT_SETTINGS_DIR`), not in the shared state.

`LUMINESCENT_SMOKE_DOCKER=1 just desktop-smoke` also runs `local/dev` on Docker
through its tab's header, in two launches of the app on one throwaway state:

1. **start**: revision 1, then revision 2 next to it, each on its own slot, each
   with a catalog of its own. The Catalog app shows each revision's catalog
   and never the other's. The Logs panel follows #1's containers. With
   `LUMINESCENT_SMOKE_LINEAGE=1`, revision 2 adds headwaters, and the Lineage
   app reads a dataset posted to it at `<gateway>/lineage/`. With
   `LUMINESCENT_SMOKE_UIS=1`, revision 1 adds MLflow, and its child webview
   must sit on the instance's host label over the Apps view's area. The app
   exits with both running.
2. Between the launches, the script checks that #2's `db` container has the env
   trestle rendered and its password secret (`docker exec … env`).
3. **resume**: the relaunch adopts both revisions (reconcile) and makes #2
   current again. Picking #1 in the revision switch shows #1's catalog only.
   **Stop all** stops both. Afterwards, no container or network of either project is left.

The script removes both projects' volumes before the first launch and on its way
out. Stop keeps an environment's data volumes, but the smoke's are throwaway, and
one kept from an older image (Postgres 17's data under Postgres 18) would break
the next run.

CI runs it on Linux (the `check (desktop, linux)` job), with the lineage checks
when the runner can pull headwaters' image from ghcr.io. The macOS runners have
no Docker. Locally, `LUMINESCENT_SMOKE_KNOBS=module.knob=value,…` points modules
at local images when a registry can't be reached, e.g.
`LUMINESCENT_SMOKE_KNOBS=unity-catalog-oss.image=unitycatalog/unitycatalog:v0.7.0`.
It refuses to run while containers of `local_dev_r1` or `local_dev_r2` exist.
The data volumes are kept, as Stop keeps them.

Before a demo, on the machine that runs it:

- Pull the images first. A Docker daemon behind a TLS-intercepting proxy can't
  pull from Docker Hub (`x509: certificate signed by unknown authority`) until
  the proxy's CA is trusted inside its VM.
- Free the host ports slot 0 uses (Postgres on 5432 among them), or the start
  is refused with the port and its owner; a second running instance takes the
  next slot.
- Colima and Docker Desktop share the home directory with their VM, not `/tmp`:
  a `TRESTLE_HOME` outside it fails on bind mounts.

## Services

The Apps view's Catalog and Lineage run the service UIs in the shell:
mangrove's Unity Catalog browser (`@open-lakehouse/unity-catalog-ui`, service
`unity-catalog`: the OSS server, `unity-catalog-oss`, or mangrove's,
`unity-catalog-rs`) and headwaters' lineage browser (`@open-lakehouse/lineage-ui`,
module `headwaters`). `src/integrations/bind.ts` points their clients at the
host's bindings, so every request goes through the Rust proxy, and the paths
are the services' own (`/api/2.1/unity-catalog/…`, `/headwaters.read.v1.ReadService/…`)
under the binding's root, where the service serves itself: a Neon Function's
URL, or on a local instance the gateway listener plus the base path trestle's
plan gives the service. That base path is empty for Unity Catalog and `/lineage`
for headwaters, which runs relocated there so its bundled UI's links resolve
behind Envoy. The function serves at its root and needs no base path.

**The Apps view** (`src/shell/apps.tsx`) lists every instance wanted running,
by environment and newest revision first, with its state and placement. The
selected instance is the one picked, else the current environment's, else the
first one up. An instance's apps are Catalog and Lineage where its revision
serves the module (a container of one of its services, by the plan, or a
function on its branch), then its UIs with a session of their own
(`run_service_uis`, below). They are listed once it is up: a starting instance
says so, and with nothing running the view says that and leads back to the
editor. An environment's header offers **Apps** on the instance it shows (the
revision switch's pick), and ⌘K each app of each instance. The editor tabs stay
mounted, hidden, while the view is open, so the diagram keeps its state.

**The current environment** is a running instance: the one Start started last,
or the one whose Catalog or Lineage is open in Apps. Opening one makes its
instance current (after a restart, for an instance the CLI started, or for
another instance picked in the view), and it shows no service until the switch
is done, so another environment's data never renders under it. There is one
current environment (`LakehouseHost.environment`), so the apps of two
instances can't show side by side: that, or a mesh of environments, would need
bindings per view. The directory then binds what its gateway exposes,
each module to its root on the gateway listener that serves it (`unity-catalog`
at `http://localhost:<gateway port>/`, `headwaters` at `…/lineage/`, RustFS at
its S3 listener). `environment-changed` carries the new environment: the UI rebinds
first, then clears its query cache and remounts the views on the new key.
Stopping the current environment leaves none, and nothing bound.

**On a Neon branch.** A module can run as a Neon Function instead of in compose
(the inspector's hosted choice, `vendor = "neon-function"`; trestle's
`docs/multi-env.md`), and local modules can use a branch's Postgres or object
storage. Such a revision runs on its environment's **Neon target**: a branch of
a project. The project is the workspace's (`trestle ws set-neon <ws> --project
<id>`, else `LUMINESCENT_NEON_PROJECT`, else the project `neon.ts` deploys to,
`cool-poetry-28343550`) unless the environment picks its own, so a dev and a
prod environment can run side by side on different branches or projects.

- **The header shows the target** of an environment whose head revision needs
  Neon (`neon_needs`, the plan's `neon_requirements()`), or whose draft puts
  anything there (a module as a Neon Function, the branch's Postgres or
  storage), running or not, as its **Neon** detail: `demo` (copyable),
  `luminescent-us / main`, or `no branch yet`. So the branch can be picked
  while the draft is edited, before a revision needs one. **☁ Pick a Neon
  branch** (or **Change branch** once there is one) picks the target (while the
  environment is stopped): a project of the organizations the Neon CLI is
  signed in to (`neon orgs list`, `neon projects list --org-id`), and an
  existing branch, a new one made at the next Start, or one made on the spot
  under a name of your own ("Create now", `neon_create_branch`). A new branch
  comes from the workspace's parent (`ws set-neon --parent`, else the project's
  default), with its functions, lives 12 hours, a day or 7 days, and is owned
  by trestle. Targets live in the app's `settings.json`, by environment.
- **Start** binds the target without asking; a new branch then becomes the
  target, so later starts reuse it. Without a target, Start opens the picker
  first. A revision that needs Neon refuses to start unbound.
- Binding is `trestle up --neon-branch`'s (shell-core's `neon`, on
  olai-stack-runtime's `bind`): the branch must run every function the plan
  needs, missing databases and buckets are created, and a storage credential is
  minted for the instance when it uses object storage (the secret in the
  keychain). The instance stays bound, so the CLI sees the same binding.
- Nothing of a function-hosted module runs locally, but the gateway (Envoy)
  does, so an environment on Neon needs Docker too. The gateway fronts the
  functions as it fronts local services, over TLS to their invocation URLs
  (trestle's `docs/multi-env.md`), so the environment keeps one origin. The
  header shows the branch as its Neon detail, next to `local slot 0`; the tree
  row shows `☁ demo` for an instance without a local slot. The
  editor's graph draws the function's routes from the gateway and its uses of
  the branch's Postgres.
- As the current environment, an instance binds its gateway's services as
  `local` and the branch's functions as `remote`, each under the key of the
  module the plan runs it for (`NeonRequirements.functions[].module`):
  `unitycatalog` runs `unity-catalog-rs`, so it binds as `unity-catalog`; a slug
  the plan doesn't run keeps its own name. A function the gateway fronts is
  reached at its gateway root (headwaters at `…/lineage/`); one it doesn't (a
  URL with a path) at its invocation URL (`neon functions list`). Its Catalog
  and Lineage apps then show the branch's data.
- **Settings → Neon** lists the workspace project's branches, and makes a new one
  on the spot. Those trestle made can be reset to their parent or deleted; a delete is refused while an instance runs
  on the branch, and unbinds the stopped ones (their credentials revoked).

The proxy reaches the
functions over rustls with the platform verifier, so a CA the system trusts
(a TLS-intercepting proxy's) is trusted too.

`just desktop-demo-seed <branch>` (`scripts/demo-seed.ts`) gives a branch
something to show. It finds the branch's function URLs with `neon functions
list` and, over the functions' own APIs, creates the Unity Catalog catalog `demo` (schemas `raw` and `sales`, five
EXTERNAL Delta tables with columns) and posts OpenLineage runs to headwaters:
five jobs in `demo-pipelines` that connect the landing files, the `raw` tables
and the `sales` tables into one graph. The server wants a catalog's storage root
and every external table inside a registered external location, so the script
registers a placeholder credential (`demo_storage`) and an external location on
`s3://luminescent-demo` first. Nothing is ever read from there. A rerun leaves
whatever already exists alone.
Unity Catalog takes the `unity-catalog` token of `LUMINESCENT_SERVICE_TOKENS`
(below); headwaters is anonymous for now ([auth.md](auth.md)).

**MLflow, Jaeger, marimo and Omnigent** keep a cookie session, which an iframe
under the app's origin can't hold (see the spike findings below). Each shows in
a top-level webview on the instance's own host name, embedded in the app or in a
window of its own:
`http://<host label>.localhost:<gateway port><base path>/`, e.g.
`http://local-dev-r1.localhost:9080/mlflow/` (the label is trestle's
`<workspace>-<env>-r<revision>`). Cookies ignore ports, so the label is what
keeps two instances' sessions apart. Envoy's shared listener routes on the path
for any host, and `*.localhost` resolves to the loopback address. A window is
a page of the service, not of the app: a remote origin, which Tauri grants IPC
access only when a capability lists its URL under `remote`, and none does.
Omnigent runs single-user locally, which trusts only a loopback `Origin` for
uploads (submitting a prompt) and WebSockets. trestle's module therefore allows
`http://*.localhost:<gateway port>` (`OMNIGENT_WS_ALLOWED_ORIGINS`, since
olai-stack-topology 0.0.23), so the host-label page passes the check.

**`EmbeddedServiceUi`** (`src/shell/EmbeddedServiceUi.tsx`) shows one of an
instance's UIs (`run_service_uis`) wherever it is mounted: give it the instance
and the UI, and it owns the rest. The page is a **child webview of the main
window** (`Window::add_child`, tauri's `unstable` feature), not a frame: a
browsing context of its own, as a window is. The component keeps it over its own
area (`embed_service_ui`, in CSS pixels, each frame the area changes) and hides
it when it unmounts. A native view paints over the whole document, so it also
hides whenever something would be painted over: a modal (Radix marks everything
outside it `aria-hidden`, which covers the ⌘K palette and dialogs) or a menu or
list over its area. Tooltips and toasts don't hide it (a hover shouldn't blank
the page), so one over the page isn't seen. Keys go to the focused webview: with
the page focused, ⌘K is the page's. **Pop out** moves the UI to a window of its
own (`open_service_window`) and **Show here** brings it back. A UI shows in one
place at a time, so either one closes the other, and which UIs are popped out
outlives a remount. On Linux it doesn't embed: there tauri packs a child webview
into the window's `gtk::Box` next to the main one, where it can be neither placed
nor sized (`service_embeds_supported` says so), so the component offers the
UI's window only. A window is focused when opened again. Windows and embeds
close when their instance stops, and the embeds also close when the main page
(re)loads.

The Apps view mounts it for each running instance's UIs, next to Catalog and
Lineage. Going back to the editor unmounts it, which hides the page.

**Switching environments never leaks data.** The service views sit under
`EnvironmentBoundary` (Seams): a cache per environment key and a remount on it.
A result cached for one instance, or a response still in flight from it, never
renders under the next (`EnvironmentBoundary.test.tsx`).

`LUMINESCENT_SERVICES=module=url[,module=url…]` binds modules by hand, ahead of
the current environment's. A loopback URL is `local`, any other `remote`:

```bash
LUMINESCENT_SERVICES=unity-catalog=https://<branch>-unitycatalog.compute.<region>.aws.neon.tech,headwaters=http://localhost:9080 \
  just desktop-dev
```

The deployed Unity Catalog function authenticates its callers
([auth.md](auth.md)), and the shell doesn't sign in with Neon Auth yet. Until it
does, `LUMINESCENT_SERVICE_TOKENS=module=token[,module=token…]` gives a module its
bearer token: the proxy sends it as `Authorization` to the module's remote
upstreams only, whether bound by hand or by a running instance (a function the
gateway fronts too, for the gateway to forward), never to a local one, and the
UI never sees it. Use a service account's API key on the branch:

```bash
just admin <branch> service-account create desktop --description "the desktop shell"
just admin <branch> api-key issue desktop --expires-in-days 30   # prints the key once
just admin <branch> grant add service-account:desktop select UnityCatalog::Catalog <catalog>
LUMINESCENT_SERVICE_TOKENS=unity-catalog=olk_… just desktop-dev
```

The account sees what its grants allow ([auth.md](auth.md#unity-catalog)):
`select` on a catalog to browse and query it, more to change it.

The views show an instance's services, so a module bound by hand is reached
through the proxy only, not shown in Apps. `just desktop-smoke` checks that a
stopped environment offers no apps (and the Apps view says nothing runs), and that hand-bound
services answer through the proxy with `SMOKE_SERVICES` (the same syntax), e.g.
`SMOKE_SERVICES=unity-catalog=http://localhost:9080,headwaters=http://localhost:9080`.
`SMOKE_NEON_BRANCH=<branch> just desktop-smoke` also points `neon-demo` (Unity
Catalog and headwaters as Neon Functions) at the branch in its header, starts it,
and checks that its Catalog app shows `demo` (and the tables of `demo.sales`) and
its Lineage app `demo.sales.daily_revenue` and `build_daily_revenue`. Seed the
branch first. This needs the Neon CLI, signed in, and Docker, so it doesn't run
in CI.

## Query

SQL runs in the host, in [hydrofoil](https://github.com/open-lakehouse/hydrofoil)'s
DataFusion engine (Unity Catalog and Delta), embedded in `src-tauri`
(`src/query.rs`). There are no ports, and the webview never gets a URL or a
credential. The UI's `QueryRunner` (`@open-lakehouse/query-runner`,
`src/host/query.ts`) calls `query_run`, and the rows come back over a `Channel`,
one chunk per record batch: its row count (8 bytes, little endian), then its Arrow
IPC stream. `query_cancel` stops a run, and so does the UI's `AbortSignal` or
dropping the iterator.

The engine is behind `src-tauri`'s `query` feature, which is off by default:
DataFusion, delta-rs and their kin are most of a build's time and `target/`, and
most work doesn't touch them. A build without it keeps the commands (the bindings
are the same), but `query_run` fails with `unavailable`, which the Query app
shows. `just desktop-dev query` builds it in; `just desktop-build`, the smoke
test and `just desktop-check` (so CI) always do. Working on `src-tauri/src/query/` in an
editor, turn the feature on for rust-analyzer (`rust-analyzer.cargo.features =
["query"]`), or its code reads as disabled.

- **One host, an engine per instance queried.** A query names its instance, or
  runs against the current environment's when it names none. Each instance's
  engine is bound to that instance's Unity Catalog, found the same way the proxy
  finds it: bound by hand first, then its gateway root or its branch's function
  (`CurrentDirectory::resolve_instance`). It also carries the same credential
  (`LUMINESCENT_SERVICE_TOKENS`), which goes to a remote catalog only. So two
  instances side by side query their own catalogs, current or not.
- **Built on the first query, dropped when the instance stops.** Each engine runs
  a CPU runtime of its own, so building one on every start would be a waste. Every
  query gets a session of its own, so vended storage credentials never outlive
  it. At most 4 queries run at once, and more wait. A query returns 1,000 rows
  unless it asks for more, and never more than 10,000.
- **What it runs:** reads. The engine refuses DataFusion's own DDL and DML, and
  Unity Catalog decides what the caller may read. On a branch, the app's service
  account needs usage of the catalog and schema and `SELECT` to plan a query, and
  an explicit `EXTERNAL_USE_SCHEMA` to read the data, because the engine is
  external compute ([auth.md](auth.md#credential-vending-on-neon)):

  ```bash
  just admin <branch> grant add service-account:desktop USE_CATALOG UnityCatalog::Catalog <catalog>
  just admin <branch> grant add service-account:desktop USE_SCHEMA UnityCatalog::Schema <catalog>.<schema>
  just admin <branch> grant add service-account:desktop SELECT UnityCatalog::Schema <catalog>.<schema>
  just admin <branch> grant add service-account:desktop EXTERNAL_USE_SCHEMA UnityCatalog::Schema <catalog>.<schema>
  ```

  The errors say what is missing: `unauthenticated` for a missing or bad token,
  `forbidden` for a missing grant, `invalid` when the SQL doesn't plan.
- **Reading a branch's Neon storage is untested.** UC REST hands out no S3
  endpoint ([auth.md](auth.md), "Client setup"), and nothing here sets one yet.
  Local instances read their own storage.
- **The Query app** (Apps, on each instance that serves Unity Catalog, and
  `Apps: Query on …` in ⌘K): mangrove's Monaco editor (`@open-lakehouse/editor`)
  above arrow-grid's `DataGrid`. Cmd/Ctrl+Enter runs the selection, or all of it.
  It names its instance on every run, so switching the current environment can't
  send a query to another one. Completion comes from the catalog through the
  instance's binding (`ucCatalogProvider`). The text stays with each instance until
  the app quits.
- **The Catalog app** gets SQL from it too: its table preview
  (`VITE_ENABLE_PREVIEW`, which `vite.config.ts` turns on) and SQL in its volume
  editor. Both run against the current environment, which is the instance the
  Apps view selected.

hydrofoil is unpublished, so `apps/desktop/Cargo.toml` pins it by `rev`, a full
SHA on its `main`. Its own git pin comes along: the roeap/delta-rs fork
(deltalake-core); breakwater, datafusion-openlineage and mangrove's crates come
from crates.io. To move it, set the `rev`, then let any Cargo command (`cargo
metadata`) relock just what changed; `cargo update -p hydrofoil` re-resolves
unrelated crates too. hydrofoil moves onto sibling releases as they come out, so
expect them behind the crates.io proxy: patch them as the
[upstream pins](../AGENTS.md#upstream-pins) are. The fork pins `brotli-decompressor =6.0.0`,
which brotli 9.1 (through tauri-utils) can't use, so the lock holds brotli
9.0.0. A plain `cargo update` brings that conflict back; restore it with
`cargo update -p brotli --precise 9.0.0`.

## Omnigent client

`crates/omnigent-client` (in the root workspace) is the Rust client of
[Omnigent](https://github.com/omnigent-ai/omnigent)'s REST API, pinned to
v0.22.0. It is the one Omnigent client, shared by the desktop, which builds it
by path from its own workspace, and the agentic factory
([docs/factory](factory/README.md)), whose crates are in the same root
workspace. The factory reuses it for its Omnigent driver, which the reconciler
brings; the factory crates don't depend on it yet.
It is unpublished (`publish = false`) and released with nothing. The pin
follows Omnigent's releases; trestle's image may lag a release behind, which
the client tolerates because every field a newer version adds is optional.
The desktop's UI never calls Omnigent itself; it goes through the shell's
commands.

- **Generated from protos.** `proto/omnigent/v1/` holds hand-written protos with
  `google.api.http` routes, for the subset of the API the desktop and the factory call:
  sessions (create, list, get, update), events (`message`, `interrupt`,
  `function_call_output`), elicitations, a session's pull requests, agents and
  hosts. Loose payloads are
  `google.protobuf.Struct`. trestle generates the client from them, client only
  (`trestle.yaml`): snake_case field names on the wire, int64 as JSON numbers,
  unknown fields and enum values ignored. Its plain reqwest transport doesn't
  retry, so a retried `POST …/events` can't send a message twice.
  `just omnigent-codegen` regenerates `src/gen/`, which is committed. Edit the
  protos, never `src/gen/`.
- **Written by hand:** the SSE stream of a session (`stream.rs`, `sse.rs`; trestle
  would buffer a streaming body whole), its typed events (`events.rs`), and the
  fold of events, or of a session snapshot, into chat messages (`fold.rs`, a port
  of the Python SDK's block stream).
- **What the factory's sessions use.** A session is created with its labels
  (`factory.*`), a git worktree (`git: {branch_name, base_branch}`), where it
  runs (`host_type`, `sandbox_provider`, `sandbox_profile_id`) and overrides
  (`model_override`, `harness_override`, `reasoning_effort`). The snapshot has
  `total_cost_usd` (unset while unpriced), `last_task_error`, `harness` and
  `git_branch`.
  - **Finding a session by its labels** is `find_sessions`. Omnigent has no label
    filter on `GET /v1/sessions` (v0.22), so it pages through every session
    (`kind=any`) and filters on the client: a scan.
  - **Detected pull requests** aren't on the snapshot. `pull_requests` pages
    `GET /v1/sessions/{id}/pull-requests`: each PR's `state` (`OPEN`, `MERGED`,
    `CLOSED`) and `relationship` (`created`, `worked_on`, `attached`, `inferred`),
    and `synced`, without which an empty list means "not known yet". A list item
    carries the latest PR only (`pull_request`).
- **The caller's transport and base URL.** `OmnigentClient::new(reqwest::Client,
  base_url)` keeps a gateway prefix (`…/omnigent/`).
- **Tests:** routes against a fake server; SSE chunking (every cut of a fixture);
  the fold over `fixtures/grant-turn.sse` and its snapshot, a grant turn
  recorded from v0.21.0 on trestle's `agents-rs` scenario (the request, the
  form's outcome as an `<mcp-app-context>` message, the agent's confirmation;
  [Agent](#agent) has how), whose assumptions were re-checked against v0.22.0's
  source when the pin moved; and a contract test
  that diffs the protos' routes and fields against the server's OpenAPI document
  of v0.22.0 (`fixtures/openapi-v0.22.0.json`). What that document leaves out
  comes from the source at the same tag, parsed and never run
  (`fixtures/source-v0.22.0.json`, `scripts/extract-source.py`): the bodies
  `POST /v1/sessions` parses by hand (`SessionCreateRequest`), and the routes
  hidden from the document (`/events`, `/elicitations/{id}/resolve`) with
  their bodies. To move the pin, run `just omnigent-openapi <tag>` (both
  fixtures), set `PIN` in `tests/contract.rs`, delete the old ones, then fix
  what the contract test reports.
- **Drift.** The `omnigent-drift` workflow runs the contract test daily against
  Omnigent's newest release (`just omnigent-contract [tag]` does the same
  locally, `latest` by default). When it fails, it opens an issue for that
  release (labelled `omnigent-drift`, one per release) with the test's output:
  a field or route the client uses changed. Fix the protos and move the pin as
  above. When a newer release passes, the run's summary says to move the pin,
  and no issue is opened.
- **Tool names.** Omnigent names an MCP server's tool `<server>__<tool>`
  (`uc__open_grant_form` for a server the agent's YAML calls `uc`), and that is
  what the stream carries, under the claude-sdk harness too (the recorded turn).
  The harness itself sees them behind Omnigent's in-process MCP server, as
  `mcp__omnigent__<server>__<tool>`; `ToolName::parse` takes either form, in
  case that name ever reaches the stream.
- **Agents.** `GET /v1/agents` lists the server's agents only. An agent
  installed with `omnigent agent add` (`just agent-install`) is the caller's,
  listed with `scope=user`; shell-core looks there first.

### Agent sessions

The Agent app drives an Omnigent session through the shell's commands
(`src-tauri/src/agent.rs`, the logic in shell-core's `agent`). Each command
names its instance, and reaches that instance's `omnigent` module the way the
query engine reaches Unity Catalog: bound by hand first, then its gateway root,
with the app's token only for a remote upstream. Locally Omnigent runs without
auth (`OMNIGENT_AUTH_ENABLED=0`).

- `agent_status(instance)`: whether the `lakehouse` agent is installed and a host
  is online. An Omnigent that doesn't answer is reported, not raised.
- `session_create(instance, title)`: a session of `lakehouse` on the online host,
  in `/workspace` (the trestle host's working directory).
- `session_subscribe(instance, sessionId, channel)`: the session's messages. The
  stream has no replay, so this opens the stream first, then reads the snapshot,
  and resolves once both are in: subscribe before sending. The channel gets a
  `snapshot`, then an `upsert` per message that changes, plus `elicitations`
  and `status`. When the stream drops it gets `reconnecting`, and after the
  reconnect a new `snapshot` that replaces what the UI holds.
  `session_unsubscribe` ends it, and so does a webview that goes away.
- `session_send(instance, sessionId, text)`: posts the message, then waits for
  the stream's next event, 1.5 s at most (`observed` says whether it came).
  `session_interrupt` stops the turn.

The messages are chatcn's `ChatcnMessage` (the AI SDK's `UIMessage`):
`{id, role, parts}`, with `text`, `reasoning` and `dynamic-tool` parts.
Omnigent's tools are only known at run time, so every call is a `dynamic-tool`
with its `toolName`, `input`, `output` or `errorText`, a `ToolState`, and an
`approval` when a policy asks for one.

## Agent

The Agent app (`src/integrations/agent/`) is a chat with the `lakehouse` agent
([agents/lakehouse](../agents/lakehouse/README.md)) on an instance's Omnigent,
in which mangrove's grant form renders inline as an MCP App. Omnigent runs the
session; the desktop renders it. The Apps view offers it on an instance that
serves both `omnigent` and `uc-mcp` (trestle's `agents-rs` scenario has both),
and loads it lazily, the MCP SDK with it.

```
 AgentView ── session commands ──▶ Omnigent ──▶ the agent ── uc (MCP) ──┐
     │   (snapshot, upserts, status)                                    ▼
     └─ AgentAppPart ── the desktop's own MCP connection ──▶ uc mcp serve (uc-mcp)
            │  re-runs the read-only tool, reads the ui:// app, app-only tools
            └─ McpApp (mcp-app:// sandbox) ── ui/update-model-context ──▶ context.ts ──▶ session
```

- **Why a side channel.** Omnigent flattens an MCP result to text, so
  `structuredContent`, `_meta` and embedded resources never reach a client, and
  it can't read `ui://` resources (omnigent-ai/omnigent#8389 was closed as not
  planned). So the desktop holds its own connection to the same `uc mcp serve`
  (`src/integrations/mcp/connection.ts`, at `/uc-mcp/mcp` under the module's
  gateway root). It declares the MCP Apps extension, so the server lists the
  app-only tools (`apply_grants`, `whoami`) to it, and never to the agent.
- **Which calls are apps** (`correlate.ts`). A call to the `uc` server's tool
  (`uc__<tool>`, or `mcp__omnigent__uc__<tool>`: omnigent-client's
  `ToolName::parse` rules) whose tool, on the desktop's connection, has
  `_meta.ui.resourceUri` and `readOnlyHint`. Any other call is a plain tool card.
- **The app** (`AgentAppPart.tsx`). Once the agent's call has finished, the
  desktop runs the tool again with the agent's arguments, once per call, for
  the full `CallToolResult`, reads the app's View, and renders `McpApp` with
  them as tool input and result. The re-run is why only read-only tools match:
  `open_grant_form` changes nothing. The app's own calls go through the
  desktop's connection (`from: "app"`, refused for model-only tools), and its
  links through `open_external` (`http`/`https` only, the system browser).
- **What the app tells the agent** (`context.ts`). `ui/message` is posted to
  the session as it comes. `ui/update-model-context` keeps the latest per app.
  When the app's call waits on the user (`structuredContent.status` is
  `awaiting_user`, as `open_grant_form`'s is), it is posted as soon as the
  session is idle, as `<mcp-app-context tool="…" call_id="…">…</mcp-app-context>`
  (the app's text escaped, so it can't close the element). Otherwise it goes in
  front of the user's next message. Such messages show as chips, not as the
  user's. The agent's prompt reads them as the form's outcome and never claims a
  grant before one arrives.
- **History.** A call that was already in the session when the view opened it
  (a snapshot, also after a reconnect or a reload) shows collapsed, with "Open
  form again": nothing runs again by itself. The session is kept per instance,
  in `sessionStorage` too; New chat starts another.
- **Notices.** `agent_status` says what's missing: Omnigent not answering, the
  agent not installed (`just agent-install`), or no host online (the
  environment's `omnigent-host` connects to the server through the gateway).
- **Tests.** bun: the correlation tables, the context policy, and `AgentView`
  on a scripted Omnigent (`src/host/fake-agent.ts`) and a fake connection.
  Playwright (`e2e/agent.e2e.ts`): the whole flow on mocked IPC, with
  mangrove's real grants app behind a stand-in `uc mcp serve`
  (`src/host/fake-uc-mcp.ts`): request, tool card, form, submit,
  `apply_grants`, the context post, the confirmation.

### Running it

1. An environment with `unity-catalog-rs`, `omnigent` and `uc-mcp`, and
   usually `mlflow` (trestle's `agents-rs` scenario has all four).
2. Its **Models** (the environment's header, or the first Start asks): where
   the agent's model calls go.
   - **A Databricks profile**: one of the Databricks CLI's profiles
     (`databricks auth login` makes a U2M one). It writes the literal variables
     `DATABRICKS_CONFIG_PROFILE` and `DATABRICKS_HOST`, and the `upstream` knob:
     `mlflow.upstream=databricks` with MLflow in the stack, whose gateway seeds
     `claude-{opus,sonnet,haiku}-5-5` on the workspace's serving endpoints
     (`databricks-claude-*`); without MLflow, `omnigent.upstream=databricks`,
     and Omnigent's host calls the workspace itself. Either way the tokens come
     from the app's [token endpoint](#the-token-endpoint) while the
     environment runs; no token is stored.
   - **Provider keys**: `ANTHROPIC_API_KEY` (and the others) as variables or in
     the app's environment; the dialog says when none is set.
   - The Neon AI Gateway of the environment's branch comes next (#169); the
     dialog shows it disabled.

   The knob is part of a revision: when the draft is the head apart from it,
   choosing commits it; a draft with other changes asks to be committed first.
   The dialog also lists the secrets the head revision requires that nothing
   sets, so a Start doesn't fail on them by surprise. A change applies at the
   next Start.
3. `just agent-install http://localhost:<gateway port>/omnigent`.
4. Apps → the instance → Agent: "grant SELECT on main.default.t to group x"
   (the table has to exist in the environment's Unity Catalog; a fresh one has
   none).

**Omnigent v0.21–v0.22 and the grant form.** Omnigent turns a non-text MCP content
block into a string with `json.dumps(block.model_dump())`
(`omnigent/tools/mcp.py`), which fails on an embedded resource's `uri`
(`TypeError: Object of type AnyUrl is not JSON serializable`). `open_grant_form`
returns its A2UI as one, so the agent gets an error in place of the result,
and says it couldn't open the form, while the desktop, which runs the tool on
its own connection, shows it anyway. mangrove 0.0.23 (mangrove#373) sends the
A2UI resource only to clients that declare the MCP Apps extension, which also
keeps it out of the agent's context, so the grant turn runs on a stock
Omnigent 0.22 (tested with a Databricks profile, with and without MLflow).

## Demo

The demo runs against a Neon branch called `demo`, a child of staging. A child
branch carries the functions, their env (`UC_SERVER_KEK` included) and the
`unitycatalog` database, so it needs no deploy of its own. Create it before the
demo and delete it afterwards; only production and staging are kept.

Before the demo, on the machine that runs it:

1. `neon me` works: the app lists branches as the CLI is signed in. Docker runs
   (the environment's gateway runs locally).
2. Create the branch from staging, and check that it has the functions:
   ```bash
   neon branches create --project-id cool-poetry-28343550 --name demo --parent staging
   neon functions list --project-id cool-poetry-28343550 --branch demo   # unitycatalog, headwaters
   ```
3. Issue the app a Unity Catalog API key on the branch, and export it for the
   seed and the smoke (and, for the `.app` opened from the Finder, for launchd):
   ```bash
   just admin demo service-account create desktop --description "the demo"
   just admin demo api-key issue desktop --expires-in-days 7   # prints the key once
   # The seed creates the catalog, its credential and external location.
   just admin demo grant add service-account:desktop all_privileges UnityCatalog::Metastore metastore
   export LUMINESCENT_SERVICE_TOKENS=unity-catalog=olk_…
   launchctl setenv LUMINESCENT_SERVICE_TOKENS "$LUMINESCENT_SERVICE_TOKENS"
   ```
   A key issued on staging works on `demo` too, if staging's account existed
   before the branch was created.
4. `just desktop-demo-seed demo` seeds it. A rerun reports `exists` for
   everything.
5. `just desktop-build`, then `SMOKE_NEON_BRANCH=demo just desktop-smoke
   "apps/desktop/target/release/bundle/macos/Luminescent Lakehouse.app/Contents/MacOS/luminescent-desktop"`
   passes. The first calls wake the functions (about 2 s on a cold start), so
   run it shortly before the demo.
6. Open the `.app` from the Finder once (right-click → Open: it is unsigned).

Afterwards: `neon branches delete demo --project-id cool-poetry-28343550` (the
key goes with the branch) and `launchctl unsetenv LUMINESCENT_SERVICE_TOKENS`.

The walkthrough (the Neon smoke leaves a `neon-demo` environment, Unity Catalog
and headwaters as Neon Functions, in the smoke's own state; in the demo's, add it
in the editor, or with `trestle env create`):

1. **Workspace**: the `local` workspace and its `dev` environment in the editor;
   the same state `trestle ws list` shows.
2. **Doctor** (Settings → Doctor): Docker, Compose, the daemon, and the Neon and
   Databricks CLIs the app found, from the login shell's `PATH` even when
   started from the Finder. A running environment whose Databricks profile
   can't refresh shows here too (see [The token endpoint](#the-token-endpoint)).
3. **Neon**: Settings → Neon, the project's branches.
4. **`neon-demo`**: its header says where it runs; pick branch `demo` there
   (its Neon detail reads `demo`), then **Start**. Unity Catalog and headwaters show as functions on
   the canvas.
5. **Catalog** tab: catalog `demo`, schemas `raw` and `sales`, a table's columns
   (`sales.daily_revenue`).
6. **Lineage** tab: Datasets lists the `demo.*` tables and the landing files.
   Jobs lists the five `demo-pipelines` jobs. Open `demo.sales.daily_revenue` to
   show its graph back to the landing files.
7. Optional: **Start** `dev`, all local. Its tabs show its (empty) services;
   `neon-demo`'s tabs switch back to the branch.

### The service UIs' packages

mangrove and headwaters publish their UI packages, and `apps/desktop/package.json`
pins their GitHub release tarballs, as it does trestle's (below). That covers
`@open-lakehouse/unity-catalog-ui` and the packages it peers on
(`unity-catalog-client`, `editor`, `query-runner`, `delta-log`,
`volume-files`, `arrow-grid`), plus `@open-lakehouse/lineage-ui` and
`lineage-client`. The pins are tarballs, not registry versions, because the
local registry proxy hides versions less than a week old. For the same reason
the root `overrides` force the transitive specs on them (`arrow-grid`,
`lineage-client`) to the tarballs.

The UI packages pin their peers exactly (`@open-lakehouse/host`, ui-kit, each
other), so bump them together. An upstream release that still peers on an
older host or client installs a second copy, with its own React context and
client registry, and the app never sees that copy. After a bump, `bun.lock`
should hold one copy of each `@open-lakehouse/*` package and of
`monaco-editor`. Vite's `dedupe` guards the shared ones, and tsc checks the
packages' own types.

To try an unreleased upstream change, pack it in its repository (stamped the
way its release stamps it) and point the dependency at the tarball
(`file:/path/to.tgz`). That is for the local run only: never commit such a spec
or a lock resolved from it.

## Security

The CSP (`src-tauri/tauri.conf.json`):

| Directive | Value | Why |
|---|---|---|
| `default-src` | `'self'` | |
| `script-src` | `'self'` | no eval, no wasm (Shiki runs its JS regex engine) |
| `worker-src` | (unset: `script-src`'s `'self'`) | Monaco's workers (editor, JSON, pgsql) are same-origin chunks (`?worker`); none is a `blob:` |
| `style-src` | `'self' 'unsafe-inline'` | React Flow and Radix set inline styles and a `<style>` element |
| `img-src` / `font-src` | `'self' data:` (+ `blob:` for images) | diagram export; no remote fonts |
| `connect-src` | `ipc: http://ipc.localhost` | all service traffic goes through IPC |
| `frame-src` | `mcp-app://localhost http://mcp-app.localhost` | only the MCP App sandbox proxy (below); the UIs with a session are child webviews or windows of their own (Services) |
| `object-src`, `base-uri`, `form-action` | `'none'` | |

The smoke test records `securitypolicyviolation` events from the first script
on and fails on any. Dropping `'unsafe-inline'` from `style-src` makes it fail.

The only capability is `core:default` for the main window; no plugin is
enabled. Shell commands need no capability. Links leave the app through one
command, `open_external`: `http`/`https` only, without credentials, in the
system's browser (`shell_core::external`), for MCP Apps' `ui/open-link`.

### The token endpoint

An environment that takes its models from a Databricks CLI profile (Agent →
Models) runs containers that need workspace tokens, which last about an hour,
and they have neither the CLI nor its token cache. The app serves them the
Databricks SDK's `metadata-service` protocol (`shell_core::token_endpoint`):

| Property | What the endpoint does |
|---|---|
| Listener | One HTTP/1 listener for every instance, on an ephemeral port, opened on the first Start that needs it and closed when the last such instance stops. |
| Bind address | macOS (and Docker Desktop on Linux): `127.0.0.1`, which Docker Desktop, OrbStack and Colima forward `host.docker.internal` to. A Linux engine has no such name (trestle's compose maps none), so the listener binds the default bridge's gateway (`docker network inspect bridge`, usually `172.17.0.1`), the host's address on `docker0`, and the URL names it by address. Neither is reachable from the network. |
| URL | `http://<host>:<port>/<64 hex chars>`: a random path per instance, minted at Start and never stored. It reaches Compose as a secret, through a runtime source (`databricks-profile`) whose binding holds only the profile and its host, so the containers get it by name, and SQLite, the instance directory and the rendered tree never have it. An unknown path is a 404. |
| Request | `GET` with `X-Databricks-Metadata-Version: 1` and `X-Databricks-Host`; without them, a 400. A host that isn't the profile's (compared without a trailing slash, case-insensitively) is a 403, and the CLI isn't asked. |
| Answer | `{access_token, token_type, expires_on}` from `databricks auth token -p <profile> -o json`, which refreshes from the CLI's cache when the token has expired; `expires_on` is the CLI's `expiry` in seconds since the epoch. No token is kept or logged. |
| Can't refresh | When the CLI fails (the refresh token expired or was revoked), a 503. The instance shows "run `databricks auth login -p <profile>`" in the Run panel, and the Doctor lists the profile, until a request succeeds again. |
| Lifetime | Stop drops the instance's path (404 from then on) and its source binding, so `trestle up` from the CLI, which serves no endpoint, still starts the instance. An instance the CLI stops goes too (Docker's events). The endpoint lives in the app: when it quits, the containers' calls fail, and an instance still running at the next launch says to restart it. |

### MCP App sandbox

An MCP App (a `ui://` resource, `text/html;profile=mcp-app`, such as mangrove's
`ui://uc/grants`) is one HTML document with inline scripts. A `srcdoc` frame in
the shell inherits the shell's CSP, which allows neither, so the desktop follows
the MCP Apps double-iframe convention:

1. The shell frames the sandbox proxy, `mcp-app://localhost/sandbox.html`
   (`http://mcp-app.localhost/…` on Windows; `mcpAppSandboxUrl()` in the shell
   host). The `mcp-app` scheme (`src-tauri/src/mcp_app.rs`) serves only
   `sandbox.html` and `sandbox.js` (`src-tauri/assets/mcp-app/`); anything else
   is a 404. The proxy frame is `sandbox="allow-scripts allow-same-origin
   allow-forms"`: same-origin is its own origin, not the shell's.
2. The scheme sends the proxy's CSP as a header (`csp.txt`): `default-src
   'none'`, inline script and style, `connect-src 'none'`, `frame-src 'none'`,
   no forms, no base URI, and `frame-ancestors` the shell's origins
   (`tauri://localhost`, `http://tauri.localhost`, and `http://localhost:4323` in
   debug builds).
3. `sandbox.js` talks only to a parent on that list (`location.ancestorOrigins`),
   announces itself (`ui/notifications/sandbox-proxy-ready`), and on
   `ui/notifications/sandbox-resource-ready` puts the View into an inner
   `<iframe sandbox="allow-scripts allow-forms" srcdoc>` (an opaque origin) with
   a meta CSP built from `_meta.ui.csp`. It relays JSON-RPC both ways, checking
   `event.source` (the parent, or the inner frame) and the parent's origin, and
   never relays `ui/notifications/sandbox-*`.
4. The host side is chatcn's `McpAppFrame` (`src/components/ui/chatcn/`),
   adapted to load the proxy instead of a `srcdoc`, through `McpApp`
   (`src/integrations/mcp/`), and `@modelcontextprotocol/ext-apps`' `AppBridge`.

The inner frame inherits the proxy's CSP, and the View's own can only narrow
it. So `connectDomains` (and `resourceDomains`) declared in `_meta.ui.csp` have
no effect yet: a View has no network. The grants app needs none, since every
Unity Catalog call is an MCP tool call through the host.

The smoke test checks the proxy loads and announces itself, an inline-script
View completes a handshake through it, and a frame on the scheme
(`smoke-probe.html`, served to `--smoke` runs only, with no meta CSP) has no
`__TAURI_INTERNALS__` or `window.ipc`, can reach neither `ipc://localhost` nor
`http://ipc.localhost`, and runs under the header's policy. Playwright
(`e2e/mcp-app.e2e.ts`) runs mangrove's real grants app through the same files,
served on a second origin with the same header (`vite.config.ts`).

Findings (#152), per webview:

- **WKWebView (macOS 26, local `just desktop-e2e`).**
  - A custom-scheme frame loads from the shell under `frame-src mcp-app://localhost`.
    Its origin is `mcp-app://localhost`, and `location.ancestorOrigins` names the
    shell (`tauri://localhost`).
  - The scheme's CSP header is honoured. On the probe page, which has no meta CSP,
    a `fetch` is blocked by `connect-src`, and the violation reports the header's
    policy, `frame-ancestors` included.
  - Neither the proxy frame nor the inner `srcdoc` frame has
    `__TAURI_INTERNALS__` or `window.ipc`. Tauri's IPC script is main-frame only
    there. Both `ipc://localhost` and `http://ipc.localhost` are unreachable from them.
  - An inline-script View in the inner `srcdoc` runs, and completes a JSON-RPC
    round trip through the proxy.
  - A `new Function` blocked inside the probe frame also showed up on the shell's
    own `securitypolicyviolation` listener (as `script-src blocked eval`). So the
    smoke test doesn't probe `eval` in the frame: it would trip the shell's
    "no CSP violations" check.
- **WebKit (Playwright, on the same files served from a second origin).**
  mangrove's real grants app (1 MB, inline module scripts) completes
  `ui/initialize` through the proxy, renders the `open_grant_form` payload, and
  calls a tool (`search_principals`) through the host.
- **WebKitGTK 4.1 (Linux CI, `desktop.yml` under Xvfb): the same.** The frame
  loads on `mcp-app://localhost`, the header's CSP holds, neither frame has IPC,
  and the inline-script View completes its handshake. Playwright's grants-app
  test passes there as well.
- **WebView2 (Windows): untested.** wry adds initialization scripts to subframes
  there whatever the main-frame flag says, so `__TAURI_INTERNALS__` may exist in
  the proxy frame. The smoke check would catch it if Windows builds ever come.

## PATH

An app launched from the Finder or the Dock gets launchd's `PATH`
(`/usr/bin:/bin:/usr/sbin:/sbin`), which holds none of Homebrew, OrbStack,
Docker Desktop or npm's global bin. At startup the shell asks the user's login
shell for its `PATH` (`$SHELL -ilc`, with markers so profile output can't leak
into it, and a 10 s timeout) and puts it ahead of the inherited one. Measured on
macOS 26, the bundled app launched through LaunchServices: with the merge,
`docker` resolves to `/opt/homebrew/bin/docker`; without it, `docker` is not
found. The Neon CLI is found either way, because `olai-stack-runtime`'s
`locate_neon` also checks the Homebrew prefixes.

## Spike findings (#16)

Measured on macOS (WKWebView) with the scaffold and a throwaway app:

- **invoke + Channel streaming** carries the proxy: 64 MiB in about 110 ms
  (~560 MiB/s, release build), with about 10 ms to the first byte of a small
  response. Fast enough for query results and streams; there is no reason for
  the UI to talk to services directly.
- **Iframes get no cookies.** Frames on `http://a.localhost:<port>` and
  `http://b.localhost:<port>` load, but WebKit treats them as third-party
  under the app's `tauri://localhost` origin: neither `Set-Cookie` nor
  `document.cookie` sticks. A custom URI scheme frame (`svc://localhost`) gets
  no cookies either. So a UI with a cookie session (anything behind Authelia's
  forward auth, MLflow's UI) can't run in an iframe.
- **Top-level webviews do get cookies, kept per host label.** A webview window
  navigated to `http://a.localhost:<port>` keeps its cookies across loads, and
  one on `b.localhost` doesn't see them. Cookies ignore ports (RFC 6265), so two
  environments on one host label would share them; the per-instance
  `host_label` from #14 is what keeps them apart.
- **Decision.** Data UIs (Unity Catalog, lineage) run in the shell and reach
  their services through the proxy. UIs that need their own session (MLflow,
  Jaeger, marimo) open in a top-level webview on the instance's
  `<label>.localhost`, never in an iframe; `frame-src` stays `'none'`.
  HTTPS on `*.localhost` (the gateway's own CA) wasn't tested: it needs the CA
  in the keychain, and is a question for #18.
- **Child webviews (#93)** show these UIs inside the main window without a
  frame: each is a webview of its own over part of it, with a window's
  top-level cookie behaviour. The Docker smoke (`LUMINESCENT_SMOKE_UIS=1`)
  checks that MLflow's loads on the instance's host label over its component's
  area, and that a cookie its page sets sticks on that label, isn't seen on
  another, and outlives the webview (Pop out, then Show here builds a new one:
  the cookie store is the app's, shared by all its webviews). On Linux
  (WebKitGTK) it can't work with tauri 2.12: `add_child` packs the webview into
  the window's `gtk::Box` (wry places one only inside a `gtk::Fixed`), so CI's
  run found MLflow's at 0,0 and 1280×410, beside the main webview, instead of
  over the tab. Linux opens the UIs in their own windows, and the smoke checks
  that instead. A native view paints over the DOM, so the component hides it
  under modals and menus rather than relying on stacking.
- **CI build time**: the macOS job runs on pushes to `main` only (pull
  requests and the merge queue run Linux), and prints how long `tauri build`
  takes as a notice. Locally (M-series, cold), the release build takes about a minute.

## Running and testing

From a fresh clone (no local Cargo patch or registry configuration needed), with
`protoc` on the `PATH` (`brew install protobuf`, `apt install protobuf-compiler`):
cedar-policy, under the query engine, compiles `.proto` schemas when it builds.

```bash
bun install                  # at the repo root (desktop-dev/-build also run it, frozen)
just desktop-dev             # Vite on :4323 + the app, hot reload
just desktop-check           # Biome, tsc, bun test, fmt, clippy, cargo test, bindings
just desktop-build           # unsigned .app (macOS) / binary (Linux)
just desktop-e2e [binary]    # smoke test, then Playwright WebKit
```

Tests, from fastest:

- `cargo test` in `apps/desktop`: the doctor against canned `docker` output
  (Docker Desktop, OrbStack, Colima, a stopped daemon, missing tools), the
  proxy's path handling and streaming against a local server, the bindings.
- `bun test`: the fetch adapter on mocked IPC, the views on fakes.
- **The smoke test**, `luminescent-desktop --smoke`: the real binary in the
  real webview (WKWebView, WebKitGTK under Xvfb). The UI checks the shell, the
  workspace editor, the doctor, the unbound-service error, a streamed response
  through the proxy (from `scripts/smoke-echo.ts`) and the CSP, then reports to
  Rust, which exits 0 or 1. Optional checks: `LUMINESCENT_SMOKE_DOCKER=1` (two
  revisions on Docker, a relaunch; in CI on Linux) and `SMOKE_NEON_BRANCH` (a Neon branch's seeded data). This is the only test of IPC, the CSP and the
  bundled assets, and it runs on macOS, where there is no WebDriver for
  WKWebView (tauri-driver supports Linux and Windows only).
- **Playwright** (`e2e/`, WebKit): the UI built with `--mode e2e`, where
  `src/host/ipc-mock.ts` answers the commands with fixtures. For UI flows
  (navigation, the doctor, committing a revision); the later sub-issues extend
  it.

## Releases of the shared packages

The trestle packages (`@open-lakehouse/host`, `env-editor`, `ui-kit`,
`stack-wasm`, and the `olai-stack-*` crates) are pinned to exact `0.0.x`
releases. Fresh releases sit behind the local registry proxies (7 days for
crates, about 24 h for npm), but CI resolves from the public registries.

**Cargo** pins the crates by exact `version` from crates.io as soon as they are
published. Until the local proxy lists a fresh release, build locally under
trestle's crates-proxy patch (`~/code/trestle/scripts/crates-proxy/patch.sh
trestle`, a gitignored `.cargo/config.toml` that points the crates at a trestle
checkout on the release commit), and restore the lock before committing:
`lock-registry.sh`, then `check-lock.sh`. A build under the patch rewrites the
lock to path sources; `lock-registry.sh` puts the crates.io source and checksum
back. Never commit the config or a lock resolved under it.

**npm** must build from a fresh clone with `bun install`, with no local
registry setting, so until the npm proxy lists the current releases it pins the
release artifacts directly:

| Side | Pinned to | Drop when |
|---|---|---|
| npm (`apps/desktop/package.json`) | trestle's GitHub release tarballs, `…/releases/download/<pkg>-v<ver>/open-lakehouse-<pkg>-<ver>.tgz` (the artifacts trestle's CI publishes to npm; `bun.lock` records their sha512) | the npm proxy lists the version, and `env-editor`'s dependencies are fixed (below) |

`env-editor@0.0.1` declares its sibling dependencies as `0.0.0` (trestle
rewrote `workspace:*` before stamping the version), so the root `package.json`
overrides `ui-kit` and `stack-wasm` to the 0.0.1 tarballs as well, until
trestle publishes an `env-editor` with correct dependencies.

To try unreleased trestle changes, use the crates-proxy tools for Cargo (a
gitignored patch, never committed, nor a lock resolved under it) and packed
tarballs for npm. Not `bun link`: a link resolves React from trestle's
`node_modules`, which breaks hooks.

## Not yet

Code signing and notarization (the unsigned `.app` needs a right-click → Open
once), Touch ID for the keychain (needs a signed app), an updater, Windows.
[desktop-release.md](desktop-release.md) is the plan for releases, signing,
updates and Homebrew, waiting on the app's name. The Neon AI Gateway as an
environment's Models ([ai-gateway.md](ai-gateway.md), #169); the Databricks
profile and provider keys are there (Agent → Running it).
