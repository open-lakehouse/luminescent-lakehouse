#!/usr/bin/env bash
# Apply the migrations of every function neon.ts deploys that has a schema to a
# Neon branch, after the `authz` schema they share. Runs before every
# `neon deploy` (`just deploy`, CI): the functions never migrate, they only
# check the schema at startup.
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
# neondb, by name: a branch with more than one database (unitycatalog's) has no
# default. luminescent-migrate moves Unity Catalog's migrations to its own.
url=$(bunx neon connection-string "$branch" --database-name neondb ${NEON_PROJECT_ID:+--project-id "$NEON_PROJECT_ID"})
if [ -n "${GITHUB_ACTIONS:-}" ]; then echo "::add-mask::$url"; fi

migrate() {
    echo "→ migrating $1 on $branch"
    if [ -n "${LUMINESCENT_MIGRATE:-}" ]; then
        DATABASE_URL="$url" "$LUMINESCENT_MIGRATE" "$1"
    else
        # Offline: with DATABASE_URL set, sqlx's macros (mangrove's) would
        # check their queries against this branch at compile time.
        SQLX_OFFLINE=true DATABASE_URL="$url" cargo run -q -p luminescent-migrate --bin luminescent-migrate -- "$1"
    fi
}

# A database on the branch, created when missing. Child branches inherit it.
# Owned by neondb_owner, the role the functions connect as (the branch has
# other roles, e.g. authz_reader, so the owner has to be named).
ensure_database() {
    if ! bunx neon databases list --branch "$branch" ${NEON_PROJECT_ID:+--project-id "$NEON_PROJECT_ID"} -o json |
        jq -e --arg db "$1" 'any(.[]; .name == $db)' >/dev/null; then
        echo "→ creating database $1 on $branch"
        bunx neon databases create --name "$1" --owner-name neondb_owner --branch "$branch" ${NEON_PROJECT_ID:+--project-id "$NEON_PROJECT_ID"} >/dev/null
    fi
}

migrate authz
for fn in $(node scripts/deployed.mjs); do
    (
        source scripts/function.sh "$fn"
        [ "$NEEDS_PG" = 1 ] || exit 0
        # Unity Catalog lives in its own database (its migration ledger can't
        # share neondb's with headwaters); luminescent-migrate swaps it into the URL.
        if [ "$fn" = unitycatalog ]; then ensure_database unitycatalog; fi
        migrate "$fn"
    )
done
