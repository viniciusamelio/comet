# Rocket Worker Upstream Notes

This is the issue/PR draft for the vendored Rocket delta in `comet`.

## Problem

Cloudflare Workers need Rocket's routing, guards, responders, and body
machinery without Rocket's native socket server, Hyper listener stack, Tokio
runtime assumptions, or `Send` route-future requirement. Workers run request
handling inside a JavaScript isolate, expose fetch requests/responses directly,
and provide many binding futures that are `!Send`.

## Proposed Patch Split

1. Add a `worker` feature that builds Rocket's transport-independent core on
   `wasm32-unknown-unknown`.
2. Gate native server/listener/launch APIs behind the existing `server`
   surface instead of compiling them for Worker builds.
3. Expose external dispatch hooks:
   - `Rocket<Build>::orbit_external()`
   - `Rocket<Orbit>::dispatch_external(...)`
   - `dispatch_external()` uses the same `'rocket: 'request` relationship as
     the internal `dispatch()` instead of tying the Rocket borrow, request
     borrow, body data, and response to one lifetime.
4. Add streaming request body construction for external adapters:
   - `RawStream::Worker`
   - `Data::from_stream(...)`
5. Under `worker`, use local-boxed route/catcher futures and local async
   response bounds so route handlers can await `!Send` Worker futures.

## Comet Adapter Responsibilities

The Rocket patch should not know about Cloudflare-specific bindings. `comet`
owns the Worker adapter layer: converting `worker::Request`/`Response`, caching
the ignited `Rocket<Orbit>` per isolate, exposing typed binding guards, and
bridging Rocket-style WebSocket routes to Worker `WebSocketPair` responses.

## Validation

Current branch validation as of 2026-07-02:

```sh
cargo fmt --check
cargo test --features cloudflare,cloudflare-d1,cloudflare-queue,cloudflare-kv,cloudflare-r2,cloudflare-service,cloudflare-hyperdrive,cloudflare-websocket
cargo check --manifest-path examples/cloudflare-worker/Cargo.toml
cd examples/cloudflare-worker && npm run test:integration
cd examples/cloudflare-worker && npm run test:perf
RUSTC="$(rustup which rustc)" "$(rustup which cargo)" check --manifest-path vendor/rocket/core/lib/Cargo.toml
RUSTC="$(rustup which rustc)" "$(rustup which cargo)" check --manifest-path vendor/rocket/core/lib/Cargo.toml --target wasm32-unknown-unknown --no-default-features --features worker
```

Known warning baseline: native builds report existing `cfg(nightly)` and
`rust_analyzer` check-cfg warnings. Worker builds also report unused
server-oriented items that still compile in the transport-independent core.
Those warnings are not functional blockers for the Worker adapter.

## Open Upstream Questions

- Whether the feature should be named `worker`, `external-dispatch`, or split
  into separate compile-target and external-dispatch features.
- Whether local-boxed route futures should be tied specifically to Worker
  builds or exposed as a more general single-threaded runtime mode.
- Whether `Data::from_stream(...)` should be Worker-specific or a generic
  external request body constructor.
