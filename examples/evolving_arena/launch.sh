#!/bin/sh
set -eu
cd "$(dirname "$0")"
mode="${1:-demo}"
ARENA_PORT="${ARENA_PORT:-8780}"
export ARENA_PORT
case "$mode" in demo|agents) ;; *) echo 'Usage: ./launch.sh [demo|agents]' >&2; exit 2;; esac
cargo build --locked --manifest-path ../../Cargo.toml
mkdir -p runtime
printf '%s' "$mode" > runtime/mode.txt
keel=../../target/debug/keel
"$keel" build . -o runtime/arena
"$keel" build worker-red.json -o runtime/worker-0
"$keel" build worker-blue.json -o runtime/worker-1
for player in 0 1; do
    cat common.keel referee.keel candidate_support.keel > "runtime/ability-$player-0.keel"
    printf '\nfn ability(state: read List<Int>) -> List<Int> { return [3,3,2,2] }\n' >> "runtime/ability-$player-0.keel"
    "$keel" build "runtime/ability-$player-0.keel" -o "runtime/ability-$player-0"
done
if [ "${ARENA_BUILD_ONLY:-0}" = 1 ]; then exit 0; fi
printf '\nKeel evolving arena: http://127.0.0.1:%s\nMode: %s\n' "$ARENA_PORT" "$mode"
set -- "--allow-net=127.0.0.1:$ARENA_PORT" --allow-env=ARENA_PORT --allow-clock=monotonic --allow-read=public \
    --allow-read=runtime/mode.txt --allow-read=runtime/state.json --allow-write=runtime/state.json \
    --allow-write=runtime/observation.json
for player in 0 1; do
    set -- "$@" "--allow-exec=runtime/worker-$player"
    for slot in 0 1; do set -- "$@" "--allow-exec=runtime/ability-$player-$slot"; done
    for item in active feedback history result task; do
        set -- "$@" "--allow-read=runtime/$item-$player.json" "--allow-write=runtime/$item-$player.json"
    done
done
exec runtime/arena "$@"
