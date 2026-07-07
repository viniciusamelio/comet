use std::marker::PhantomData;

use rocket::request::{FromRequest, Outcome};
use rocket::{Request, async_trait};
use serde::{Deserialize, Serialize};

use crate::{AuthError, AuthSession};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuthorizationRequirement {
    pub roles: &'static [&'static str],
    pub permissions: &'static [&'static str],
    pub scopes: &'static [&'static str],
}

impl AuthorizationRequirement {
    pub const fn new(
        roles: &'static [&'static str],
        permissions: &'static [&'static str],
        scopes: &'static [&'static str],
    ) -> Self {
        Self {
            roles,
            permissions,
            scopes,
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
        requirement.roles.iter().all(|role| self.has_role(role))
            && requirement
                .permissions
                .iter()
                .all(|permission| self.has_permission(permission))
            && requirement
                .scopes
                .iter()
                .all(|scope| self.has_permission(scope))
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
    let db = env.d1(state.db_binding)?;
    let claims = D1AuthorizationStore::new(db)
        .claims_for_user(&session.user.id)
        .await?;

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

    pub async fn claims_for_user(&self, user_id: &str) -> Result<AuthorizationClaims, AuthError> {
        #[derive(Debug, Deserialize)]
        struct ClaimRow {
            name: String,
        }

        let role_rows = self
            .db
            .prepare(
                "SELECT role_name AS name \
                 FROM comet_auth_user_roles \
                 WHERE user_id = ?1 AND resource = '' \
                 ORDER BY role_name",
            )
            .bind(&[js(user_id)])?
            .all()
            .await?
            .results::<ClaimRow>()?;

        let permission_rows = self
            .db
            .prepare(
                "SELECT permission_name AS name \
                 FROM comet_auth_user_permissions \
                 WHERE user_id = ?1 AND resource = '' \
                 UNION \
                 SELECT rp.permission_name AS name \
                 FROM comet_auth_user_roles ur \
                 JOIN comet_auth_role_permissions rp ON rp.role_name = ur.role_name \
                 WHERE ur.user_id = ?1 AND ur.resource = '' AND rp.resource = '' \
                 ORDER BY name",
            )
            .bind(&[js(user_id)])?
            .all()
            .await?
            .results::<ClaimRow>()?;

        Ok(AuthorizationClaims {
            roles: role_rows.into_iter().map(|row| row.name).collect(),
            permissions: permission_rows.into_iter().map(|row| row.name).collect(),
        })
    }
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
    use super::{AuthorizationClaims, AuthorizationRequirement};

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
}
