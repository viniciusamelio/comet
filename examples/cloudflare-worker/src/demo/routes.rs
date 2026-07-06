use comet::cloudflare::{WebSocketResponse, WebSocketUpgrade};
use rocket::futures::StreamExt;
use worker::WebsocketEvent;

#[get("/")]
pub fn index() -> &'static str {
    "hello from Rocket on Cloudflare Workers\n"
}

#[post("/echo", data = "<body>")]
pub fn echo(body: String) -> String {
    body
}

/// Proves comet's response streaming actually streams: each chunk is only
/// produced after a real, Workers-native delay (`worker::Delay`, backed by
/// `setTimeout`, not a tokio timer that wouldn't run under Workers). If
/// comet buffered the whole body before responding, a client would see all
/// chunks arrive at once after ~1.2s; streamed, they arrive ~400ms apart.
#[get("/stream")]
pub fn stream_demo(
) -> rocket::response::stream::ByteStream<impl rocket::futures::stream::Stream<Item = Vec<u8>>> {
    let raw = rocket::response::stream::stream! {
        for chunk in 0..3u8 {
            yield vec![b'0' + chunk; 4096];
            worker::Delay::from(std::time::Duration::from_millis(400)).await;
        }
    };

    rocket::response::stream::ByteStream(comet::cloudflare::local_stream(raw))
}

#[get("/ws/echo")]
pub async fn websocket_echo(ws: WebSocketUpgrade) -> WebSocketResponse {
    ws.accept(|socket| async move {
        let mut events = socket.events()?;
        while let Some(event) = events.next().await {
            match event? {
                WebsocketEvent::Message(message) => {
                    if let Some(text) = message.text() {
                        socket.send_with_str(text)?;
                    } else if let Some(bytes) = message.bytes() {
                        socket.send_with_bytes(bytes)?;
                    }
                }
                WebsocketEvent::Close(_) => break,
            }
        }

        Ok(())
    })
}
