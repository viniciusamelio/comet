use std::marker::PhantomData;

use super::Entity;
use super::column::{Column, ColumnRef, Expr, Ordering, format_ordering, quote_ident};
use super::migration::is_indexed_in_table;
use super::value::Value;

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
    pub(crate) fn new() -> Self {
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
    pub(crate) fn new() -> Self {
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
    pub(crate) fn new() -> Self {
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
    pub(crate) fn new() -> Self {
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

    is_indexed_in_table(E::TABLE, column.name)
}
