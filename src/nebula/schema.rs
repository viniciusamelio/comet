use serde::{Deserialize, Serialize};

use super::{ColumnDef, ForeignKeyDef, IndexDef, SchemaManifest, SqlType, TableDef};

fn leak_str(value: String) -> &'static str {
    Box::leak(value.into_boxed_str())
}

fn leak_slice<T>(values: Vec<T>) -> &'static [T] {
    Box::leak(values.into_boxed_slice())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OwnedColumnDef {
    pub name: String,
    pub sql_type: SqlType,
    pub nullable: bool,
    pub primary_key: bool,
    pub auto_increment: bool,
    pub unique: bool,
    pub indexed: bool,
    pub default_sql: Option<String>,
}

impl From<&ColumnDef> for OwnedColumnDef {
    fn from(column: &ColumnDef) -> Self {
        Self {
            name: column.name.to_owned(),
            sql_type: column.sql_type,
            nullable: column.nullable,
            primary_key: column.primary_key,
            auto_increment: column.auto_increment,
            unique: column.unique,
            indexed: column.indexed,
            default_sql: column.default_sql.map(str::to_owned),
        }
    }
}

impl OwnedColumnDef {
    fn leak(self) -> ColumnDef {
        ColumnDef {
            name: leak_str(self.name),
            sql_type: self.sql_type,
            nullable: self.nullable,
            primary_key: self.primary_key,
            auto_increment: self.auto_increment,
            unique: self.unique,
            indexed: self.indexed,
            default_sql: self.default_sql.map(leak_str),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OwnedIndexDef {
    pub name: String,
    pub columns: Vec<String>,
    pub unique: bool,
}

impl From<&IndexDef> for OwnedIndexDef {
    fn from(index: &IndexDef) -> Self {
        Self {
            name: index.name.to_owned(),
            columns: index
                .columns
                .iter()
                .map(|column| (*column).to_owned())
                .collect(),
            unique: index.unique,
        }
    }
}

impl OwnedIndexDef {
    fn leak(self) -> IndexDef {
        IndexDef {
            name: leak_str(self.name),
            columns: leak_slice(self.columns.into_iter().map(leak_str).collect()),
            unique: self.unique,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OwnedForeignKeyDef {
    pub columns: Vec<String>,
    pub references_table: String,
    pub references_columns: Vec<String>,
}

impl From<&ForeignKeyDef> for OwnedForeignKeyDef {
    fn from(foreign_key: &ForeignKeyDef) -> Self {
        Self {
            columns: foreign_key
                .columns
                .iter()
                .map(|column| (*column).to_owned())
                .collect(),
            references_table: foreign_key.references_table.to_owned(),
            references_columns: foreign_key
                .references_columns
                .iter()
                .map(|column| (*column).to_owned())
                .collect(),
        }
    }
}

impl OwnedForeignKeyDef {
    fn leak(self) -> ForeignKeyDef {
        ForeignKeyDef {
            columns: leak_slice(self.columns.into_iter().map(leak_str).collect()),
            references_table: leak_str(self.references_table),
            references_columns: leak_slice(
                self.references_columns.into_iter().map(leak_str).collect(),
            ),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OwnedTableDef {
    pub name: String,
    pub columns: Vec<OwnedColumnDef>,
    pub indexes: Vec<OwnedIndexDef>,
    pub foreign_keys: Vec<OwnedForeignKeyDef>,
}

impl From<&TableDef> for OwnedTableDef {
    fn from(table: &TableDef) -> Self {
        Self {
            name: table.name.to_owned(),
            columns: table.columns.iter().map(OwnedColumnDef::from).collect(),
            indexes: table.indexes.iter().map(OwnedIndexDef::from).collect(),
            foreign_keys: table
                .foreign_keys
                .iter()
                .map(OwnedForeignKeyDef::from)
                .collect(),
        }
    }
}

impl OwnedTableDef {
    fn leak(self) -> TableDef {
        TableDef {
            name: leak_str(self.name),
            columns: leak_slice(self.columns.into_iter().map(OwnedColumnDef::leak).collect()),
            indexes: leak_slice(self.indexes.into_iter().map(OwnedIndexDef::leak).collect()),
            foreign_keys: leak_slice(
                self.foreign_keys
                    .into_iter()
                    .map(OwnedForeignKeyDef::leak)
                    .collect(),
            ),
        }
    }
}

/// An owned, JSON-round-trippable snapshot of a [`SchemaManifest`].
///
/// Callers persist this to disk after a successful migration and load it
/// back to build the "current" side of a [`SchemaManifest::diff`] call
/// against the schema the code declares today.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct SchemaSnapshot {
    pub tables: Vec<OwnedTableDef>,
}

impl SchemaSnapshot {
    pub fn from_manifest(manifest: &SchemaManifest) -> Self {
        Self {
            tables: manifest.tables.iter().map(OwnedTableDef::from).collect(),
        }
    }

    /// Leaks every identifier in this snapshot to `'static` and rebuilds
    /// a [`SchemaManifest`] from it. Intended for short-lived processes
    /// (a CLI invocation) where leaking the snapshot's memory for the
    /// process lifetime is an acceptable, deliberate trade-off in
    /// exchange for reusing the existing diff engine unchanged.
    pub fn to_manifest(self) -> SchemaManifest {
        SchemaManifest::new(self.tables.into_iter().map(OwnedTableDef::leak))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const COLUMNS: &[ColumnDef] = &[
        ColumnDef {
            name: "id",
            sql_type: SqlType::Integer,
            nullable: false,
            primary_key: true,
            auto_increment: true,
            unique: true,
            indexed: true,
            default_sql: None,
        },
        ColumnDef::new("title", SqlType::Text),
    ];

    const FOREIGN_KEYS: &[ForeignKeyDef] = &[ForeignKeyDef {
        columns: &["board_id"],
        references_table: "boards",
        references_columns: &["id"],
    }];

    const TABLE: TableDef = TableDef {
        name: "tasks",
        columns: COLUMNS,
        indexes: &[],
        foreign_keys: FOREIGN_KEYS,
    };

    #[test]
    fn snapshot_round_trips_through_json() {
        let manifest = SchemaManifest::new([TABLE]);
        let snapshot = SchemaSnapshot::from_manifest(&manifest);

        let json = serde_json::to_string(&snapshot).expect("serialize snapshot");
        let restored: SchemaSnapshot = serde_json::from_str(&json).expect("deserialize snapshot");

        assert_eq!(restored, snapshot);
    }

    #[test]
    fn snapshot_leaks_back_into_an_equivalent_manifest() {
        let manifest = SchemaManifest::new([TABLE]);
        let snapshot = SchemaSnapshot::from_manifest(&manifest);

        let restored = snapshot.to_manifest();

        assert_eq!(restored, manifest);
    }

    #[test]
    fn snapshot_diff_reuses_the_existing_diff_engine() {
        const DESIRED_COLUMNS: &[ColumnDef] = &[
            ColumnDef {
                name: "id",
                sql_type: SqlType::Integer,
                nullable: false,
                primary_key: true,
                auto_increment: true,
                unique: true,
                indexed: true,
                default_sql: None,
            },
            ColumnDef::new("title", SqlType::Text),
            ColumnDef {
                name: "done",
                sql_type: SqlType::Boolean,
                nullable: false,
                primary_key: false,
                auto_increment: false,
                unique: false,
                indexed: false,
                default_sql: Some("0"),
            },
        ];
        const DESIRED_TABLE: TableDef = TableDef {
            name: "tasks",
            columns: DESIRED_COLUMNS,
            indexes: &[],
            foreign_keys: FOREIGN_KEYS,
        };

        let current = SchemaSnapshot::from_manifest(&SchemaManifest::new([TABLE]));
        let desired = SchemaManifest::new([DESIRED_TABLE]);

        let plan = current.to_manifest().diff(&desired);

        assert!(plan.is_safe());
        assert_eq!(
            plan.statements,
            vec!["ALTER TABLE \"tasks\" ADD COLUMN \"done\" INTEGER NOT NULL DEFAULT 0"]
        );
    }
}
