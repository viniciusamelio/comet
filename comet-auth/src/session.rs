use base64ct::{Base64UrlUnpadded, Encoding};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::AuthError;

#[allow(dead_code)]
const TOKEN_BYTES: usize = 32;
#[allow(dead_code)]
const ID_BYTES: usize = 16;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CurrentUser {
    pub id: String,
    pub primary_email: Option<String>,
    pub email_verified: bool,
    pub name: Option<String>,
    pub avatar_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredUser {
    pub id: String,
    pub primary_email: Option<String>,
    pub email_verified: bool,
    pub name: Option<String>,
    pub avatar_url: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

impl From<StoredUser> for CurrentUser {
    fn from(user: StoredUser) -> Self {
        Self {
            id: user.id,
            primary_email: user.primary_email,
            email_verified: user.email_verified,
            name: user.name,
            avatar_url: user.avatar_url,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredSession {
    pub id: String,
    pub user: StoredUser,
    pub token_hash: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub expires_at: i64,
    pub revoked_at: Option<i64>,
    pub user_agent_hash: Option<String>,
    pub ip_hash: Option<String>,
}

impl StoredSession {
    pub fn is_active_at(&self, now: i64) -> bool {
        self.revoked_at.is_none() && self.expires_at > now
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CachedSession {
    pub id: String,
    pub user: CurrentUser,
    pub token_hash: String,
    pub expires_at: i64,
}

impl CachedSession {
    pub fn is_active_at(&self, now: i64) -> bool {
        self.expires_at > now
    }
}

impl From<&StoredSession> for CachedSession {
    fn from(session: &StoredSession) -> Self {
        Self {
            id: session.id.clone(),
            user: session.user.clone().into(),
            token_hash: session.token_hash.clone(),
            expires_at: session.expires_at,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewSession {
    pub user_id: String,
    pub ttl_seconds: u64,
    pub token_pepper: Option<String>,
    pub user_agent_hash: Option<String>,
    pub ip_hash: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IssuedSession {
    pub token: String,
    pub stored: StoredSession,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderIdentity {
    pub provider: String,
    pub provider_account_id: String,
    pub email: Option<String>,
    pub email_verified: bool,
    pub name: Option<String>,
    pub avatar_url: Option<String>,
    pub raw_profile_json: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthSession {
    pub id: String,
    pub user: CurrentUser,
    pub expires_at: i64,
}

impl AuthSession {
    pub fn user(&self) -> &CurrentUser {
        &self.user
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OptionalAuthSession(pub Option<AuthSession>);

#[allow(dead_code)]
pub fn generate_token() -> Result<String, AuthError> {
    let mut bytes = [0u8; TOKEN_BYTES];
    getrandom::fill(&mut bytes).map_err(AuthError::Random)?;
    Ok(Base64UrlUnpadded::encode_string(&bytes))
}

#[allow(dead_code)]
pub fn generate_id(prefix: &str) -> Result<String, AuthError> {
    let mut bytes = [0u8; ID_BYTES];
    getrandom::fill(&mut bytes).map_err(AuthError::Random)?;
    Ok(format!("{prefix}_{}", Base64UrlUnpadded::encode_string(&bytes)))
}

#[allow(dead_code)]
pub fn hash_token(token: &str, pepper: Option<&str>) -> String {
    let mut hasher = Sha256::new();
    if let Some(pepper) = pepper {
        hasher.update(pepper.as_bytes());
        hasher.update([0]);
    }
    hasher.update(token.as_bytes());
    Base64UrlUnpadded::encode_string(&hasher.finalize())
}

#[allow(dead_code)]
pub fn hash_optional_header(value: Option<&str>, pepper: Option<&str>) -> Option<String> {
    value.map(|value| hash_token(value, pepper))
}

#[allow(dead_code)]
pub fn now_unix() -> i64 {
    #[cfg(target_arch = "wasm32")]
    {
        (js_sys::Date::now() / 1000.0) as i64
    }

    #[cfg(not(target_arch = "wasm32"))]
    {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64
    }
}

#[cfg(test)]
mod tests {
    use super::{hash_token, now_unix};

    #[test]
    fn token_hash_uses_pepper() {
        assert_ne!(hash_token("token", None), hash_token("token", Some("pepper")));
    }

    #[test]
    fn now_is_unix_seconds() {
        assert!(now_unix() > 1_700_000_000);
    }
}
