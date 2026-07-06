use std::marker::PhantomData;
use std::ops::Deref;
use std::pin::Pin;

use rocket::http::{Header, Status};
use rocket::request::{FromRequest, Outcome};
use worker::{Env, Headers, Result};

use super::bindings::{BindingError, BindingName};
use super::to_io_error;

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
