use rocket::async_trait;
use serde::{Deserialize, Serialize};

#[cfg(feature = "cloudflare")]
use crate::session;
use crate::session::{CachedSession, IssuedSession, NewSession, ProviderIdentity};
use crate::{AuthError, StoredSession, StoredUser};

#[async_trait(?Send)]
pub trait SessionStore {
    async fn create_session(&self, input: NewSession) -> Result<IssuedSession, AuthError>;
    async fn find_session(&self, token_hash: &str) -> Result<Option<StoredSession>, AuthError>;
    async fn revoke_session(&self, session_id: &str) -> Result<(), AuthError>;
    async fn upsert_account(&self, identity: ProviderIdentity) -> Result<StoredUser, AuthError>;
}

#[async_trait(?Send)]
pub trait SessionCache {
    async fn get(&self, token_hash: &str) -> Result<Option<CachedSession>, AuthError>;
    async fn put(&self, session: &CachedSession, ttl_seconds: u64) -> Result<(), AuthError>;
    async fn delete(&self, token_hash: &str) -> Result<(), AuthError>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OAuthState {
    pub provider: String,
    pub code_verifier: String,
    pub nonce: Option<String>,
    pub redirect_after: Option<String>,
    pub expires_at: i64,
}

#[async_trait(?Send)]
pub trait OAuthStateStore {
    async fn put(
        &self,
        state_hash: &str,
        state: &OAuthState,
        ttl_seconds: u64,
    ) -> Result<(), AuthError>;
    async fn take(&self, state_hash: &str) -> Result<Option<OAuthState>, AuthError>;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct NoopSessionCache;

#[async_trait(?Send)]
impl SessionCache for NoopSessionCache {
    async fn get(&self, _token_hash: &str) -> Result<Option<CachedSession>, AuthError> {
        Ok(None)
    }

    async fn put(&self, _session: &CachedSession, _ttl_seconds: u64) -> Result<(), AuthError> {
        Ok(())
    }

    async fn delete(&self, _token_hash: &str) -> Result<(), AuthError> {
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct NoopOAuthStateStore;

#[async_trait(?Send)]
impl OAuthStateStore for NoopOAuthStateStore {
    async fn put(
        &self,
        _state_hash: &str,
        _state: &OAuthState,
        _ttl_seconds: u64,
    ) -> Result<(), AuthError> {
        Ok(())
    }

    async fn take(&self, _state_hash: &str) -> Result<Option<OAuthState>, AuthError> {
        Ok(None)
    }
}

#[cfg(feature = "cloudflare")]
#[derive(Debug)]
pub struct D1SessionStore {
    db: worker::D1Database,
}

#[cfg(feature = "cloudflare")]
impl D1SessionStore {
    pub fn new(db: worker::D1Database) -> Self {
        Self { db }
    }
}

#[cfg(feature = "cloudflare")]
#[derive(Debug, Deserialize)]
struct SessionRow {
    session_id: String,
    user_id: String,
    token_hash: String,
    session_created_at: i64,
    session_updated_at: i64,
    expires_at: i64,
    revoked_at: Option<i64>,
    user_agent_hash: Option<String>,
    ip_hash: Option<String>,
    primary_email: Option<String>,
    email_verified: i64,
    name: Option<String>,
    avatar_url: Option<String>,
    user_created_at: i64,
    user_updated_at: i64,
}

#[cfg(feature = "cloudflare")]
impl From<SessionRow> for StoredSession {
    fn from(row: SessionRow) -> Self {
        Self {
            id: row.session_id,
            user: StoredUser {
                id: row.user_id,
                primary_email: row.primary_email,
                email_verified: row.email_verified != 0,
                name: row.name,
                avatar_url: row.avatar_url,
                created_at: row.user_created_at,
                updated_at: row.user_updated_at,
            },
            token_hash: row.token_hash,
            created_at: row.session_created_at,
            updated_at: row.session_updated_at,
            expires_at: row.expires_at,
            revoked_at: row.revoked_at,
            user_agent_hash: row.user_agent_hash,
            ip_hash: row.ip_hash,
        }
    }
}

#[cfg(feature = "cloudflare")]
#[derive(Debug, Deserialize)]
struct UserRow {
    id: String,
    primary_email: Option<String>,
    email_verified: i64,
    name: Option<String>,
    avatar_url: Option<String>,
    created_at: i64,
    updated_at: i64,
}

#[cfg(feature = "cloudflare")]
impl From<UserRow> for StoredUser {
    fn from(row: UserRow) -> Self {
        Self {
            id: row.id,
            primary_email: row.primary_email,
            email_verified: row.email_verified != 0,
            name: row.name,
            avatar_url: row.avatar_url,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}

#[cfg(feature = "cloudflare")]
#[async_trait(?Send)]
impl SessionStore for D1SessionStore {
    async fn create_session(&self, input: NewSession) -> Result<IssuedSession, AuthError> {
        let token = session::generate_token()?;
        let token_hash = session::hash_token(&token, input.token_pepper.as_deref());
        let session_id = session::generate_id("ses")?;
        let now = session::now_unix();
        let expires_at = now + input.ttl_seconds as i64;

        self.db
            .prepare(
                "INSERT INTO comet_auth_sessions \
                 (id, user_id, token_hash, created_at, updated_at, expires_at, revoked_at, user_agent_hash, ip_hash) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL, ?7, ?8)",
            )
            .bind(&[
                js(&session_id),
                js(&input.user_id),
                js(&token_hash),
                js(now),
                js(now),
                js(expires_at),
                js_opt(input.user_agent_hash.as_deref()),
                js_opt(input.ip_hash.as_deref()),
            ])?
            .run()
            .await?;

        let stored = self
            .find_session(&token_hash)
            .await?
            .ok_or_else(|| AuthError::Storage("created session could not be reloaded".into()))?;

        Ok(IssuedSession { token, stored })
    }

    async fn find_session(&self, token_hash: &str) -> Result<Option<StoredSession>, AuthError> {
        let row: Option<SessionRow> = self
            .db
            .prepare(
                "SELECT \
                    s.id AS session_id, s.user_id, s.token_hash, \
                    s.created_at AS session_created_at, s.updated_at AS session_updated_at, \
                    s.expires_at, s.revoked_at, s.user_agent_hash, s.ip_hash, \
                    u.primary_email, u.email_verified, u.name, u.avatar_url, \
                    u.created_at AS user_created_at, u.updated_at AS user_updated_at \
                 FROM comet_auth_sessions s \
                 JOIN comet_auth_users u ON u.id = s.user_id \
                 WHERE s.token_hash = ?1 \
                 LIMIT 1",
            )
            .bind(&[js(token_hash)])?
            .first(None)
            .await?;

        Ok(row.map(StoredSession::from))
    }

    async fn revoke_session(&self, session_id: &str) -> Result<(), AuthError> {
        let now = session::now_unix();
        self.db
            .prepare(
                "UPDATE comet_auth_sessions \
                 SET revoked_at = COALESCE(revoked_at, ?1), updated_at = ?2 \
                 WHERE id = ?3",
            )
            .bind(&[js(now), js(now), js(session_id)])?
            .run()
            .await?;
        Ok(())
    }

    async fn upsert_account(&self, identity: ProviderIdentity) -> Result<StoredUser, AuthError> {
        let now = session::now_unix();
        let existing_user_id: Option<String> = self
            .db
            .prepare(
                "SELECT user_id FROM comet_auth_accounts \
                 WHERE provider = ?1 AND provider_account_id = ?2 \
                 LIMIT 1",
            )
            .bind(&[js(&identity.provider), js(&identity.provider_account_id)])?
            .first(Some("user_id"))
            .await?;

        let user_id = match existing_user_id {
            Some(user_id) => {
                self.db
                    .prepare(
                        "UPDATE comet_auth_users \
                         SET primary_email = COALESCE(primary_email, ?1), \
                             email_verified = MAX(email_verified, ?2), \
                             name = COALESCE(?3, name), \
                             avatar_url = COALESCE(?4, avatar_url), \
                             updated_at = ?5 \
                         WHERE id = ?6",
                    )
                    .bind(&[
                        js_opt(identity.email.as_deref()),
                        js(if identity.email_verified { 1 } else { 0 }),
                        js_opt(identity.name.as_deref()),
                        js_opt(identity.avatar_url.as_deref()),
                        js(now),
                        js(&user_id),
                    ])?
                    .run()
                    .await?;
                user_id
            }
            None => {
                let user_id = session::generate_id("usr")?;
                self.db
                    .prepare(
                        "INSERT INTO comet_auth_users \
                         (id, primary_email, email_verified, name, avatar_url, created_at, updated_at) \
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    )
                    .bind(&[
                        js(&user_id),
                        js_opt(identity.email.as_deref()),
                        js(if identity.email_verified { 1 } else { 0 }),
                        js_opt(identity.name.as_deref()),
                        js_opt(identity.avatar_url.as_deref()),
                        js(now),
                        js(now),
                    ])?
                    .run()
                    .await?;
                user_id
            }
        };

        let account_id = session::generate_id("acc")?;
        self.db
            .prepare(
                "INSERT INTO comet_auth_accounts \
                 (id, user_id, provider, provider_account_id, email, email_verified, name, avatar_url, raw_profile_json, created_at, updated_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11) \
                 ON CONFLICT(provider, provider_account_id) DO UPDATE SET \
                    email = excluded.email, \
                    email_verified = excluded.email_verified, \
                    name = excluded.name, \
                    avatar_url = excluded.avatar_url, \
                    raw_profile_json = excluded.raw_profile_json, \
                    updated_at = excluded.updated_at",
            )
            .bind(&[
                js(&account_id),
                js(&user_id),
                js(&identity.provider),
                js(&identity.provider_account_id),
                js_opt(identity.email.as_deref()),
                js(if identity.email_verified { 1 } else { 0 }),
                js_opt(identity.name.as_deref()),
                js_opt(identity.avatar_url.as_deref()),
                js_opt(identity.raw_profile_json.as_deref()),
                js(now),
                js(now),
            ])?
            .run()
            .await?;

        load_user(&self.db, &user_id).await
    }
}

#[cfg(feature = "cloudflare")]
async fn load_user(db: &worker::D1Database, user_id: &str) -> Result<StoredUser, AuthError> {
    let row: Option<UserRow> = db
        .prepare(
            "SELECT id, primary_email, email_verified, name, avatar_url, created_at, updated_at \
             FROM comet_auth_users WHERE id = ?1 LIMIT 1",
        )
        .bind(&[js(user_id)])?
        .first(None)
        .await?;

    row.map(StoredUser::from)
        .ok_or_else(|| AuthError::Storage(format!("user `{user_id}` not found")))
}

#[cfg(feature = "cloudflare")]
#[derive(Debug)]
pub struct KvSessionCache {
    kv: worker::kv::KvStore,
    prefix: String,
}

#[cfg(feature = "cloudflare")]
impl KvSessionCache {
    pub fn new(kv: worker::kv::KvStore) -> Self {
        Self {
            kv,
            prefix: "session:".to_owned(),
        }
    }

    pub fn with_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.prefix = prefix.into();
        self
    }

    fn key(&self, token_hash: &str) -> String {
        format!("{}{token_hash}", self.prefix)
    }
}

#[cfg(feature = "cloudflare")]
#[async_trait(?Send)]
impl SessionCache for KvSessionCache {
    async fn get(&self, token_hash: &str) -> Result<Option<CachedSession>, AuthError> {
        let Some(value) = self.kv.get(&self.key(token_hash)).text().await? else {
            return Ok(None);
        };
        Ok(Some(serde_json::from_str(&value)?))
    }

    async fn put(&self, session: &CachedSession, ttl_seconds: u64) -> Result<(), AuthError> {
        self.kv
            .put(
                &self.key(&session.token_hash),
                serde_json::to_string(session)?,
            )?
            .expiration_ttl(ttl_seconds)
            .execute()
            .await?;
        Ok(())
    }

    async fn delete(&self, token_hash: &str) -> Result<(), AuthError> {
        self.kv.delete(&self.key(token_hash)).await?;
        Ok(())
    }
}

#[cfg(feature = "cloudflare")]
#[derive(Debug)]
pub struct KvOAuthStateStore {
    kv: worker::kv::KvStore,
    prefix: String,
}

#[cfg(feature = "cloudflare")]
impl KvOAuthStateStore {
    pub fn new(kv: worker::kv::KvStore) -> Self {
        Self {
            kv,
            prefix: "oauth_state:".to_owned(),
        }
    }

    fn key(&self, state_hash: &str) -> String {
        format!("{}{state_hash}", self.prefix)
    }
}

#[cfg(feature = "cloudflare")]
#[async_trait(?Send)]
impl OAuthStateStore for KvOAuthStateStore {
    async fn put(
        &self,
        state_hash: &str,
        state: &OAuthState,
        ttl_seconds: u64,
    ) -> Result<(), AuthError> {
        self.kv
            .put(&self.key(state_hash), serde_json::to_string(state)?)?
            .expiration_ttl(ttl_seconds)
            .execute()
            .await?;
        Ok(())
    }

    async fn take(&self, state_hash: &str) -> Result<Option<OAuthState>, AuthError> {
        let key = self.key(state_hash);
        let value = self.kv.get(&key).text().await?;
        self.kv.delete(&key).await?;
        value
            .map(|value| serde_json::from_str(&value).map_err(AuthError::from))
            .transpose()
    }
}

#[cfg(not(feature = "cloudflare"))]
#[derive(Debug)]
pub struct D1SessionStore;

#[cfg(not(feature = "cloudflare"))]
#[derive(Debug)]
pub struct KvSessionCache;

#[cfg(not(feature = "cloudflare"))]
#[derive(Debug)]
pub struct KvOAuthStateStore;

#[cfg(feature = "cloudflare")]
fn js(value: impl Into<worker::wasm_bindgen::JsValue>) -> worker::wasm_bindgen::JsValue {
    value.into()
}

#[cfg(feature = "cloudflare")]
fn js_opt(value: Option<&str>) -> worker::wasm_bindgen::JsValue {
    value
        .map(worker::wasm_bindgen::JsValue::from_str)
        .unwrap_or_else(worker::wasm_bindgen::JsValue::null)
}

#[cfg(test)]
mod tests {
    use rocket::async_test;

    use super::{NoopSessionCache, SessionCache};

    #[async_test]
    async fn noop_cache_misses() {
        assert!(NoopSessionCache.get("missing").await.unwrap().is_none());
    }
}
