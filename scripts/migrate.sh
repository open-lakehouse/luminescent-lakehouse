#!/usr/bin/env bash
# The `deploy.before` hook in neon.ts: apply the migrations of every deployed
# function that has a schema to the branch being deployed. `neon deploy`
# injects the branch's DATABASE_URL_UNPOOLED; the migrators need the direct
# URL (they hold a session advisory lock, which the pooler can't keep).
#
# LUMINESCENT_MIGRATE: a prebuilt `luminescent-migrate` (CI); defaults to
# `cargo run`.
set -euo pipefail
cd "$(dirname "$0")/.."

url="${DATABASE_URL_UNPOOLED:?run this through neon deploy, which injects the branch connection strings}"
for fn in $(node scripts/deployed.mjs); do
    (
        source scripts/function.sh "$fn"
        [ "$NEEDS_PG" = 1 ] || exit 0
        echo "→ migrating $fn"
        if [ -n "${LUMINESCENT_MIGRATE:-}" ]; then
            DATABASE_URL="$url" "$LUMINESCENT_MIGRATE" "$fn"
        else
            DATABASE_URL="$url" cargo run -q -p luminescent-migrate -- "$fn"
        fi
    )
done
