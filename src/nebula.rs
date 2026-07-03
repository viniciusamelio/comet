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
        Column, ColumnDef, ColumnRef, Entity, IndexDef, QueryLint, SqlType, TableDef, Value,
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
}
