pub use comet_macros::Entity;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    feature = "nebula-schema",
    derive(serde::Serialize, serde::Deserialize)
)]
pub enum SqlType {
    Integer,
    Real,
    Text,
    Blob,
    Boolean,
}

impl SqlType {
    pub const fn name(self) -> &'static str {
        match self {
            SqlType::Integer => "INTEGER",
            SqlType::Real => "REAL",
            SqlType::Text => "TEXT",
            SqlType::Blob => "BLOB",
            SqlType::Boolean => "BOOLEAN",
        }
    }
}

impl SqlType {
    pub const fn as_sql(self) -> &'static str {
        match self {
            SqlType::Integer => "INTEGER",
            SqlType::Real => "REAL",
            SqlType::Text => "TEXT",
            SqlType::Blob => "BLOB",
            SqlType::Boolean => "INTEGER",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColumnDef {
    pub name: &'static str,
    pub sql_type: SqlType,
    pub nullable: bool,
    pub primary_key: bool,
    pub auto_increment: bool,
    pub unique: bool,
    pub indexed: bool,
    pub default_sql: Option<&'static str>,
}

impl ColumnDef {
    pub const fn new(name: &'static str, sql_type: SqlType) -> Self {
        Self {
            name,
            sql_type,
            nullable: false,
            primary_key: false,
            auto_increment: false,
            unique: false,
            indexed: false,
            default_sql: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IndexDef {
    pub name: &'static str,
    pub columns: &'static [&'static str],
    pub unique: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ForeignKeyDef {
    pub columns: &'static [&'static str],
    pub references_table: &'static str,
    pub references_columns: &'static [&'static str],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    feature = "nebula-schema",
    derive(serde::Serialize, serde::Deserialize)
)]
pub enum RlsOperation {
    Select,
    Insert,
    Update,
    Delete,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    feature = "nebula-schema",
    derive(serde::Serialize, serde::Deserialize)
)]
pub enum RlsPolicyKind {
    Public,
    Owner,
    Tenant,
    Rbac,
    Custom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    feature = "nebula-schema",
    derive(serde::Serialize, serde::Deserialize)
)]
pub enum RlsMatchMode {
    All,
    Any,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RlsAuthorizationDef {
    pub mode: RlsMatchMode,
    pub roles: &'static [&'static str],
    pub permissions: &'static [&'static str],
    pub scopes: &'static [&'static str],
    pub resource: Option<&'static str>,
}

impl RlsAuthorizationDef {
    pub const fn empty() -> Self {
        Self {
            mode: RlsMatchMode::All,
            roles: &[],
            permissions: &[],
            scopes: &[],
            resource: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RlsPolicyDef {
    pub operations: &'static [RlsOperation],
    pub kind: RlsPolicyKind,
    pub column: Option<&'static str>,
    pub authorization: RlsAuthorizationDef,
    pub custom: Option<&'static str>,
}

impl RlsPolicyDef {
    pub const fn public() -> Self {
        Self {
            operations: &[],
            kind: RlsPolicyKind::Public,
            column: None,
            authorization: RlsAuthorizationDef::empty(),
            custom: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TableDef {
    pub name: &'static str,
    pub columns: &'static [ColumnDef],
    pub indexes: &'static [IndexDef],
    pub foreign_keys: &'static [ForeignKeyDef],
    pub rls: &'static [RlsPolicyDef],
}

pub trait Entity {
    const TABLE: TableDef;

    fn select() -> Select<Self>
    where
        Self: Sized,
    {
        Select::new()
    }

    fn insert() -> Insert<Self>
    where
        Self: Sized,
    {
        Insert::new()
    }

    fn update() -> Update<Self>
    where
        Self: Sized,
    {
        Update::new()
    }

    fn delete() -> Delete<Self>
    where
        Self: Sized,
    {
        Delete::new()
    }

    fn validate_custom_predicates_with(
        predicates: &impl CustomPredicateProvider,
    ) -> Result<(), RlsError>
    where
        Self: Sized,
    {
        rls::validate_custom_predicates(Self::TABLE, predicates)
    }

    fn select_scoped(context: &AccessContext) -> Result<Select<Self>, RlsError>
    where
        Self: Sized,
    {
        Self::select().apply_rls(context, &NoCustomPredicates)
    }

    fn select_scoped_with(
        context: &AccessContext,
        predicates: &impl CustomPredicateProvider,
    ) -> Result<Select<Self>, RlsError>
    where
        Self: Sized,
    {
        Self::select().apply_rls(context, predicates)
    }

    fn insert_scoped(context: &AccessContext) -> Result<Insert<Self>, RlsError>
    where
        Self: Sized,
    {
        Self::insert().apply_rls(context, &NoCustomPredicates)
    }

    fn insert_scoped_with(
        context: &AccessContext,
        predicates: &impl CustomPredicateProvider,
    ) -> Result<Insert<Self>, RlsError>
    where
        Self: Sized,
    {
        Self::insert().apply_rls(context, predicates)
    }

    fn update_scoped(context: &AccessContext) -> Result<Update<Self>, RlsError>
    where
        Self: Sized,
    {
        Self::update().apply_rls(context, &NoCustomPredicates)
    }

    fn update_scoped_with(
        context: &AccessContext,
        predicates: &impl CustomPredicateProvider,
    ) -> Result<Update<Self>, RlsError>
    where
        Self: Sized,
    {
        Self::update().apply_rls(context, predicates)
    }

    fn delete_scoped(context: &AccessContext) -> Result<Delete<Self>, RlsError>
    where
        Self: Sized,
    {
        Self::delete().apply_rls(context, &NoCustomPredicates)
    }

    fn delete_scoped_with(
        context: &AccessContext,
        predicates: &impl CustomPredicateProvider,
    ) -> Result<Delete<Self>, RlsError>
    where
        Self: Sized,
    {
        Self::delete().apply_rls(context, predicates)
    }
}

mod column;
mod migration;
mod query;
mod relationships;
mod rls;
mod value;

pub use column::{Column, ColumnRef, Direction, Expr, Ordering};
pub use migration::{
    MigrationBlocker, MigrationPlan, MigrationWriteError, SchemaLint, SchemaManifest,
};
pub use query::{
    Delete, Insert, QueryCheckError, QueryLint, QueryLintSeverity, Select, Statement, Update,
};
pub use relationships::{BelongsTo, HasMany, belongs_to, has_many};
pub use rls::{
    AccessContext, CustomPredicateProvider, CustomPredicateRegistration, NoCustomPredicates,
    RlsError,
};
pub use value::Value;

#[cfg(feature = "nebula-d1")]
pub mod d1;

#[cfg(feature = "nebula-schema")]
pub mod schema;

#[cfg(test)]
mod tests;
