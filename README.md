# comet

Run [Rocket](https://rocket.rs) routes on Cloudflare Workers.

Rocket's published releases pull in Hyper/Tokio networking that doesn't
compile to `wasm32-unknown-unknown`. `comet` pairs a patched Rocket (vendored
in this repo, see [`vendor/rocket/COMET_NOTES.md`](vendor/rocket/COMET_NOTES.md))
with an adapter that dispatches `worker::Request`s straight through Rocket's
routing, guards, and responders — no socket, no Hyper — and converts the
result back into a `worker::Response`.

```rust
use worker::{event, Context, Env, Request, Response, Result};

#[macro_use]
extern crate rocket;

#[get("/")]
fn index() -> &'static str {
    "hello from Rocket on Cloudflare Workers"
}

fn rocket(env: Env, _ctx: Context) -> rocket::Rocket<rocket::Build> {
    rocket::build().manage(env).mount("/", routes![index])
}

#[event(fetch)]
pub async fn main(req: Request, env: Env, ctx: Context) -> Result<Response> {
    comet::cloudflare::fetch(req, env, ctx, rocket).await
}
```

See [`examples/cloudflare-worker`](examples/cloudflare-worker) for a complete
app: a D1-backed task API with async D1/Queue calls, custom struct
(de)serialization, R2 object streaming, a Worker WebSocket echo endpoint, an
async queue consumer, and both unit and end-to-end tests.

## Status

This is an early-stage adapter, not a finished framework integration. What
works today:

- Request and response bodies are streamed through Rocket, not buffered —
  neither side is read fully into memory before the other can start working
  with it. `examples/cloudflare-worker`'s `tests/integration.sh` proves this
  isn't just "it compiles": a `/stream` route response's time-to-first-byte
  is checked against its total time, and a 1MiB request body is checked for
  an exact round-trip. See "Streaming adapter" in
  [the roadmap](docs/rocket-worker-roadmap.md) for how.
- JSON data guards and responders.
- Cloudflare bindings (`Env`, D1, Queues, KV, R2, service bindings, and
  Hyperdrive) via Rocket managed state — `comet::cloudflare::D1`,
  `QueueBinding`, `Kv`, `R2Bucket`, `ServiceBinding`, and `Hyperdrive`
  provide typed request guards for named bindings. Worker builds use
  local-boxed Rocket route futures, so routes can await D1/Queue calls
  directly. `comet::cloudflare::local()` remains available for manual
  compatibility cases, and `local_stream()` bridges Rocket's `Send`-bound
  streaming responders with `!Send` Worker streams.
- R2-backed object responses via `comet::cloudflare::R2Object`, which streams
  an object body through Rocket, preserves R2 HTTP metadata, and avoids
  pretending that local filesystem responders work in Workers.
- Worker WebSocket routes via `WebSocketUpgrade` and `WebSocketResponse` when
  the `cloudflare-websocket` feature is enabled. WebSockets still become
  Cloudflare `WebSocketPair` responses under the hood, but applications can
  mount them with normal Rocket route syntax.

What's not there yet: full storage-backed replacements for filesystem APIs
such as `FileServer`, `NamedFile`, and disk-backed `TempFile`. See
[`docs/rocket-worker-roadmap.md`](docs/rocket-worker-roadmap.md) for the full
plan.

## Using this in another project

`comet` isn't published to crates.io yet. Verified with `cargo package`:

```text
error: failed to verify manifest at `.../Cargo.toml`
Caused by:
  all dependencies must have a version requirement specified when packaging.
  dependency `rocket` does not specify a version
  Note: The packaged dependency will use the version from crates.io,
  the `path` specification will be removed from the dependency declaration.
```

Adding a `version` to the `rocket` path dependency would silence that error,
but it wouldn't fix anything: crates.io replaces path dependencies with the
matching *registry* version at publish time, so consumers would get the
unpatched, non-`worker`-featured Rocket instead of the fork vendored here.
Publishing `comet` to crates.io needs the patched Rocket fork to have its own
public, versioned home first (either published under a different crate name,
or `rocket-worker-feature.patch` upstreamed) — see
[`docs/rocket-worker-roadmap.md`](docs/rocket-worker-roadmap.md).

Until then, depend on it via git once this repository has a public remote:

```toml
[dependencies]
comet = { git = "https://github.com/viniciusamelio/comet", default-features = false, features = ["cloudflare"] }
```

A `git` dependency clones the whole repo, so the vendored `rocket` path
dependency resolves the same way it does locally — no extra setup needed on
the consumer's side.

## Features

- `native-client` (default): a `RocketWorker` that dispatches
  `WorkerRequest`/`WorkerResponse` through Rocket's local async client.
  Useful for testing the request/response shapes without a `worker` runtime.
- `cloudflare`: the `comet::cloudflare` module — `fetch()`, `FetchApp`,
  `serve()`, the `Application` impl for `Rocket<Build>`, `local()`, and
  `local_stream()`. Requires `worker`.
- `cloudflare-d1`, `cloudflare-queue`, `cloudflare-kv`, `cloudflare-r2`,
  `cloudflare-service`, `cloudflare-hyperdrive`: typed request guards for the
  corresponding Cloudflare bindings. `cloudflare-r2` also enables the
  `R2Object` responder.
- `cloudflare-websocket`: `WebSocketUpgrade`, `WebSocketResponse`, and
  low-level Worker WebSocket helpers. Keep it off for HTTP-only Workers.

## Development

See [`docs/testing.md`](docs/testing.md) for how to choose between native
route tests, Comet adapter dispatch tests, and `wrangler dev` integration
tests.

```sh
cargo test --features native-client   # adapter tests, run natively
cargo test --no-default-features --features cloudflare   # also runs natively
cargo bench --bench native_adapter   # adapter-only performance baseline
cd examples/cloudflare-worker && npm install && npm run test && npm run test:integration
```

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option. The vendored Rocket fork under
`vendor/rocket` retains its own original MIT/Apache-2.0 licensing from the
[Rocket project](https://github.com/rwf2/Rocket).
