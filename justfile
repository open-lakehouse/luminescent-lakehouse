# The Neon Function harness. Every service is a napi addon crate under crates/
# plus a dependency-free fetch handler (`function/index.ts`); the recipes take
# the function name (`headwaters`, `opensharing`, `unitycatalog`, `authz`), see
# scripts/function.sh.

set dotenv-load := true

# The Neon project neon.ts deploys to (CI takes it from the NEON_PROJECT_ID
# repository variable). The desktop app's own default is shell-core's
# `neon::DEFAULT_PROJECT`.
neon_project := env("NEON_PROJECT_ID", "cool-poetry-28343550")

# mangrove's sqlx `query!` macros check against DATABASE_URL at compile time
# when it is set, which the test and migrate recipes do (to a database the
# build is about to migrate). Build from the offline query data it ships.
export SQLX_OFFLINE := "true"

# Neon Functions run Node 24 on linux-arm64 glibc; 2.28 is the glibc floor of
# the official Node 24 binaries.
TARGET := "aarch64-unknown-linux-gnu.2.28"
# Neon's deploy limits: 10 MiB compressed, 64 MiB uncompressed.
MAX_ZIP := "10485760"
MAX_UNZIPPED := "67108864"

# Local Postgres for the tests of functions that need one (headwaters,
# unitycatalog, authz).
PG_CONTAINER := "luminescent-postgres"
PG_IMAGE := "postgres:16-alpine"
PG_PORT := "5432"
PG_USER := "postgres"
PG_PASSWORD := "postgres"
PG_DB := "lineage"
UC_DB := "unitycatalog"
DATABASE_URL := "postgres://" + PG_USER + ":" + PG_PASSWORD + "@localhost:" + PG_PORT + "/" + PG_DB

_default:
    @just --list

# Needs `zig` + cargo-zigbuild >= 0.23.
#
# build a function's deployable bundle + zip; fails over Neon's size limits
build fn:
    #!/usr/bin/env bash
    set -euo pipefail
    source scripts/function.sh {{ fn }}
    cargo zigbuild -p "$CRATE" --profile "$PROFILE" --target {{ TARGET }}
    just _dist {{ fn }} "target/aarch64-unknown-linux-gnu/$PROFILE/$LIB.so"
    rm -f "$ZIP"
    (cd "$FN_DIR/dist" && zip -9 -q -r "$OLDPWD/$ZIP" .)
    zipped=$(wc -c < "$ZIP" | tr -d ' ')
    unzipped=$(unzip -l "$ZIP" | tail -1 | awk '{print $1}')
    echo "→ $ZIP: $zipped bytes zipped (limit {{ MAX_ZIP }}), $unzipped unzipped (limit {{ MAX_UNZIPPED }})"
    if [ "$zipped" -ge {{ MAX_ZIP }} ] || [ "$unzipped" -ge {{ MAX_UNZIPPED }} ]; then
        echo "error: bundle exceeds Neon's deploy limits" >&2; exit 1
    fi

# build a function's bundle with a host-native (debug) addon, for local testing
build-host fn:
    #!/usr/bin/env bash
    set -euo pipefail
    source scripts/function.sh {{ fn }}
    cargo build -p "$CRATE"
    lib="target/debug/$LIB.dylib"
    [ -f "$lib" ] || lib="target/debug/$LIB.so"
    just _dist {{ fn }} "$lib"

# Stage `dist/`: the addon under native/, plus index.mjs emitted by Node's
# built-in TypeScript type stripping (Node >= 22.13; no npm deps).
# Install the root's Bun dependencies as bun.lock pins them; fails if the lock
# is out of date.
_bun-install:
    bun install --frozen-lockfile

_dist fn lib:
    #!/usr/bin/env bash
    set -euo pipefail
    source scripts/function.sh {{ fn }}
    rm -rf "$FN_DIR/dist" && mkdir -p "$FN_DIR/dist/native"
    cp "{{ lib }}" "$FN_DIR/dist/native/$NODE"
    FN_DIR="$FN_DIR" node --no-warnings --input-type=module -e "import { readFileSync, writeFileSync } from 'node:fs'; import { stripTypeScriptTypes } from 'node:module'; const d = process.env.FN_DIR; writeFileSync(d + '/dist/index.mjs', stripTypeScriptTypes(readFileSync(d + '/index.ts', 'utf8')));"
    if [ -n "$STAGE" ]; then eval "$STAGE"; fi

# smoke-test a function handler on the host (Postgres-backed ones on the local dev Postgres)
test fn: (build-host fn)
    #!/usr/bin/env bash
    set -euo pipefail
    source scripts/function.sh {{ fn }}
    if [ "$NEEDS_PG" = 1 ]; then
        just PG_CONTAINER={{ PG_CONTAINER }} PG_PORT={{ PG_PORT }} pg-up
        DATABASE_URL="{{ DATABASE_URL }}" cargo run -q -p luminescent-migrate --bin luminescent-migrate -- {{ fn }}
        just PG_CONTAINER={{ PG_CONTAINER }} PG_PORT={{ PG_PORT }} _seed {{ fn }}
        export DATABASE_URL="{{ DATABASE_URL }}"
        # SQL for the tests to run, e.g. to change a user mid-test.
        export TEST_PSQL="docker exec -i {{ PG_CONTAINER }} psql -q -v ON_ERROR_STOP=1 -h 127.0.0.1 -U {{ PG_USER }} -d {{ PG_DB }}"
    fi
    cd "$FN_DIR" && sh -c "$TESTS"

# The callers a function's tests authenticate as (`SEED` in scripts/function.sh):
# the `authz` schema, then the fixture, in the test database.
_seed fn:
    #!/usr/bin/env bash
    set -euo pipefail
    source scripts/function.sh {{ fn }}
    [ -n "$SEED" ] || exit 0
    DATABASE_URL="{{ DATABASE_URL }}" cargo run -q -p luminescent-migrate --bin luminescent-migrate -- authz
    docker exec -i {{ PG_CONTAINER }} psql -q -v ON_ERROR_STOP=1 -h 127.0.0.1 -U {{ PG_USER }} -d {{ PG_DB }} < "$SEED"

# Proves the shipped artifact loads on the real target ABI (linux-arm64 glibc,
# Node 24). Native speed on Apple Silicon; emulated elsewhere.
#
# smoke-test a function's deployable linux-arm64 bundle in a Node 24 container
test-arm64 fn: (build fn)
    #!/usr/bin/env bash
    set -euo pipefail
    source scripts/function.sh {{ fn }}
    env=()
    if [ "$NEEDS_PG" = 1 ]; then
        just PG_CONTAINER={{ PG_CONTAINER }} PG_PORT={{ PG_PORT }} pg-up
        DATABASE_URL="{{ DATABASE_URL }}" cargo run -q -p luminescent-migrate --bin luminescent-migrate -- {{ fn }}
        just PG_CONTAINER={{ PG_CONTAINER }} PG_PORT={{ PG_PORT }} _seed {{ fn }}
        # No TEST_PSQL: the container has no psql, so tests that change data
        # mid-test skip.
        env=(--add-host host.docker.internal:host-gateway
            -e "DATABASE_URL=postgres://{{ PG_USER }}:{{ PG_PASSWORD }}@host.docker.internal:{{ PG_PORT }}/{{ PG_DB }}")
    fi
    docker run --rm --platform linux/arm64 ${env[@]+"${env[@]}"} \
        -v "$PWD/$FN_DIR:/fn:ro" -w /fn \
        node:24-bookworm-slim sh -c "$TESTS"

# A release's CLI for one target (deploy.yml builds each): luminescent-migrate
# and luminescent-admin, with the migrations embedded, so it applies exactly the
# schema the release's functions check. Linux builds go through cargo-zigbuild
# for the functions' glibc 2.28 floor.
#
# build a release's CLI tarball for a target into target/release-assets/
cli-archive version target:
    #!/usr/bin/env bash
    set -euo pipefail
    case "{{ target }}" in
    *-linux-gnu) cargo zigbuild --locked --release -p luminescent-migrate --bins --target "{{ target }}.2.28" ;;
    *) cargo build --locked --release -p luminescent-migrate --bins --target "{{ target }}" ;;
    esac
    name="luminescent-{{ version }}-{{ target }}"
    out=target/release-assets
    rm -rf "$out/$name" && mkdir -p "$out/$name"
    cp "target/{{ target }}/release/luminescent-migrate" "target/{{ target }}/release/luminescent-admin" LICENSE "$out/$name/"
    tar -C "$out" -czf "$out/$name.tar.gz" "$name"
    rm -rf "$out/$name"
    echo "→ $out/$name.tar.gz"

# A release's assets (docs/deployment.md, "Release assets"): each neon.ts
# function's zipped dist/, as `just build` left it (CI: the bundles that
# staged), the web tarball, manifest.json and SHA256SUMS, next to any CLI
# tarballs `cli-archive` already put there.
#
# assemble a release's assets in target/release-assets/
release-assets version:
    #!/usr/bin/env bash
    set -euo pipefail
    out=target/release-assets
    mkdir -p "$out"
    for fn in $(node scripts/deployed.mjs); do
        (
            source scripts/function.sh "$fn"
            addon="$FN_DIR/dist/native/$NODE"
            # Not the host-native debug addon `just test` leaves there.
            if ! file -b "$addon" 2>/dev/null | grep -q 'ELF 64-bit.*aarch64'; then
                echo "error: $addon is not a linux-arm64 addon; run \`just build $fn\` first" >&2
                exit 1
            fi
            zip="$PWD/$out/$fn-{{ version }}-linux-arm64.zip"
            rm -f "$zip"
            (cd "$FN_DIR/dist" && zip -9 -q -r "$zip" .)
        )
    done
    just web-bundle {{ version }}
    bun scripts/manifest.ts {{ version }} "$out"

# Every function in neon.ts, with its triggers and branch tuning, onto one Neon
# branch, after migrating it (scripts/migrate.sh), then its Neon Auth settings
# (scripts/auth-setup.ts).
# CI deploys preview, staging and production this way; see docs/deployment.md.
# Prompts before touching a protected branch (production).
#
# build the bundles neon.ts deploys and apply neon.ts to a Neon branch (needs `bun install`, `neon login`)
deploy branch:
    #!/usr/bin/env bash
    set -euo pipefail
    for fn in $(node scripts/deployed.mjs); do just build "$fn"; done
    scripts/migrate.sh {{ branch }}
    LUMINESCENT_NEON_BRANCH={{ branch }} bunx neon deploy --branch {{ branch }} --no-env-pull
    bun scripts/auth-setup.ts {{ branch }}
    bun scripts/storage-seed.ts {{ branch }}

# Against the local test Postgres (`pg-up`), one database per test.
#
# test luminescent-migrate and luminescent-admin: the authz migrations and operations
test-migrate:
    just PG_CONTAINER={{ PG_CONTAINER }} PG_PORT={{ PG_PORT }} pg-up
    LUMINESCENT_TEST_DATABASE_URL={{ DATABASE_URL }} cargo test -p luminescent-migrate

# Migrations are never applied by a function itself; `just deploy` runs them
# first. This is for migrating by hand, over the branch's *direct* connection
# string (the migrators hold a session advisory lock, which Neon's pooled
# DATABASE_URL can't keep), taken from DATABASE_URL_UNPOOLED or looked up.
#
# apply a function's migrations to a Neon branch
migrate fn branch="":
    #!/usr/bin/env bash
    set -euo pipefail
    url="${DATABASE_URL_UNPOOLED:-}"
    if [ -z "$url" ]; then
        [ -n "{{ branch }}" ] || { echo "error: pass a Neon branch or set DATABASE_URL_UNPOOLED" >&2; exit 1; }
        url=$(bunx neon connection-string "{{ branch }}" --database-name neondb)
    fi
    DATABASE_URL="$url" cargo run -p luminescent-migrate --bin luminescent-migrate -- {{ fn }}

# Service accounts, API keys, GitHub Actions trust, groups and grants; see
# docs/auth.md. Over the branch's direct connection string, like `migrate`, in
# `neon_project` (NEON_PROJECT_ID overrides it).
#
# manage a Neon branch's authz data, e.g. `just admin staging grant list`
admin branch *args:
    #!/usr/bin/env bash
    set -euo pipefail
    url=$(bunx neon connection-string "{{ branch }}" --database-name neondb --project-id {{ quote(neon_project) }})
    # Offline: with DATABASE_URL set, sqlx's macros (mangrove's) would check
    # their queries against this branch at compile time.
    SQLX_OFFLINE=true DATABASE_URL="$url" cargo run -q -p luminescent-migrate --bin luminescent-admin -- {{ args }}

# Neon's agent setup (https://neon.com/prompts/agent-setup.md): the Neon CLI and
# the Neon plugin (skills + MCP server) for your coding agent, user-level. Then
# sign in once with `neon login`. Needs Node >= 22.20.
#
# install the Neon CLI and the Neon agent plugin (default agent: claude-code)
agent-setup agent="claude-code":
    bun add -g neon@latest
    neon plugins --global --agent {{ agent }} -y

# The desktop Agent app's agent (agents/lakehouse/) into a running environment's
# Omnigent, at its gateway root. A reinstall updates the agent in place.
#
# install the lakehouse agent into an environment's Omnigent
agent-install server="http://localhost:9080/omnigent":
    omnigent agent add agents/lakehouse --server {{ server }}

# The Neon Auth web app (apps/web, docs/web.md). Needs `bun install` at the
# root. Open http://localhost:4322/a/<auth-host>/<db>/, a branch's Auth URL
# without the scheme (`bunx neon neon-auth status --branch staging --project-id
# cool-poetry-28343550`). It needs no configuration.
#
# run the web app's dev server
web-dev:
    bun run --cwd apps/web dev

# typecheck and build the web app into apps/web/dist
web-build:
    bun run --cwd apps/web build

# lint, typecheck and unit-test the web app (what CI runs)
web-check:
    bun run --cwd apps/web lint
    bun run --cwd apps/web typecheck
    bun run --cwd apps/web test

# For hosting the pages yourself (docs/web.md, "Self-hosting"): the build, with
# the headers and SPA fallback for Vercel, Netlify and Cloudflare Pages.
#
# build the web app's self-hosting tarball, target/release-assets/luminescent-web-<version>.tar.gz
web-bundle version="dev":
    #!/usr/bin/env bash
    set -euo pipefail
    bun run --cwd apps/web build
    name="luminescent-web-{{ version }}"
    out=target/release-assets
    rm -rf "$out/$name" && mkdir -p "$out/$name"
    cp -R apps/web/dist/. apps/web/vercel.json apps/web/bundle/README.md "$out/$name/"
    tar -C "$out" -czf "$out/$name.tar.gz" "$name"
    rm -rf "$out/$name"
    echo "→ $out/$name.tar.gz"

# test the deploy tooling: neon.ts's Neon Auth settings, the release manifest
tooling-test:
    bun test ./scripts

# Signs up a throwaway user on the branch (preview/* only) and deletes it
# afterwards; see docs/web.md. Needs `neon login` (or NEON_API_KEY) and
# `bunx playwright install chromium` in apps/web once.
#
# end-to-end OAuth sign-in through the web app's pages on a preview branch
web-e2e branch:
    E2E_NEON_BRANCH={{ branch }} NEON_PROJECT_ID={{ quote(neon_project) }} bun run --cwd apps/web e2e

# Unity Catalog's credential vending end to end on a preview/* branch: 401, 403
# without EXTERNAL USE, read and write keys on the branch's S3, the Delta API's
# endpoint, and the minter's own branch and GC.
vending-e2e branch:
    E2E_NEON_BRANCH={{ branch }} NEON_PROJECT_ID={{ quote(neon_project) }} bun test ./crates/unitycatalog-neon/e2e/vending.e2e.ts

# The desktop shell (apps/desktop, docs/desktop.md): a Tauri app with its own
# Cargo workspace. On Linux it needs the WebKitGTK development packages (see
# .github/workflows/desktop.yml). `desktop-dev` and `desktop-build` install the
# root's Bun dependencies first, frozen to bun.lock: a node_modules behind the
# lock bundles stale packages, and Vite keeps its pre-bundled copy until the
# lock changes again. The query engine (src-tauri's `query` feature) is most of a
# build, so `desktop-dev` leaves it out unless asked: `just desktop-dev query`.
#
# run the desktop shell against the Vite dev server (hot reload)
desktop-dev features="": _bun-install
    bun run --cwd apps/desktop tauri dev {{ if features == "" { "" } else { "--features " + quote(features) } }}

# Unsigned for now. macOS: apps/desktop/target/release/bundle/macos/*.app;
# Linux: the bare binary (no packages yet).
#
# build the desktop shell for release
desktop-build: _bun-install
    bun run --cwd apps/desktop tauri build --features query {{ if os() == "macos" { "--bundles app" } else { "--no-bundle" } }}

# Also fails if src/host/bindings.ts (generated by the Rust tests) is stale.
#
# lint, typecheck and test the desktop shell's UI and Rust (what CI runs)
desktop-check:
    #!/usr/bin/env bash
    set -euo pipefail
    cd apps/desktop
    bun run lint
    bun run typecheck
    bun run test
    # src-tauri embeds dist/ (tauri.conf.json `frontendDist`), so build it first.
    bun run build
    cargo fmt --all --check
    cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
    # Without the query engine too: the build everyday work uses.
    cargo clippy --locked --workspace --all-targets -- -D warnings
    cargo test --locked --workspace --all-features
    git diff --exit-code -- src/host/bindings.ts || { echo "src/host/bindings.ts is stale: commit the regenerated file" >&2; exit 1; }

# crates/omnigent-client: buf builds the hand-written omnigent.v1
# protos and trestle generates the client into src/gen/ (committed; needs `buf`,
# `trestle` and access to the Buf Schema Registry). Edit the protos, not src/gen/.
#
# regenerate the Omnigent client from its protos
omnigent-codegen:
    #!/usr/bin/env bash
    set -euo pipefail
    cd crates/omnigent-client
    rm -rf src/gen/codegen src/gen/models/_gen
    trestle generate -c trestle.yaml
    cd ../.. && cargo fmt -p omnigent-client

# What the client's contract test (tests/contract.rs) checks the protos against, for
# Omnigent at `tag`: its OpenAPI document, normalized, and the request models and
# hidden routes from its source. To move the pin, run this with the new tag, delete
# the old fixtures and set `PIN` in tests/contract.rs.
#
# refresh the Omnigent contract fixtures
omnigent-openapi tag="v0.22.0":
    crates/omnigent-client/scripts/fetch-omnigent.sh {{ quote(tag) }} crates/omnigent-client/fixtures

# The client's contract test against Omnigent at `tag` (`latest`: the newest
# release), fetched into a temporary directory: does the pinned client still fit
# it? The scheduled `omnigent-drift` workflow runs this.
#
# check omnigent-client against an Omnigent release
omnigent-contract tag="latest":
    #!/usr/bin/env bash
    set -euo pipefail
    tag={{ quote(tag) }}
    if [[ "$tag" == latest ]]; then
        tag="$(gh api repos/omnigent-ai/omnigent/releases/latest --jq .tag_name)"
    fi
    dir="$(mktemp -d)"
    trap 'rm -rf "$dir"' EXIT
    crates/omnigent-client/scripts/fetch-omnigent.sh "$tag" "$dir"
    echo "omnigent-client against Omnigent $tag"
    OMNIGENT_OPENAPI="$dir/openapi-$tag.json" OMNIGENT_SOURCE="$dir/source-$tag.json" \
        cargo test --locked -p omnigent-client --test contract

# The real binary in the real webview (WKWebView, WebKitGTK): IPC, CSP, the
# proxy. Pass a built binary (e.g. the .app's Contents/MacOS/luminescent-desktop)
# or get a debug build.
#
# run the in-app smoke test (`--smoke`)
desktop-smoke binary="":
    apps/desktop/scripts/smoke.sh {{ quote(binary) }}

# Creates a Unity Catalog catalog `demo` with schemas and tables, and posts
# OpenLineage runs to headwaters that connect them, over the branch's function
# URLs (`neon functions list`). Reruns leave what exists alone.
#
# seed a Neon branch with the desktop demo's data
desktop-demo-seed branch="demo":
    bun apps/desktop/scripts/demo-seed.ts {{ quote(branch) }} --project "${LUMINESCENT_NEON_PROJECT:-{{ neon_project }}}"

# Copies mangrove's grants app and its `open_grant_form` payload at `rev` (a commit
# on its main) into apps/desktop/e2e/fixtures/ (see the README there).
#
# refresh the MCP App e2e fixtures from mangrove
desktop-fixtures rev:
    #!/usr/bin/env bash
    set -euo pipefail
    out=apps/desktop/e2e/fixtures
    base="https://raw.githubusercontent.com/open-lakehouse/mangrove/{{ rev }}"
    curl -fsSL "$base/crates/cli/assets/mcp/grants-app.html" -o "$out/grants-app.html"
    curl -fsSL "$base/node/uc-a2ui/fixtures/open_grant_form.json" -o "$out/open_grant_form.json"
    shasum -a 256 "$out/grants-app.html" "$out/open_grant_form.json"
    echo "update the rev and checksums in $out/README.md"

# Needs `bunx playwright install webkit` in apps/desktop once.
#
# the in-app smoke test, then the UI flows in Playwright's WebKit on mocked IPC
desktop-e2e binary="": (desktop-smoke binary)
    bun run --cwd apps/desktop e2e

# Idempotent: reuses (and starts, if stopped) an existing container. Override
# PG_PORT etc. on the command line, e.g. `just PG_PORT=55440 test headwaters`.
#
# start the local test Postgres in Docker and wait until it accepts connections
pg-up:
    #!/usr/bin/env bash
    set -euo pipefail
    if [ -n "$(docker ps -aq -f name='^{{ PG_CONTAINER }}$')" ]; then
        docker start {{ PG_CONTAINER }} >/dev/null
    else
        echo "→ creating {{ PG_CONTAINER }} ({{ PG_IMAGE }}) on :{{ PG_PORT }}"
        docker run -d --name {{ PG_CONTAINER }} \
            -e POSTGRES_USER={{ PG_USER }} \
            -e POSTGRES_PASSWORD={{ PG_PASSWORD }} \
            -e POSTGRES_DB={{ PG_DB }} \
            -p {{ PG_PORT }}:5432 \
            {{ PG_IMAGE }} >/dev/null
    fi
    for _ in $(seq 1 30); do
        # Over TCP: the image's temporary init server answers on the socket only,
        # then stops again.
        if docker exec {{ PG_CONTAINER }} pg_isready -h 127.0.0.1 -U {{ PG_USER }} -d {{ PG_DB }} >/dev/null 2>&1; then
            # Unity Catalog's own database (its migration ledger can't share one
            # with headwaters'); the addon and its migrator swap it into DATABASE_URL.
            if [ -z "$(docker exec {{ PG_CONTAINER }} psql -h 127.0.0.1 -U {{ PG_USER }} -d {{ PG_DB }} -tAc \
                "SELECT 1 FROM pg_database WHERE datname = '{{ UC_DB }}'")" ]; then
                docker exec {{ PG_CONTAINER }} createdb -h 127.0.0.1 -U {{ PG_USER }} {{ UC_DB }}
            fi
            exit 0
        fi
        sleep 1
    done
    echo "error: Postgres did not become ready in time" >&2; exit 1

# remove the local test Postgres container (and its data)
pg-down:
    -docker rm -f {{ PG_CONTAINER }} 2>/dev/null

# The agentic factory (docs/factory): plan-tree and the factory-* crates, the
# control room's packages (packages/factory-client, packages/factory-ui) and its
# standalone app (apps/factory). fmt and clippy run over the whole workspace;
# these are its tests and the UI's checks. buf comes from PATH, or the
# npm-published one where it isn't installed (CI).
#
# test the factory crates (the conformance suite on every olai-store backend, the API, workgraph-mcp over HTTP, the plan-tree goldens)
factory-test:
    cargo test --locked -p plan-tree -p factory-proto -p factory-core -p factory-server -p workgraph-mcp --all-features

# lint, typecheck and test the factory's protos, packages and app, and build the app (what CI runs)
factory-check:
    #!/usr/bin/env bash
    set -euo pipefail
    buf() { if command -v buf >/dev/null; then command buf "$@"; else bunx @bufbuild/buf@1.75.0 "$@"; fi; }
    (cd crates/factory-proto && buf lint && buf build proto -o /dev/null)
    for dir in packages/factory-client packages/factory-ui apps/factory; do
        bun run --cwd "$dir" lint
        bun run --cwd "$dir" typecheck
    done
    bun run --cwd packages/factory-client test
    bun run --cwd packages/factory-ui test
    bun run --cwd apps/factory build

# Needs buf and the protoc plugins named in crates/factory-proto/buf.gen.yaml and
# packages/factory-client/buf.gen.yaml. The output is committed.
#
# regenerate the factory's protobuf code (Rust, TypeScript) and the descriptor set GetSchema serves
factory-gen:
    #!/usr/bin/env bash
    set -euo pipefail
    cd crates/factory-proto
    buf lint
    buf generate
    buf build proto -o src/descriptors.binpb
    cd ../.. && cargo fmt -p factory-proto
    bun run --cwd packages/factory-client gen

# run the reference factory-server (built-in extension only) on :7410 with the demo plan, in memory (db=sqlite://factory.db?mode=rwc to persist)
factory-server db="memory":
    cargo run -p factory-server -- --db {{ quote(db) }} --seed demo --registry crates/factory-core/registry.example.yaml

# run the reference factory-server with the demo plan, and the control room on http://localhost:4330
factory-dev: _bun-install
    #!/usr/bin/env bash
    set -euo pipefail
    cargo build -p factory-server
    target/debug/factory-server --seed demo --registry crates/factory-core/registry.example.yaml & server=$!
    trap 'kill $server' EXIT
    bun run --cwd apps/factory dev

# build the control room and serve it from the reference factory-server on http://127.0.0.1:7410
factory-serve-app db="memory": _bun-install
    bun run --cwd apps/factory build
    cargo run -p factory-server -- --db {{ quote(db) }} --seed demo --registry crates/factory-core/registry.example.yaml --static apps/factory/dist

# run Storybook for the control-room components, on the in-memory fake
factory-storybook: _bun-install
    bun run --cwd apps/factory storybook
