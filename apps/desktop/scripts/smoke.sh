#!/usr/bin/env bash
# The in-app smoke test: run the real binary with `--smoke` against a local
# streaming service (scripts/smoke-echo.ts, bound as `smoke-echo` through
# LUMINESCENT_SERVICES), on a throwaway TRESTLE_HOME so it never touches the
# shared state the trestle CLI uses (set SMOKE_TRESTLE_HOME to keep one). The UI
# runs src/host/smoke.ts and the binary exits 0 when every check passed.
# SMOKE_SERVICES adds bindings (e.g. `unity-catalog=http://localhost:9080,
# headwaters=http://localhost:9080` for a running environment), so the service
# views are checked against real services; without it they check the unbound
# state.
#
#   scripts/smoke.sh [binary]   default: a debug build with the bundled assets
#
# LUMINESCENT_SMOKE_DOCKER=1 also starts local/dev on Docker through the Run view
# and stops it again (pulls images on a first run).
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
# Docker mode renders compose trees there with bind mounts: keep it under the
# worktree (target/), since Colima and Docker Desktop share the home directory but
# not /tmp or /var/folders.
if [[ "${LUMINESCENT_SMOKE_DOCKER:-}" == "1" ]]; then
    mkdir -p target
    scratch="$(mktemp -d "$PWD/target/smoke-trestle-home.XXXX")"
else
    scratch="$(mktemp -d)"
fi
trestle_home="${SMOKE_TRESTLE_HOME:-$scratch}"
bun scripts/smoke-echo.ts >"$port_file" &
echo_pid=$!
trap 'kill $echo_pid 2>/dev/null || true; rm -f "$port_file"; rm -rf "${scratch:?}"' EXIT
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
TRESTLE_HOME="$trestle_home" LUMINESCENT_SERVICES="smoke-echo=http://127.0.0.1:$port${SMOKE_SERVICES:+,$SMOKE_SERVICES}" "${run[@]}"
