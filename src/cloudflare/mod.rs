use std::future::Future;
use std::pin::Pin;

use bytes::Bytes;
use futures_util::StreamExt;
use rocket::http::uri::Origin;
use rocket::http::{Header, Method, Status};
use rocket::tokio::io::AsyncReadExt;
use rocket::{Build, Orbit, Rocket};
use std::cell::RefCell;
use std::rc::Rc;
use worker::{Context, Env, Error, Headers, Request, Response, Result};

use crate::{AdapterError, BoxedByteStream, WorkerBody, WorkerRequest, WorkerResponse};

use websocket::{PENDING_WEBSOCKET, WebSocketHandler, websocket_response_boxed};

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

impl<F, Fut> Application for F
where
    F: FnOnce(WorkerRequest) -> Fut + 'static,
    Fut: Future<Output = WorkerResponse> + 'static,
{
    fn dispatch(self, request: WorkerRequest) -> DispatchFuture {
        Box::pin(async move { Ok(self(request).await) })
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
        let request_ptr =
            unsafe { rocket_request.as_mut().get_unchecked_mut() as *mut rocket::Request<'static> };
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

mod bindings;
mod r2;
mod websocket;

#[cfg(feature = "cloudflare-d1")]
pub use bindings::D1;
#[cfg(feature = "cloudflare-hyperdrive")]
pub use bindings::Hyperdrive;
#[cfg(feature = "cloudflare-kv")]
pub use bindings::Kv;
#[cfg(feature = "cloudflare-queue")]
pub use bindings::QueueBinding;
#[cfg(feature = "cloudflare-service")]
pub use bindings::ServiceBinding;
pub use bindings::{BindingError, BindingName};
#[cfg(feature = "cloudflare-r2")]
pub use r2::{R2Bucket, R2Object};
#[cfg(feature = "cloudflare-websocket")]
pub use websocket::{
    WebSocketResponse, WebSocketUpgrade, WebSocketUpgradeError, is_websocket_upgrade,
    websocket_response,
};

#[cfg(test)]
mod tests;
