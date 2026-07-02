use std::pin::Pin;

#[cfg(feature = "native-client")]
use rocket::http::{Header, Method};
#[cfg(feature = "native-client")]
use rocket::local::asynchronous::Client;
#[cfg(feature = "native-client")]
use rocket::{Build, Rocket};

#[derive(Debug, thiserror::Error)]
pub enum AdapterError {
    #[error("invalid HTTP method: {0}")]
    InvalidMethod(String),
    #[cfg(feature = "native-client")]
    #[error("rocket failed to ignite: {0}")]
    Rocket(#[from] rocket::Error),
    #[error("failed to read response body")]
    Body,
    #[error("streamed bodies are not supported by the native-client adapter")]
    UnsupportedStreamedBody,
}

/// A byte stream not yet fully read. `'static` and not required to be
/// `Send`: the only producers/consumers of this today run on
/// `wasm32-unknown-unknown` under Workers, which has no threads.
pub type BoxedByteStream = Pin<Box<dyn futures_util::Stream<Item = std::io::Result<bytes::Bytes>>>>;

/// The body of a [`WorkerRequest`] or [`WorkerResponse`].
///
/// `native-client` only ever produces or accepts [`WorkerBody::Buffered`].
/// The `cloudflare` adapter uses [`WorkerBody::Streamed`] so request/response
/// bodies don't have to be read into memory in full before Rocket can start
/// working with them.
pub enum WorkerBody {
    Buffered(Vec<u8>),
    Streamed(BoxedByteStream),
}

impl WorkerBody {
    pub fn is_empty(&self) -> bool {
        matches!(self, WorkerBody::Buffered(bytes) if bytes.is_empty())
    }

    /// Returns the buffered bytes, or `None` if this body is streamed.
    pub fn into_bytes(self) -> Option<Vec<u8>> {
        match self {
            WorkerBody::Buffered(bytes) => Some(bytes),
            WorkerBody::Streamed(_) => None,
        }
    }
}

impl std::fmt::Debug for WorkerBody {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WorkerBody::Buffered(bytes) => f.debug_tuple("Buffered").field(&bytes.len()).finish(),
            WorkerBody::Streamed(_) => f.write_str("Streamed(..)"),
        }
    }
}

impl From<Vec<u8>> for WorkerBody {
    fn from(bytes: Vec<u8>) -> Self {
        WorkerBody::Buffered(bytes)
    }
}

#[derive(Debug)]
pub struct WorkerRequest {
    pub method: String,
    pub uri: String,
    pub headers: Vec<(String, String)>,
    pub body: WorkerBody,
}

impl WorkerRequest {
    pub fn get(uri: impl Into<String>) -> Self {
        Self {
            method: "GET".into(),
            uri: uri.into(),
            headers: Vec::new(),
            body: WorkerBody::Buffered(Vec::new()),
        }
    }

    pub fn post(uri: impl Into<String>, body: impl Into<Vec<u8>>) -> Self {
        Self {
            method: "POST".into(),
            uri: uri.into(),
            headers: Vec::new(),
            body: WorkerBody::Buffered(body.into()),
        }
    }

    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }
}

#[derive(Debug)]
pub struct WorkerResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: WorkerBody,
}

#[cfg(feature = "cloudflare")]
pub mod cloudflare {
    use std::future::Future;
    use std::pin::Pin;

    use bytes::Bytes;
    use futures_util::StreamExt;
    use rocket::http::uri::Origin;
    use rocket::http::{Header, Method};
    use rocket::tokio::io::AsyncReadExt;
    use rocket::{Build, Orbit, Rocket};
    use std::cell::RefCell;
    use std::rc::Rc;
    use worker::{Error, Headers, Request, Response, Result};

    use crate::{AdapterError, BoxedByteStream, WorkerBody, WorkerRequest, WorkerResponse};

    /// Bodies at or under this size skip the chunked-read loop entirely: one
    /// `to_bytes()` call replaces a 64KiB scratch-buffer allocation plus a
    /// `read()` loop that, for a typical small API response, would only ever
    /// run once anyway. Chosen to match Rocket's own default `Limits::STRING`
    /// (8KiB) — a reasonable proxy for "ordinary small response", not a hard
    /// correctness boundary.
    const SMALL_BODY_THRESHOLD: usize = 8 * 1024;

    pub trait Application {
        fn dispatch(self, request: WorkerRequest) -> DispatchFuture;
    }

    pub type DispatchFuture = Pin<Box<dyn Future<Output = Result<WorkerResponse>>>>;

    impl<F, Fut> Application for F
    where
        F: FnOnce(WorkerRequest) -> Fut + 'static,
        Fut: Future<Output = WorkerResponse> + 'static,
    {
        fn dispatch(self, request: WorkerRequest) -> DispatchFuture {
            Box::pin(async move { Ok(self(request).await) })
        }
    }

    /// Wraps a future so it satisfies Rocket's `Future + Send` bound on route
    /// handlers.
    ///
    /// D1, Queue, and other `worker` binding calls resolve through
    /// `wasm_bindgen_futures::JsFuture`, which is `!Send`. `wasm32-unknown-unknown`
    /// under Workers has no threads, so asserting `Send` here is sound:
    /// [`SendWrapper`] only panics if polled from a different thread than the
    /// one it was created on, which cannot happen. Wrap the whole body of any
    /// route handler that awaits a `worker` binding call:
    ///
    /// ```ignore
    /// #[get("/tasks")]
    /// async fn list_tasks(env: &State<Env>) -> Json<Vec<Task>> {
    ///     comet::cloudflare::local(async {
    ///         // ... .await on D1/Queue calls here ...
    ///     })
    ///     .await
    /// }
    /// ```
    ///
    /// This must be a plain function returning `impl Future`, not an `async
    /// fn`: an `async fn` wrapper would itself become a generator holding the
    /// non-`Send` argument future in its pre-poll state, which defeats the
    /// purpose (the `Send` bound would leak right back through).
    pub fn local<F: Future>(fut: F) -> impl Future<Output = F::Output> {
        send_wrapper::SendWrapper::new(fut)
    }

    /// The [`local()`] wrapper, for streaming responders (`rocket::response::
    /// stream::ByteStream!` and friends) that await a `worker` binding or
    /// primitive (e.g. `worker::Delay`) between yields. Rocket's stream
    /// responders require `S: Send` for the same reason route handlers
    /// require `Future + Send`; see [`local()`] for why this has to be a
    /// plain function.
    pub fn local_stream<S: futures_util::Stream>(
        stream: S,
    ) -> impl futures_util::Stream<Item = S::Item> {
        send_wrapper::SendWrapper::new(stream)
    }

    pub async fn serve<A: Application>(mut req: Request, app: A) -> Result<Response> {
        let request = request_from_worker(&mut req).await?;
        let response = app.dispatch(request).await?;
        response_to_worker(response)
    }

    thread_local! {
        // wasm32 under Workers is single-threaded, so a thread-local is
        // effectively a per-isolate cache: it survives across `#[event(fetch)]`
        // invocations that land on an isolate the runtime chose to reuse, and
        // starts fresh (`None`) on a new isolate. `Rc`, not `Arc`: no atomics
        // needed for the same reason.
        static ORBIT: RefCell<Option<Rc<Rocket<Orbit>>>> = const { RefCell::new(None) };
    }

    /// Like [`serve()`], but ignites `build_rocket()` — running route
    /// mounting, sentinel checks, and liftoff fairings — at most once per
    /// isolate, reusing the resulting `Rocket<Orbit>` across every request
    /// that lands on that isolate afterward.
    ///
    /// Workers isolates are commonly reused across many requests specifically
    /// so expensive per-isolate setup doesn't have to repeat; `serve()`
    /// ignites fresh on every single call regardless, throwing that away.
    /// `dispatch_external()` takes `&self` — Rocket is already designed to
    /// serve arbitrarily many requests from one `Rocket<Orbit>` — so reusing
    /// it here isn't a workaround, it's the intended usage the `Application`
    /// trait's per-request ignition was accidentally not taking advantage of.
    ///
    /// `build_rocket` is only called on a cache miss (this isolate's first
    /// request, or after an eviction), so mounting routes is skipped too, not
    /// just ignition. It's a `FnOnce`, so it's fine for it to move in
    /// request-scoped things like `Env` — they're simply unused when cached.
    ///
    /// This assumes bindings (`Env`) are stable for the isolate's lifetime,
    /// which they are: Cloudflare rolls out binding/config changes as new
    /// isolates, never by mutating a running one.
    pub async fn serve_cached<F>(mut req: Request, build_rocket: F) -> Result<Response>
    where
        F: FnOnce() -> Rocket<Build>,
    {
        let request = request_from_worker(&mut req).await?;

        let cached = ORBIT.with(|cache| cache.borrow().clone());
        let rocket = match cached {
            Some(rocket) => rocket,
            None => {
                let rocket = Rc::new(
                    build_rocket()
                        .orbit_external()
                        .await
                        .map_err(to_worker_error)?,
                );
                ORBIT.with(|cache| *cache.borrow_mut() = Some(rocket.clone()));
                rocket
            }
        };

        let response = dispatch_on_orbit(rocket, request).await?;
        response_to_worker(response)
    }

    impl Application for Rocket<Build> {
        fn dispatch(self, request: WorkerRequest) -> DispatchFuture {
            Box::pin(async move {
                let rocket = Rc::new(self.orbit_external().await.map_err(to_worker_error)?);
                dispatch_on_orbit(rocket, request).await
            })
        }
    }

    /// The shared core of [`Application for Rocket<Build>`] and
    /// [`serve_cached()`]: dispatch a request through an already-ignited
    /// `Rocket<Orbit>`.
    fn dispatch_on_orbit(rocket: Rc<Rocket<Orbit>>, request: WorkerRequest) -> DispatchFuture {
        Box::pin(async move {
            // Rocket only knows the response status/headers once
            // `dispatch_external()` has actually run the route handler
            // (side effects and all) to completion, so that part can't be
            // deferred into the body stream below. But `rocket_request` and
            // `response` self-reference `rocket` (`rocket_request` borrows
            // it, `response` borrows `rocket_request`), so once dispatch is
            // done, the *rest* of the work (reading the body incrementally)
            // can only happen inside the same generator that holds all
            // three — it can't be handed off to a separately-owned task.
            // `try_stream!` builds exactly that: one self-contained,
            // `'static` generator. It sends status/headers out over a
            // oneshot the moment they're known (always strictly before
            // the first body byte, or before the stream ends if the body
            // is empty), and this function drives the stream by exactly
            // one item to guarantee that has already happened before
            // returning a `WorkerResponse`.
            let (meta_tx, meta_rx) = futures_channel::oneshot::channel();

            let mut body_stream: BoxedByteStream = Box::pin(async_stream::try_stream! {
                let method = parse_method(&request.method).map_err(to_io_error)?;
                let uri = Origin::parse_owned(request.uri)
                    .map_err(|error| to_io_error(format!("invalid URI: {error}")))?;

                let mut rocket_request = rocket::Request::new(&rocket, method, uri, None);
                for (name, value) in request.headers {
                    rocket_request.add_header(Header::new(name, value));
                }

                let data = match request.body {
                    WorkerBody::Buffered(bytes) => rocket::Data::local(bytes),
                    WorkerBody::Streamed(stream) => rocket::Data::from_stream(stream),
                };

                let mut response = rocket
                    .dispatch_external(&mut rocket_request, data)
                    .await;

                let meta = ResponseMeta::from_response(&response);

                match response.body().preset_size() {
                    // Known-size, small: one exact-sized read beats a 64KiB
                    // scratch buffer plus a read() loop that would only ever
                    // run once anyway — this is the overwhelmingly common
                    // case (typical JSON API responses, static text, ...).
                    Some(size) if size <= SMALL_BODY_THRESHOLD => {
                        let bytes = response.body_mut().to_bytes().await.map_err(to_io_error)?;
                        let _ = meta_tx.send((meta, Some(bytes)));
                    }
                    _ if response.body().is_some() => {
                        let _ = meta_tx.send((meta, None));
                        let mut buf = vec![0u8; 64 * 1024];
                        loop {
                            let n = response.body_mut().read(&mut buf).await.map_err(to_io_error)?;
                            if n == 0 {
                                break;
                            }

                            yield Bytes::copy_from_slice(&buf[..n]);
                        }
                    }
                    _ => {
                        let _ = meta_tx.send((meta, Some(Vec::new())));
                    }
                }
            });

            let first_chunk = body_stream
                .next()
                .await
                .transpose()
                .map_err(to_worker_error)?;
            let (meta, buffered_body) = meta_rx.await.map_err(|_| {
                Error::RustError("rocket dispatch ended without producing a response".into())
            })?;

            if let Some(body) = buffered_body {
                return Ok(WorkerResponse {
                    status: meta.status,
                    headers: meta.headers,
                    body: WorkerBody::Buffered(body),
                });
            }

            let body: BoxedByteStream =
                Box::pin(futures_util::stream::iter(first_chunk.map(Ok)).chain(body_stream));

            Ok(WorkerResponse {
                status: meta.status,
                headers: meta.headers,
                body: WorkerBody::Streamed(body),
            })
        })
    }

    struct ResponseMeta {
        status: u16,
        headers: Vec<(String, String)>,
    }

    impl ResponseMeta {
        fn from_response(response: &rocket::Response<'_>) -> Self {
            Self {
                status: response.status().code,
                headers: response
                    .headers()
                    .iter()
                    .map(|header| (header.name().to_string(), header.value().to_string()))
                    .collect(),
            }
        }
    }

    async fn request_from_worker(req: &mut Request) -> Result<WorkerRequest> {
        let url = req.url()?;
        let uri = match url.query() {
            Some(query) => format!("{}?{}", url.path(), query),
            None => url.path().to_owned(),
        };

        // A request with no body (the common case: GET/HEAD/... with nothing
        // sent) makes `req.stream()` fail, since there is no underlying JS
        // `ReadableStream` to bridge at all. Treat that as an empty body
        // rather than propagating the error.
        let body = match req.stream() {
            Ok(stream) => {
                let stream = stream.map(|chunk| chunk.map(Bytes::from).map_err(to_io_error));
                WorkerBody::Streamed(Box::pin(stream))
            }
            Err(_) => WorkerBody::Buffered(Vec::new()),
        };

        Ok(WorkerRequest {
            method: req.method().to_string(),
            uri,
            headers: req.headers().entries().collect(),
            body,
        })
    }

    fn response_to_worker(response: WorkerResponse) -> Result<Response> {
        let headers = Headers::new();
        for (name, value) in response.headers {
            headers.set(&name, &value)?;
        }

        if !(200..=599).contains(&response.status) {
            return Err(Error::RustError(format!(
                "invalid Worker response status from comet: {}",
                response.status
            )));
        }

        match response.body {
            WorkerBody::Buffered(bytes) => Ok(Response::builder()
                .with_status(response.status)
                .with_headers(headers)
                .fixed(bytes)),
            WorkerBody::Streamed(stream) => Response::builder()
                .with_status(response.status)
                .with_headers(headers)
                .from_stream(stream),
        }
    }

    fn parse_method(method: &str) -> std::result::Result<Method, AdapterError> {
        match method.to_ascii_uppercase().as_str() {
            "GET" => Ok(Method::Get),
            "PUT" => Ok(Method::Put),
            "POST" => Ok(Method::Post),
            "DELETE" => Ok(Method::Delete),
            "HEAD" => Ok(Method::Head),
            "OPTIONS" => Ok(Method::Options),
            "PATCH" => Ok(Method::Patch),
            "TRACE" => Ok(Method::Trace),
            "CONNECT" => Ok(Method::Connect),
            _ => Err(AdapterError::InvalidMethod(method.to_owned())),
        }
    }

    fn to_worker_error(error: impl std::fmt::Display) -> Error {
        Error::RustError(error.to_string())
    }

    fn to_io_error(error: impl std::fmt::Display) -> std::io::Error {
        std::io::Error::other(error.to_string())
    }

    #[cfg(test)]
    mod tests {
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

        #[rocket::get("/small")]
        fn small() -> &'static str {
            "ok"
        }

        #[rocket::get("/large")]
        fn large() -> String {
            "x".repeat(super::SMALL_BODY_THRESHOLD + 1)
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
        async fn dispatch_streams_large_response_bodies() {
            let app = rocket::build().mount("/", rocket::routes![large]);
            let response = app.dispatch(WorkerRequest::get("/large")).await.unwrap();

            match response.body {
                WorkerBody::Buffered(_) => panic!("expected large body to remain streamed"),
                WorkerBody::Streamed(_) => {}
            }
        }
    }
}

#[cfg(feature = "native-client")]
pub struct RocketWorker {
    client: Client,
}

#[cfg(feature = "native-client")]
impl RocketWorker {
    pub async fn new(rocket: Rocket<Build>) -> Result<Self, AdapterError> {
        let client = Client::untracked(rocket).await?;
        Ok(Self { client })
    }

    pub async fn dispatch(&self, request: WorkerRequest) -> Result<WorkerResponse, AdapterError> {
        let method = parse_method(&request.method)?;
        let mut local = self.client.req(method, request.uri);

        for (name, value) in request.headers {
            local.add_header(Header::new(name, value));
        }

        let body = request
            .body
            .into_bytes()
            .ok_or(AdapterError::UnsupportedStreamedBody)?;
        if !body.is_empty() {
            local.set_body(body);
        }

        let response = local.dispatch().await;
        let status = response.status().code;
        let headers = response
            .headers()
            .iter()
            .map(|header| (header.name().as_str().to_owned(), header.value().to_owned()))
            .collect();

        let body = response.into_bytes().await.ok_or(AdapterError::Body)?;
        Ok(WorkerResponse {
            status,
            headers,
            body: WorkerBody::Buffered(body),
        })
    }
}

#[cfg(feature = "native-client")]
fn parse_method(method: &str) -> Result<Method, AdapterError> {
    match method.to_ascii_uppercase().as_str() {
        "GET" => Ok(Method::Get),
        "PUT" => Ok(Method::Put),
        "POST" => Ok(Method::Post),
        "DELETE" => Ok(Method::Delete),
        "HEAD" => Ok(Method::Head),
        "OPTIONS" => Ok(Method::Options),
        "PATCH" => Ok(Method::Patch),
        "TRACE" => Ok(Method::Trace),
        "CONNECT" => Ok(Method::Connect),
        _ => Err(AdapterError::InvalidMethod(method.to_owned())),
    }
}

#[cfg(all(test, feature = "native-client"))]
mod tests {
    use super::{RocketWorker, WorkerRequest};
    use rocket::serde::{Deserialize, Serialize, json::Json};

    #[derive(Debug, Deserialize, Serialize)]
    #[serde(crate = "rocket::serde")]
    struct Echo<'a> {
        value: &'a str,
    }

    #[rocket::get("/")]
    fn index() -> &'static str {
        "rocket on a worker-shaped adapter"
    }

    #[rocket::post("/echo", data = "<payload>")]
    fn echo<'a>(payload: Json<Echo<'a>>) -> Json<Echo<'a>> {
        payload
    }

    #[rocket::async_test]
    async fn dispatches_get_route() {
        let app = rocket::build().mount("/", rocket::routes![index, echo]);
        let worker = RocketWorker::new(app).await.unwrap();

        let response = worker.dispatch(WorkerRequest::get("/")).await.unwrap();

        assert_eq!(response.status, 200);
        assert_eq!(
            String::from_utf8(response.body.into_bytes().unwrap()).unwrap(),
            "rocket on a worker-shaped adapter"
        );
    }

    #[rocket::async_test]
    async fn dispatches_json_body() {
        let app = rocket::build().mount("/", rocket::routes![index, echo]);
        let worker = RocketWorker::new(app).await.unwrap();

        let response = worker
            .dispatch(
                WorkerRequest::post("/echo", br#"{ "value": "ok" }"#.to_vec())
                    .header("content-type", "application/json"),
            )
            .await
            .unwrap();

        assert_eq!(response.status, 200);
        assert_eq!(
            String::from_utf8(response.body.into_bytes().unwrap()).unwrap(),
            r#"{"value":"ok"}"#
        );
    }
}
