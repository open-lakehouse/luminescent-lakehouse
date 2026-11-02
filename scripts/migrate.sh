#!/usr/bin/env bash
# Apply the migrations of every function neon.ts deploys that has a schema to a
# Neon branch. Runs before every `neon deploy` (`just deploy`, CI): the functions
# never migrate, they only check the schema at startup.
#
# Usage: scripts/migrate.sh <neon-branch>
#
# Over the branch's direct connection string, which the migrators need (they
# hold a session advisory lock, which the pooler can't keep). It is looked up
# with the logged-in CLI or NEON_API_KEY; NEON_PROJECT_ID selects the project
# (default: the linked one). LUMINESCENT_MIGRATE: a prebuilt
# `luminescent-migrate` (CI); defaults to `cargo run`.
set -euo pipefail
cd "$(dirname "$0")/.."

branch="${1:?usage: scripts/migrate.sh <neon-branch>}"
url=$(npx neon connection-string "$branch" ${NEON_PROJECT_ID:+--project-id "$NEON_PROJECT_ID"})
if [ -n "${GITHUB_ACTIONS:-}" ]; then echo "::add-mask::$url"; fi

for fn in $(node scripts/deployed.mjs); do
    (
        source scripts/function.sh "$fn"
        [ "$NEEDS_PG" = 1 ] || exit 0
        echo "→ migrating $fn on $branch"
        if [ -n "${LUMINESCENT_MIGRATE:-}" ]; then
            DATABASE_URL="$url" "$LUMINESCENT_MIGRATE" "$fn"
        else
            DATABASE_URL="$url" cargo run -q -p luminescent-migrate -- "$fn"
        fi
    )
done
