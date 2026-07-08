use super::{
    Expr, RlsAuthorizationDef, RlsMatchMode, RlsOperation, RlsPolicyDef, RlsPolicyKind, SqlType,
    TableDef, Value,
};

#[derive(Debug, Clone, PartialEq, Default)]
pub struct AccessContext {
    pub user_id: Option<Value>,
    pub tenant_id: Option<Value>,
    pub roles: Vec<String>,
    pub permissions: Vec<String>,
    pub scopes: Vec<String>,
    pub resource: Option<String>,
}

impl AccessContext {
    pub fn authenticated(user_id: impl Into<String>) -> Self {
        Self::authenticated_value(user_id.into())
    }

    pub fn authenticated_value(user_id: impl Into<Value>) -> Self {
        Self {
            user_id: Some(user_id.into()),
            ..Self::default()
        }
    }

    pub fn with_tenant(mut self, tenant_id: impl Into<String>) -> Self {
        self.tenant_id = Some(Value::Text(tenant_id.into()));
        self
    }

    pub fn with_user_value(mut self, user_id: impl Into<Value>) -> Self {
        self.user_id = Some(user_id.into());
        self
    }

    pub fn with_tenant_value(mut self, tenant_id: impl Into<Value>) -> Self {
        self.tenant_id = Some(tenant_id.into());
        self
    }

    pub fn with_roles(mut self, roles: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.roles = roles.into_iter().map(Into::into).collect();
        self
    }

    pub fn with_permissions(
        mut self,
        permissions: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        self.permissions = permissions.into_iter().map(Into::into).collect();
        self
    }

    pub fn with_scopes(mut self, scopes: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.scopes = scopes.into_iter().map(Into::into).collect();
        self
    }

    pub fn with_resource(mut self, resource: impl Into<String>) -> Self {
        self.resource = Some(resource.into());
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RlsError {
    MissingUser {
        table: &'static str,
    },
    MissingTenant {
        table: &'static str,
    },
    Forbidden {
        table: &'static str,
    },
    TypeMismatch {
        table: &'static str,
        column: &'static str,
        expected: SqlType,
    },
    MissingCustomPredicate {
        table: &'static str,
        name: &'static str,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CustomPredicateRegistration {
    pub name: &'static str,
    pub operations: &'static [RlsOperation],
}

pub trait CustomPredicateProvider {
    fn predicate(
        &self,
        table: &'static str,
        name: &'static str,
        operation: RlsOperation,
        context: &AccessContext,
    ) -> Result<Expr, RlsError>;

    fn registered_predicates(&self) -> &'static [&'static str] {
        &[]
    }

    fn registered_predicate_rules(&self) -> &'static [CustomPredicateRegistration] {
        &[]
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct NoCustomPredicates;

impl CustomPredicateProvider for NoCustomPredicates {
    fn predicate(
        &self,
        table: &'static str,
        name: &'static str,
        _operation: RlsOperation,
        _context: &AccessContext,
    ) -> Result<Expr, RlsError> {
        Err(RlsError::MissingCustomPredicate { table, name })
    }
}

pub(crate) fn validate_policy_value_type(
    table: TableDef,
    column: &'static str,
    value: &Value,
) -> Result<(), RlsError> {
    let Some(column_def) = table
        .columns
        .iter()
        .find(|candidate| candidate.name == column)
    else {
        return Ok(());
    };

    let matches = matches!(
        (column_def.sql_type, value),
        (SqlType::Integer, Value::Integer(_))
            | (SqlType::Real, Value::Real(_))
            | (SqlType::Text, Value::Text(_))
            | (SqlType::Blob, Value::Blob(_))
            | (SqlType::Boolean, Value::Bool(_))
            | (SqlType::Boolean, Value::Integer(0 | 1))
            | (_, Value::Null)
    );

    if matches {
        Ok(())
    } else {
        Err(RlsError::TypeMismatch {
            table: table.name,
            column,
            expected: column_def.sql_type,
        })
    }
}

pub(crate) fn policy_applies(policy: &RlsPolicyDef, operation: RlsOperation) -> bool {
    policy.operations.is_empty() || policy.operations.contains(&operation)
}

pub(crate) fn table_has_protected_rls(table: TableDef) -> bool {
    table
        .rls
        .iter()
        .any(|policy| policy.kind != RlsPolicyKind::Public)
}

pub(crate) fn validate_custom_predicates(
    table: TableDef,
    predicates: &impl CustomPredicateProvider,
) -> Result<(), RlsError> {
    let registered = predicates.registered_predicates();
    let registered_rules = predicates.registered_predicate_rules();

    for policy in table
        .rls
        .iter()
        .filter(|policy| policy.kind == RlsPolicyKind::Custom)
    {
        let Some(name) = policy.custom else {
            continue;
        };
        let registered_for_operation = policy.operations.iter().all(|operation| {
            registered_rules.iter().any(|rule| {
                rule.name == name
                    && (rule.operations.is_empty() || rule.operations.contains(operation))
            }) || registered.contains(&name)
        });
        let registered_for_all_operations = policy.operations.is_empty()
            && (registered.contains(&name)
                || registered_rules
                    .iter()
                    .any(|rule| rule.name == name && rule.operations.is_empty()));

        if !(registered_for_operation || registered_for_all_operations) {
            return Err(RlsError::MissingCustomPredicate {
                table: table.name,
                name,
            });
        }
    }

    Ok(())
}

pub(crate) fn authorize_policy(
    table: &'static str,
    policy: &RlsPolicyDef,
    context: &AccessContext,
) -> Result<(), RlsError> {
    match policy.kind {
        RlsPolicyKind::Rbac => authorize_rbac(table, &policy.authorization, context),
        _ => Ok(()),
    }
}

fn authorize_rbac(
    table: &'static str,
    authorization: &RlsAuthorizationDef,
    context: &AccessContext,
) -> Result<(), RlsError> {
    if let Some(resource) = authorization.resource {
        if context
            .resource
            .as_deref()
            .is_some_and(|value| value != resource)
        {
            return Err(RlsError::Forbidden { table });
        }
    }

    let role_matches = authorization
        .roles
        .iter()
        .map(|role| context.roles.iter().any(|claim| claim == role));
    let permission_matches = authorization
        .permissions
        .iter()
        .map(|permission| context.permissions.iter().any(|claim| claim == permission));
    let scope_matches = authorization
        .scopes
        .iter()
        .map(|scope| context.scopes.iter().any(|claim| claim == scope));
    let mut matches = role_matches.chain(permission_matches).chain(scope_matches);

    let authorized = match authorization.mode {
        RlsMatchMode::All => matches.all(|matched| matched),
        RlsMatchMode::Any => matches.any(|matched| matched),
    };

    if authorized {
        Ok(())
    } else {
        Err(RlsError::Forbidden { table })
    }
}
