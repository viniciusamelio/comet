use rocket::http::uri::Origin;
use rocket::http::{Header, Method};
use rocket::{Build, Orbit, Rocket};

#[derive(Debug, thiserror::Error)]
pub enum AdapterError {
    #[error("invalid HTTP method: {0}")]
    InvalidMethod(String),
    #[error("invalid URI: {0}")]
    InvalidUri(String),
    #[error("rocket failed to ignite: {0}")]
    Rocket(#[from] rocket::Error),
    #[error("failed to read response body: {0}")]
    Body(#[from] std::io::Error),
}

pub struct WorkerRequest {
    pub method: String,
    pub uri: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

pub struct WorkerResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

pub struct RocketWorker {
    rocket: Rocket<Orbit>,
}

impl RocketWorker {
    pub async fn new(rocket: Rocket<Build>) -> Result<Self, AdapterError> {
        let rocket = rocket.orbit_external().await?;
        Ok(Self { rocket })
    }

    pub async fn dispatch(&self, request: WorkerRequest) -> Result<WorkerResponse, AdapterError> {
        let method = parse_method(&request.method)?;
        let uri = Origin::parse_owned(request.uri)
            .map_err(|error| AdapterError::InvalidUri(error.to_string()))?;

        let mut rocket_request = rocket::Request::new(&self.rocket, method, uri, None);
        for (name, value) in request.headers {
            rocket_request.add_header(Header::new(name, value));
        }

        let data = rocket::Data::local(request.body);
        let mut response = self
            .rocket
            .dispatch_external(&mut rocket_request, data)
            .await;

        let status = response.status().code;
        let headers = response
            .headers()
            .iter()
            .map(|header| (header.name().to_string(), header.value().to_string()))
            .collect();
        let body = if response.body().is_none() {
            Vec::new()
        } else {
            response.body_mut().to_bytes().await?
        };

        Ok(WorkerResponse {
            status,
            headers,
            body,
        })
    }
}

fn parse_method(method: &str) -> Result<Method, AdapterError> {
    match method {
        "GET" => Ok(Method::Get),
        "PUT" => Ok(Method::Put),
        "POST" => Ok(Method::Post),
        "DELETE" => Ok(Method::Delete),
        "OPTIONS" => Ok(Method::Options),
        "HEAD" => Ok(Method::Head),
        "TRACE" => Ok(Method::Trace),
        "CONNECT" => Ok(Method::Connect),
        "PATCH" => Ok(Method::Patch),
        other => Err(AdapterError::InvalidMethod(other.to_string())),
    }
}
