use std::marker::PhantomData;

use rocket::request::{FromRequest, Outcome};
use rocket::{Request, async_trait};
use serde::{Deserialize, Serialize};

use crate::{AuthError, AuthSession};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuthorizationRequirement {
    pub mode: AuthorizationMode,
    pub roles: &'static [&'static str],
    pub permissions: &'static [&'static str],
    pub scopes: &'static [&'static str],
    pub resource: Option<&'static str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthorizationMode {
    All,
    Any,
}

impl AuthorizationRequirement {
    pub const fn new(
        roles: &'static [&'static str],
        permissions: &'static [&'static str],
        scopes: &'static [&'static str],
    ) -> Self {
        Self::with_mode_and_resource(AuthorizationMode::All, roles, permissions, scopes, None)
    }

    pub const fn with_mode_and_resource(
        mode: AuthorizationMode,
        roles: &'static [&'static str],
        permissions: &'static [&'static str],
        scopes: &'static [&'static str],
        resource: Option<&'static str>,
    ) -> Self {
        Self {
            mode,
            roles,
            permissions,
            scopes,
            resource,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.roles.is_empty() && self.permissions.is_empty() && self.scopes.is_empty()
    }
}

pub trait RequiredAuthorization {
    const REQUIREMENT: AuthorizationRequirement;
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthorizationClaims {
    pub roles: Vec<String>,
    pub permissions: Vec<String>,
}

impl AuthorizationClaims {
    pub fn has_role(&self, role: &str) -> bool {
        self.roles.iter().any(|claim| claim == role)
    }

    pub fn has_permission(&self, permission: &str) -> bool {
        self.permissions.iter().any(|claim| claim == permission)
    }

    pub fn satisfies(&self, requirement: &AuthorizationRequirement) -> bool {
        if requirement.is_empty() {
            return true;
        }

        let role_matches = requirement.roles.iter().map(|role| self.has_role(role));
        let permission_matches = requirement
            .permissions
            .iter()
            .map(|permission| self.has_permission(permission));
        let scope_matches = requirement
            .scopes
            .iter()
            .map(|scope| self.has_permission(scope));
        let mut matches = role_matches.chain(permission_matches).chain(scope_matches);

        match requirement.mode {
            AuthorizationMode::All => matches.all(|matched| matched),
            AuthorizationMode::Any => matches.any(|matched| matched),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthorizedSession<P> {
    pub session: AuthSession,
    pub claims: AuthorizationClaims,
    #[serde(skip)]
    _policy: PhantomData<P>,
}

impl<P> AuthorizedSession<P> {
    pub fn session(&self) -> &AuthSession {
        &self.session
    }

    pub fn claims(&self) -> &AuthorizationClaims {
        &self.claims
    }

    #[allow(dead_code)]
    fn new(session: AuthSession, claims: AuthorizationClaims) -> Self {
        Self {
            session,
            claims,
            _policy: PhantomData,
        }
    }
}

#[async_trait]
impl<'r, P> FromRequest<'r> for AuthorizedSession<P>
where
    P: RequiredAuthorization + Send + Sync + 'static,
{
    type Error = AuthError;

    async fn from_request(request: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        match load_authorized_session::<P>(request).await {
            Ok(session) => Outcome::Success(session),
            Err(error) => Outcome::Error((error.status(), error)),
        }
    }
}

#[cfg(feature = "cloudflare")]
async fn load_authorized_session<P>(
    request: &Request<'_>,
) -> Result<AuthorizedSession<P>, AuthError>
where
    P: RequiredAuthorization + Send + Sync + 'static,
{
    comet::cloudflare::local(load_authorized_session_inner::<P>(request)).await
}

#[cfg(feature = "cloudflare")]
async fn load_authorized_session_inner<P>(
    request: &Request<'_>,
) -> Result<AuthorizedSession<P>, AuthError>
where
    P: RequiredAuthorization + Send + Sync + 'static,
{
    let Some(session) = crate::routes::load_session_guard(request).await? else {
        return Err(AuthError::MissingSession);
    };

    if P::REQUIREMENT.is_empty() {
        return Ok(AuthorizedSession::new(
            session,
            AuthorizationClaims::default(),
        ));
    }

    let state = request
        .rocket()
        .state::<crate::routes::AuthState>()
        .ok_or(AuthError::MissingConfig)?;
    let env = request
        .rocket()
        .state::<worker::Env>()
        .ok_or(AuthError::MissingEnv)?;
    let resource = P::REQUIREMENT.resource.unwrap_or("");
    let cache_key = claims_cache_key(&session.user.id, resource);
    let claims = if state.config.authorization_claims_cache_ttl_seconds > 0 {
        if let Ok(kv) = env.kv(state.kv_binding) {
            if let Some(claims) = load_claims_from_cache(&kv, &cache_key).await? {
                claims
            } else {
                let db = env.d1(state.db_binding)?;
                let claims = D1AuthorizationStore::new(db)
                    .claims_for_user(&session.user.id, P::REQUIREMENT.resource)
                    .await?;
                cache_claims(
                    &kv,
                    &cache_key,
                    &claims,
                    state.config.authorization_claims_cache_ttl_seconds,
                )
                .await?;
                claims
            }
        } else {
            let db = env.d1(state.db_binding)?;
            D1AuthorizationStore::new(db)
                .claims_for_user(&session.user.id, P::REQUIREMENT.resource)
                .await?
        }
    } else {
        let db = env.d1(state.db_binding)?;
        D1AuthorizationStore::new(db)
            .claims_for_user(&session.user.id, P::REQUIREMENT.resource)
            .await?
    };

    if claims.satisfies(&P::REQUIREMENT) {
        Ok(AuthorizedSession::new(session, claims))
    } else {
        Err(AuthError::Forbidden)
    }
}

#[cfg(not(feature = "cloudflare"))]
async fn load_authorized_session<P>(
    _request: &Request<'_>,
) -> Result<AuthorizedSession<P>, AuthError>
where
    P: RequiredAuthorization + Send + Sync + 'static,
{
    Err(AuthError::MissingSession)
}

#[cfg(feature = "cloudflare")]
#[derive(Debug)]
pub struct D1AuthorizationStore {
    db: worker::D1Database,
}

#[cfg(feature = "cloudflare")]
impl D1AuthorizationStore {
    pub fn new(db: worker::D1Database) -> Self {
        Self { db }
    }

    pub async fn claims_for_user(
        &self,
        user_id: &str,
        resource: Option<&str>,
    ) -> Result<AuthorizationClaims, AuthError> {
        #[derive(Debug, Deserialize)]
        struct ClaimRow {
            name: String,
        }

        let role_rows = self
            .db
            .prepare(
                "SELECT role_name AS name \
                 FROM comet_auth_user_roles \
                 WHERE user_id = ?1 AND (resource = '' OR resource = ?2) \
                 ORDER BY role_name",
            )
            .bind(&[js(user_id), js(resource.unwrap_or(""))])?
            .all()
            .await?
            .results::<ClaimRow>()?;

        let permission_rows = self
            .db
            .prepare(
                "SELECT permission_name AS name \
                 FROM comet_auth_user_permissions \
                 WHERE user_id = ?1 AND (resource = '' OR resource = ?2) \
                 UNION \
                 SELECT rp.permission_name AS name \
                 FROM comet_auth_user_roles ur \
                 JOIN comet_auth_role_permissions rp ON rp.role_name = ur.role_name \
                 WHERE ur.user_id = ?1 \
                    AND (ur.resource = '' OR ur.resource = ?2) \
                    AND (rp.resource = '' OR rp.resource = ?2) \
                 ORDER BY name",
            )
            .bind(&[js(user_id), js(resource.unwrap_or(""))])?
            .all()
            .await?
            .results::<ClaimRow>()?;

        Ok(AuthorizationClaims {
            roles: role_rows.into_iter().map(|row| row.name).collect(),
            permissions: permission_rows.into_iter().map(|row| row.name).collect(),
        })
    }
}

#[cfg(feature = "cloudflare")]
async fn load_claims_from_cache(
    kv: &worker::kv::KvStore,
    key: &str,
) -> Result<Option<AuthorizationClaims>, AuthError> {
    let Some(value) = kv.get(key).text().await? else {
        return Ok(None);
    };
    Ok(Some(serde_json::from_str(&value)?))
}

#[cfg(feature = "cloudflare")]
async fn cache_claims(
    kv: &worker::kv::KvStore,
    key: &str,
    claims: &AuthorizationClaims,
    ttl_seconds: u64,
) -> Result<(), AuthError> {
    kv.put(key, serde_json::to_string(claims)?)?
        .expiration_ttl(ttl_seconds)
        .execute()
        .await?;
    Ok(())
}

#[cfg(feature = "cloudflare")]
fn claims_cache_key(user_id: &str, resource: &str) -> String {
    format!("authz:{user_id}:{resource}")
}

#[cfg(not(feature = "cloudflare"))]
#[derive(Debug)]
pub struct D1AuthorizationStore;

#[cfg(feature = "cloudflare")]
fn js(value: impl Into<worker::wasm_bindgen::JsValue>) -> worker::wasm_bindgen::JsValue {
    value.into()
}

#[cfg(test)]
mod tests {
    use super::{AuthorizationClaims, AuthorizationMode, AuthorizationRequirement};

    #[test]
    fn scopes_are_permissions_for_enforcement() {
        let claims = AuthorizationClaims {
            roles: vec!["admin".to_owned()],
            permissions: vec!["boards:write".to_owned()],
        };
        let requirement = AuthorizationRequirement::new(&["admin"], &[], &["boards:write"]);

        assert!(claims.satisfies(&requirement));
    }

    #[test]
    fn missing_claim_fails_requirement() {
        let claims = AuthorizationClaims {
            roles: vec!["member".to_owned()],
            permissions: Vec::new(),
        };
        let requirement = AuthorizationRequirement::new(&["admin"], &[], &[]);

        assert!(!claims.satisfies(&requirement));
    }

    #[test]
    fn any_mode_allows_one_matching_claim() {
        let claims = AuthorizationClaims {
            roles: vec!["member".to_owned()],
            permissions: vec!["boards:read".to_owned()],
        };
        let requirement = AuthorizationRequirement::with_mode_and_resource(
            AuthorizationMode::Any,
            &["admin"],
            &["boards:read"],
            &[],
            None,
        );

        assert!(claims.satisfies(&requirement));
    }
}
