#!/usr/bin/env bash
# Stage the authz function's policies into its dist/: the policies and the
# `Lakehouse` fragment from crates/authz-neon/function/policies/, plus each
# upstream's schema fragment, taken from the source cargo resolved for the
# pinned rev (so a pin bump brings its fragment along; nothing is copied by
# hand). The addon composes and validates them in strict mode at start.
#
# Usage: scripts/authz-policies.sh <dest>   (`just _dist authz` runs it)
set -euo pipefail
cd "$(dirname "$0")/.."

dest="${1:?usage: scripts/authz-policies.sh <dest>}"
src=crates/authz-neon/function/policies

# `<cargo package>:<path in it>:<file name>`. headwaters and the sharing server
# join once they publish theirs (headwaters#85, distributary#32).
FRAGMENTS=(
    "olai-uc-server:cedar/UnityCatalog.cedarschema:UnityCatalog.cedarschema"
)

rm -rf "$dest" && mkdir -p "$dest"
cp "$src"/*.cedar "$src"/*.cedarschema "$dest/"
metadata=$(cargo metadata --locked --format-version 1)
for fragment in "${FRAGMENTS[@]}"; do
    IFS=: read -r package path name <<<"$fragment"
    dir=$(PACKAGE="$package" node -e '
        let json = "";
        process.stdin.on("data", (d) => (json += d)).on("end", () => {
            const pkg = JSON.parse(json).packages.find((p) => p.name === process.env.PACKAGE);
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
