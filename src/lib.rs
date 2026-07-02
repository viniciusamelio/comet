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
    use std::marker::PhantomData;
    use std::ops::Deref;
    use std::pin::Pin;

    use bytes::Bytes;
    use futures_util::StreamExt;
    use rocket::http::Status;
    use rocket::http::uri::Origin;
    use rocket::http::{Header, Method};
    use rocket::request::{FromRequest, Outcome};
    use rocket::tokio::io::AsyncReadExt;
    use rocket::{Build, Orbit, Rocket};
    use std::cell::RefCell;
    use std::rc::Rc;
    use worker::{Context, Env, Error, Headers, Request, Response, Result};

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

    struct RocketDispatchRequest {
        method: Method,
        uri: String,
        headers: Vec<(String, String)>,
        body: WorkerBody,
    }
    #[cfg(feature = "cloudflare-websocket")]
    type WebSocketHandler =
        Box<dyn FnOnce(worker::WebSocket) -> Pin<Box<dyn Future<Output = Result<()>>>>>;

    impl<F, Fut> Application for F
    where
        F: FnOnce(WorkerRequest) -> Fut + 'static,
        Fut: Future<Output = WorkerResponse> + 'static,
    {
        fn dispatch(self, request: WorkerRequest) -> DispatchFuture {
            Box::pin(async move { Ok(self(request).await) })
        }
    }

    #[cfg(feature = "cloudflare-websocket")]
    thread_local! {
        static PENDING_WEBSOCKET: RefCell<Option<WebSocketHandler>> = const { RefCell::new(None) };
    }

    #[cfg(feature = "cloudflare-websocket")]
    #[derive(Debug)]
    pub enum WebSocketUpgradeError {
        NotUpgrade,
    }

    /// Request guard for Worker WebSocket upgrade routes.
    ///
    /// Use this in a normal Rocket route and return [`WebSocketResponse`] from
    /// [`WebSocketUpgrade::accept()`]. The Cloudflare adapter intercepts that
    /// response and returns a real `worker::Response::from_websocket(...)`.
    #[cfg(feature = "cloudflare-websocket")]
    pub struct WebSocketUpgrade;

    #[cfg(feature = "cloudflare-websocket")]
    #[rocket::async_trait]
    impl<'r> FromRequest<'r> for WebSocketUpgrade {
        type Error = WebSocketUpgradeError;

        async fn from_request(request: &'r rocket::Request<'_>) -> Outcome<Self, Self::Error> {
            let is_upgrade = request
                .headers()
                .get_one("upgrade")
                .is_some_and(|value| value.eq_ignore_ascii_case("websocket"));

            if is_upgrade {
                Outcome::Success(Self)
            } else {
                Outcome::Error((Status::UpgradeRequired, WebSocketUpgradeError::NotUpgrade))
            }
        }
    }

    #[cfg(feature = "cloudflare-websocket")]
    impl WebSocketUpgrade {
        pub fn accept<H, Fut>(self, handler: H) -> WebSocketResponse
        where
            H: FnOnce(worker::WebSocket) -> Fut + 'static,
            Fut: Future<Output = Result<()>> + 'static,
        {
            WebSocketResponse {
                handler: Box::new(|socket| Box::pin(handler(socket))),
            }
        }
    }

    /// Route response for a Worker WebSocket upgrade.
    #[cfg(feature = "cloudflare-websocket")]
    pub struct WebSocketResponse {
        handler: WebSocketHandler,
    }

    #[cfg(feature = "cloudflare-websocket")]
    impl<'r> rocket::response::Responder<'r, 'static> for WebSocketResponse {
        fn respond_to(self, _: &'r rocket::Request<'_>) -> rocket::response::Result<'static> {
            PENDING_WEBSOCKET.with(|pending| {
                *pending.borrow_mut() = Some(self.handler);
            });

            rocket::Response::build()
                .status(Status::SwitchingProtocols)
                .raw_header("x-comet-websocket-upgrade", "1")
                .ok()
        }
    }

    /// Wraps a non-`Send` future in a same-thread `Send` wrapper for manual
    /// compatibility cases.
    ///
    /// D1, Queue, and other `worker` binding calls resolve through
    /// `wasm_bindgen_futures::JsFuture`, which is `!Send`. `wasm32-unknown-unknown`
    /// under Workers has no threads, so asserting `Send` here is sound:
    /// [`SendWrapper`] only panics if polled from a different thread than the
    /// one it was created on, which cannot happen.
    ///
    /// Normal Rocket route handlers no longer need this wrapper in Worker
    /// builds: the vendored Rocket uses local-boxed route futures under the
    /// `worker` feature. Keep `local()` for custom/manual futures that still
    /// flow through an external `Send` bound.
    ///
    /// ```ignore
    /// let value = comet::cloudflare::local(async {
    ///     worker_future.await
    /// }).await;
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

    /// Names a Cloudflare binding for typed request guards.
    ///
    /// Define a zero-sized marker type per binding:
    ///
    /// ```ignore
    /// struct DB;
    ///
    /// impl comet::cloudflare::BindingName for DB {
    ///     const NAME: &'static str = "DB";
    /// }
    /// ```
    pub trait BindingName {
        const NAME: &'static str;
    }

    #[derive(Debug, thiserror::Error)]
    pub enum BindingError {
        #[error("worker Env is not managed by Rocket")]
        MissingEnv,
        #[error("failed to load binding `{name}`: {source}")]
        Worker { name: &'static str, source: Error },
    }

    #[cfg(feature = "cloudflare-d1")]
    #[derive(Debug)]
    pub struct D1<B: BindingName> {
        database: worker::D1Database,
        _binding: PhantomData<B>,
    }

    #[cfg(feature = "cloudflare-d1")]
    impl<B: BindingName> D1<B> {
        pub fn into_inner(self) -> worker::D1Database {
            self.database
        }
    }

    #[cfg(feature = "cloudflare-d1")]
    impl<B: BindingName> Deref for D1<B> {
        type Target = worker::D1Database;

        fn deref(&self) -> &Self::Target {
            &self.database
        }
    }

    #[cfg(feature = "cloudflare-d1")]
    #[rocket::async_trait]
    impl<'r, B> FromRequest<'r> for D1<B>
    where
        B: BindingName + Send + Sync + 'static,
    {
        type Error = BindingError;

        async fn from_request(request: &'r rocket::Request<'_>) -> Outcome<Self, Self::Error> {
            let Some(env) = request.rocket().state::<Env>() else {
                return Outcome::Error((Status::InternalServerError, BindingError::MissingEnv));
            };

            match env.d1(B::NAME) {
                Ok(database) => Outcome::Success(Self {
                    database,
                    _binding: PhantomData,
                }),
                Err(source) => Outcome::Error((
                    Status::InternalServerError,
                    BindingError::Worker {
                        name: B::NAME,
                        source,
                    },
                )),
            }
        }
    }

    #[cfg(feature = "cloudflare-queue")]
    #[derive(Debug)]
    pub struct QueueBinding<B: BindingName> {
        queue: worker::Queue,
        _binding: PhantomData<B>,
    }

    #[cfg(feature = "cloudflare-queue")]
    impl<B: BindingName> QueueBinding<B> {
        pub fn into_inner(self) -> worker::Queue {
            self.queue
        }
    }

    #[cfg(feature = "cloudflare-queue")]
    impl<B: BindingName> Deref for QueueBinding<B> {
        type Target = worker::Queue;

        fn deref(&self) -> &Self::Target {
            &self.queue
        }
    }

    #[cfg(feature = "cloudflare-queue")]
    #[rocket::async_trait]
    impl<'r, B> FromRequest<'r> for QueueBinding<B>
    where
        B: BindingName + Send + Sync + 'static,
    {
        type Error = BindingError;

        async fn from_request(request: &'r rocket::Request<'_>) -> Outcome<Self, Self::Error> {
            let Some(env) = request.rocket().state::<Env>() else {
                return Outcome::Error((Status::InternalServerError, BindingError::MissingEnv));
            };

            match env.queue(B::NAME) {
                Ok(queue) => Outcome::Success(Self {
                    queue,
                    _binding: PhantomData,
                }),
                Err(source) => Outcome::Error((
                    Status::InternalServerError,
                    BindingError::Worker {
                        name: B::NAME,
                        source,
                    },
                )),
            }
        }
    }

    #[cfg(feature = "cloudflare-kv")]
    #[derive(Debug)]
    pub struct Kv<B: BindingName> {
        store: worker::kv::KvStore,
        _binding: PhantomData<B>,
    }

    #[cfg(feature = "cloudflare-kv")]
    impl<B: BindingName> Kv<B> {
        pub fn into_inner(self) -> worker::kv::KvStore {
            self.store
        }
    }

    #[cfg(feature = "cloudflare-kv")]
    impl<B: BindingName> Deref for Kv<B> {
        type Target = worker::kv::KvStore;

        fn deref(&self) -> &Self::Target {
            &self.store
        }
    }

    #[cfg(feature = "cloudflare-kv")]
    #[rocket::async_trait]
    impl<'r, B> FromRequest<'r> for Kv<B>
    where
        B: BindingName + Send + Sync + 'static,
    {
        type Error = BindingError;

        async fn from_request(request: &'r rocket::Request<'_>) -> Outcome<Self, Self::Error> {
            let Some(env) = request.rocket().state::<Env>() else {
                return Outcome::Error((Status::InternalServerError, BindingError::MissingEnv));
            };

            match env.kv(B::NAME) {
                Ok(store) => Outcome::Success(Self {
                    store,
                    _binding: PhantomData,
                }),
                Err(source) => Outcome::Error((
                    Status::InternalServerError,
                    BindingError::Worker {
                        name: B::NAME,
                        source,
                    },
                )),
            }
        }
    }

    #[cfg(feature = "cloudflare-r2")]
    #[derive(Debug)]
    pub struct R2Bucket<B: BindingName> {
        bucket: worker::Bucket,
        _binding: PhantomData<B>,
    }

    #[cfg(feature = "cloudflare-r2")]
    // Workers wasm is single-threaded; this mirrors `worker`'s own Send/Sync
    // impls for other JS-backed bindings so Rocket's Send-bound route futures
    // can carry the guard.
    unsafe impl<B> Send for R2Bucket<B> where B: BindingName + Send {}

    #[cfg(feature = "cloudflare-r2")]
    // See the Send impl above.
    unsafe impl<B> Sync for R2Bucket<B> where B: BindingName + Sync {}

    #[cfg(feature = "cloudflare-r2")]
    impl<B: BindingName> R2Bucket<B> {
        pub fn into_inner(self) -> worker::Bucket {
            self.bucket
        }
    }

    #[cfg(feature = "cloudflare-r2")]
    impl<B: BindingName> Deref for R2Bucket<B> {
        type Target = worker::Bucket;

        fn deref(&self) -> &Self::Target {
            &self.bucket
        }
    }

    #[cfg(feature = "cloudflare-r2")]
    #[rocket::async_trait]
    impl<'r, B> FromRequest<'r> for R2Bucket<B>
    where
        B: BindingName + Send + Sync + 'static,
    {
        type Error = BindingError;

        async fn from_request(request: &'r rocket::Request<'_>) -> Outcome<Self, Self::Error> {
            let Some(env) = request.rocket().state::<Env>() else {
                return Outcome::Error((Status::InternalServerError, BindingError::MissingEnv));
            };

            match env.bucket(B::NAME) {
                Ok(bucket) => Outcome::Success(Self {
                    bucket,
                    _binding: PhantomData,
                }),
                Err(source) => Outcome::Error((
                    Status::InternalServerError,
                    BindingError::Worker {
                        name: B::NAME,
                        source,
                    },
                )),
            }
        }
    }

    /// A Rocket responder for an R2 object body.
    ///
    /// Use [`R2Object::get()`] or [`R2Object::get_range()`] from a route that
    /// receives an [`R2Bucket`] guard. The response preserves R2 HTTP metadata,
    /// ETag, content length, and streams the object through Rocket instead of
    /// buffering it as a local file.
    #[cfg(feature = "cloudflare-r2")]
    #[derive(Debug)]
    pub struct R2Object {
        object: worker::Object,
        status: Status,
    }

    #[cfg(feature = "cloudflare-r2")]
    impl R2Object {
        pub fn from_object(object: worker::Object) -> Option<Self> {
            object.body().is_some().then_some(Self {
                object,
                status: Status::Ok,
            })
        }

        pub async fn get(bucket: &worker::Bucket, key: impl Into<String>) -> Result<Option<Self>> {
            Ok(bucket.get(key).execute().await?.and_then(Self::from_object))
        }

        pub async fn get_range(
            bucket: &worker::Bucket,
            key: impl Into<String>,
            range: worker::Range,
        ) -> Result<Option<Self>> {
            let object = bucket.get(key).range(range).execute().await?;
            Ok(object.and_then(|object| {
                Self::from_object(object).map(|mut response| {
                    response.status = Status::PartialContent;
                    response
                })
            }))
        }

        pub fn object(&self) -> &worker::Object {
            &self.object
        }
    }

    #[cfg(feature = "cloudflare-r2")]
    impl<'r> rocket::response::Responder<'r, 'static> for R2Object {
        fn respond_to(self, _: &'r rocket::Request<'_>) -> rocket::response::Result<'static> {
            let headers = Headers::new();
            self.object
                .write_http_metadata(headers.clone())
                .map_err(|_| Status::InternalServerError)?;

            let Some(body) = self.object.body() else {
                return Err(Status::InternalServerError);
            };

            let stream = body.stream().map_err(|_| Status::InternalServerError)?;
            let reader = R2BodyReader::new(stream);
            let size = self.object.size();

            let mut response = rocket::Response::build();
            response.status(self.status);
            response.raw_header("content-length", size.to_string());
            response.raw_header("etag", self.object.http_etag());
            response.raw_header("accept-ranges", "bytes");
            if self.status == Status::PartialContent {
                if let Ok(range) = self.object.range() {
                    if let Some(content_range) = content_range_header(&range, size) {
                        response.raw_header("content-range", content_range);
                    }
                }
            }

            for (name, value) in headers.entries() {
                response.header(Header::new(name, value));
            }

            response.streamed_body(reader).ok()
        }
    }

    #[cfg(feature = "cloudflare-r2")]
    fn content_range_header(range: &worker::Range, size: u64) -> Option<String> {
        let (start, end) = match *range {
            worker::Range::OffsetWithLength { offset, length } => {
                if length == 0 {
                    return None;
                }

                (offset, offset.saturating_add(length).saturating_sub(1))
            }
            worker::Range::OffsetToEnd { offset } => {
                if offset >= size {
                    return None;
                }

                (offset, size.saturating_sub(1))
            }
            worker::Range::Prefix { length } => {
                if length == 0 || size == 0 {
                    return None;
                }

                (0, length.min(size).saturating_sub(1))
            }
            worker::Range::Suffix { suffix } => {
                if suffix == 0 || size == 0 {
                    return None;
                }

                (size.saturating_sub(suffix), size.saturating_sub(1))
            }
        };

        Some(format!(
            "bytes {start}-{}/{size}",
            end.min(size.saturating_sub(1))
        ))
    }

    #[cfg(feature = "cloudflare-r2")]
    struct R2BodyReader {
        stream: Pin<Box<worker::ByteStream>>,
        current: Option<std::io::Cursor<Vec<u8>>>,
    }

    #[cfg(feature = "cloudflare-r2")]
    impl R2BodyReader {
        fn new(stream: worker::ByteStream) -> Self {
            Self {
                stream: Box::pin(stream),
                current: None,
            }
        }
    }

    #[cfg(feature = "cloudflare-r2")]
    // Workers wasm is single-threaded; this reader is polled only inside the
    // request isolate. The wrapped JS stream is never moved to another thread.
    unsafe impl Send for R2BodyReader {}

    #[cfg(feature = "cloudflare-r2")]
    impl rocket::tokio::io::AsyncRead for R2BodyReader {
        fn poll_read(
            mut self: Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
            buf: &mut rocket::tokio::io::ReadBuf<'_>,
        ) -> std::task::Poll<std::io::Result<()>> {
            loop {
                if let Some(current) = self.current.as_mut() {
                    let position = current.position() as usize;
                    let bytes = current.get_ref();
                    if position < bytes.len() {
                        let remaining = &bytes[position..];
                        let len = remaining.len().min(buf.remaining());
                        buf.put_slice(&remaining[..len]);
                        current.set_position((position + len) as u64);
                        return std::task::Poll::Ready(Ok(()));
                    }
                }

                match futures_util::Stream::poll_next(self.stream.as_mut(), cx) {
                    std::task::Poll::Pending => return std::task::Poll::Pending,
                    std::task::Poll::Ready(Some(Ok(chunk))) => {
                        self.current = Some(std::io::Cursor::new(chunk));
                    }
                    std::task::Poll::Ready(Some(Err(error))) => {
                        return std::task::Poll::Ready(Err(to_io_error(error)));
                    }
                    std::task::Poll::Ready(None) => return std::task::Poll::Ready(Ok(())),
                }
            }
        }
    }

    /// Returns `true` when `req` is a Worker WebSocket upgrade request.
    ///
    /// Most applications should prefer a normal Rocket route that takes
    /// [`WebSocketUpgrade`] and returns [`WebSocketResponse`]. This lower-level
    /// helper remains available for applications that intentionally handle
    /// upgrades before Rocket dispatch.
    #[cfg(feature = "cloudflare-websocket")]
    pub fn is_websocket_upgrade(req: &Request) -> Result<bool> {
        Ok(req
            .headers()
            .get("upgrade")?
            .is_some_and(|value| value.eq_ignore_ascii_case("websocket")))
    }

    /// Creates a `101 Switching Protocols` Worker response and runs `handler`
    /// on the server side of a new [`worker::WebSocketPair`].
    ///
    /// Most applications should prefer a normal Rocket route that takes
    /// [`WebSocketUpgrade`] and returns [`WebSocketResponse`]. This lower-level
    /// helper remains available for applications that intentionally handle
    /// upgrades before Rocket dispatch. The returned response owns the client
    /// side of the pair; `handler` receives the accepted server side and can
    /// drive `socket.events()` directly.
    #[cfg(feature = "cloudflare-websocket")]
    pub fn websocket_response<H, Fut>(handler: H) -> Result<Response>
    where
        H: FnOnce(worker::WebSocket) -> Fut + 'static,
        Fut: Future<Output = Result<()>> + 'static,
    {
        websocket_response_boxed(Box::new(|socket| Box::pin(handler(socket))))
    }

    #[cfg(feature = "cloudflare-websocket")]
    fn websocket_response_boxed(handler: WebSocketHandler) -> Result<Response> {
        let pair = worker::WebSocketPair::new()?;
        let client = pair.client;
        let server = pair.server;
        server.accept()?;

        worker::wasm_bindgen_futures::spawn_local(async move {
            if let Err(error) = handler(server).await {
                worker::console_error!("websocket handler failed: {}", error);
            }
        });

        Response::from_websocket(client)
    }

    #[cfg(feature = "cloudflare-service")]
    #[derive(Debug)]
    pub struct ServiceBinding<B: BindingName> {
        fetcher: worker::Fetcher,
        _binding: PhantomData<B>,
    }

    #[cfg(feature = "cloudflare-service")]
    // Workers wasm is single-threaded; this mirrors `worker`'s own Send/Sync
    // impls for other JS-backed bindings so Rocket's Send-bound route futures
    // can carry the guard.
    unsafe impl<B> Send for ServiceBinding<B> where B: BindingName + Send {}

    #[cfg(feature = "cloudflare-service")]
    // See the Send impl above.
    unsafe impl<B> Sync for ServiceBinding<B> where B: BindingName + Sync {}

    #[cfg(feature = "cloudflare-service")]
    impl<B: BindingName> ServiceBinding<B> {
        pub fn into_inner(self) -> worker::Fetcher {
            self.fetcher
        }
    }

    #[cfg(feature = "cloudflare-service")]
    impl<B: BindingName> Deref for ServiceBinding<B> {
        type Target = worker::Fetcher;

        fn deref(&self) -> &Self::Target {
            &self.fetcher
        }
    }

    #[cfg(feature = "cloudflare-service")]
    #[rocket::async_trait]
    impl<'r, B> FromRequest<'r> for ServiceBinding<B>
    where
        B: BindingName + Send + Sync + 'static,
    {
        type Error = BindingError;

        async fn from_request(request: &'r rocket::Request<'_>) -> Outcome<Self, Self::Error> {
            let Some(env) = request.rocket().state::<Env>() else {
                return Outcome::Error((Status::InternalServerError, BindingError::MissingEnv));
            };

            match env.service(B::NAME) {
                Ok(fetcher) => Outcome::Success(Self {
                    fetcher,
                    _binding: PhantomData,
                }),
                Err(source) => Outcome::Error((
                    Status::InternalServerError,
                    BindingError::Worker {
                        name: B::NAME,
                        source,
                    },
                )),
            }
        }
    }

    #[cfg(feature = "cloudflare-hyperdrive")]
    #[derive(Debug)]
    pub struct Hyperdrive<B: BindingName> {
        hyperdrive: worker::Hyperdrive,
        _binding: PhantomData<B>,
    }

    #[cfg(feature = "cloudflare-hyperdrive")]
    impl<B: BindingName> Hyperdrive<B> {
        pub fn into_inner(self) -> worker::Hyperdrive {
            self.hyperdrive
        }
    }

    #[cfg(feature = "cloudflare-hyperdrive")]
    impl<B: BindingName> Deref for Hyperdrive<B> {
        type Target = worker::Hyperdrive;

        fn deref(&self) -> &Self::Target {
            &self.hyperdrive
        }
    }

    #[cfg(feature = "cloudflare-hyperdrive")]
    #[rocket::async_trait]
    impl<'r, B> FromRequest<'r> for Hyperdrive<B>
    where
        B: BindingName + Send + Sync + 'static,
    {
        type Error = BindingError;

        async fn from_request(request: &'r rocket::Request<'_>) -> Outcome<Self, Self::Error> {
            let Some(env) = request.rocket().state::<Env>() else {
                return Outcome::Error((Status::InternalServerError, BindingError::MissingEnv));
            };

            match env.hyperdrive(B::NAME) {
                Ok(hyperdrive) => Outcome::Success(Self {
                    hyperdrive,
                    _binding: PhantomData,
                }),
                Err(source) => Outcome::Error((
                    Status::InternalServerError,
                    BindingError::Worker {
                        name: B::NAME,
                        source,
                    },
                )),
            }
        }
    }

    pub async fn serve<A: Application>(mut req: Request, app: A) -> Result<Response> {
        let request = worker_request_from_worker(&mut req).await?;
        let response = app.dispatch(request).await?;
        response_to_worker(response)
    }

    /// A reusable Cloudflare Worker fetch adapter backed by Rocket.
    ///
    /// Most applications can call [`fetch()`] directly instead. Use this type
    /// when you intentionally want to name the adapter object, for example in a
    /// `static`:
    ///
    /// ```ignore
    /// static ROCKET: comet::cloudflare::FetchApp =
    ///     comet::cloudflare::FetchApp::new(|env, _ctx| rocket(env));
    ///
    /// #[event(fetch)]
    /// async fn main(req: Request, env: Env, ctx: Context) -> Result<Response> {
    ///     ROCKET.fetch(req, env, ctx).await
    /// }
    /// ```
    ///
    /// The builder is only called on this isolate's first request. Subsequent
    /// requests reuse the already-ignited `Rocket<Orbit>`.
    pub struct WorkerFetchApp<F> {
        build_rocket: F,
    }

    /// The common `static` app shape: a function pointer that receives the
    /// per-request Worker `Env` and `Context` and builds `Rocket<Build>`.
    pub type FetchApp = WorkerFetchApp<fn(Env, Context) -> Rocket<Build>>;

    impl FetchApp {
        pub const fn new(build_rocket: fn(Env, Context) -> Rocket<Build>) -> Self {
            WorkerFetchApp { build_rocket }
        }
    }

    impl<F> WorkerFetchApp<F>
    where
        F: Fn(Env, Context) -> Rocket<Build>,
    {
        pub async fn fetch(&self, req: Request, env: Env, ctx: Context) -> Result<Response> {
            serve_cached(req, || (self.build_rocket)(env, ctx)).await
        }
    }

    /// Dispatches a Cloudflare Worker fetch event through a cached Rocket app.
    ///
    /// This is the zero-static entrypoint for applications that do not need to
    /// name their adapter object:
    ///
    /// ```ignore
    /// #[event(fetch)]
    /// async fn main(req: Request, env: Env, ctx: Context) -> Result<Response> {
    ///     comet::cloudflare::fetch(req, env, ctx, rocket).await
    /// }
    /// ```
    ///
    /// Like [`WorkerFetchApp::fetch()`], this still caches the ignited
    /// `Rocket<Orbit>` per Worker isolate; `build_rocket` is only called on a
    /// cache miss.
    pub async fn fetch<F>(req: Request, env: Env, ctx: Context, build_rocket: F) -> Result<Response>
    where
        F: FnOnce(Env, Context) -> Rocket<Build>,
    {
        serve_cached(req, || build_rocket(env, ctx)).await
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
        let request = rocket_request_from_worker(&mut req).await?;

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

        match dispatch_on_orbit(rocket, request).await? {
            DispatchOutcome::Http(response) => response_to_worker(response),
            #[cfg(feature = "cloudflare-websocket")]
            DispatchOutcome::WebSocket(handler) => websocket_response_boxed(handler),
        }
    }

    impl Application for Rocket<Build> {
        fn dispatch(self, request: WorkerRequest) -> DispatchFuture {
            Box::pin(async move {
                let rocket = Rc::new(self.orbit_external().await.map_err(to_worker_error)?);
                match dispatch_on_orbit(rocket, request.try_into()?).await? {
                    DispatchOutcome::Http(response) => Ok(response),
                    #[cfg(feature = "cloudflare-websocket")]
                    DispatchOutcome::WebSocket(_) => Err(Error::RustError(
                        "websocket route responses require the Cloudflare fetch adapter".into(),
                    )),
                }
            })
        }
    }

    /// The shared core of [`Application for Rocket<Build>`] and
    /// [`serve_cached()`]: dispatch a request through an already-ignited
    /// `Rocket<Orbit>`.
    enum DispatchOutcome {
        Http(WorkerResponse),
        #[cfg(feature = "cloudflare-websocket")]
        WebSocket(WebSocketHandler),
    }

    type OrbitDispatchFuture = Pin<Box<dyn Future<Output = Result<DispatchOutcome>>>>;

    impl TryFrom<WorkerRequest> for RocketDispatchRequest {
        type Error = Error;

        fn try_from(request: WorkerRequest) -> Result<Self> {
            Ok(Self {
                method: parse_method(&request.method).map_err(to_worker_error)?,
                uri: request.uri,
                headers: request.headers,
                body: request.body,
            })
        }
    }

    fn dispatch_on_orbit(
        rocket: Rc<Rocket<Orbit>>,
        request: RocketDispatchRequest,
    ) -> OrbitDispatchFuture {
        Box::pin(async move {
            let uri = Origin::parse_owned(request.uri)
                .map_err(|error| to_worker_error(format!("invalid URI: {error}")))?;

            // Rocket's public response lifetime is tied to the request passed
            // to `dispatch_external()`. Keep the request pinned so any
            // response borrows remain stable while a streamed body is read
            // after this function returns.
            // SAFETY: `rocket` is an `Rc`, so the `Rocket<Orbit>` allocation
            // stays at a stable address. Any response that uses this widened
            // reference is either consumed before `rocket` is dropped or stored
            // with an `Rc` clone in `StreamedRocketResponse`.
            let rocket_static = unsafe { rocket_static_ref(&rocket) };
            let mut rocket_request = Box::pin(rocket::Request::new(
                rocket_static,
                request.method,
                uri,
                None,
            ));
            for (name, value) in request.headers {
                rocket_request.add_header(Header::new(name, value));
            }

            let data = match request.body {
                WorkerBody::Buffered(bytes) => rocket::Data::local(bytes),
                WorkerBody::Streamed(stream) => rocket::Data::from_stream(stream),
            };

            // SAFETY: the request is pinned in a `Box`; moving the box into
            // `StreamedRocketResponse` does not move the request allocation.
            // The response is dropped before the request by field order.
            let request_ptr = unsafe {
                rocket_request.as_mut().get_unchecked_mut() as *mut rocket::Request<'static>
            };
            let mut response = unsafe {
                rocket_static
                    .dispatch_external(&mut *request_ptr, data)
                    .await
            };

            #[cfg(feature = "cloudflare-websocket")]
            if response.status() == Status::SwitchingProtocols
                && response.headers().get_one("x-comet-websocket-upgrade") == Some("1")
            {
                let handler = PENDING_WEBSOCKET.with(|pending| pending.borrow_mut().take());
                return match handler {
                    Some(handler) => Ok(DispatchOutcome::WebSocket(handler)),
                    None => Err(to_worker_error(
                        "websocket route did not register an upgrade handler",
                    )),
                };
            }

            let meta = ResponseMeta::from_response(&response);

            if response.body().preset_size().unwrap_or(usize::MAX) <= SMALL_BODY_THRESHOLD
                || response.body().is_none()
            {
                let body = response
                    .body_mut()
                    .to_bytes()
                    .await
                    .map_err(to_worker_error)?;
                return Ok(DispatchOutcome::Http(WorkerResponse {
                    status: meta.status,
                    headers: meta.headers,
                    body: WorkerBody::Buffered(body),
                }));
            }

            let body = stream_rocket_response(StreamedRocketResponse {
                response,
                request: rocket_request,
                rocket,
            });

            Ok(DispatchOutcome::Http(WorkerResponse {
                status: meta.status,
                headers: meta.headers,
                body: WorkerBody::Streamed(body),
            }))
        })
    }

    struct StreamedRocketResponse {
        // Drop response before request, and request before rocket.
        response: rocket::Response<'static>,
        request: Pin<Box<rocket::Request<'static>>>,
        rocket: Rc<Rocket<Orbit>>,
    }

    fn stream_rocket_response(mut state: StreamedRocketResponse) -> BoxedByteStream {
        Box::pin(async_stream::try_stream! {
            let mut buf = vec![0u8; 64 * 1024];
            loop {
                let n = state.response.body_mut().read(&mut buf).await.map_err(to_io_error)?;
                if n == 0 {
                    break;
                }

                yield Bytes::copy_from_slice(&buf[..n]);
            }

            let _ = (&state.request, &state.rocket);
        })
    }

    unsafe fn rocket_static_ref(rocket: &Rc<Rocket<Orbit>>) -> &'static Rocket<Orbit> {
        unsafe { &*(&**rocket as *const Rocket<Orbit>) }
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

    async fn worker_request_from_worker(req: &mut Request) -> Result<WorkerRequest> {
        let method = req.method().to_string();
        let request = rocket_request_from_worker(req).await?;

        Ok(WorkerRequest {
            method,
            uri: request.uri,
            headers: request.headers,
            body: request.body,
        })
    }

    async fn rocket_request_from_worker(req: &mut Request) -> Result<RocketDispatchRequest> {
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

        Ok(RocketDispatchRequest {
            method: parse_worker_method(req.method())?,
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
        if method.eq_ignore_ascii_case("GET") {
            Ok(Method::Get)
        } else if method.eq_ignore_ascii_case("PUT") {
            Ok(Method::Put)
        } else if method.eq_ignore_ascii_case("POST") {
            Ok(Method::Post)
        } else if method.eq_ignore_ascii_case("DELETE") {
            Ok(Method::Delete)
        } else if method.eq_ignore_ascii_case("HEAD") {
            Ok(Method::Head)
        } else if method.eq_ignore_ascii_case("OPTIONS") {
            Ok(Method::Options)
        } else if method.eq_ignore_ascii_case("PATCH") {
            Ok(Method::Patch)
        } else if method.eq_ignore_ascii_case("TRACE") {
            Ok(Method::Trace)
        } else if method.eq_ignore_ascii_case("CONNECT") {
            Ok(Method::Connect)
        } else {
            Err(AdapterError::InvalidMethod(method.to_owned()))
        }
    }

    fn parse_worker_method(method: worker::Method) -> Result<Method> {
        match method {
            worker::Method::Head => Ok(Method::Head),
            worker::Method::Get => Ok(Method::Get),
            worker::Method::Post => Ok(Method::Post),
            worker::Method::Put => Ok(Method::Put),
            worker::Method::Patch => Ok(Method::Patch),
            worker::Method::Delete => Ok(Method::Delete),
            worker::Method::Options => Ok(Method::Options),
            worker::Method::Connect => Ok(Method::Connect),
            worker::Method::Trace => Ok(Method::Trace),
            worker::Method::Report => Err(Error::RustError(
                "unsupported HTTP method for Rocket dispatch: REPORT".into(),
            )),
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
    if method.eq_ignore_ascii_case("GET") {
        Ok(Method::Get)
    } else if method.eq_ignore_ascii_case("PUT") {
        Ok(Method::Put)
    } else if method.eq_ignore_ascii_case("POST") {
        Ok(Method::Post)
    } else if method.eq_ignore_ascii_case("DELETE") {
        Ok(Method::Delete)
    } else if method.eq_ignore_ascii_case("HEAD") {
        Ok(Method::Head)
    } else if method.eq_ignore_ascii_case("OPTIONS") {
        Ok(Method::Options)
    } else if method.eq_ignore_ascii_case("PATCH") {
        Ok(Method::Patch)
    } else if method.eq_ignore_ascii_case("TRACE") {
        Ok(Method::Trace)
    } else if method.eq_ignore_ascii_case("CONNECT") {
        Ok(Method::Connect)
    } else {
        Err(AdapterError::InvalidMethod(method.to_owned()))
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
