use std::marker::PhantomData;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
pub struct TableDef {
    pub name: &'static str,
    pub columns: &'static [ColumnDef],
    pub indexes: &'static [IndexDef],
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Column<T> {
    table: &'static str,
    name: &'static str,
    _ty: PhantomData<T>,
}

impl<T> Column<T> {
    pub const fn new(table: &'static str, name: &'static str) -> Self {
        Self {
            table,
            name,
            _ty: PhantomData,
        }
    }

    pub const fn name(self) -> &'static str {
        self.name
    }

    pub const fn table(self) -> &'static str {
        self.table
    }

    pub fn eq<V>(self, value: V) -> Expr
    where
        V: Into<Value>,
    {
        self.binary("=", value)
    }

    pub fn ne<V>(self, value: V) -> Expr
    where
        V: Into<Value>,
    {
        self.binary("!=", value)
    }

    pub fn gt<V>(self, value: V) -> Expr
    where
        V: Into<Value>,
    {
        self.binary(">", value)
    }

    pub fn gte<V>(self, value: V) -> Expr
    where
        V: Into<Value>,
    {
        self.binary(">=", value)
    }

    pub fn lt<V>(self, value: V) -> Expr
    where
        V: Into<Value>,
    {
        self.binary("<", value)
    }

    pub fn lte<V>(self, value: V) -> Expr
    where
        V: Into<Value>,
    {
        self.binary("<=", value)
    }

    pub fn like<V>(self, value: V) -> Expr
    where
        V: Into<Value>,
    {
        self.binary("LIKE", value)
    }

    pub fn is_null(self) -> Expr {
        Expr {
            sql: format!("{} IS NULL", qualified_column(self.table, self.name)),
            binds: Vec::new(),
            columns: vec![ColumnRef {
                table: self.table,
                name: self.name,
            }],
        }
    }

    pub fn is_not_null(self) -> Expr {
        Expr {
            sql: format!("{} IS NOT NULL", qualified_column(self.table, self.name)),
            binds: Vec::new(),
            columns: vec![ColumnRef {
                table: self.table,
                name: self.name,
            }],
        }
    }

    pub fn asc(self) -> Ordering {
        Ordering {
            table: self.table,
            column: self.name,
            direction: Direction::Asc,
        }
    }

    pub fn desc(self) -> Ordering {
        Ordering {
            table: self.table,
            column: self.name,
            direction: Direction::Desc,
        }
    }

    fn binary<V>(self, op: &'static str, value: V) -> Expr
    where
        V: Into<Value>,
    {
        Expr {
            sql: format!("{} {op} ?", qualified_column(self.table, self.name)),
            binds: vec![value.into()],
            columns: vec![ColumnRef {
                table: self.table,
                name: self.name,
            }],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColumnRef {
    pub table: &'static str,
    pub name: &'static str,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Integer(i64),
    Real(f64),
    Text(String),
    Blob(Vec<u8>),
    Bool(bool),
}

impl From<i64> for Value {
    fn from(value: i64) -> Self {
        Value::Integer(value)
    }
}

impl From<i32> for Value {
    fn from(value: i32) -> Self {
        Value::Integer(value.into())
    }
}

impl From<u32> for Value {
    fn from(value: u32) -> Self {
        Value::Integer(value.into())
    }
}

impl From<f64> for Value {
    fn from(value: f64) -> Self {
        Value::Real(value)
    }
}

impl From<bool> for Value {
    fn from(value: bool) -> Self {
        Value::Bool(value)
    }
}

impl From<String> for Value {
    fn from(value: String) -> Self {
        Value::Text(value)
    }
}

impl From<&str> for Value {
    fn from(value: &str) -> Self {
        Value::Text(value.to_owned())
    }
}

impl From<Vec<u8>> for Value {
    fn from(value: Vec<u8>) -> Self {
        Value::Blob(value)
    }
}

impl From<&[u8]> for Value {
    fn from(value: &[u8]) -> Self {
        Value::Blob(value.to_vec())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Expr {
    sql: String,
    binds: Vec<Value>,
    columns: Vec<ColumnRef>,
}

impl Expr {
    pub fn and(self, other: Expr) -> Expr {
        let mut binds = self.binds;
        binds.extend(other.binds);
        let mut columns = self.columns;
        columns.extend(other.columns);

        Expr {
            sql: format!("({}) AND ({})", self.sql, other.sql),
            binds,
            columns,
        }
    }

    pub fn or(self, other: Expr) -> Expr {
        let mut binds = self.binds;
        binds.extend(other.binds);
        let mut columns = self.columns;
        columns.extend(other.columns);

        Expr {
            sql: format!("({}) OR ({})", self.sql, other.sql),
            binds,
            columns,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Asc,
    Desc,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ordering {
    table: &'static str,
    column: &'static str,
    direction: Direction,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Statement {
    pub sql: String,
    pub binds: Vec<Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueryLint {
    MissingLimit,
    UnindexedFilter { column: ColumnRef },
    UnindexedOrdering { column: ColumnRef },
    BroadUpdate,
    BroadDelete,
}

#[derive(Debug, Clone)]
pub struct Select<E> {
    columns: Vec<&'static str>,
    filter: Option<Expr>,
    orderings: Vec<Ordering>,
    limit: Option<u32>,
    offset: Option<u32>,
    allow_full_table_scan: bool,
    allow_unbounded_select: bool,
    _entity: PhantomData<E>,
}

impl<E: Entity> Select<E> {
    fn new() -> Self {
        Self {
            columns: E::TABLE.columns.iter().map(|column| column.name).collect(),
            filter: None,
            orderings: Vec::new(),
            limit: None,
            offset: None,
            allow_full_table_scan: false,
            allow_unbounded_select: false,
            _entity: PhantomData,
        }
    }

    pub fn columns(mut self, columns: impl IntoIterator<Item = &'static str>) -> Self {
        self.columns = columns.into_iter().collect();
        self
    }

    pub fn where_(mut self, filter: Expr) -> Self {
        self.filter = Some(filter);
        self
    }

    pub fn and_where(mut self, filter: Expr) -> Self {
        self.filter = Some(match self.filter {
            Some(current) => current.and(filter),
            None => filter,
        });
        self
    }

    pub fn order_by(mut self, ordering: Ordering) -> Self {
        self.orderings.push(ordering);
        self
    }

    pub fn limit(mut self, limit: u32) -> Self {
        self.limit = Some(limit);
        self
    }

    pub fn offset(mut self, offset: u32) -> Self {
        self.offset = Some(offset);
        self
    }

    pub fn allow_full_table_scan(mut self) -> Self {
        self.allow_full_table_scan = true;
        self
    }

    pub fn allow_unbounded_select(mut self) -> Self {
        self.allow_unbounded_select = true;
        self
    }

    pub fn lint(&self) -> Vec<QueryLint> {
        let mut lints = Vec::new();

        if self.limit.is_none() && !self.allow_unbounded_select {
            lints.push(QueryLint::MissingLimit);
        }

        if !self.allow_full_table_scan {
            if let Some(filter) = &self.filter {
                push_unindexed_filter_lints::<E>(&mut lints, &filter.columns);
            }

            for ordering in &self.orderings {
                push_unindexed_ordering_lint::<E>(
                    &mut lints,
                    ColumnRef {
                        table: ordering.table,
                        name: ordering.column,
                    },
                );
            }
        }

        lints
    }

    pub fn to_statement(self) -> Statement {
        let columns = self
            .columns
            .into_iter()
            .map(quote_ident)
            .collect::<Vec<_>>()
            .join(", ");
        let mut sql = format!("SELECT {columns} FROM {}", quote_ident(E::TABLE.name));
        let mut binds = Vec::new();

        if let Some(filter) = self.filter {
            sql.push_str(" WHERE ");
            sql.push_str(&filter.sql);
            binds.extend(filter.binds);
        }

        if !self.orderings.is_empty() {
            let orderings = self
                .orderings
                .into_iter()
                .map(format_ordering)
                .collect::<Vec<_>>()
                .join(", ");
            sql.push_str(" ORDER BY ");
            sql.push_str(&orderings);
        }

        if let Some(limit) = self.limit {
            sql.push_str(" LIMIT ?");
            binds.push(Value::Integer(limit.into()));
        }

        if let Some(offset) = self.offset {
            sql.push_str(" OFFSET ?");
            binds.push(Value::Integer(offset.into()));
        }

        Statement { sql, binds }
    }
}

#[derive(Debug, Clone)]
pub struct Insert<E> {
    columns: Vec<&'static str>,
    values: Vec<Value>,
    returning: Vec<&'static str>,
    _entity: PhantomData<E>,
}

impl<E: Entity> Insert<E> {
    fn new() -> Self {
        Self {
            columns: Vec::new(),
            values: Vec::new(),
            returning: Vec::new(),
            _entity: PhantomData,
        }
    }

    pub fn set<T, V>(mut self, column: Column<T>, value: V) -> Self
    where
        V: Into<Value>,
    {
        self.columns.push(column.name());
        self.values.push(value.into());
        self
    }

    pub fn returning(mut self, columns: impl IntoIterator<Item = &'static str>) -> Self {
        self.returning = columns.into_iter().collect();
        self
    }

    pub fn to_statement(self) -> Statement {
        let columns = self
            .columns
            .into_iter()
            .map(quote_ident)
            .collect::<Vec<_>>()
            .join(", ");
        let placeholders = vec!["?"; self.values.len()].join(", ");
        let sql = format!(
            "INSERT INTO {} ({columns}) VALUES ({placeholders})",
            quote_ident(E::TABLE.name),
        );
        let sql = append_returning(sql, self.returning);

        Statement {
            sql,
            binds: self.values,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Update<E> {
    assignments: Vec<(&'static str, Value)>,
    filter: Option<Expr>,
    returning: Vec<&'static str>,
    allow_full_table_scan: bool,
    allow_broad_write: bool,
    _entity: PhantomData<E>,
}

impl<E: Entity> Update<E> {
    fn new() -> Self {
        Self {
            assignments: Vec::new(),
            filter: None,
            returning: Vec::new(),
            allow_full_table_scan: false,
            allow_broad_write: false,
            _entity: PhantomData,
        }
    }

    pub fn set<T, V>(mut self, column: Column<T>, value: V) -> Self
    where
        V: Into<Value>,
    {
        self.assignments.push((column.name(), value.into()));
        self
    }

    pub fn where_(mut self, filter: Expr) -> Self {
        self.filter = Some(filter);
        self
    }

    pub fn returning(mut self, columns: impl IntoIterator<Item = &'static str>) -> Self {
        self.returning = columns.into_iter().collect();
        self
    }

    pub fn allow_full_table_scan(mut self) -> Self {
        self.allow_full_table_scan = true;
        self
    }

    pub fn allow_broad_write(mut self) -> Self {
        self.allow_broad_write = true;
        self
    }

    pub fn lint(&self) -> Vec<QueryLint> {
        let mut lints = Vec::new();

        match &self.filter {
            Some(filter) if !self.allow_full_table_scan => {
                push_unindexed_filter_lints::<E>(&mut lints, &filter.columns);
            }
            None if !self.allow_broad_write => lints.push(QueryLint::BroadUpdate),
            _ => {}
        }

        lints
    }

    pub fn to_statement(self) -> Statement {
        let assignments = self
            .assignments
            .iter()
            .map(|(column, _)| format!("{} = ?", quote_ident(column)))
            .collect::<Vec<_>>()
            .join(", ");
        let mut binds = self
            .assignments
            .into_iter()
            .map(|(_, value)| value)
            .collect::<Vec<_>>();
        let mut sql = format!("UPDATE {} SET {assignments}", quote_ident(E::TABLE.name));

        if let Some(filter) = self.filter {
            sql.push_str(" WHERE ");
            sql.push_str(&filter.sql);
            binds.extend(filter.binds);
        }

        sql = append_returning(sql, self.returning);

        Statement { sql, binds }
    }
}

#[derive(Debug, Clone)]
pub struct Delete<E> {
    filter: Option<Expr>,
    allow_full_table_scan: bool,
    allow_broad_write: bool,
    _entity: PhantomData<E>,
}

impl<E: Entity> Delete<E> {
    fn new() -> Self {
        Self {
            filter: None,
            allow_full_table_scan: false,
            allow_broad_write: false,
            _entity: PhantomData,
        }
    }

    pub fn where_(mut self, filter: Expr) -> Self {
        self.filter = Some(filter);
        self
    }

    pub fn allow_full_table_scan(mut self) -> Self {
        self.allow_full_table_scan = true;
        self
    }

    pub fn allow_broad_write(mut self) -> Self {
        self.allow_broad_write = true;
        self
    }

    pub fn lint(&self) -> Vec<QueryLint> {
        let mut lints = Vec::new();

        match &self.filter {
            Some(filter) if !self.allow_full_table_scan => {
                push_unindexed_filter_lints::<E>(&mut lints, &filter.columns);
            }
            None if !self.allow_broad_write => lints.push(QueryLint::BroadDelete),
            _ => {}
        }

        lints
    }

    pub fn to_statement(self) -> Statement {
        let mut sql = format!("DELETE FROM {}", quote_ident(E::TABLE.name));
        let mut binds = Vec::new();

        if let Some(filter) = self.filter {
            sql.push_str(" WHERE ");
            sql.push_str(&filter.sql);
            binds.extend(filter.binds);
        }

        Statement { sql, binds }
    }
}

fn quote_ident(identifier: &str) -> String {
    format!("\"{}\"", identifier.replace('"', "\"\""))
}

fn qualified_column(table: &str, column: &str) -> String {
    format!("{}.{}", quote_ident(table), quote_ident(column))
}

fn format_ordering(ordering: Ordering) -> String {
    let direction = match ordering.direction {
        Direction::Asc => "ASC",
        Direction::Desc => "DESC",
    };

    format!(
        "{} {direction}",
        qualified_column(ordering.table, ordering.column)
    )
}

fn append_returning(mut sql: String, returning: Vec<&'static str>) -> String {
    if !returning.is_empty() {
        let returning = returning
            .into_iter()
            .map(quote_ident)
            .collect::<Vec<_>>()
            .join(", ");
        sql.push_str(" RETURNING ");
        sql.push_str(&returning);
    }

    sql
}

fn push_unindexed_filter_lints<E: Entity>(lints: &mut Vec<QueryLint>, columns: &[ColumnRef]) {
    for &column in columns {
        if !is_indexed::<E>(column) {
            push_unique_lint(lints, QueryLint::UnindexedFilter { column });
        }
    }
}

fn push_unindexed_ordering_lint<E: Entity>(lints: &mut Vec<QueryLint>, column: ColumnRef) {
    if !is_indexed::<E>(column) {
        push_unique_lint(lints, QueryLint::UnindexedOrdering { column });
    }
}

fn push_unique_lint(lints: &mut Vec<QueryLint>, lint: QueryLint) {
    if !lints.contains(&lint) {
        lints.push(lint);
    }
}

fn is_indexed<E: Entity>(column: ColumnRef) -> bool {
    if column.table != E::TABLE.name {
        return false;
    }

    E::TABLE.columns.iter().any(|definition| {
        definition.name == column.name
            && (definition.primary_key || definition.unique || definition.indexed)
    }) || E::TABLE
        .indexes
        .iter()
        .any(|index| index.columns.first() == Some(&column.name))
}

fn initial_table_migration(table: TableDef) -> Vec<String> {
    let mut statements = vec![create_table_statement(table)];
    statements.extend(index_statements(table));
    statements
}

fn create_table_statement(table: TableDef) -> String {
    let columns = table
        .columns
        .iter()
        .map(format_column_def)
        .collect::<Vec<_>>()
        .join(", ");

    format!("CREATE TABLE {} ({columns})", quote_ident(table.name))
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

#[cfg(feature = "nebula-d1")]
pub mod d1 {
    use worker::wasm_bindgen::JsValue;

    use super::{Statement, Value};

    const MAX_SAFE_JS_INTEGER: i64 = 9_007_199_254_740_991;
    const MIN_SAFE_JS_INTEGER: i64 = -9_007_199_254_740_991;

    /// Executes statements through D1's `batch()` API.
    ///
    /// D1 executes a batch transactionally: if one statement fails, the batch
    /// is aborted/rolled back by D1 and the error is returned by the Worker
    /// runtime. Nebula keeps this explicit so ordinary single-statement calls
    /// do not accidentally imply transaction boundaries.
    pub async fn batch_d1(
        db: &worker::D1Database,
        statements: impl IntoIterator<Item = Statement>,
    ) -> worker::Result<Vec<worker::D1Result>> {
        let prepared = statements
            .into_iter()
            .map(|statement| statement.prepare_d1(db))
            .collect::<worker::Result<Vec<_>>>()?;

        db.batch(prepared).await
    }

    impl Statement {
        pub fn bind_js_values(&self) -> worker::Result<Vec<JsValue>> {
            self.binds.iter().map(value_to_js).collect()
        }

        pub fn prepare_d1(
            &self,
            db: &worker::D1Database,
        ) -> worker::Result<worker::D1PreparedStatement> {
            let statement = db.prepare(&self.sql);
            let binds = self.bind_js_values()?;
            statement.bind(&binds)
        }

        pub async fn execute_d1(
            &self,
            db: &worker::D1Database,
        ) -> worker::Result<worker::D1Result> {
            self.prepare_d1(db)?.run().await
        }

        pub async fn fetch_all_d1(
            &self,
            db: &worker::D1Database,
        ) -> worker::Result<worker::D1Result> {
            self.prepare_d1(db)?.all().await
        }

        pub async fn fetch_optional_d1<T>(
            &self,
            db: &worker::D1Database,
        ) -> worker::Result<Option<T>>
        where
            T: for<'de> serde::Deserialize<'de>,
        {
            self.prepare_d1(db)?.first(None).await
        }

        pub async fn fetch_one_d1<T>(&self, db: &worker::D1Database) -> worker::Result<T>
        where
            T: for<'de> serde::Deserialize<'de>,
        {
            self.fetch_optional_d1(db).await?.ok_or_else(|| {
                worker::Error::RustError("Nebula query expected one row and returned none".into())
            })
        }
    }

    fn value_to_js(value: &Value) -> worker::Result<JsValue> {
        match value {
            Value::Null => Ok(JsValue::null()),
            Value::Integer(value) => {
                if !(MIN_SAFE_JS_INTEGER..=MAX_SAFE_JS_INTEGER).contains(value) {
                    return Err(worker::Error::RustError(format!(
                        "D1 integer bind exceeds JavaScript safe integer range: {value}"
                    )));
                }

                Ok(JsValue::from_f64(*value as f64))
            }
            Value::Real(value) => Ok(JsValue::from_f64(*value)),
            Value::Text(value) => Ok(JsValue::from_str(value)),
            Value::Blob(value) => {
                worker::d1::serde_wasm_bindgen::to_value(value).map_err(worker::Error::from)
            }
            Value::Bool(value) => Ok(JsValue::from_bool(*value)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Column, ColumnDef, ColumnRef, Entity, IndexDef, MigrationBlocker, MigrationPlan,
        MigrationWriteError, QueryLint, SchemaManifest, SqlType, TableDef, Value,
    };

    struct Task;

    impl Task {
        const ID: Column<i64> = Column::new("tasks", "id");
        const TITLE: Column<String> = Column::new("tasks", "title");
        const DONE: Column<bool> = Column::new("tasks", "done");
        const CREATED_AT: Column<String> = Column::new("tasks", "created_at");
    }

    const TASK_COLUMNS: &[ColumnDef] = &[
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
        ColumnDef::new("done", SqlType::Boolean),
        ColumnDef::new("created_at", SqlType::Text),
    ];

    const TASK_INDEXES: &[IndexDef] = &[IndexDef {
        name: "idx_tasks_done_created_at",
        columns: &["done", "created_at"],
        unique: false,
    }];

    impl Entity for Task {
        const TABLE: TableDef = TableDef {
            name: "tasks",
            columns: TASK_COLUMNS,
            indexes: TASK_INDEXES,
        };
    }

    #[test]
    fn select_statement_is_deterministic() {
        let statement = Task::select()
            .where_(Task::DONE.eq(false))
            .order_by(Task::CREATED_AT.desc())
            .limit(50)
            .offset(10)
            .to_statement();

        assert_eq!(
            statement.sql,
            "SELECT \"id\", \"title\", \"done\", \"created_at\" FROM \"tasks\" \
             WHERE \"tasks\".\"done\" = ? ORDER BY \"tasks\".\"created_at\" DESC LIMIT ? OFFSET ?"
        );
        assert_eq!(
            statement.binds,
            vec![Value::Bool(false), Value::Integer(50), Value::Integer(10)]
        );
    }

    #[test]
    fn select_can_combine_filters() {
        let statement = Task::select()
            .where_(Task::DONE.eq(false))
            .and_where(Task::TITLE.like("%docs%"))
            .to_statement();

        assert_eq!(
            statement.sql,
            "SELECT \"id\", \"title\", \"done\", \"created_at\" FROM \"tasks\" \
             WHERE (\"tasks\".\"done\" = ?) AND (\"tasks\".\"title\" LIKE ?)"
        );
        assert_eq!(
            statement.binds,
            vec![Value::Bool(false), Value::Text("%docs%".into())]
        );
    }

    #[test]
    fn insert_statement_preserves_bind_order() {
        let statement = Task::insert()
            .set(Task::TITLE, "write tests")
            .set(Task::DONE, false)
            .returning(["id", "title", "done", "created_at"])
            .to_statement();

        assert_eq!(
            statement.sql,
            "INSERT INTO \"tasks\" (\"title\", \"done\") VALUES (?, ?) \
             RETURNING \"id\", \"title\", \"done\", \"created_at\""
        );
        assert_eq!(
            statement.binds,
            vec![Value::Text("write tests".into()), Value::Bool(false)]
        );
    }

    #[test]
    fn update_statement_puts_assignments_before_filter_binds() {
        let statement = Task::update()
            .set(Task::DONE, true)
            .where_(Task::ID.eq(42))
            .returning(["id", "title", "done", "created_at"])
            .to_statement();

        assert_eq!(
            statement.sql,
            "UPDATE \"tasks\" SET \"done\" = ? WHERE \"tasks\".\"id\" = ? \
             RETURNING \"id\", \"title\", \"done\", \"created_at\""
        );
        assert_eq!(statement.binds, vec![Value::Bool(true), Value::Integer(42)]);
    }

    #[test]
    fn delete_statement_keeps_filter_bind() {
        let statement = Task::delete().where_(Task::ID.eq(42)).to_statement();

        assert_eq!(
            statement.sql,
            "DELETE FROM \"tasks\" WHERE \"tasks\".\"id\" = ?"
        );
        assert_eq!(statement.binds, vec![Value::Integer(42)]);
    }

    #[test]
    fn identifiers_are_quoted() {
        let statement = Task::select().columns(["weird\"name"]).to_statement();

        assert_eq!(statement.sql, "SELECT \"weird\"\"name\" FROM \"tasks\"");
    }

    #[test]
    fn select_lints_missing_limit_and_unindexed_columns() {
        let lints = Task::select()
            .where_(Task::TITLE.like("%docs%"))
            .order_by(Task::CREATED_AT.desc())
            .lint();

        assert_eq!(
            lints,
            vec![
                QueryLint::MissingLimit,
                QueryLint::UnindexedFilter {
                    column: ColumnRef {
                        table: "tasks",
                        name: "title",
                    },
                },
                QueryLint::UnindexedOrdering {
                    column: ColumnRef {
                        table: "tasks",
                        name: "created_at",
                    },
                },
            ]
        );
    }

    #[test]
    fn select_lints_accept_indexed_limited_queries() {
        let lints = Task::select()
            .where_(Task::DONE.eq(false))
            .order_by(Task::ID.asc())
            .limit(25)
            .lint();

        assert_eq!(lints, Vec::new());
    }

    #[test]
    fn select_lints_support_explicit_escape_hatches() {
        let lints = Task::select()
            .where_(Task::TITLE.like("%docs%"))
            .order_by(Task::CREATED_AT.desc())
            .allow_full_table_scan()
            .allow_unbounded_select()
            .lint();

        assert_eq!(lints, Vec::new());
    }

    #[test]
    fn write_lints_flag_broad_writes() {
        assert_eq!(
            Task::update().set(Task::DONE, true).lint(),
            vec![QueryLint::BroadUpdate]
        );
        assert_eq!(Task::delete().lint(), vec![QueryLint::BroadDelete]);
    }

    #[test]
    fn write_lints_support_explicit_escape_hatches() {
        assert_eq!(
            Task::update()
                .set(Task::DONE, true)
                .allow_broad_write()
                .lint(),
            Vec::new()
        );
        assert_eq!(Task::delete().allow_broad_write().lint(), Vec::new());
    }

    #[test]
    fn write_lints_flag_unindexed_filters_once() {
        let lints = Task::update()
            .set(Task::DONE, true)
            .where_(Task::TITLE.eq("docs").and(Task::TITLE.like("%docs%")))
            .lint();

        assert_eq!(
            lints,
            vec![QueryLint::UnindexedFilter {
                column: ColumnRef {
                    table: "tasks",
                    name: "title",
                },
            }]
        );
    }

    #[test]
    fn schema_manifest_string_is_deterministic() {
        const OTHER_COLUMNS: &[ColumnDef] = &[ColumnDef::new("id", SqlType::Integer)];
        let other = TableDef {
            name: "audit",
            columns: OTHER_COLUMNS,
            indexes: &[],
        };

        let manifest = SchemaManifest::new([Task::TABLE, other]);

        assert_eq!(
            manifest.to_manifest_string(),
            "table audit\n\
             column id INTEGER nullable=false primary_key=false auto_increment=false unique=false indexed=false default=\n\n\
             table tasks\n\
             column id INTEGER nullable=false primary_key=true auto_increment=true unique=true indexed=true default=\n\
             column title TEXT nullable=false primary_key=false auto_increment=false unique=false indexed=false default=\n\
             column done INTEGER nullable=false primary_key=false auto_increment=false unique=false indexed=false default=\n\
             column created_at TEXT nullable=false primary_key=false auto_increment=false unique=false indexed=false default=\n\
             index idx_tasks_done_created_at columns=done,created_at unique=false"
        );
    }

    #[test]
    fn initial_migration_generates_create_table_and_indexes() {
        let manifest = SchemaManifest::new([Task::TABLE]);

        assert_eq!(
            manifest.initial_migration(),
            vec![
                "CREATE TABLE \"tasks\" (\"id\" INTEGER PRIMARY KEY AUTOINCREMENT, \"title\" TEXT NOT NULL, \"done\" INTEGER NOT NULL, \"created_at\" TEXT NOT NULL)",
                "CREATE INDEX \"idx_tasks_done_created_at\" ON \"tasks\" (\"done\", \"created_at\")",
            ]
        );
    }

    #[test]
    fn migration_diff_generates_safe_additive_changes() {
        const CURRENT_COLUMNS: &[ColumnDef] = &[
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
                indexed: true,
                default_sql: Some("0"),
            },
            ColumnDef {
                name: "notes",
                sql_type: SqlType::Text,
                nullable: true,
                primary_key: false,
                auto_increment: false,
                unique: false,
                indexed: false,
                default_sql: None,
            },
        ];
        let current = SchemaManifest::new([TableDef {
            name: "tasks",
            columns: CURRENT_COLUMNS,
            indexes: &[],
        }]);
        let desired = SchemaManifest::new([TableDef {
            name: "tasks",
            columns: DESIRED_COLUMNS,
            indexes: &[],
        }]);

        let plan = current.diff(&desired);

        assert!(plan.is_safe());
        assert_eq!(plan.blockers, Vec::new());
        assert_eq!(
            plan.statements,
            vec![
                "ALTER TABLE \"tasks\" ADD COLUMN \"done\" INTEGER NOT NULL DEFAULT 0",
                "ALTER TABLE \"tasks\" ADD COLUMN \"notes\" TEXT",
                "CREATE INDEX \"idx_tasks_done\" ON \"tasks\" (\"done\")",
            ]
        );
    }

    #[test]
    fn migration_diff_blocks_destructive_or_ambiguous_changes() {
        const CURRENT_COLUMNS: &[ColumnDef] = &[
            ColumnDef::new("id", SqlType::Integer),
            ColumnDef::new("title", SqlType::Text),
            ColumnDef {
                name: "legacy",
                sql_type: SqlType::Text,
                nullable: true,
                primary_key: false,
                auto_increment: false,
                unique: false,
                indexed: false,
                default_sql: None,
            },
        ];
        const DESIRED_COLUMNS: &[ColumnDef] = &[
            ColumnDef::new("id", SqlType::Integer),
            ColumnDef {
                name: "title",
                sql_type: SqlType::Text,
                nullable: true,
                primary_key: false,
                auto_increment: false,
                unique: false,
                indexed: false,
                default_sql: None,
            },
            ColumnDef::new("required", SqlType::Text),
        ];
        let current = SchemaManifest::new([TableDef {
            name: "tasks",
            columns: CURRENT_COLUMNS,
            indexes: &[],
        }]);
        let desired = SchemaManifest::new([TableDef {
            name: "tasks",
            columns: DESIRED_COLUMNS,
            indexes: &[],
        }]);

        let plan = current.diff(&desired);

        assert!(!plan.is_safe());
        assert_eq!(
            plan.blockers,
            vec![
                MigrationBlocker::DropColumn {
                    table: "tasks".into(),
                    column: "legacy".into(),
                },
                MigrationBlocker::ChangeColumn {
                    table: "tasks".into(),
                    column: "title".into(),
                },
                MigrationBlocker::UnsafeAddColumn {
                    table: "tasks".into(),
                    column: "required".into(),
                },
            ]
        );
        assert_eq!(plan.statements, Vec::<String>::new());
    }

    #[test]
    fn migration_plan_formats_sql_file_contents() {
        let plan = MigrationPlan {
            statements: vec![
                "CREATE TABLE \"tasks\" (\"id\" INTEGER NOT NULL)".into(),
                "CREATE INDEX \"idx_tasks_id\" ON \"tasks\" (\"id\")".into(),
            ],
            blockers: Vec::new(),
        };

        assert_eq!(
            plan.to_sql_file_contents().unwrap(),
            "CREATE TABLE \"tasks\" (\"id\" INTEGER NOT NULL);\n\
             CREATE INDEX \"idx_tasks_id\" ON \"tasks\" (\"id\");\n"
        );
    }

    #[test]
    fn migration_file_name_is_deterministic_and_rejects_paths() {
        assert_eq!(
            MigrationPlan::migration_file_name(7, "Add Task Done").unwrap(),
            "0007_add_task_done.sql"
        );
        assert_eq!(
            MigrationPlan::migration_file_name(12, "  add---task___done  ").unwrap(),
            "0012_add_task_done.sql"
        );
        assert_eq!(
            MigrationPlan::migration_file_name(1, "../escape").unwrap_err(),
            MigrationWriteError::InvalidName
        );
        assert_eq!(
            MigrationPlan::migration_file_name(1, "   ").unwrap_err(),
            MigrationWriteError::InvalidName
        );
    }

    #[test]
    fn migration_plan_refuses_to_write_empty_or_unsafe_plans() {
        let empty = MigrationPlan {
            statements: Vec::new(),
            blockers: Vec::new(),
        };
        assert_eq!(
            empty.to_sql_file_contents().unwrap_err(),
            MigrationWriteError::EmptyPlan
        );

        let unsafe_plan = MigrationPlan {
            statements: vec!["ALTER TABLE \"tasks\" ADD COLUMN \"done\" INTEGER".into()],
            blockers: vec![MigrationBlocker::DropColumn {
                table: "tasks".into(),
                column: "legacy".into(),
            }],
        };
        assert_eq!(
            unsafe_plan.to_sql_file_contents().unwrap_err(),
            MigrationWriteError::UnsafePlan {
                blockers: vec![MigrationBlocker::DropColumn {
                    table: "tasks".into(),
                    column: "legacy".into(),
                }],
            }
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn migration_plan_writes_sql_file() {
        let plan = MigrationPlan {
            statements: vec!["CREATE TABLE \"tasks\" (\"id\" INTEGER NOT NULL)".into()],
            blockers: Vec::new(),
        };
        let directory = std::env::temp_dir().join(format!(
            "comet-nebula-test-{}-{}",
            std::process::id(),
            "migration_plan_writes_sql_file"
        ));
        let _ = std::fs::remove_dir_all(&directory);

        let path = plan
            .write_sql_file(&directory, 3, "Initial Tasks")
            .expect("write migration file");

        assert_eq!(path.file_name().unwrap(), "0003_initial_tasks.sql");
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "CREATE TABLE \"tasks\" (\"id\" INTEGER NOT NULL);\n"
        );

        std::fs::remove_dir_all(directory).unwrap();
    }
}
