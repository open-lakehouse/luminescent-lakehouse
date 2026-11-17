#!/usr/bin/env bash
# Fetch what tests/contract.rs checks the protos against, for Omnigent at <tag>, into
# <dir>: openapi-<tag>.json (the server's OpenAPI document, normalized) and
# source-<tag>.json (request models and hidden routes, extracted from the source).
#
# Usage: fetch-omnigent.sh <tag> <dir>
set -euo pipefail
tag="${1:?usage: fetch-omnigent.sh <tag> <dir>}"
dir="${2:?usage: fetch-omnigent.sh <tag> <dir>}"
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo=omnigent-ai/omnigent
# The route files that hold the routes upstream hides from its document.
routes=(
    omnigent/server/routes/sessions/routes_events.py
    omnigent/server/routes/sessions/routes_elicitations.py
)

# Upstream files are data: each in a fresh directory, read by the scripts here.
raw="$(mktemp -d)"
trap 'rm -rf "$raw"' EXIT
fetch() {
    gh api "repos/$repo/contents/$1?ref=$tag" -H 'Accept: application/vnd.github.raw' > "$raw/$2"
}

fetch openapi.json openapi.json
python3 -I "$here/normalize-openapi.py" "$raw/openapi.json" "$dir/openapi-$tag.json"

fetch omnigent/server/schemas.py schemas.py
args=()
for path in "${routes[@]}"; do
    fetch "$path" "$(basename "$path")"
    args+=("$raw/$(basename "$path")=$path")
done
python3 -I "$here/extract-source.py" "$dir/source-$tag.json" "$raw/schemas.py" "${args[@]}"
