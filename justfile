# The Neon Function harness. Every service is a napi addon crate under crates/
# plus a dependency-free fetch handler (`function/index.ts`); the recipes take
# the function name (`headwaters`, `opensharing`), see scripts/function.sh.

set dotenv-load := true

# Neon Functions run Node 24 on linux-arm64 glibc; 2.28 is the glibc floor of
# the official Node 24 binaries.
TARGET := "aarch64-unknown-linux-gnu.2.28"
# Neon's deploy limits: 10 MiB compressed, 64 MiB uncompressed.
MAX_ZIP := "10485760"
MAX_UNZIPPED := "67108864"

# Local Postgres for the tests of functions that need one (headwaters).
PG_CONTAINER := "luminescent-postgres"
PG_IMAGE := "postgres:16-alpine"
PG_PORT := "5432"
PG_USER := "postgres"
PG_PASSWORD := "postgres"
PG_DB := "lineage"
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
        DATABASE_URL="{{ DATABASE_URL }}" cargo run -q -p luminescent-migrate -- {{ fn }}
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
        DATABASE_URL="{{ DATABASE_URL }}" cargo run -q -p luminescent-migrate -- {{ fn }}
        env=(--add-host host.docker.internal:host-gateway
            -e "DATABASE_URL=postgres://{{ PG_USER }}:{{ PG_PASSWORD }}@host.docker.internal:{{ PG_PORT }}/{{ PG_DB }}")
    fi
    docker run --rm --platform linux/arm64 ${env[@]+"${env[@]}"} \
        -v "$PWD/$FN_DIR:/fn:ro" -w /fn \
        node:24-bookworm-slim sh -c "$TESTS"

# Migrations are never applied by a function itself; run this once per schema
# change. Use the branch's *direct* connection string (the migrators hold a
# session advisory lock, which Neon's pooled DATABASE_URL can't keep).
#
# apply a function's migrations to a Neon branch (needs DATABASE_URL_UNPOOLED)
migrate fn:
    DATABASE_URL="${DATABASE_URL_UNPOOLED:?set DATABASE_URL_UNPOOLED to the Neon branch direct (unpooled) connection string}" \
    cargo run -p luminescent-migrate -- {{ fn }}

# deploy a function's built bundle (needs the `neon` CLI, logged in)
deploy fn slug=fn: (build fn)
    #!/usr/bin/env bash
    set -euo pipefail
    source scripts/function.sh {{ fn }}
    neon functions deploy {{ slug }} --src "$FN_DIR/dist" --no-bundle --wait

# headwaters projects on demand (headwaters ADR 0016). By default ingest settles
# via `waitUntil` and this trigger only folds what an evicted isolate left
# behind, so a slow cadence costs little freshness. With
# HEADWATERS_NEON_SETTLE_ON_INGEST=false the trigger is the only fold, and the
# cadence is the read lag. A cadence under Neon's 5 min scale-to-zero window
# keeps the branch compute awake around the clock. Run once per branch; to
# change it later: `neon triggers update <trigger-id> --cron '<cron>'`.
#
# create the cron trigger that keeps the headwaters projection caught up (UTC cron, ≥ 1 min)
trigger-headwaters slug="headwaters" cron="*/10 * * * *":
    neon triggers create --function-slug {{ slug }} --name {{ slug }}-projection \
        --cron '{{ cron }}' --function-path /internal/projection/run

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
        if docker exec {{ PG_CONTAINER }} pg_isready -U {{ PG_USER }} -d {{ PG_DB }} >/dev/null 2>&1; then
            exit 0
        fi
        sleep 1
    done
    echo "error: Postgres did not become ready in time" >&2; exit 1

# remove the local test Postgres container (and its data)
pg-down:
    -docker rm -f {{ PG_CONTAINER }} 2>/dev/null
