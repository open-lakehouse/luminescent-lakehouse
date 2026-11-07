# The Neon Function harness. Every service is a napi addon crate under crates/
# plus a dependency-free fetch handler (`function/index.ts`); the recipes take
# the function name (`headwaters`, `opensharing`, `unitycatalog`), see
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
# unitycatalog).
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

# smoke-test a function handler on the host (Postgres-backed ones on the local dev Postgres)
test fn: (build-host fn)
    #!/usr/bin/env bash
    set -euo pipefail
    source scripts/function.sh {{ fn }}
    if [ "$NEEDS_PG" = 1 ]; then
        just PG_PORT={{ PG_PORT }} pg-up
        DATABASE_URL="{{ DATABASE_URL }}" cargo run -q -p luminescent-migrate --bin luminescent-migrate -- {{ fn }}
        export DATABASE_URL="{{ DATABASE_URL }}"
    fi
    cd "$FN_DIR" && sh -c "$TESTS"

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
        just PG_PORT={{ PG_PORT }} pg-up
        DATABASE_URL="{{ DATABASE_URL }}" cargo run -q -p luminescent-migrate --bin luminescent-migrate -- {{ fn }}
        env=(--add-host host.docker.internal:host-gateway
            -e "DATABASE_URL=postgres://{{ PG_USER }}:{{ PG_PASSWORD }}@host.docker.internal:{{ PG_PORT }}/{{ PG_DB }}")
    fi
    docker run --rm --platform linux/arm64 ${env[@]+"${env[@]}"} \
        -v "$PWD/$FN_DIR:/fn:ro" -w /fn \
        node:24-bookworm-slim sh -c "$TESTS"

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
    bunx neon deploy --branch {{ branch }} --no-env-pull
    bun scripts/auth-setup.ts {{ branch }}

# Against the local test Postgres (`pg-up`), one database per test.
#
# test luminescent-migrate and luminescent-admin: the authz migrations and operations
test-migrate:
    just PG_PORT={{ PG_PORT }} pg-up
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
# docs/auth.md. Over the branch's direct connection string, like `migrate`.
#
# manage a Neon branch's authz data, e.g. `just admin staging grant list`
admin branch *args:
    #!/usr/bin/env bash
    set -euo pipefail
    url=$(bunx neon connection-string "{{ branch }}" --database-name neondb)
    DATABASE_URL="$url" cargo run -q -p luminescent-migrate --bin luminescent-admin -- {{ args }}

# Neon's agent setup (https://neon.com/prompts/agent-setup.md): the Neon CLI and
# the Neon plugin (skills + MCP server) for your coding agent, user-level. Then
# sign in once with `neon login`. Needs Node >= 22.20.
#
# install the Neon CLI and the Neon agent plugin (default agent: claude-code)
agent-setup agent="claude-code":
    bun add -g neon@latest
    neon plugins --global --agent {{ agent }} -y

# The Neon Auth web app (apps/web, docs/web.md). Needs `bun install` at the
# root and apps/web/.env.local (from .env.example). Open
# http://localhost:4322/b/<endpoint-id>/; a branch's endpoint id is the host of
# its Auth URL (`bunx neon neon-auth status --branch staging --project-id
# cool-poetry-28343550`).
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

# Signs up a throwaway user on the branch (preview/* only) and deletes it
# afterwards; see docs/web.md. Needs `neon login` (or NEON_API_KEY) and
# `bunx playwright install chromium` in apps/web once.
#
# end-to-end OAuth sign-in through the web app's pages on a preview branch
web-e2e branch:
    E2E_NEON_BRANCH={{ branch }} NEON_PROJECT_ID={{ quote(neon_project) }} bun run --cwd apps/web e2e

# The desktop shell (apps/desktop, docs/desktop.md): a Tauri app with its own
# Cargo workspace. On Linux it needs the WebKitGTK development packages (see
# .github/workflows/desktop.yml). `desktop-dev` and `desktop-build` install the
# root's Bun dependencies first, frozen to bun.lock: a node_modules behind the
# lock bundles stale packages, and Vite keeps its pre-bundled copy until the
# lock changes again.
#
# run the desktop shell against the Vite dev server (hot reload)
desktop-dev: _bun-install
    bun run --cwd apps/desktop tauri dev

# Unsigned for now. macOS: apps/desktop/target/release/bundle/macos/*.app;
# Linux: the bare binary (no packages yet).
#
# build the desktop shell for release
desktop-build: _bun-install
    bun run --cwd apps/desktop tauri build {{ if os() == "macos" { "--bundles app" } else { "--no-bundle" } }}

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
    cargo test --locked --workspace
    git diff --exit-code -- src/host/bindings.ts || { echo "src/host/bindings.ts is stale: commit the regenerated file" >&2; exit 1; }

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
