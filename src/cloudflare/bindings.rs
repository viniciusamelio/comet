use std::marker::PhantomData;
use std::ops::Deref;

use rocket::http::Status;
use rocket::request::{FromRequest, Outcome};
use worker::{Env, Error};

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
