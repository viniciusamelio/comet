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
pub struct TableDef {
    pub name: &'static str,
    pub columns: &'static [ColumnDef],
    pub indexes: &'static [IndexDef],
    pub foreign_keys: &'static [ForeignKeyDef],
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
}

mod column;
mod migration;
mod query;
mod relationships;
mod value;

pub use column::{Column, ColumnRef, Direction, Expr, Ordering};
pub use migration::{
    MigrationBlocker, MigrationPlan, MigrationWriteError, SchemaLint, SchemaManifest,
};
pub use query::{Delete, Insert, QueryLint, Select, Statement, Update};
pub use relationships::{BelongsTo, HasMany, belongs_to, has_many};
pub use value::Value;

#[cfg(feature = "nebula-d1")]
pub mod d1;

#[cfg(feature = "nebula-schema")]
pub mod schema;

#[cfg(test)]
mod tests;
