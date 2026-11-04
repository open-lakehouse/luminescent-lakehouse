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
# LUMINESCENT_SMOKE_DOCKER=1 also runs local/dev on Docker through the Run view, in
# two launches of the app on the same state (pulls images on a first run):
#   1. start: revision 1, then revision 2 next to it (with headwaters under
#      LUMINESCENT_SMOKE_LINEAGE=1), each with a catalog of its own; the views show
#      each one's data only, Logs and the Lineage view (<gateway>/lineage/) work. The
#      app exits with both running.
#   2. resume: the relaunch adopts both, makes #2 current again, switches to #1,
#      and stops both.
# In between, the env trestle renders is checked in a container (`docker exec … env`);
# afterwards, that no container or network of either is left.
# LUMINESCENT_SMOKE_KNOBS=module.knob=value,… overrides knobs (e.g. local images:
# `unity-catalog.image=unitycatalog/unitycatalog:v0.7.0`).
#
# SMOKE_NEON_BRANCH=<branch> also uses that Neon branch through the Run view's
# Neon card and checks the Catalog and Lineage views show its seeded data
# (`just desktop-demo-seed <branch>`). Needs the Neon CLI, signed in.
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
launch() {
    TRESTLE_HOME="$trestle_home" LUMINESCENT_SETTINGS_DIR="$scratch/settings" \
        LUMINESCENT_SMOKE_DOCKER="$1" LUMINESCENT_SMOKE_NEON="${SMOKE_NEON_BRANCH:-}" \
        LUMINESCENT_SERVICES="smoke-echo=http://127.0.0.1:$port${SMOKE_SERVICES:+,$SMOKE_SERVICES}" \
        "${run[@]}"
}

# The compose projects of local/dev's two revisions.
projects=(local_dev_r1 local_dev_r2)

leftovers() {
    local p found=""
    for p in "${projects[@]}"; do
        found+="$(docker ps -aq --filter "label=com.docker.compose.project=$p")"
        found+="$(docker network ls -q --filter "label=com.docker.compose.project=$p")"
    done
    [[ -z "$found" ]]
}

if [[ "${LUMINESCENT_SMOKE_DOCKER:-}" != "1" ]]; then
    launch ""
    exit
fi

leftovers || { echo "smoke: containers of ${projects[*]} exist already; stop them first" >&2; exit 1; }
# A failed phase leaves what it started: take it down on the way out (volumes kept).
trap 'for p in "${projects[@]}"; do docker compose -p "$p" down >/dev/null 2>&1 || true; done
      kill $echo_pid 2>/dev/null || true; rm -f "$port_file"; rm -rf "${scratch:?}"' EXIT
launch start
# What trestle renders reaches the container: the Postgres module's env file, and the
# password secret resolved at launch.
db="$(docker ps -q --filter label=com.docker.compose.project=local_dev_r2 --filter label=com.docker.compose.service=db | head -1)"
if [[ -n "$db" ]] &&
    docker exec "$db" env | grep -qx 'POSTGRES_PASSWORD_FILE=/run/secrets/postgres_password' &&
    docker exec "$db" sh -c 'test -s "$POSTGRES_PASSWORD_FILE"'; then
    echo "smoke: ok   dev #2's db container has its injected env and its password secret"
else
    echo "smoke: FAIL dev #2's db container (${db:-none}) lacks its env or secret; its env:" >&2
    [[ -n "$db" ]] && docker exec "$db" env | sort >&2
    status=1
fi
launch resume
if leftovers; then
    echo "smoke: ok   no container or network of ${projects[*]} is left"
else
    echo "smoke: FAIL left over:" >&2
    for p in "${projects[@]}"; do
        docker ps -a --filter "label=com.docker.compose.project=$p" >&2
        docker network ls --filter "label=com.docker.compose.project=$p" >&2
    done
    exit 1
fi
exit "${status:-0}"
