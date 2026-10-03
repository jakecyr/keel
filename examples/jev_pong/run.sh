#!/bin/sh
set -eu
cd "$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
case "${1:-offline}" in offline|jev) PONG_MODE="${1:-offline}" ;; *) echo 'Usage: ./run.sh [offline|jev]' >&2; exit 2 ;; esac
export PONG_MODE
: "${PONG_PORT:=8766}" "${PONG_TIMEOUT:=3000}" "${PONG_MAX_DECISIONS:=12}" "${PONG_MAX_RETURNS:=20}"
export PONG_PORT PONG_TIMEOUT PONG_MAX_DECISIONS PONG_MAX_RETURNS
mkdir -p build
cargo build --locked --manifest-path ../../Cargo.toml
../../target/debug/keel build . -o build/pong
../../target/debug/keel build left-worker.json -o build/left-worker
../../target/debug/keel build right-worker.json -o build/right-worker
printf 'Pong: http://127.0.0.1:%s (%s mode; click Start to play)\n' "$PONG_PORT" "$PONG_MODE"
if [ "$PONG_MODE" = jev ]; then
    printf 'Live requests use JEV_API_KEY or ../jev/.env. Limit: %s requests per player per session.\n' "$PONG_MAX_DECISIONS"
fi
exec ./build/pong \
    "--allow-net=127.0.0.1:$PONG_PORT" --allow-read=web \
    --allow-read=build/state.json --allow-write=build/state.json \
    --allow-read=build/left-result.json --allow-read=build/right-result.json \
    --allow-write=build/left-job.json --allow-write=build/right-job.json \
    --allow-write=build/left-result.json --allow-write=build/right-result.json \
    --allow-exec=./build/left-worker --allow-exec=./build/right-worker \
    --allow-env=PONG_MODE --allow-env=PONG_PORT \
    --allow-env=PONG_TIMEOUT --allow-env=PONG_MAX_DECISIONS --allow-env=PONG_MAX_RETURNS \
    --allow-clock=monotonic
