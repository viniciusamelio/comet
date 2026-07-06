use std::pin::Pin;

#[cfg(feature = "nebula")]
extern crate self as comet;

#[cfg(feature = "nebula")]
pub mod nebula;

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
pub mod cloudflare;

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
