use rocket::http::Status;
use rocket::response::{Responder, Result as ResponseResult};
use rocket::serde::json::Json;
use rocket::{Request, Response};
use serde::Serialize;

#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("authentication is not configured on this Rocket instance")]
    MissingConfig,
    #[error("worker Env is not managed by Rocket")]
    MissingEnv,
    #[error("session cookie is missing")]
    MissingSession,
    #[error("session is invalid or expired")]
    InvalidSession,
    #[error("authenticated session is not authorized for this route")]
    Forbidden,
    #[error("unsupported auth provider: {0}")]
    UnsupportedProvider(String),
    #[error("auth provider `{0}` is not configured")]
    ProviderNotConfigured(String),
    #[error("missing auth provider setting `{setting}` for `{provider}`")]
    MissingProviderSetting {
        provider: &'static str,
        setting: &'static str,
    },
    #[error("invalid callback state")]
    InvalidOAuthState,
    #[error("invalid post-login redirect path")]
    InvalidRedirect,
    #[error("native login nonce is required")]
    MissingNonce,
    #[error("oauth provider request failed: {0}")]
    ProviderRequest(String),
    #[error("identity token is invalid: {0}")]
    InvalidIdentityToken(String),
    #[error("random bytes unavailable: {0}")]
    Random(getrandom::Error),
    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),
    #[cfg(feature = "cloudflare")]
    #[error("worker error: {0}")]
    Worker(#[from] worker::Error),
    #[cfg(feature = "cloudflare")]
    #[error("kv error: {0}")]
    Kv(#[from] worker::kv::KvError),
    #[error("storage error: {0}")]
    Storage(String),
}

#[derive(Debug, Serialize)]
#[serde(crate = "rocket::serde")]
pub struct AuthErrorBody {
    pub error: &'static str,
    pub message: String,
}

impl AuthError {
    pub fn status(&self) -> Status {
        match self {
            Self::MissingSession | Self::InvalidSession => Status::Unauthorized,
            Self::Forbidden => Status::Forbidden,
            Self::UnsupportedProvider(_)
            | Self::ProviderNotConfigured(_)
            | Self::MissingProviderSetting { .. }
            | Self::InvalidOAuthState
            | Self::InvalidRedirect
            | Self::MissingNonce
            | Self::ProviderRequest(_)
            | Self::InvalidIdentityToken(_) => Status::BadRequest,
            _ => Status::InternalServerError,
        }
    }

    pub fn code(&self) -> &'static str {
        match self {
            Self::MissingConfig => "missing_config",
            Self::MissingEnv => "missing_env",
            Self::MissingSession => "missing_session",
            Self::InvalidSession => "invalid_session",
            Self::Forbidden => "forbidden",
            Self::UnsupportedProvider(_) => "unsupported_provider",
            Self::ProviderNotConfigured(_) => "provider_not_configured",
            Self::MissingProviderSetting { .. } => "missing_provider_setting",
            Self::InvalidOAuthState => "invalid_oauth_state",
            Self::InvalidRedirect => "invalid_redirect",
            Self::MissingNonce => "missing_nonce",
            Self::ProviderRequest(_) => "provider_request",
            Self::InvalidIdentityToken(_) => "invalid_identity_token",
            Self::Random(_) => "random",
            Self::Serde(_) => "serde",
            #[cfg(feature = "cloudflare")]
            Self::Worker(_) => "worker",
            #[cfg(feature = "cloudflare")]
            Self::Kv(_) => "kv",
            Self::Storage(_) => "storage",
        }
    }
}

impl<'r> Responder<'r, 'static> for AuthError {
    fn respond_to(self, request: &'r Request<'_>) -> ResponseResult<'static> {
        let status = self.status();
        let body = AuthErrorBody {
            error: self.code(),
            message: self.to_string(),
        };

        Response::build_from(Json(body).respond_to(request)?)
            .status(status)
            .ok()
    }
}
