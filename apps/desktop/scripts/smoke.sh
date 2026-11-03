#!/usr/bin/env bash
# The in-app smoke test: run the real binary with `--smoke` against a local
# streaming service (scripts/smoke-echo.ts, bound as `smoke-echo` through
# LUMINESCENT_SERVICES), on a throwaway TRESTLE_HOME so it never touches the
# shared state the trestle CLI uses (set SMOKE_TRESTLE_HOME to keep one). The UI
# runs src/host/smoke.ts and the binary exits 0 when every check passed.
#
#   scripts/smoke.sh [binary]   default: a debug build with the bundled assets
#
# Linux without a display runs under xvfb-run.
set -euo pipefail

binary="${1:-}"
# Relative to where we were called from, before the cd below.
[[ -z "$binary" || "$binary" == /* ]] || binary="$PWD/$binary"
cd "$(dirname "${BASH_SOURCE[0]}")/.."
if [[ -z "$binary" ]]; then
    [[ -f dist/index.html ]] || bun run build
    # `custom-protocol` serves dist/ from the binary, as `tauri build` does;
    # without it a debug build loads the dev server URL.
    cargo build -p luminescent-desktop --features tauri/custom-protocol
    binary=target/debug/luminescent-desktop
fi

port_file="$(mktemp)"
trestle_home="${SMOKE_TRESTLE_HOME:-$(mktemp -d)}"
bun scripts/smoke-echo.ts >"$port_file" &
echo_pid=$!
trap 'kill $echo_pid 2>/dev/null || true; rm -f "$port_file"; [[ -n "${SMOKE_TRESTLE_HOME:-}" ]] || rm -rf "${trestle_home:?}"' EXIT
for _ in $(seq 1 50); do
    [[ -s "$port_file" ]] && break
    sleep 0.1
done
port="$(head -1 "$port_file")"
[[ -n "$port" ]] || { echo "smoke-echo did not start" >&2; exit 1; }

run=("$binary" --smoke)
if [[ "$(uname)" == "Linux" && -z "${DISPLAY:-}${WAYLAND_DISPLAY:-}" ]]; then
    run=(xvfb-run -a "${run[@]}")
fi
TRESTLE_HOME="$trestle_home" LUMINESCENT_SERVICES="smoke-echo=http://127.0.0.1:$port" "${run[@]}"
