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
- `crates/migrate`: `luminescent-migrate <fn>`, applies a service's migrations.
  The functions never migrate; they only check the schema at startup.
- `justfile` + `scripts/function.sh`: the harness. Recipes take the function
  name; per-function settings (crate, profile, addon name, test command) are in
  `scripts/function.sh`.

To host another service, add a crate under `crates/` following the existing two,
a case in `scripts/function.sh`, and the function to the CI matrices.

## Commands

| Command | Purpose |
|---|---|
| `just test <fn>` | Host-native debug addon + `node --test` (starts Postgres via `just pg-up` if the function needs it) |
| `just build <fn>` | `cargo zigbuild` for `aarch64-unknown-linux-gnu.2.28`, zip, fail over Neon's 10 MiB / 64 MiB limits |
| `just test-arm64 <fn>` | `build`, then the smoke suite in `node:24-bookworm-slim` |
| `just migrate <fn>` | Migrate a Neon branch over `DATABASE_URL_UNPOOLED` |
| `just deploy <fn> [slug]` | `neon functions deploy` (logged-in `neon` CLI) |
| `just trigger-headwaters` | The headwaters projection cron trigger (once per branch) |

## Upstream pins

The service crates are unpublished, so they are git dependencies pinned by
`rev` in `[workspace.dependencies]` of the root `Cargo.toml`. The addons and
`luminescent-migrate` share the pin, so a deploy always migrates the schema the
addon expects. To deploy a newer upstream:

1. Set `rev` to the upstream commit (a full SHA on its `main`).
2. `cargo update -p headwaters` (or `-p opensharing-server`).
3. `just test <fn>` and `just test-arm64 <fn>`. Upstream changes to the
   `embed` API surface here as build errors.
4. After merge: `just migrate <fn>` if the schema changed, then `just deploy <fn>`.

Each addon must use the axum major its upstream builds its router with
(headwaters 0.7, distributary 0.8), so axum is a per-crate dependency for now.

The upstreams pull fresh sibling releases (trestle `olai-*`, mangrove
`olai-uc-*`), which the local crates.io proxy hides for 7 days. Patch them from
local checkouts with trestle's `scripts/crates-proxy/` tools, and restore the
lock with `lock-registry.sh` before committing (see the machine-wide
conventions and trestle's `docs/crates-proxy.md`). Never commit a
`.cargo/config.toml` or a `[patch]`.

## Hard rules

- Never commit to `main`. Stage files by name, never `git add -A` / `git add .`.
- Never commit a `Cargo.lock` resolved under a local patch (`check-lock.sh`).
- Keep `function/index.ts` erasable TypeScript (no enums, namespaces or
  parameter properties). The build emits `index.mjs` with Node's type
  stripping, with no bundler and no npm dependencies.
- The size gate is the binding constraint. Measure `just build <fn>` before
  adding dependencies or features to an addon.

## Testing

The addons can't link as Rust test binaries (the `napi_*` symbols resolve only
inside Node), so they are tested end to end with `node --test` against the
built `dist/`. Changes ship with tests; never delete or comment out a passing
test to green a suite.

## Style

`cargo fmt` + `cargo clippy --workspace --all-targets --all-features -- -D warnings`.
`typos` runs in CI. Trim trailing whitespace; end files with a newline.

## PRs

PRs are squash-merged; the PR title is a conventional commit (`feat` `fix`
`refactor` `docs` `test` `chore` `perf` `build` `ci`, `!` for breaking), linted
by `pr-title.yml`. Scope: the crate directory (`headwaters-neon`,
`opensharing-neon`, `migrate`), or none for the harness. Re-trigger CI with an
empty commit, never by closing and reopening the PR.

## Disk

Each `release-neon*` build is a fat-LTO build of a whole service. Run
`cargo clean` in a worktree before removing it.
