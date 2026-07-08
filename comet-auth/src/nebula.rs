use comet::nebula::AccessContext;

use crate::{AuthSession, AuthorizationClaims, AuthorizedSession};

pub trait NebulaAccessContextExt {
    fn to_nebula_access_context(&self) -> AccessContext;
}

impl NebulaAccessContextExt for AuthSession {
    fn to_nebula_access_context(&self) -> AccessContext {
        AccessContext::authenticated(self.user.id.clone())
    }
}

impl<P> NebulaAccessContextExt for AuthorizedSession<P> {
    fn to_nebula_access_context(&self) -> AccessContext {
        self.session
            .to_nebula_access_context()
            .with_roles(self.claims.roles.clone())
            .with_permissions(self.claims.permissions.clone())
            .with_scopes(self.claims.permissions.clone())
    }
}

impl NebulaAccessContextExt for AuthorizationClaims {
    fn to_nebula_access_context(&self) -> AccessContext {
        AccessContext::default()
            .with_roles(self.roles.clone())
            .with_permissions(self.permissions.clone())
            .with_scopes(self.permissions.clone())
    }
}
