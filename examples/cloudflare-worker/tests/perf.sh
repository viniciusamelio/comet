#!/usr/bin/env bash
# Load-tests a real `wrangler dev` instance with autocannon to measure how
# many requests/second the comet adapter sustains inside a Worker.
#
# This is a measurement, not a strict pass/fail gate: absolute req/sec
# depends heavily on the machine it runs on (CI runners, sandboxes, and dev
# laptops all differ a lot), so there's no hardcoded throughput threshold.
# What *does* fail the script is any connection error or non-2xx response
# under load — that indicates something is actually broken, not just slow.
#
# Requires `rustup` with the wasm32-unknown-unknown target, and node/npm
# dependencies already installed (`npm install`, which pulls in autocannon).
set -euo pipefail

cd "$(dirname "$0")/.."

PORT=8789
BASE="http://localhost:$PORT"
LOG="$(mktemp)"
WRANGLER_PID=""

# Override with e.g. `COMET_PERF_DURATION=30 COMET_PERF_CONNECTIONS=50 npm run test:perf`.
DURATION="${COMET_PERF_DURATION:-10}"
CONNECTIONS="${COMET_PERF_CONNECTIONS:-10}"

cleanup() {
  if [[ -n "$WRANGLER_PID" ]]; then
    kill "$WRANGLER_PID" 2>/dev/null || true
    wait "$WRANGLER_PID" 2>/dev/null || true
  fi
}
trap cleanup EXIT

echo "Resetting local D1 state and applying migrations..."
rm -rf .wrangler/state/v3/d1
npx wrangler d1 migrations apply DB --local >"$LOG" 2>&1

echo "Starting wrangler dev on port $PORT..."
PATH="$(dirname "$(rustup which cargo)"):$PATH" npx wrangler dev --port "$PORT" >"$LOG" 2>&1 &
WRANGLER_PID=$!

ready=false
for _ in $(seq 1 60); do
  if curl -s -o /dev/null "$BASE/"; then
    ready=true
    break
  fi
  sleep 1
done

if [[ "$ready" != "true" ]]; then
  echo "wrangler dev did not become ready in time; log:"
  cat "$LOG"
  exit 1
fi

# A couple of rows so GET /tasks reflects a realistic (non-empty) D1 read
# instead of the fast-path empty-result-set case.
curl -s -X POST "$BASE/tasks" -H 'content-type: application/json' -d '{"title":"perf seed 1"}' >/dev/null
curl -s -X POST "$BASE/tasks" -H 'content-type: application/json' -d '{"title":"perf seed 2"}' >/dev/null

overall_ok=true

run_bench() {
  local name="$1" path="$2"
  echo
  echo "== $name ($BASE$path, ${CONNECTIONS} connections for ${DURATION}s) =="

  local result
  result=$(npx autocannon -c "$CONNECTIONS" -d "$DURATION" -j "$BASE$path")

  if echo "$result" | python3 -c "
import json, sys

d = json.load(sys.stdin)
errors = d['errors']
non2xx = d['non2xx']

print(f\"requests/sec: {d['requests']['average']:.0f} avg, {d['requests']['p99']} p99\")
print(f\"latency: {d['latency']['average']:.2f}ms avg, {d['latency']['p99']}ms p99\")
print(f\"{d['2xx']} 2xx, {non2xx} non-2xx, {errors} connection errors\")

sys.exit(1 if errors or non2xx else 0)
"; then
    echo "ok - $name completed under load with no errors"
  else
    echo "not ok - $name had connection errors or non-2xx responses under load"
    overall_ok=false
  fi

  return 0
}

run_bench "GET / (pure adapter, no D1/Queue)" "/"
run_bench "GET /tasks (D1-backed read)" "/tasks"

echo
if [[ "$overall_ok" == "true" ]]; then
  echo "performance run completed without errors"
else
  echo "performance run reported errors"
  exit 1
fi
