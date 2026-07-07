use std::marker::PhantomData;

use rocket::fairing::AdHoc;
use rocket::http::{CookieJar, Status};
use rocket::request::{FromRequest, Outcome};
use rocket::response::Redirect;
use rocket::serde::json::Json;
#[cfg(feature = "cloudflare")]
use rocket::{Build, Rocket};
use rocket::{Request, Route};
use serde::Serialize;

#[cfg(feature = "cloudflare")]
use crate::session::{self as session_core, CachedSession};
#[cfg(feature = "cloudflare")]
use crate::store::{OAuthStateStore, SessionCache, SessionStore};
use crate::{
    AuthConfig, AuthError, AuthSession, CurrentUser, OptionalAuthSession, remove_session_cookie,
};
#[cfg(feature = "cloudflare")]
use crate::{D1SessionStore, KvOAuthStateStore, KvSessionCache};
#[cfg(feature = "cloudflare")]
use crate::{NewSession, OAuthProviderId, OAuthState, build_session_cookie};

#[cfg(feature = "cloudflare")]
use comet::cloudflare::BindingName;

#[derive(Debug, Clone)]
#[cfg_attr(not(feature = "cloudflare"), allow(dead_code))]
pub struct AuthState {
    pub config: AuthConfig,
    pub db_binding: &'static str,
    pub kv_binding: &'static str,
}

pub struct Auth<DB, KV> {
    _bindings: PhantomData<(DB, KV)>,
}

#[cfg(feature = "cloudflare")]
impl<DB, KV> Auth<DB, KV>
where
    DB: BindingName + Send + Sync + 'static,
    KV: BindingName + Send + Sync + 'static,
{
    pub fn fairing(config: AuthConfig) -> AdHoc {
        AdHoc::on_ignite("Comet Auth", |rocket| async move {
            register_auth_state::<DB, KV>(rocket, config)
        })
    }
}

#[cfg(not(feature = "cloudflare"))]
impl<DB, KV> Auth<DB, KV> {
    pub fn fairing(config: AuthConfig) -> AdHoc {
        AdHoc::on_ignite("Comet Auth", |rocket| async move {
            rocket.manage(AuthState {
                config,
                db_binding: "",
                kv_binding: "",
            })
        })
    }
}

#[cfg(feature = "cloudflare")]
fn register_auth_state<DB, KV>(rocket: Rocket<Build>, config: AuthConfig) -> Rocket<Build>
where
    DB: BindingName + Send + Sync + 'static,
    KV: BindingName + Send + Sync + 'static,
{
    rocket.manage(AuthState {
        config,
        db_binding: DB::NAME,
        kv_binding: KV::NAME,
    })
}

#[cfg(feature = "cloudflare")]
pub fn routes<DB, KV>() -> Vec<Route>
where
    DB: BindingName + Send + Sync + 'static,
    KV: BindingName + Send + Sync + 'static,
{
    rocket::routes![session, logout, oauth_start, oauth_callback]
}

#[cfg(not(feature = "cloudflare"))]
pub fn routes<DB, KV>() -> Vec<Route> {
    rocket::routes![session, logout, oauth_start, oauth_callback]
}

#[derive(Debug, Serialize)]
#[serde(crate = "rocket::serde")]
pub struct SessionResponse {
    pub authenticated: bool,
    pub session: Option<AuthSession>,
    pub user: Option<CurrentUser>,
}

#[rocket::get("/session")]
pub async fn session(optional: OptionalAuthSession) -> Json<SessionResponse> {
    let session = optional.0;
    let user = session.as_ref().map(|session| session.user.clone());
    Json(SessionResponse {
        authenticated: session.is_some(),
        session,
        user,
    })
}

#[cfg(feature = "cloudflare")]
#[rocket::get("/<provider>/start?<redirect_after>")]
pub async fn oauth_start(
    provider: &str,
    redirect_after: Option<&str>,
    state: &rocket::State<AuthState>,
    env: &rocket::State<worker::Env>,
) -> Result<Redirect, AuthError> {
    let provider = OAuthProviderId::parse(provider)?;
    let start = crate::oauth::start_oauth(
        &state.config,
        env.inner(),
        provider,
        redirect_after.map(str::to_owned),
    )?;
    let kv = env.kv(state.kv_binding)?;
    let expires_at = session_core::now_unix() + crate::oauth::OAUTH_STATE_TTL_SECONDS as i64;
    KvOAuthStateStore::new(kv)
        .put(
            &start.state_hash,
            &OAuthState {
                provider: provider.as_str().to_owned(),
                code_verifier: start.code_verifier,
                nonce: Some(start.nonce),
                redirect_after: start.redirect_after,
                expires_at,
            },
            crate::oauth::OAUTH_STATE_TTL_SECONDS,
        )
        .await?;

    Ok(Redirect::to(start.authorize_url))
}

#[cfg(not(feature = "cloudflare"))]
#[rocket::get("/<provider>/start?<redirect_after>")]
pub async fn oauth_start(
    provider: &str,
    redirect_after: Option<&str>,
) -> Result<Redirect, AuthError> {
    let _ = (provider, redirect_after);
    Err(AuthError::MissingEnv)
}

#[cfg(feature = "cloudflare")]
#[rocket::get("/<provider>/callback?<code>&<state>&<error>&<error_description>")]
pub async fn oauth_callback(
    provider: &str,
    code: Option<&str>,
    state: Option<&str>,
    error: Option<&str>,
    error_description: Option<&str>,
    jar: &CookieJar<'_>,
    auth_state: &rocket::State<AuthState>,
    env: &rocket::State<worker::Env>,
) -> Result<Redirect, AuthError> {
    if let Some(error) = error {
        return Err(AuthError::ProviderRequest(
            error_description.unwrap_or(error).to_owned(),
        ));
    }

    let provider = OAuthProviderId::parse(provider)?;
    let code = code.ok_or(AuthError::InvalidOAuthState)?;
    let raw_state = state.ok_or(AuthError::InvalidOAuthState)?;
    let state_hash = session_core::hash_token(raw_state, None);
    let kv = env.kv(auth_state.kv_binding)?;
    let oauth_state = KvOAuthStateStore::new(kv)
        .take(&state_hash)
        .await?
        .ok_or(AuthError::InvalidOAuthState)?;

    if oauth_state.provider != provider.as_str() || oauth_state.expires_at <= session_core::now_unix() {
        return Err(AuthError::InvalidOAuthState);
    }

    let tokens = crate::oauth::exchange_code(
        &auth_state.config,
        env.inner(),
        provider,
        code,
        &oauth_state.code_verifier,
    )
    .await?;
    let identity = crate::oauth::fetch_identity(provider, &tokens).await?;
    let db = env.d1(auth_state.db_binding)?;
    let store = D1SessionStore::new(db);
    let user = store.upsert_account(identity).await?;
    let pepper = token_pepper(env, &auth_state.config)?;
    let issued = store
        .create_session(NewSession {
            user_id: user.id,
            ttl_seconds: auth_state.config.session_ttl_seconds,
            token_pepper: pepper,
            user_agent_hash: None,
            ip_hash: None,
        })
        .await?;

    if let Ok(kv) = env.kv(auth_state.kv_binding) {
        let ttl = issued
            .stored
            .expires_at
            .saturating_sub(session_core::now_unix()) as u64;
        if ttl > 0 {
            let _ = KvSessionCache::new(kv)
                .put(&CachedSession::from(&issued.stored), ttl)
                .await;
        }
    }

    jar.add(build_session_cookie(&auth_state.config, issued.token));
    Ok(Redirect::to(
        oauth_state.redirect_after.unwrap_or_else(|| "/".to_owned()),
    ))
}

#[cfg(not(feature = "cloudflare"))]
#[rocket::get("/<provider>/callback?<code>&<state>&<error>&<error_description>")]
pub async fn oauth_callback(
    provider: &str,
    code: Option<&str>,
    state: Option<&str>,
    error: Option<&str>,
    error_description: Option<&str>,
) -> Result<Redirect, AuthError> {
    let _ = (provider, code, state, error, error_description);
    Err(AuthError::MissingEnv)
}

#[cfg(feature = "cloudflare")]
#[rocket::post("/logout")]
pub async fn logout(
    session: Option<AuthSession>,
    jar: &CookieJar<'_>,
    state: &rocket::State<AuthState>,
    env: &rocket::State<worker::Env>,
) -> Result<Status, AuthError> {
    if let Some(session) = session {
        let db = env.d1(state.db_binding)?;
        D1SessionStore::new(db).revoke_session(&session.id).await?;
    }

    jar.remove(remove_session_cookie(&state.config));
    Ok(Status::NoContent)
}

#[cfg(not(feature = "cloudflare"))]
#[rocket::post("/logout")]
pub async fn logout(
    session: Option<AuthSession>,
    jar: &CookieJar<'_>,
    state: &rocket::State<AuthState>,
) -> Result<Status, AuthError> {
    let _ = session;

    jar.remove(remove_session_cookie(&state.config));
    Ok(Status::NoContent)
}

#[rocket::async_trait]
impl<'r> FromRequest<'r> for AuthSession {
    type Error = AuthError;

    async fn from_request(request: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        match load_session_guard(request).await {
            Ok(Some(session)) => Outcome::Success(session),
            Ok(None) => Outcome::Error((Status::Unauthorized, AuthError::MissingSession)),
            Err(error) => Outcome::Error((error.status(), error)),
        }
    }
}

#[rocket::async_trait]
impl<'r> FromRequest<'r> for OptionalAuthSession {
    type Error = AuthError;

    async fn from_request(request: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        match load_session_guard(request).await {
            Ok(session) => Outcome::Success(OptionalAuthSession(session)),
            Err(AuthError::MissingSession | AuthError::InvalidSession) => {
                Outcome::Success(OptionalAuthSession(None))
            }
            Err(error) => Outcome::Error((error.status(), error)),
        }
    }
}

#[cfg(feature = "cloudflare")]
async fn load_session_guard(request: &Request<'_>) -> Result<Option<AuthSession>, AuthError> {
    comet::cloudflare::local(load_session(request)).await
}

#[cfg(not(feature = "cloudflare"))]
async fn load_session_guard(request: &Request<'_>) -> Result<Option<AuthSession>, AuthError> {
    load_session(request).await
}

#[cfg(feature = "cloudflare")]
async fn load_session(request: &Request<'_>) -> Result<Option<AuthSession>, AuthError> {
    let state = request
        .rocket()
        .state::<AuthState>()
        .ok_or(AuthError::MissingConfig)?;
    let token = request
        .cookies()
        .get(&state.config.session_cookie)
        .map(|cookie| cookie.value().to_owned())
        .ok_or(AuthError::MissingSession)?;
    let env = request
        .rocket()
        .state::<worker::Env>()
        .ok_or(AuthError::MissingEnv)?;
    let pepper = token_pepper(env, &state.config)?;
    let token_hash = session_core::hash_token(&token, pepper.as_deref());
    let now = session_core::now_unix();

    if let Ok(kv) = env.kv(state.kv_binding) {
        let cache = KvSessionCache::new(kv);
        if let Some(cached) = cache.get(&token_hash).await? {
            if cached.is_active_at(now) {
                return Ok(Some(auth_session_from_cached(cached)));
            }
            cache.delete(&token_hash).await?;
        }
    }

    let db = env.d1(state.db_binding)?;
    let store = D1SessionStore::new(db);
    let Some(stored) = store.find_session(&token_hash).await? else {
        return Err(AuthError::InvalidSession);
    };

    if !stored.is_active_at(now) {
        return Err(AuthError::InvalidSession);
    }

    if let Ok(kv) = env.kv(state.kv_binding) {
        let ttl = stored.expires_at.saturating_sub(now) as u64;
        if ttl > 0 {
            let _ = KvSessionCache::new(kv).put(&CachedSession::from(&stored), ttl).await;
        }
    }

    Ok(Some(AuthSession {
        id: stored.id,
        user: stored.user.into(),
        expires_at: stored.expires_at,
    }))
}

#[cfg(not(feature = "cloudflare"))]
async fn load_session(_request: &Request<'_>) -> Result<Option<AuthSession>, AuthError> {
    Err(AuthError::MissingSession)
}

#[cfg(feature = "cloudflare")]
fn auth_session_from_cached(cached: CachedSession) -> AuthSession {
    AuthSession {
        id: cached.id,
        user: cached.user,
        expires_at: cached.expires_at,
    }
}

#[cfg(feature = "cloudflare")]
fn token_pepper(env: &worker::Env, config: &AuthConfig) -> Result<Option<String>, AuthError> {
    let Some(binding) = config.token_pepper_env.as_deref() else {
        return Ok(None);
    };

    match env.secret(binding) {
        Ok(secret) => Ok(Some(secret.to_string())),
        Err(_) => match env.var(binding) {
            Ok(var) => Ok(Some(var.to_string())),
            Err(_) => Ok(None),
        },
    }
}

#[cfg(test)]
mod tests {
    use rocket::http::{Cookie, Status};
    use rocket::local::asynchronous::Client;

    use crate::{Auth, AuthConfig, OptionalAuthSession};

    #[rocket::get("/probe")]
    async fn probe(session: OptionalAuthSession) -> &'static str {
        if session.0.is_some() { "yes" } else { "no" }
    }

    #[rocket::async_test]
    async fn optional_session_allows_anonymous_requests() {
        let app = rocket::build()
            .attach(Auth::<(), ()>::fairing(AuthConfig::default()))
            .mount("/", rocket::routes![probe]);
        let client = Client::untracked(app).await.unwrap();

        let response = client
            .get("/probe")
            .cookie(Cookie::new("__Host-comet_session", "missing"))
            .dispatch()
            .await;

        assert_eq!(response.status(), Status::Ok);
        assert_eq!(response.into_string().await.unwrap(), "no");
    }
}
