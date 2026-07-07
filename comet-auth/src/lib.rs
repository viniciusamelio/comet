mod config;
mod cookie;
mod error;
mod oauth;
mod routes;
mod session;
mod store;

pub use config::{
    AppleProviderConfig, AuthConfig, CookieSameSite, GitHubProviderConfig, GoogleProviderConfig,
    ProviderConfig, providers,
};
pub use cookie::{build_session_cookie, remove_session_cookie};
pub use error::{AuthError, AuthErrorBody};
pub use oauth::{OAuthProviderId, OAuthStart, ProviderTokens};
pub use routes::{Auth, routes};
pub use session::{
    AuthSession, CachedSession, CurrentUser, IssuedSession, NewSession, OptionalAuthSession,
    ProviderIdentity, StoredSession, StoredUser,
};
pub use store::{
    D1SessionStore, KvOAuthStateStore, KvSessionCache, NoopOAuthStateStore, NoopSessionCache,
    OAuthState, OAuthStateStore, SessionCache, SessionStore,
};

pub const DEFAULT_SESSION_COOKIE: &str = "__Host-comet_session";
pub const DEFAULT_SESSION_TTL_SECONDS: u64 = 60 * 60 * 24 * 30;
