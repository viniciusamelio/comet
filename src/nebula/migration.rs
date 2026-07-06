use super::column::quote_ident;
use super::{ColumnDef, ForeignKeyDef, TableDef};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaManifest {
    pub tables: Vec<TableDef>,
}

impl SchemaManifest {
    pub fn new(tables: impl IntoIterator<Item = TableDef>) -> Self {
        let mut tables = tables.into_iter().collect::<Vec<_>>();
        tables.sort_by_key(|table| table.name);
        Self { tables }
    }

    pub fn from_entities(tables: impl IntoIterator<Item = TableDef>) -> Self {
        Self::new(tables)
    }

    pub fn to_manifest_string(&self) -> String {
        self.tables
            .iter()
            .map(format_table_manifest)
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    pub fn lint(&self) -> Vec<SchemaLint> {
        self.tables.iter().flat_map(|table| table.lint()).collect()
    }

    pub fn initial_migration(&self) -> Vec<String> {
        self.tables
            .iter()
            .flat_map(|table| initial_table_migration(*table))
            .collect()
    }

    pub fn diff(&self, desired: &SchemaManifest) -> MigrationPlan {
        let mut statements = Vec::new();
        let mut blockers = Vec::new();

        for current_table in &self.tables {
            if desired.table(current_table.name).is_none() {
                blockers.push(MigrationBlocker::DropTable {
                    table: current_table.name.to_owned(),
                });
            }
        }

        for desired_table in &desired.tables {
            let Some(current_table) = self.table(desired_table.name) else {
                statements.extend(initial_table_migration(*desired_table));
                continue;
            };

            diff_table(
                current_table,
                *desired_table,
                &mut statements,
                &mut blockers,
            );
        }

        MigrationPlan {
            statements,
            blockers,
        }
    }

    fn table(&self, name: &str) -> Option<TableDef> {
        self.tables.iter().copied().find(|table| table.name == name)
    }
}

impl TableDef {
    pub fn lint(&self) -> Vec<SchemaLint> {
        self.foreign_keys
            .iter()
            .flat_map(|foreign_key| foreign_key.columns.iter())
            .filter(|column| !is_indexed_in_table(*self, column))
            .map(|column| SchemaLint::UnindexedForeignKey {
                table: self.name,
                column,
            })
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationPlan {
    pub statements: Vec<String>,
    pub blockers: Vec<MigrationBlocker>,
}

impl MigrationPlan {
    pub fn is_safe(&self) -> bool {
        self.blockers.is_empty()
    }

    pub fn to_sql_file_contents(&self) -> Result<String, MigrationWriteError> {
        if !self.is_safe() {
            return Err(MigrationWriteError::UnsafePlan {
                blockers: self.blockers.clone(),
            });
        }

        if self.statements.is_empty() {
            return Err(MigrationWriteError::EmptyPlan);
        }

        Ok(format!("{};\n", self.statements.join(";\n")))
    }

    pub fn migration_file_name(
        sequence: u32,
        name: impl AsRef<str>,
    ) -> Result<String, MigrationWriteError> {
        let slug = migration_name_slug(name.as_ref())?;
        Ok(format!("{sequence:04}_{slug}.sql"))
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn write_sql_file(
        &self,
        directory: impl AsRef<std::path::Path>,
        sequence: u32,
        name: impl AsRef<str>,
    ) -> Result<std::path::PathBuf, MigrationWriteError> {
        let contents = self.to_sql_file_contents()?;
        let file_name = Self::migration_file_name(sequence, name)?;
        let directory = directory.as_ref();
        std::fs::create_dir_all(directory).map_err(MigrationWriteError::Io)?;
        let path = directory.join(file_name);
        std::fs::write(&path, contents).map_err(MigrationWriteError::Io)?;
        Ok(path)
    }
}

#[derive(Debug)]
pub enum MigrationWriteError {
    UnsafePlan { blockers: Vec<MigrationBlocker> },
    EmptyPlan,
    InvalidName,
    Io(std::io::Error),
}

impl PartialEq for MigrationWriteError {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (
                MigrationWriteError::UnsafePlan { blockers: left },
                MigrationWriteError::UnsafePlan { blockers: right },
            ) => left == right,
            (MigrationWriteError::EmptyPlan, MigrationWriteError::EmptyPlan)
            | (MigrationWriteError::InvalidName, MigrationWriteError::InvalidName) => true,
            (MigrationWriteError::Io(left), MigrationWriteError::Io(right)) => {
                left.kind() == right.kind()
            }
            _ => false,
        }
    }
}

impl Eq for MigrationWriteError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MigrationBlocker {
    DropTable { table: String },
    DropColumn { table: String, column: String },
    ChangeColumn { table: String, column: String },
    UnsafeAddColumn { table: String, column: String },
    DropIndex { table: String, index: String },
    ChangeIndex { table: String, index: String },
    AddForeignKey { table: String, columns: Vec<String> },
    DropForeignKey { table: String, columns: Vec<String> },
    ChangeForeignKey { table: String, columns: Vec<String> },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchemaLint {
    UnindexedForeignKey {
        table: &'static str,
        column: &'static str,
    },
}

pub(crate) fn is_indexed_in_table(table: TableDef, column: &str) -> bool {
    table.columns.iter().any(|definition| {
        definition.name == column
            && (definition.primary_key || definition.unique || definition.indexed)
    }) || table
        .indexes
        .iter()
        .any(|index| index.columns.first() == Some(&column))
}

fn initial_table_migration(table: TableDef) -> Vec<String> {
    let mut statements = vec![create_table_statement(table)];
    statements.extend(index_statements(table));
    statements
}

fn create_table_statement(table: TableDef) -> String {
    let mut definitions = table
        .columns
        .iter()
        .map(format_column_def)
        .collect::<Vec<_>>();
    definitions.extend(table.foreign_keys.iter().map(format_foreign_key_def));
    let definitions = definitions.join(", ");

    format!("CREATE TABLE {} ({definitions})", quote_ident(table.name))
}

fn add_column_statement(table: TableDef, column: ColumnDef) -> String {
    format!(
        "ALTER TABLE {} ADD COLUMN {}",
        quote_ident(table.name),
        format_column_def(&column)
    )
}

fn index_statements(table: TableDef) -> Vec<String> {
    let mut statements = Vec::new();

    for column in table.columns {
        if column.indexed && !column.primary_key && !column.unique {
            statements.push(create_index_statement(
                table.name,
                &generated_index_name(table.name, column.name),
                &[column.name],
                false,
            ));
        }
    }

    for index in table.indexes {
        statements.push(create_index_statement(
            table.name,
            index.name,
            index.columns,
            index.unique,
        ));
    }

    statements
}

fn create_index_statement(table: &str, name: &str, columns: &[&str], unique: bool) -> String {
    let unique = if unique { "UNIQUE " } else { "" };
    let columns = columns
        .iter()
        .map(|column| quote_ident(column))
        .collect::<Vec<_>>()
        .join(", ");

    format!(
        "CREATE {unique}INDEX {} ON {} ({columns})",
        quote_ident(name),
        quote_ident(table)
    )
}

fn generated_index_name(table: &str, column: &str) -> String {
    format!("idx_{table}_{column}")
}

fn format_column_def(column: &ColumnDef) -> String {
    let mut parts = vec![
        quote_ident(column.name),
        column.sql_type.as_sql().to_owned(),
    ];

    if column.primary_key {
        parts.push("PRIMARY KEY".to_owned());
    }

    if column.auto_increment {
        parts.push("AUTOINCREMENT".to_owned());
    }

    if column.unique && !column.primary_key {
        parts.push("UNIQUE".to_owned());
    }

    if !column.nullable && !column.primary_key {
        parts.push("NOT NULL".to_owned());
    }

    if let Some(default_sql) = column.default_sql {
        parts.push(format!("DEFAULT {default_sql}"));
    }

    parts.join(" ")
}

fn format_foreign_key_def(foreign_key: &ForeignKeyDef) -> String {
    let columns = foreign_key
        .columns
        .iter()
        .map(|column| quote_ident(column))
        .collect::<Vec<_>>()
        .join(", ");
    let references_columns = foreign_key
        .references_columns
        .iter()
        .map(|column| quote_ident(column))
        .collect::<Vec<_>>()
        .join(", ");

    format!(
        "FOREIGN KEY ({columns}) REFERENCES {} ({references_columns})",
        quote_ident(foreign_key.references_table)
    )
}

fn diff_table(
    current: TableDef,
    desired: TableDef,
    statements: &mut Vec<String>,
    blockers: &mut Vec<MigrationBlocker>,
) {
    for current_column in current.columns {
        if find_column(desired, current_column.name).is_none() {
            blockers.push(MigrationBlocker::DropColumn {
                table: current.name.to_owned(),
                column: current_column.name.to_owned(),
            });
        }
    }

    for desired_column in desired.columns {
        match find_column(current, desired_column.name) {
            Some(current_column) if current_column != *desired_column => {
                blockers.push(MigrationBlocker::ChangeColumn {
                    table: current.name.to_owned(),
                    column: desired_column.name.to_owned(),
                });
            }
            Some(_) => {}
            None if can_add_column(*desired_column) => {
                statements.push(add_column_statement(desired, *desired_column));
            }
            None => blockers.push(MigrationBlocker::UnsafeAddColumn {
                table: current.name.to_owned(),
                column: desired_column.name.to_owned(),
            }),
        }
    }

    diff_indexes(current, desired, statements, blockers);
    diff_foreign_keys(current, desired, blockers);
}

fn diff_indexes(
    current: TableDef,
    desired: TableDef,
    statements: &mut Vec<String>,
    blockers: &mut Vec<MigrationBlocker>,
) {
    let current_indexes = index_manifest(current);
    let desired_indexes = index_manifest(desired);

    for current_index in &current_indexes {
        match desired_indexes
            .iter()
            .find(|index| index.name == current_index.name)
        {
            Some(desired_index) if desired_index != current_index => {
                blockers.push(MigrationBlocker::ChangeIndex {
                    table: current.name.to_owned(),
                    index: current_index.name.clone(),
                });
            }
            Some(_) => {}
            None => blockers.push(MigrationBlocker::DropIndex {
                table: current.name.to_owned(),
                index: current_index.name.clone(),
            }),
        }
    }

    for desired_index in &desired_indexes {
        if current_indexes
            .iter()
            .all(|index| index.name != desired_index.name)
        {
            statements.push(create_index_statement(
                desired.name,
                &desired_index.name,
                &desired_index.columns,
                desired_index.unique,
            ));
        }
    }
}

fn diff_foreign_keys(current: TableDef, desired: TableDef, blockers: &mut Vec<MigrationBlocker>) {
    let current_foreign_keys = foreign_key_manifest(current);
    let desired_foreign_keys = foreign_key_manifest(desired);

    for current_foreign_key in &current_foreign_keys {
        match desired_foreign_keys
            .iter()
            .find(|foreign_key| foreign_key.columns == current_foreign_key.columns)
        {
            Some(desired_foreign_key) if desired_foreign_key != current_foreign_key => {
                blockers.push(MigrationBlocker::ChangeForeignKey {
                    table: current.name.to_owned(),
                    columns: current_foreign_key.columns.clone(),
                });
            }
            Some(_) => {}
            None => blockers.push(MigrationBlocker::DropForeignKey {
                table: current.name.to_owned(),
                columns: current_foreign_key.columns.clone(),
            }),
        }
    }

    for desired_foreign_key in &desired_foreign_keys {
        if current_foreign_keys
            .iter()
            .all(|foreign_key| foreign_key.columns != desired_foreign_key.columns)
        {
            blockers.push(MigrationBlocker::AddForeignKey {
                table: desired.name.to_owned(),
                columns: desired_foreign_key.columns.clone(),
            });
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct IndexManifest {
    name: String,
    columns: Vec<&'static str>,
    unique: bool,
}

fn index_manifest(table: TableDef) -> Vec<IndexManifest> {
    let mut indexes = Vec::new();

    for column in table.columns {
        if column.indexed && !column.primary_key && !column.unique {
            indexes.push(IndexManifest {
                name: generated_index_name(table.name, column.name),
                columns: vec![column.name],
                unique: false,
            });
        }
    }

    indexes.extend(table.indexes.iter().map(|index| IndexManifest {
        name: index.name.to_owned(),
        columns: index.columns.to_vec(),
        unique: index.unique,
    }));
    indexes.sort_by(|left, right| left.name.cmp(&right.name));
    indexes
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ForeignKeyManifest {
    columns: Vec<String>,
    references_table: String,
    references_columns: Vec<String>,
}

fn foreign_key_manifest(table: TableDef) -> Vec<ForeignKeyManifest> {
    let mut foreign_keys = table
        .foreign_keys
        .iter()
        .map(|foreign_key| ForeignKeyManifest {
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
        })
        .collect::<Vec<_>>();
    foreign_keys.sort_by(|left, right| left.columns.cmp(&right.columns));
    foreign_keys
}

fn find_column(table: TableDef, name: &str) -> Option<ColumnDef> {
    table
        .columns
        .iter()
        .copied()
        .find(|column| column.name == name)
}

fn can_add_column(column: ColumnDef) -> bool {
    !column.primary_key
        && !column.auto_increment
        && !column.unique
        && (column.nullable || column.default_sql.is_some())
}

fn format_table_manifest(table: &TableDef) -> String {
    let mut lines = vec![format!("table {}", table.name)];

    for column in table.columns {
        lines.push(format!(
            "column {} {} nullable={} primary_key={} auto_increment={} unique={} indexed={} default={}",
            column.name,
            column.sql_type.as_sql(),
            column.nullable,
            column.primary_key,
            column.auto_increment,
            column.unique,
            column.indexed,
            column.default_sql.unwrap_or("")
        ));
    }

    let mut indexes = table.indexes.to_vec();
    indexes.sort_by_key(|index| index.name);
    for index in indexes {
        lines.push(format!(
            "index {} columns={} unique={}",
            index.name,
            index.columns.join(","),
            index.unique
        ));
    }

    let mut foreign_keys = table.foreign_keys.to_vec();
    foreign_keys.sort_by_key(|foreign_key| foreign_key.columns.join(","));
    for foreign_key in foreign_keys {
        lines.push(format!(
            "foreign_key columns={} references={}.{}",
            foreign_key.columns.join(","),
            foreign_key.references_table,
            foreign_key.references_columns.join(",")
        ));
    }

    lines.join("\n")
}

fn migration_name_slug(name: &str) -> Result<String, MigrationWriteError> {
    let mut slug = String::new();
    let mut last_was_separator = false;

    for character in name.trim().chars() {
        if character.is_ascii_alphanumeric() {
            slug.push(character.to_ascii_lowercase());
            last_was_separator = false;
        } else if (character == '_' || character == '-' || character.is_ascii_whitespace())
            && !slug.is_empty()
            && !last_was_separator
        {
            slug.push('_');
            last_was_separator = true;
        } else if character == '.' || character == '/' || character == '\\' {
            return Err(MigrationWriteError::InvalidName);
        }
    }

    while slug.ends_with('_') {
        slug.pop();
    }

    if slug.is_empty() {
        return Err(MigrationWriteError::InvalidName);
    }

    Ok(slug)
}
