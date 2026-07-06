use super::{Application, local, local_stream};
use crate::{WorkerBody, WorkerRequest};
use futures_util::Stream;
use std::cell::RefCell;
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::task::{Context, Poll};

/// Mirrors the shape of `wasm_bindgen_futures::JsFuture`: a `!Send`
/// future that resolves immediately on first poll.
struct NotSendFuture(Rc<RefCell<i32>>);

impl Future for NotSendFuture {
    type Output = i32;

    fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<i32> {
        Poll::Ready(*self.0.borrow())
    }
}

/// Mirrors a `!Send` streaming responder awaiting `worker::Delay`
/// between yields: an `Rc`-backed stream that yields its remaining
/// items one per poll.
struct NotSendStream(Rc<RefCell<Vec<i32>>>);

impl Stream for NotSendStream {
    type Item = i32;

    fn poll_next(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Option<i32>> {
        Poll::Ready(self.0.borrow_mut().pop())
    }
}

fn assert_send<T: Send>(_: T) {}
fn assert_send_sync_type<T: Send + Sync>() {}

struct TestBinding;

impl super::BindingName for TestBinding {
    const NAME: &'static str = "TEST_BINDING";
}

#[rocket::get("/small")]
fn small() -> &'static str {
    "ok"
}

#[rocket::get("/large")]
fn large() -> String {
    "x".repeat(super::SMALL_BODY_THRESHOLD + 1)
}

#[rocket::get("/empty")]
fn empty() -> rocket::http::Status {
    rocket::http::Status::NoContent
}

#[test]
fn local_wraps_a_non_send_future_into_a_send_one() {
    let fut = local(async {
        let rc = Rc::new(RefCell::new(41));
        NotSendFuture(rc).await + 1
    });

    // This is the property `local` exists for: Rocket's route dispatch
    // requires `Future + Send`, but D1/Queue calls resolve through a
    // `!Send` future. If this didn't type-check, `local` would be
    // useless for its one job.
    assert_send(&fut);
}

#[test]
fn local_still_resolves_the_wrapped_future() {
    let mut fut = Box::pin(local(async {
        let rc = Rc::new(RefCell::new(41));
        NotSendFuture(rc).await + 1
    }));

    let waker = std::task::Waker::noop();
    let mut cx = Context::from_waker(waker);
    match fut.as_mut().poll(&mut cx) {
        Poll::Ready(value) => assert_eq!(value, 42),
        Poll::Pending => panic!("expected the wrapped future to resolve immediately"),
    }
}

#[test]
fn local_stream_wraps_a_non_send_stream_into_a_send_one() {
    let stream = local_stream(NotSendStream(Rc::new(RefCell::new(vec![3, 2, 1]))));

    assert_send(&stream);
}

#[test]
fn local_stream_still_yields_the_wrapped_items() {
    let mut stream = Box::pin(local_stream(NotSendStream(Rc::new(RefCell::new(vec![
        3, 2, 1,
    ])))));

    let waker = std::task::Waker::noop();
    let mut cx = Context::from_waker(waker);

    let mut items = Vec::new();
    loop {
        match stream.as_mut().poll_next(&mut cx) {
            Poll::Ready(Some(item)) => items.push(item),
            Poll::Ready(None) => break,
            Poll::Pending => panic!("expected NotSendStream to never be pending"),
        }
    }

    assert_eq!(items, vec![1, 2, 3]);
}

#[rocket::async_test]
async fn dispatch_buffers_known_small_response_bodies() {
    let app = rocket::build().mount("/", rocket::routes![small]);
    let response = app.dispatch(WorkerRequest::get("/small")).await.unwrap();

    match response.body {
        WorkerBody::Buffered(bytes) => assert_eq!(bytes, b"ok"),
        WorkerBody::Streamed(_) => panic!("expected small known-size body to be buffered"),
    }
}

#[rocket::async_test]
async fn dispatch_buffers_empty_response_bodies() {
    let app = rocket::build().mount("/", rocket::routes![empty]);
    let response = app.dispatch(WorkerRequest::get("/empty")).await.unwrap();

    assert_eq!(response.status, 204);
    match response.body {
        WorkerBody::Buffered(bytes) => assert!(bytes.is_empty()),
        WorkerBody::Streamed(_) => panic!("expected empty body to be buffered"),
    }
}

#[rocket::async_test]
async fn dispatch_streams_large_response_bodies() {
    let app = rocket::build().mount("/", rocket::routes![large]);
    let response = app.dispatch(WorkerRequest::get("/large")).await.unwrap();

    match response.body {
        WorkerBody::Buffered(_) => panic!("expected large body to remain streamed"),
        WorkerBody::Streamed(_) => {}
    }
}

#[test]
fn binding_guards_are_send_and_sync_route_inputs() {
    #[cfg(feature = "cloudflare-d1")]
    assert_send_sync_type::<super::D1<TestBinding>>();
    #[cfg(feature = "cloudflare-queue")]
    assert_send_sync_type::<super::QueueBinding<TestBinding>>();
    #[cfg(feature = "cloudflare-kv")]
    assert_send_sync_type::<super::Kv<TestBinding>>();
    #[cfg(feature = "cloudflare-r2")]
    assert_send_sync_type::<super::R2Bucket<TestBinding>>();
    #[cfg(feature = "cloudflare-service")]
    assert_send_sync_type::<super::ServiceBinding<TestBinding>>();
    #[cfg(feature = "cloudflare-hyperdrive")]
    assert_send_sync_type::<super::Hyperdrive<TestBinding>>();
}
