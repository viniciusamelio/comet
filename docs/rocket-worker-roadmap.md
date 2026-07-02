# Rocket on Cloudflare Workers Roadmap

This repository started with a native proof of concept: `RocketWorker` receives
a Worker-shaped request, dispatches it through Rocket's local async client, and
returns a Worker-shaped response.

The prototype validates the important framework behavior first:

- Rocket route matching
- request headers and body forwarding
- JSON data guards
- Rocket responders
- response status, headers, and body extraction

## Current Shape

The native-client adapter is still intentionally buffered:

```rust
let app = rocket::build().mount("/", rocket::routes![index]);
let worker = RocketWorker::new(app).await?;
let response = worker.dispatch(WorkerRequest::get("/")).await?;
```

The Cloudflare adapter is no longer just that proof of concept. It now:

- converts `worker::Request` into Rocket request metadata
- streams Worker request bodies into Rocket `Data`
- dispatches directly through patched Rocket's `dispatch_external()`
- streams large or unknown-size Rocket response bodies back to Workers
- buffers known small response bodies to avoid the streaming machinery
- can cache an already-ignited `Rocket<Orbit>` per Worker isolate with
  `serve_cached(req, || rocket(env))`

The remaining work is tracked task-by-task in
[`docs/worker-implementation-tracker.md`](worker-implementation-tracker.md).

## Target Shape

The end state should be a `rocket-worker` crate that exposes a fetch adapter:

```rust
#[event(fetch)]
async fn main(
    req: worker::Request,
    env: worker::Env,
    ctx: worker::Context,
) -> worker::Result<worker::Response> {
    comet::cloudflare::fetch(req, env, ctx, rocket).await
}
```

For application authors, the target development experience should preserve
Rocket's route syntax and keep Worker conversion details out of user code:

```rust
#[macro_use]
extern crate rocket;

#[get("/")]
fn index() -> &'static str {
    "hello from Rocket on Cloudflare Workers"
}

fn rocket(_env: worker::Env, _ctx: worker::Context) -> rocket::Rocket<rocket::Build> {
    rocket::build().mount("/", routes![index])
}

#[event(fetch)]
async fn main(
    req: worker::Request,
    env: worker::Env,
    ctx: worker::Context,
) -> worker::Result<worker::Response> {
    comet::cloudflare::fetch(req, env, ctx, rocket).await
}
```

Internally, that adapter should:

- convert `worker::Request` into Rocket request metadata
- expose request body data as Rocket `Data`
- dispatch through Rocket without opening sockets
- convert Rocket `Response` into `worker::Response`
- make Cloudflare bindings available to Rocket guards or managed state

## Required Rocket Changes

Rocket does not currently compile to `wasm32-unknown-unknown` because the core
crate always pulls in Tokio networking through Hyper server dependencies. A
baseline check against the Rocket repository with:

```sh
RUSTC="$(rustup which rustc)" "$(rustup which cargo)" \
  check -p rocket --target wasm32-unknown-unknown --no-default-features
```

currently fails in `mio`:

```text
This wasm target is unsupported by mio. If using Tokio, disable the net feature.
```

The first upstream patch should separate Rocket into transport-independent and
server-enabled surfaces:

- add a `server` feature enabled by default
- move Hyper server, Hyper util, Tokio `net`, Tokio `fs`, Tokio `signal`, TLS,
  HTTP/2, and HTTP/3 dependencies behind `server` or narrower features
- make local dispatch and core routing compile without `server`
- expose a public lifecycle API that reaches `Rocket<Orbit>` without binding a
  listener
- expose a public or semi-public dispatch API equivalent to the internal
  `preprocess()` + `dispatch()` flow

An initial patch for this split is stored at:

```text
patches/rocket-worker-feature.patch
```

Apply it from the root of a Rocket checkout:

```sh
git apply /path/to/comet/patches/rocket-worker-feature.patch
```

Both `comet` and `examples/cloudflare-worker` currently depend on this
already applied, at `vendor/rocket/core/lib` — see
`vendor/rocket/COMET_NOTES.md` for why it's vendored rather than referenced
via a `path`/`git` dependency outside the repo, and how to refresh it against
a newer Rocket commit.

A second patch, layered on top, adds the `RawStream::Worker` variant and
`Data::from_stream()` constructor the streaming adapter (below) needs:

```text
patches/rocket-worker-streaming-request.patch
```

The patch was validated against Rocket commit `3a54d07` with:

```sh
RUSTC="$(rustup which rustc)" "$(rustup which cargo)" \
  check -p rocket

RUSTC="$(rustup which rustc)" "$(rustup which cargo)" \
  check -p rocket --target wasm32-unknown-unknown \
  --no-default-features --features worker
```

The Wasm check currently passes with warnings. The warnings mostly come from
server-only code paths that are still compiled but unused under `worker`; a
follow-up cleanup should reduce those with narrower `cfg(feature = "server")`
guards.

The patch also adds the first lifecycle and direct-dispatch APIs needed by an
adapter:

```rust
let rocket = rocket::build()
    .mount("/", rocket::routes![index])
    .orbit_external()
    .await?;
```

```rust
let mut request = rocket::Request::new(
    &rocket,
    rocket::http::Method::Get,
    rocket::http::uri::Origin::parse("/").unwrap(),
    None,
);

let data = rocket::Data::local(Vec::new());
let response = rocket.dispatch_external(&mut request, data).await;
```

This avoids using `rocket::local::asynchronous::Client` as the integration
point. The response can borrow from the request, so the adapter must keep the
request alive while converting the response into a Worker response.

A reference implementation of the buffered direct-dispatch adapter is stored in:

```text
docs/direct-dispatch-adapter.rs
```

It was validated in a temporary consumer crate against the patched Rocket with:

```sh
RUSTC="$(rustup which rustc)" "$(rustup which cargo)" test --lib
```

The validation covers a plain GET route and a JSON POST route without using
Rocket's local client or server launch path.

## Feature Tiers

1. Buffered MVP
   - Read the Worker request body into memory.
   - Read Rocket response bodies into memory.
   - Suitable for small requests and proving route/guard/responder behavior.

2. Streaming adapter — **done.** Neither the request nor the response body is
   buffered into memory before Rocket/Workers can start working with it.
   Proven, not just compiled: `examples/cloudflare-worker`'s
   `tests/integration.sh` posts a 1MiB body and checks a byte-for-byte
   round-trip, and hits a `/stream` route (3 chunks, real `worker::Delay`
   between each — not a tokio timer, which wouldn't run under Workers at all)
   asserting time-to-first-byte is a small fraction of total response time.

   `WorkerRequest`/`WorkerResponse.body` is now `WorkerBody`, an enum of
   `Buffered(Vec<u8>)` (what `native-client` always uses) or
   `Streamed(Pin<Box<dyn Stream<Item = io::Result<Bytes>>>>)` (what the
   `cloudflare` adapter always uses, both directions).

   **Request body → Rocket `Data`.** Needed an actual patch to the vendored
   Rocket (`patches/rocket-worker-streaming-request.patch`, layered on
   `rocket-worker-feature.patch`): `RawStream<'r>` (`data/data_stream.rs`) was
   a closed, non-`pub` enum with no extension point, so it gained a
   `#[cfg(feature = "worker")] Worker(Pin<Box<dyn Stream<Item =
   io::Result<Bytes>> + Send + 'r>>)` variant (wired into `poll_next`/
   `size_hint`/`Display`), plus a `pub fn Data::from_stream()` constructor.
   The stream doesn't have to actually be `Send` — the constructor wraps it in
   `send_wrapper::SendWrapper` internally (Rocket gained its own
   `send_wrapper` dependency, `worker`-feature-gated, for exactly this),
   same justification as `comet::cloudflare::local()`. On the `comet` side,
   `worker::Request::stream()` (a `ByteStream`, `Item =
   worker::Result<Vec<u8>>`) maps onto this directly; a bodyless request
   (`req.stream()` erroring because there's no underlying `ReadableStream` at
   all — the common case, e.g. every `GET`) falls back to
   `WorkerBody::Buffered(vec![])`.

   **Response body → Worker stream.** No Rocket patch needed:
   `rocket::response::Body<'r>` already implements `tokio::io::AsyncRead`
   (works fine on wasm32 — it's tokio's IO utilities, not its reactor/net
   feature), so chunked reading is just `AsyncReadExt::read()` in a loop. The
   real obstacle was `'static`: `worker::Response::from_stream<S>` requires
   `S: TryStream + 'static`, but `Body<'r>`'s `'r` is tied to the
   `Rocket<Orbit>`/`Request<'r>`/`Response<'r>` trio built fresh per request —
   all local to `Application::dispatch`'s `async move` block, and status/
   headers are only known once `dispatch_external()` has actually run the
   route (side effects included), so they can't be produced eagerly either.
   An earlier version of this doc proposed solving the `'static` requirement
   with `wasm_bindgen_futures::spawn_local` + an `mpsc` channel; that turned
   out to be unnecessary complexity. What's actually there: the whole
   dispatch — using an already-ignited Rocket when available, building the request, running
   `dispatch_external()`, then looping over `body_mut().read()` — is one
   `async_stream::try_stream!` block (Rocket already depends on `async-stream`
   for its own `response::stream` module, re-exported as `rocket::async_stream`).
   Because it's one continuous generator, `rocket`/`rocket_request`/`response`
   can safely self-reference each other the same way they always could inside
   a single `async fn` body — no separate task, no channel. Status/headers are
   sent out through a `futures_channel::oneshot` the instant they're known
   (strictly before the first yielded byte, or before the generator ends if
   the body is empty); `dispatch()` drives the stream exactly one item to
   guarantee that oneshot has already fired, splices the possible first chunk
   back onto the front with `stream::iter(..).chain(..)`, and returns a
   `WorkerResponse` with the now-known status/headers and the remainder as a
   `WorkerBody::Streamed`. If the response body has a known preset size at or
   below the small-body threshold, the adapter reads it directly into
   `WorkerBody::Buffered` instead of allocating the 64KiB streaming scratch
   buffer.

   Streaming *responses* built with Rocket's own `response::stream` module
   (`ByteStream!`/`TextStream!`/etc.) that await a `worker` primitive between
   yields hit the exact same `Future`-vs-`Send` problem `local()` solves, one
   level up (`Responder for ByteStream<S>` requires `S: Send`). `comet`
   exposes `comet::cloudflare::local_stream()` for this — same wrapper, same
   justification, for `Stream` instead of `Future`.

3. Cloudflare bindings
   - Inject `worker::Env` into request-local state.
   - Provide guards for KV, R2, D1, Queues, service bindings, and Hyperdrive.
   - `examples/cloudflare-worker` demonstrates a working version of this tier:
     `Env` is injected via Rocket managed state (`.manage(env)`), typed
     guards pull named D1/Queue bindings from it, and routes await those
     Worker futures directly. Under Rocket's `worker` feature, route and
     catcher handler futures are local-boxed instead of `Send`-boxed, matching
     the single-threaded Worker isolate model. `comet::cloudflare::local()`
     remains as a compatibility helper for manual futures, while normal route
     code no longer needs it. The patch file is regenerated from the current
     vendored Rocket delta.

4. Worker-specific exclusions
   - Do not support Rocket socket launch APIs in Workers.
   - Do not support filesystem-backed `FileServer`, `NamedFile`, or disk-backed
     `TempFile` in Workers. Workers do not expose a durable local filesystem
     for request-serving assets, so `comet` should provide storage-backed
     alternatives instead of making these responders appear portable.
   - R2 is the first storage-backed replacement path: `R2Bucket<B>` names the
     binding, and `R2Object` can be returned from a route to stream an object
     body with R2 HTTP metadata, ETag, `Content-Length`, and optional ranged
     reads through `R2Object::get_range()`. Range support is explicit: the
     helper accepts `worker::Range` and emits `206 Partial Content` plus
     `Content-Range` when R2 reports the served range. Automatic parsing of
     incoming HTTP `Range` headers is intentionally left as follow-up API
     design, not hidden inside the responder.
   - Handle WebSockets with Cloudflare's Worker WebSocket APIs, not Hyper
     upgrades. The preferred API is still a normal Rocket route:
     `WebSocketUpgrade` validates the upgrade request and
     `WebSocketResponse` carries the accepted socket task back to the adapter,
     which returns a real Worker `WebSocketPair` response. The lower-level
     `is_websocket_upgrade()` and `websocket_response()` helpers remain
     available for manual fetch-level handling. The example's `/ws/echo`
     endpoint is covered by integration tests.

## Execution Tracking

Implementation progress for the remaining roadmap is tracked in
[`docs/worker-implementation-tracker.md`](worker-implementation-tracker.md).
Every task there has an ID, status, owner field, target files, and completion
criteria so multiple agents can coordinate without relying on conversation
history.
