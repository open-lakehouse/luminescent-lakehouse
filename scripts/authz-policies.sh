#!/usr/bin/env bash
# Stage the authz function's policies into its dist/: the policies from
# crates/authz-neon/function/policies/, plus each upstream's schema fragment, taken from the source cargo resolved for the
# pinned rev (so a pin bump brings its fragment along; nothing is copied by
# hand). The addon composes them after olai-pdp's built-in `Lakehouse` schema
# and baseline, and validates them in strict mode at start.
#
# Usage: scripts/authz-policies.sh <dest>   (`just _dist authz` runs it)
#
# MANGROVE_ROOT=<checkout> takes mangrove's fragments from a local checkout
# instead (crates/server, crates/handlers), e.g. to try policies against an
# unmerged mangrove change before the pin moves.
set -euo pipefail
cd "$(dirname "$0")/.."

dest="${1:?usage: scripts/authz-policies.sh <dest>}"
src=crates/authz-neon/function/policies

# `<cargo package>:<path in it>:<file name>`. headwaters and the sharing server
# join once they publish theirs (headwaters#85, distributary#32).
# mangrove's `UnityCatalog` namespace comes in four fragments: its entities, the
# actions of its API (generated, one per RPC), of its Delta API and of its
# storage proxy.
# The addon merges same-namespace fragments.
FRAGMENTS=(
    "olai-uc-server:cedar/UnityCatalog.cedarschema:UnityCatalog.cedarschema"
    "olai-uc-server:cedar/DeltaApi.cedarschema:UnityCatalogDelta.cedarschema"
    "olai-uc-server:cedar/StorageProxy.cedarschema:UnityCatalogStorageProxy.cedarschema"
    "olai-uc-handlers:src/codegen/authz.cedarschema:UnityCatalogActions.cedarschema"
)

# A package's directory in a local mangrove checkout, when MANGROVE_ROOT is set.
local_dir() {
    [ -n "${MANGROVE_ROOT:-}" ] || return 1
    case "$1" in
        olai-uc-server) echo "$MANGROVE_ROOT/crates/server" ;;
        olai-uc-handlers) echo "$MANGROVE_ROOT/crates/handlers" ;;
        *) return 1 ;;
    esac
}

rm -rf "$dest" && mkdir -p "$dest"
cp "$src"/*.cedar "$dest/"
metadata=$(cargo metadata --locked --format-version 1)
for fragment in "${FRAGMENTS[@]}"; do
    IFS=: read -r package path name <<<"$fragment"
    dir=$(local_dir "$package") || dir=$(PACKAGE="$package" node -e '
        let json = "";
        process.stdin.on("data", (d) => (json += d)).on("end", () => {
            // Other dependencies can pull in an older release of a mangrove crate
            // (distributary depends on olai-uc-handlers from crates.io), so take the
            // one from the same source as the pinned olai-uc-server.
            const { packages } = JSON.parse(json);
            const server = packages.find((p) => p.name === "olai-uc-server");
            const named = packages.filter((p) => p.name === process.env.PACKAGE);
            const pkg = named.find((p) => server && p.source === server.source) ?? named[0];
            if (!pkg) process.exit(1);
            console.log(require("node:path").dirname(pkg.manifest_path));
        });' <<<"$metadata") || { echo "error: $package is not in the workspace's dependencies" >&2; exit 1; }
    if [ ! -f "$dir/$path" ]; then
        echo "error: $package ($dir) has no $path; is its pin too old?" >&2
        exit 1
    fi
    cp "$dir/$path" "$dest/$name"
    # Also next to the sources, gitignored, so an editor or `cedar validate`
    # sees the composed schema.
    cp "$dir/$path" "$src/$name"
done
