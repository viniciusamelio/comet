#!/usr/bin/env bash
# End-to-end test against a real `wrangler dev` instance: exercises the HTTP
# routes, the async D1-backed task store, and the async queue consumer that
# writes the task_events audit trail. Requires `rustup` with the
# wasm32-unknown-unknown target and node/npm dependencies already installed
# (`npm install`).
set -euo pipefail

cd "$(dirname "$0")/.."

PORT=8788
BASE="http://localhost:$PORT"
LOG="$(mktemp)"
WRANGLER_PID=""

cleanup() {
  if [[ -n "$WRANGLER_PID" ]]; then
    kill "$WRANGLER_PID" 2>/dev/null || true
    wait "$WRANGLER_PID" 2>/dev/null || true
  fi
}
trap cleanup EXIT

echo "Resetting local D1 state and applying migrations..."
rm -rf .wrangler/state/v3/d1
rm -rf .wrangler/state/v3/r2
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

pass=0
fail=0

check() {
  local desc=$1 expected=$2 actual=$3
  if [[ "$actual" == "$expected" ]]; then
    echo "ok - $desc"
    pass=$((pass + 1))
  else
    echo "not ok - $desc (expected [$expected], got [$actual])"
    fail=$((fail + 1))
  fi
}

INDEX=$(curl -s "$BASE/")
check "index route returns greeting" "hello from Rocket on Cloudflare Workers" "$INDEX"

ECHO=$(curl -s -X POST "$BASE/echo" -d 'ping')
check "echo route returns the request body" "ping" "$ECHO"

# Requests are streamed into Rocket rather than buffered into a WorkerRequest
# up front (see comet's request_from_worker / RawStream::Worker), so a body
# well past a single chunk should still round-trip exactly. /echo takes a
# `String`, so the body has to be valid UTF-8 (base64, here) or Rocket would
# correctly reject raw random bytes with 400 regardless of streaming.
LARGE_BODY="$(mktemp)"
LARGE_ECHO="$(mktemp)"
head -c 1048576 /dev/urandom | base64 >"$LARGE_BODY"
LARGE_ECHO_STATUS=$(curl -s -X POST "$BASE/echo" --data-binary @"$LARGE_BODY" -o "$LARGE_ECHO" -w '%{http_code}')
check "large (1MiB) echo request returns 200" "200" "$LARGE_ECHO_STATUS"
if cmp -s "$LARGE_BODY" "$LARGE_ECHO"; then
  LARGE_MATCH="true"
else
  LARGE_MATCH="false"
fi
check "large (1MiB) echo body round-trips byte-for-byte" "true" "$LARGE_MATCH"
rm -f "$LARGE_BODY" "$LARGE_ECHO"

# /stream yields 3 chunks with a real (worker::Delay, not tokio) 400ms delay
# between each. If comet buffered the whole response before sending it, time
# to first byte would be close to the total time (~1.2s). Streamed, the
# first byte arrives almost immediately and the rest trickles in afterward.
STREAM_TIMING=$(curl -s -o /dev/null -w '%{time_starttransfer} %{time_total}' "$BASE/stream")
STREAM_TTFB=$(echo "$STREAM_TIMING" | awk '{print $1}')
STREAM_TOTAL=$(echo "$STREAM_TIMING" | awk '{print $2}')
STREAM_IS_PROGRESSIVE=$(awk -v ttfb="$STREAM_TTFB" -v total="$STREAM_TOTAL" \
  'BEGIN { print (ttfb < 0.3 && (total - ttfb) > 0.5) ? "true" : "false" }')
check "response body streams progressively (ttfb=${STREAM_TTFB}s, total=${STREAM_TOTAL}s)" \
  "true" "$STREAM_IS_PROGRESSIVE"

ASSET_BODY="$(mktemp)"
ASSET_GET="$(mktemp)"
head -c 1048576 /dev/urandom >"$ASSET_BODY"
ASSET_PUT_STATUS=$(curl -s -X PUT "$BASE/assets/large.bin" --data-binary @"$ASSET_BODY" -o /dev/null -w '%{http_code}')
check "large R2 asset upload returns 201" "201" "$ASSET_PUT_STATUS"
ASSET_GET_STATUS=$(curl -s "$BASE/assets/large.bin" -o "$ASSET_GET" -w '%{http_code}')
check "large R2 asset download returns 200" "200" "$ASSET_GET_STATUS"
if cmp -s "$ASSET_BODY" "$ASSET_GET"; then
  ASSET_MATCH="true"
else
  ASSET_MATCH="false"
fi
check "large R2 asset body round-trips byte-for-byte" "true" "$ASSET_MATCH"
rm -f "$ASSET_BODY" "$ASSET_GET"

WS_ECHO=$(PORT="$PORT" node <<'NODE'
const WebSocket = globalThis.WebSocket || require("ws");
const ws = new WebSocket(`ws://localhost:${process.env.PORT}/ws/echo`);
const message = "comet websocket echo";

function on(target, event, handler) {
  if (typeof target.addEventListener === "function") {
    target.addEventListener(event, handler);
  } else {
    target.on(event, handler);
  }
}

const timeout = setTimeout(() => {
  console.error("websocket timed out");
  process.exit(1);
}, 5000);

on(ws, "open", () => ws.send(message));
on(ws, "message", (event) => {
  const data = event && "data" in event ? event.data : event;
  const text = Buffer.isBuffer(data) ? data.toString("utf8") : String(data);
  clearTimeout(timeout);
  console.log(text);
  ws.close();
});
on(ws, "error", (error) => {
  clearTimeout(timeout);
  console.error(error);
  process.exit(1);
});
NODE
)
check "websocket echo round-trips text" "comet websocket echo" "$WS_ECHO"

CREATE=$(curl -s -X POST "$BASE/tasks" -H 'content-type: application/json' -d '{"title":"integration test task"}')
TASK_ID=$(echo "$CREATE" | jq -r .id)
check "create task returns the submitted title" "integration test task" "$(echo "$CREATE" | jq -r .title)"
check "create task starts out not done" "false" "$(echo "$CREATE" | jq -r .done)"

BLANK_STATUS=$(curl -s -o /dev/null -w '%{http_code}' -X POST "$BASE/tasks" -H 'content-type: application/json' -d '{"title":"   "}')
check "blank title is rejected with 400" "400" "$BLANK_STATUS"

MISSING_STATUS=$(curl -s -o /dev/null -w '%{http_code}' "$BASE/tasks/999999")
check "missing task returns 404" "404" "$MISSING_STATUS"

GET=$(curl -s "$BASE/tasks/$TASK_ID")
check "get task by id round-trips the id" "$TASK_ID" "$(echo "$GET" | jq -r .id)"

COMPLETE=$(curl -s -X POST "$BASE/tasks/$TASK_ID/complete")
check "completing a task marks it done" "true" "$(echo "$COMPLETE" | jq -r .done)"

LIST_COUNT=$(curl -s "$BASE/tasks" | jq 'length')
check "list returns exactly the created task" "1" "$LIST_COUNT"

# The two task lifecycle events (created, completed) are published to the
# TASK_EVENTS queue and picked up by the async queue consumer out of band.
# Local queues batch up to `max_batch_timeout` (5s, see wrangler.jsonc)
# before flushing, so give it comfortable headroom before asserting on the
# consumer's effects.
sleep 8

EVENT_KINDS=$(npx wrangler d1 execute DB --local --json \
  --command "SELECT kind FROM task_events WHERE task_id = $TASK_ID ORDER BY id" \
  | jq -r '.[0].results[].kind' | paste -sd, -)
check "queue consumer recorded created+completed events" "created,completed" "$EVENT_KINDS"

echo
echo "$pass passed, $fail failed"
[[ "$fail" -eq 0 ]]
