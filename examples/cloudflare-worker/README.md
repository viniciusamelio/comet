# Comet Cloudflare Worker Example

This example is a small but real Rust Cloudflare Worker built with Rocket
routes through `comet`. It's a task tracker backed by D1, with task lifecycle
changes published to a Queue and consumed asynchronously. It also exercises R2
object streaming, a Worker WebSocket route, and `comet-auth` mounted on the
same Rocket app.

It depends on `comet` with `default-features = false` and the Cloudflare
features needed by the example (`cloudflare`, D1, Queue, R2, and WebSocket), so
it can compile to `wasm32-unknown-unknown` without pulling Rocket's native
local client. `comet-auth` is included with `default-features = false` and the
`cloudflare`/`macros` features. The Worker entrypoint calls:

```rust
comet::cloudflare::fetch(req, env, ctx, rocket).await
```

## What's here

- `src/model.rs` — the `Task`/`NewTask`/`TaskEvent` structs, hand-written
  (de)serialization concerns (D1 has no boolean storage class, so `done`
  round-trips through an internal `TaskRow` before becoming a JSON `bool`),
  and unit tests for all of it.
- `src/routes.rs` — Rocket routes that read and write a `tasks` table in D1
  and publish `TaskEvent`s to a queue. The routes use typed comet binding
  guards (`D1<DB>`, `QueueBinding<TaskEvents>`, and `R2Bucket<Assets>`) so
  handlers do not manually pull bindings out of `Env`. All D1/Queue/R2 calls
  are async.
- `src/entry.rs` — the wasm-only glue: the `#[event(fetch)]` handler that
  hands requests to Rocket via `comet::cloudflare::FetchApp`, and the
  `#[event(queue)]` consumer that asynchronously records each `TaskEvent`
  into a `task_events` table.
- `src/error.rs` — an `ApiError` type that turns D1/Queue failures and
  validation errors into proper JSON error responses with the right HTTP
  status.
- `migrations/0001_init.sql` — the `tasks` and `task_events` schema.
- `migrations/0002_comet_auth.sql` — the auth schema for users, linked
  provider accounts, and sessions.

### Routes

- `GET /` — plain text greeting.
- `POST /echo` — returns the request body. Request bodies are streamed into
  Rocket (not buffered up front), so this works the same for a 2-byte body
  and a multi-megabyte one.
- `GET /stream` — 3 chunks, a real (`worker::Delay`) 400ms gap between each.
  Exists purely to prove response streaming isn't buffered — see Tests below.
- `GET /auth/session` — current auth state. Anonymous visitors get
  `{"authenticated":false,...}`.
- `GET /auth/<provider>/start` — starts Google, Apple, or GitHub OAuth when
  the corresponding provider secrets are configured.
- `GET /auth/<provider>/callback` — OAuth callback endpoint.
- `POST /auth/native/google` and `POST /auth/native/apple` — exchange native
  identity tokens for a Comet session.
- `POST /auth/logout` — revoke the current session.
- `GET /private/me` — protected route using `#[comet_auth::requires_auth]`.
- `GET /private/admin` — RBAC-protected route using
  `#[comet_auth::requires_auth(role = "admin")]`.
- `GET /private/reviewer` — RBAC-protected route using `any(...)` and
  `resource = "demo"`.
- `GET /tasks` — list all tasks.
- `POST /tasks` — create a task from a JSON body (`{"title": "..."}`) and
  publish a `created` event to the queue.
- `GET /tasks/<id>` — fetch a task by id (404 if missing).
- `POST /tasks/<id>/complete` — mark a task done and publish a `completed`
  event to the queue.
- `PUT /assets/<key..>` — store a request body in R2.
- `GET /assets/<key..>` — stream an R2 object back through Rocket.
- `GET /ws/echo` — Worker WebSocket echo route.

### Rocket + non-`Send` bindings

Cloudflare binding guards use marker types to name bindings:

```rust
struct DB;

impl comet::cloudflare::BindingName for DB {
    const NAME: &'static str = "DB";
}

#[get("/tasks")]
async fn list_tasks(db: comet::cloudflare::D1<DB>) {
    // use db.prepare(...)
}
```

Worker builds of the vendored Rocket use local-boxed route futures, so route
handlers can await D1/Queue calls directly. Streaming responder bodies still
flow through Rocket's `Stream + Send` responder bound. Streams that await
Worker primitives, like `/stream` with `worker::Delay`, should wrap the stream
with `comet::cloudflare::local_stream(...)`. `comet::cloudflare::local(...)`
remains available for manual compatibility cases outside normal route
codegen.

### R2 object responses

`comet::cloudflare::R2Object` is the Worker-side replacement path for routes
that would otherwise reach for local filesystem responders such as
`NamedFile`. It streams an R2 object body and copies R2 HTTP metadata into the
Rocket response:

```rust
struct Assets;

impl comet::cloudflare::BindingName for Assets {
    const NAME: &'static str = "ASSETS";
}

#[get("/assets/<key..>")]
async fn asset(
    key: std::path::PathBuf,
    bucket: comet::cloudflare::R2Bucket<Assets>,
) -> Option<comet::cloudflare::R2Object> {
    let key = key.to_string_lossy().replace('\\', "/");
    comet::cloudflare::R2Object::get(&bucket, key).await.ok().flatten()
}
```

### WebSocket routes

WebSockets use normal Rocket route mounting with a Worker-specific request
guard and response type. Enable `comet`'s `cloudflare-websocket` feature to use
these types:

```rust
#[get("/ws/echo")]
async fn websocket_echo(
    ws: comet::cloudflare::WebSocketUpgrade,
) -> comet::cloudflare::WebSocketResponse {
    ws.accept(|socket| async move {
        // drive socket.events() here
        Ok(())
    })
}
```

The Worker entrypoint does not need a path-specific upgrade branch; it keeps
calling `comet::cloudflare::fetch(req, env, ctx, rocket).await`.

## Setup

Create a D1 database, KV namespace, queue, and R2 bucket, and wire them into
`wrangler.jsonc`:

```sh
npx wrangler d1 create comet-cloudflare-worker-example
npx wrangler kv namespace create AUTH_KV
npx wrangler queues create task-events
npx wrangler r2 bucket create comet-cloudflare-worker-example-assets
```

Copy the `database_id` from the first command's output into the
`d1_databases[0].database_id` field in `wrangler.jsonc` and the KV namespace
`id` into `kv_namespaces[0].id` (both currently have placeholders). Then apply
migrations:

```sh
# local (used by `wrangler dev`)
npx wrangler d1 migrations apply DB --local

# remote (used by `wrangler deploy`)
npx wrangler d1 migrations apply DB --remote
```

For local development, set the public callback base URL and a token pepper:

```sh
npx wrangler secret put COMET_AUTH_TOKEN_PEPPER
npx wrangler secret put COMET_AUTH_BASE_URL
```

Use `http://localhost:8787` for `COMET_AUTH_BASE_URL` when running
`npm run dev`. For OAuth providers, configure the provider-specific secrets
from [`../../docs/auth.md`](../../docs/auth.md). The redirect URIs are:

- Google: `<COMET_AUTH_BASE_URL>/auth/google/callback`
- Apple: `<COMET_AUTH_BASE_URL>/auth/apple/callback`
- GitHub: `<COMET_AUTH_BASE_URL>/auth/github/callback`

## Run Locally

```sh
cd examples/cloudflare-worker
npm install
npm run dev
```

Then exercise it:

```sh
curl http://localhost:8787/
curl -X POST http://localhost:8787/echo -d 'hello'

curl -X POST http://localhost:8787/tasks \
  -H 'content-type: application/json' \
  -d '{"title":"write comet docs"}'

curl http://localhost:8787/tasks
curl -X POST http://localhost:8787/tasks/1/complete

curl http://localhost:8787/auth/session
curl -i http://localhost:8787/private/me

curl -X PUT http://localhost:8787/assets/hello.txt --data-binary 'hello from R2'
curl http://localhost:8787/assets/hello.txt
```

After completing a task, check that the queue consumer recorded both
lifecycle events (local queues flush within `max_batch_timeout`, 5s per
`wrangler.jsonc`):

```sh
npx wrangler d1 execute DB --local --command "SELECT * FROM task_events"
```

## Build Check

```sh
cd examples/cloudflare-worker
rustup target add wasm32-unknown-unknown
npm run check
```

## Tests

Unit tests cover the model layer (serialization, the `done` integer/bool
mapping, title validation) and run natively, no wasm toolchain needed:

```sh
npm run test
```

`tests/integration.sh` drives a real `wrangler dev` instance end to end: it
resets local D1 state, applies migrations, starts the worker, exercises every
public route over HTTP, verifies `/auth/session`, verifies that `/private/me`
and `/private/admin` return `401` without a session, confirms provider startup
fails cleanly when local OAuth secrets are absent, confirms the queue consumer actually wrote the
`task_events` audit trail, round-trips a 1MiB object through R2, verifies the
`/ws/echo` WebSocket route, and proves request/response bodies are
genuinely streamed rather than buffered — a 1MiB `/echo` body round-trips
exactly, and `/stream`'s time-to-first-byte is checked against its total
response time (streamed: first byte in a few ms, full response ~1.2s;
buffered: both would be ~1.2s). It needs `rustup` with the
`wasm32-unknown-unknown` target, `jq`, and `npm install` already run:

```sh
npm run test:integration
```

`tests/perf.sh` load-tests the same `wrangler dev` instance with
[autocannon](https://github.com/mcollina/autocannon) to measure requests/sec:
once against `GET /` (pure Rocket + comet adapter overhead, no D1/Queue) and
once against `GET /tasks` (a real D1-backed read). It's a measurement, not a
strict gate — there's no hardcoded req/sec threshold, since throughput
depends heavily on the machine it runs on. It only fails if requests
actually error out or return non-2xx under load:

```sh
npm run test:perf

# more load, longer run:
COMET_PERF_DURATION=30 COMET_PERF_CONNECTIONS=50 npm run test:perf
```

## Rocket Patch

The current published Rocket release still pulls in native server/runtime pieces
that do not compile for Cloudflare Workers. This example depends on
`../../vendor/rocket/core/lib`, a vendored copy of Rocket with
[`patches/rocket-worker-feature.patch`](../../patches/rocket-worker-feature.patch)
and [`patches/rocket-worker-streaming-request.patch`](../../patches/rocket-worker-streaming-request.patch)
applied — see [`vendor/rocket/COMET_NOTES.md`](../../vendor/rocket/COMET_NOTES.md)
for provenance and how to refresh it. Vendoring keeps the whole example
buildable straight after `git clone`, with no separate clone-and-patch step
and no dependency on a machine-local checkout.
