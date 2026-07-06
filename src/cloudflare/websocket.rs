use std::cell::RefCell;
use std::future::Future;
use std::pin::Pin;

use rocket::http::Status;
use rocket::request::{FromRequest, Outcome};
use worker::{Request, Response, Result};

#[cfg(feature = "cloudflare-websocket")]
pub(crate) type WebSocketHandler =
    Box<dyn FnOnce(worker::WebSocket) -> Pin<Box<dyn Future<Output = Result<()>>>>>;

#[cfg(feature = "cloudflare-websocket")]
thread_local! {
    pub(crate) static PENDING_WEBSOCKET: RefCell<Option<WebSocketHandler>> = const { RefCell::new(None) };
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
pub(crate) fn websocket_response_boxed(handler: WebSocketHandler) -> Result<Response> {
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
