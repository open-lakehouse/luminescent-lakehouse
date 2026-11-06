# Agent Instructions

Instructions for AI coding agents working in this repository. `CLAUDE.md`
imports this file.

## Project map

Deploys open-lakehouse services as Neon Functions. The services live upstream;
this repository holds the Neon hosts and the shared harness:

- `crates/headwaters-neon`: [headwaters](https://github.com/open-lakehouse/headwaters)
  as a napi `cdylib` + `function/index.ts` fetch handler (function `headwaters`)
- `crates/opensharing-neon`: distributary's
  [`opensharing-server`](https://github.com/open-lakehouse/distributary) the same
  way (function `opensharing`)
- `crates/unitycatalog-neon`: mangrove's Unity Catalog server
  ([`olai-uc-server`](https://github.com/open-lakehouse/mangrove)) the same way
  (function `unitycatalog`), in its own `unitycatalog` database on the branch
- `crates/migrate`: `luminescent-migrate <fn|authz>`, applies a service's
  migrations, or those of the shared `authz` permission schema. The functions
  never migrate; they only check the schema at startup. `luminescent-admin`
  manages the `authz` data ([docs/auth.md](docs/auth.md)).
- `apps/web`: the Neon Auth pages (OAuth sign-in and consent) for every Neon
  branch, as one static Vite + React app on Vercel; the branch comes from the
  path (`/b/<endpoint-id>/…`). [docs/web.md](docs/web.md) covers it and the
  Vercel setup.
- `apps/desktop`: the desktop shell for local lakehouse environments, a Tauri
  app with its own Cargo workspace and lock: `src-tauri` (commands, events, CSP)
  over `crates/shell-core` (Tauri-free, plain `cargo test`), and a Vite + React
  UI in which only `src/host/` imports `@tauri-apps/api`.
  [docs/desktop.md](docs/desktop.md) covers it.
- `justfile` + `scripts/function.sh`: the harness. Recipes take the function
  name; per-function settings (crate, profile, addon name, test command) are in
  `scripts/function.sh`.
- `neon.ts`: what is deployed (functions, triggers, per-branch tuning, Neon
  Auth settings). `.github/workflows/neon-deploy.yml` applies it to preview,
  staging and production; [docs/deployment.md](docs/deployment.md) is the
  runbook.
- `release-plz.toml` + the root `Cargo.toml`'s `luminescent-lakehouse`
  placeholder package: releases of the deployed environments (`v<version>`,
  `CHANGELOG.md`). Merging release-plz's release PR promotes the release to
  production through `deploy.yml` (approval still required). Changes to files
  the placeholder doesn't `include`, nor any crate, don't release; add such a
  path to its `include` if a deploy ships it.
- `scripts/auth-setup.ts`: applies `authSetup` from `neon.ts` to a branch's
  Neon Auth after each deploy; [docs/auth.md](docs/auth.md) covers
  authentication.

To host another service, add a crate under `crates/` following the existing two,
a case in `scripts/function.sh`, and the function to the CI matrices. To deploy
it, add it to `neon.ts` too, with its OAuth resource in `authSetup`.

For Neon work, use Neon's agent plugin (skills + MCP server): `just agent-setup`,
or https://neon.com/prompts/agent-setup.md.

## Commands

| Command | Purpose |
|---|---|
| `just test <fn>` | Host-native debug addon + `node --test` (starts Postgres via `just pg-up` if the function needs it) |
| `just build <fn>` | `cargo zigbuild` for `aarch64-unknown-linux-gnu.2.28`, zip, fail over Neon's 10 MiB / 64 MiB limits |
| `just test-arm64 <fn>` | `build`, then the smoke suite in `node:24-bookworm-slim` |
| `just deploy <branch>` | Build the `neon.ts` functions, then `neon deploy` to that Neon branch (migrates first, applies the Neon Auth settings after) |
| `just migrate <fn> [branch]` | Migrate a Neon branch by hand, over its direct connection string |
| `just admin <branch> …` | `luminescent-admin` on a Neon branch: service accounts, API keys, GitHub trust, groups, grants |
| `just test-migrate` | `luminescent-migrate`'s tests against the local Postgres (`just pg-up`) |
| `just web-dev` | `apps/web`'s dev server on `localhost:4322` (needs `apps/web/.env.local`) |
| `just web-check` | `apps/web`: Biome lint, typecheck, `bun test` |
| `just web-e2e <preview-branch>` | Playwright OAuth sign-in through the web app's pages on a `preview/*` branch (CI runs it after each preview deploy) |
| `just web-build` | `apps/web`: typecheck + production build into `apps/web/dist` |
| `just desktop-dev` | The desktop shell against its Vite dev server (`localhost:4323`) |
| `just desktop-check` | `apps/desktop`: Biome, typecheck, `bun test`, fmt, clippy, `cargo test`, bindings current |
| `just desktop-build` | The release build: an unsigned `.app` on macOS, the binary on Linux |
| `just desktop-e2e [binary]` | The in-app smoke test (`--smoke`, the real webview) and Playwright WebKit on mocked IPC |
| `just agent-setup` | Neon CLI + Neon agent plugin |

## Upstream pins

The service crates are unpublished, so they are git dependencies pinned by
`rev` in `[workspace.dependencies]` of the root `Cargo.toml`. The addons and
`luminescent-migrate` share the pin, so a deploy always migrates the schema the
addon expects. To deploy a newer upstream:

1. Set `rev` to the upstream commit (a full SHA on its `main`).
2. `cargo update -p headwaters` (or `-p opensharing-server`, `-p olai-uc-server`).
3. `just test <fn>` and `just test-arm64 <fn>`. Upstream changes to the
   `embed` API surface here as build errors.
4. After merge, `deploy.yml` migrates and deploys staging. The next release
   (merging release-plz's release PR) promotes it to production, or run
   `production.yml` by hand.

Each addon serves its upstream's router, so the workspace `axum` must stay the
major the services build with (0.8 for all three). Bump it together with the pins.

The upstreams pull fresh sibling releases (trestle `olai-*`, mangrove
`olai-uc-*`), which the local crates.io proxy hides for 7 days. Patch them from
local checkouts with trestle's `scripts/crates-proxy/` tools, and restore the
lock with `lock-registry.sh` before committing (see the machine-wide
conventions and trestle's `docs/crates-proxy.md`). Never commit a
`.cargo/config.toml` or a `[patch]`. The pins can sit on different releases of
the same sibling crate (opensharing-server on older trestle `olai-*` than
mangrove's): patch the older set from a second checkout at its release commit,
under renamed keys (`patch.crates-io.olai-store-old = { path = …, package =
"olai-store" }`).

## Hard rules

- Never commit to `main`. Stage files by name, never `git add -A` / `git add .`.
- Never commit a `Cargo.lock` resolved under a local patch (`check-lock.sh`).
- The deploy tooling and `apps/web` (a Bun workspace, sharing the root
  `bun.lock`) are installed with Bun (`bun install`). Never commit a
  `bun.lock` with local registry proxy URLs: the `postinstall` hook strips
  them (`bun run strip-lock-proxy`), and CI fails on any it finds.
- Keep `function/index.ts` erasable TypeScript (no enums, namespaces or
  parameter properties). The build emits `index.mjs` with Node's type
  stripping, with no bundler and no npm dependencies.
- The size gate is the binding constraint. Measure `just build <fn>` before
  adding dependencies or features to an addon.

## Testing

The addons can't link as Rust test binaries (the `napi_*` symbols resolve only
inside Node), so they are tested end to end with `node --test` against the
built `dist/`. `apps/web`'s logic is unit-tested with `bun test` (`just
web-check`), and the OAuth flow end to end with Playwright against a preview
(`just web-e2e`, `apps/web/e2e/`). `apps/desktop` keeps its logic in
`crates/shell-core` (plain `cargo test`) and its UI on fakes (`bun test`); the
real binary runs an in-app smoke test (`--smoke`) and the UI flows run in
Playwright's WebKit (`just desktop-e2e`). Changes ship with tests; never delete or comment out a passing
test to green a suite.

## Style

`cargo fmt` + `cargo clippy --workspace --all-targets --all-features -- -D warnings`.
`apps/web` and `apps/desktop` are formatted and linted by Biome (`bunx biome
check --write` there); `apps/desktop`'s Cargo workspace has its own
`cargo fmt`/`clippy` (run them in `apps/desktop`, or `just desktop-check`).
`apps/desktop/src/host/bindings.ts` is generated by its Rust tests; commit it
with the change that regenerated it.
`typos` runs in CI. Trim trailing whitespace; end files with a newline.

## PRs

PRs are squash-merged; the PR title is a conventional commit (`feat` `fix`
`refactor` `docs` `test` `chore` `perf` `build` `ci`, `!` for breaking), linted
by `pr-title.yml`. Scope: the crate directory (`headwaters-neon`,
`opensharing-neon`, `unitycatalog-neon`, `migrate`), `web` for `apps/web`,
`desktop` for `apps/desktop`, or none for the harness.
Re-trigger CI with an empty commit, never by closing and reopening the PR.

## Disk

Each `release-neon*` build is a fat-LTO build of a whole service. Run
`cargo clean` in a worktree before removing it.
