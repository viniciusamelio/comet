# Testing Comet Apps

Comet supports three practical testing layers. Use the cheapest layer that
exercises the behavior you care about, then keep a smaller number of
`wrangler dev` tests for Worker-only behavior.

## Native Request/Response Tests

Use `RocketWorker` when you want fast native tests for normal Rocket routes,
status codes, headers, JSON guards/responders, and buffered request/response
bodies.

`RocketWorker` is available through the `native-client` feature, which is
enabled by default. It dispatches `WorkerRequest` values through Rocket's local
async client and returns `WorkerResponse` values.

```rust
use comet::{RocketWorker, WorkerRequest};

#[rocket::async_test]
async fn get_index() {
    let app = my_app::rocket();
    let worker = RocketWorker::new(app).await.unwrap();

    let response = worker.dispatch(WorkerRequest::get("/")).await.unwrap();

    assert_eq!(response.status, 200);
    assert_eq!(response.body.into_bytes().unwrap(), b"hello");
}
```

For request bodies and headers:

```rust
use comet::{RocketWorker, WorkerRequest};

#[rocket::async_test]
async fn post_json() {
    let app = my_app::rocket();
    let worker = RocketWorker::new(app).await.unwrap();

    let response = worker
        .dispatch(
            WorkerRequest::post("/echo", br#"{ "value": "ok" }"#.to_vec())
                .header("content-type", "application/json"),
        )
        .await
        .unwrap();

    assert_eq!(response.status, 200);
}
```

Run these tests with:

```sh
cargo test --features native-client
```

Nuances:

- This path is intentionally buffered. It is not a streaming test.
- It does not create a `worker::Env`, `worker::Context`, D1 database, R2
  bucket, Queue, KV namespace, service binding, Hyperdrive binding, or
  WebSocket pair.
- It is the best default for pure route behavior because failures are normal
  Rust test failures and do not require a wasm build or `wrangler`.

## Cloudflare Adapter Dispatch Tests

Use the `cloudflare::Application` trait when you want to exercise Comet's
Cloudflare dispatch path natively without starting `wrangler dev`.

```rust
use comet::cloudflare::Application;
use comet::{WorkerBody, WorkerRequest};

#[rocket::async_test]
async fn dispatches_through_cloudflare_adapter() {
    let app = my_app::rocket();

    let response = app.dispatch(WorkerRequest::get("/")).await.unwrap();

    assert_eq!(response.status, 200);
    assert!(matches!(response.body, WorkerBody::Buffered(_)));
}
```

Run these tests with the Cloudflare feature set your app needs:

```sh
cargo test --no-default-features --features cloudflare
```

Nuances:

- This path uses Comet's `Application for Rocket<Build>` implementation, so it
  exercises the external Rocket dispatch adapter.
- It still does not provide real Worker bindings. Routes that require D1, R2,
  Queue, KV, service bindings, Hyperdrive, or real WebSockets should be covered
  by integration tests.
- Large or unknown-size responses may be returned as `WorkerBody::Streamed`.
  Tests that need to assert the body bytes should either use a small known-size
  responder or explicitly consume the stream.

## Worker Integration Tests

Use `wrangler dev` tests for behavior that only exists in the Worker runtime:

- D1 queries and migrations
- R2 object upload/download
- Queue producers and consumers
- KV, service binding, and Hyperdrive integration
- WebSocket upgrade behavior
- request/response streaming behavior through `worker::Request` and
  `worker::Response`
- wasm build and Worker compatibility

The example app includes shell-driven integration tests:

```sh
cd examples/cloudflare-worker
npm install
npm run test:integration
```

The same example has a performance smoke test:

```sh
npm run test:perf
```

Nuances:

- Do not run integration and performance tests in parallel against the same
  example directory. Both reset local D1 state.
- These tests are slower and require the Worker toolchain, but they are the
  only layer that proves bindings and runtime-specific behavior end to end.
- Keep route-level unit tests native where possible, and reserve `wrangler dev`
  for the cases above.

## Suggested Test Split

- Pure route logic: `RocketWorker`.
- Comet adapter behavior: `cloudflare::Application::dispatch`.
- Binding/runtime behavior: `wrangler dev`.
- Serialization, validation, and domain logic: ordinary Rust unit tests that
  do not start Rocket at all.
