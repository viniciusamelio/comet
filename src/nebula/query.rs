use std::marker::PhantomData;

use super::Entity;
use super::column::{Column, ColumnRef, Expr, Ordering, format_ordering, quote_ident};
use super::migration::is_indexed_in_table;
use super::rls::{
    AccessContext, CustomPredicateProvider, RlsError, authorize_policy, policy_applies,
    require_rls_operation_coverage, table_has_protected_rls, validate_policy_value_type,
};
use super::value::Value;
use super::{RlsOperation, RlsPolicyKind};

#[derive(Debug, Clone, PartialEq)]
pub struct Statement {
    pub sql: String,
    pub binds: Vec<Value>,
}

impl Statement {
    /// Builds a raw statement that bypasses Nebula's query builders and RLS.
    ///
    /// Use this only when the caller applies equivalent authorization checks
    /// manually or when the statement intentionally targets public data.
    pub fn raw_unscoped(sql: impl Into<String>, binds: impl IntoIterator<Item = Value>) -> Self {
        Self {
            sql: sql.into(),
            binds: binds.into_iter().collect(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueryLint {
    MissingLimit,
    UnindexedFilter { column: ColumnRef },
    UnindexedOrdering { column: ColumnRef },
    UnscopedRls { table: &'static str },
    BroadUpdate,
    BroadDelete,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueryLintSeverity {
    Warning,
    Error,
}

impl QueryLint {
    pub const fn severity(self) -> QueryLintSeverity {
        match self {
            QueryLint::UnscopedRls { .. } | QueryLint::BroadUpdate | QueryLint::BroadDelete => {
                QueryLintSeverity::Error
            }
            QueryLint::MissingLimit
            | QueryLint::UnindexedFilter { .. }
            | QueryLint::UnindexedOrdering { .. } => QueryLintSeverity::Warning,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueryCheckError {
    pub lints: Vec<QueryLint>,
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
    rls_applied: bool,
    unscoped_rls_reason: Option<&'static str>,
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
            rls_applied: false,
            unscoped_rls_reason: None,
            _entity: PhantomData,
        }
    }

    pub fn columns(mut self, columns: impl IntoIterator<Item = &'static str>) -> Self {
        self.columns = columns.into_iter().collect();
        self
    }

    pub fn where_(mut self, filter: Expr) -> Self {
        self.filter = Some(match self.filter {
            Some(current) => current.and(filter),
            None => filter,
        });
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

    pub fn allow_unscoped_rls(mut self, reason: &'static str) -> Self {
        self.unscoped_rls_reason = Some(reason);
        self
    }

    pub fn apply_rls(
        mut self,
        context: &AccessContext,
        predicates: &impl CustomPredicateProvider,
    ) -> Result<Self, RlsError> {
        require_rls_operation_coverage(E::TABLE, RlsOperation::Select)?;
        for policy in E::TABLE
            .rls
            .iter()
            .filter(|policy| policy_applies(policy, RlsOperation::Select))
        {
            authorize_policy(E::TABLE.name, policy, context)?;
            if let Some(predicate) =
                predicate_for_policy::<E>(policy, RlsOperation::Select, context, predicates)?
            {
                self = self.and_where(predicate);
            }
        }

        self.rls_applied = true;
        Ok(self)
    }

    pub fn lint(&self) -> Vec<QueryLint> {
        let mut lints = Vec::new();

        if self.limit.is_none() && !self.allow_unbounded_select {
            lints.push(QueryLint::MissingLimit);
        }

        push_unscoped_rls_lint::<E>(&mut lints, self.rls_applied, self.unscoped_rls_reason);

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

    pub fn to_statement_checked(self) -> Result<Statement, QueryCheckError> {
        let lints = self.lint();
        if lints
            .iter()
            .all(|lint| lint.severity() == QueryLintSeverity::Warning)
        {
            Ok(self.to_statement())
        } else {
            Err(QueryCheckError { lints })
        }
    }
}

#[derive(Debug, Clone)]
pub struct Insert<E> {
    columns: Vec<&'static str>,
    values: Vec<Value>,
    returning: Vec<&'static str>,
    rls_applied: bool,
    unscoped_rls_reason: Option<&'static str>,
    _entity: PhantomData<E>,
}

impl<E: Entity> Insert<E> {
    pub(crate) fn new() -> Self {
        Self {
            columns: Vec::new(),
            values: Vec::new(),
            returning: Vec::new(),
            rls_applied: false,
            unscoped_rls_reason: None,
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

    pub fn allow_unscoped_rls(mut self, reason: &'static str) -> Self {
        self.unscoped_rls_reason = Some(reason);
        self
    }

    pub fn apply_rls(
        mut self,
        context: &AccessContext,
        predicates: &impl CustomPredicateProvider,
    ) -> Result<Self, RlsError> {
        require_rls_operation_coverage(E::TABLE, RlsOperation::Insert)?;
        for policy in E::TABLE
            .rls
            .iter()
            .filter(|policy| policy_applies(policy, RlsOperation::Insert))
        {
            authorize_policy(E::TABLE.name, policy, context)?;
            match policy.kind {
                RlsPolicyKind::Owner => {
                    let user_id = context.user_id.clone().ok_or(RlsError::MissingUser {
                        table: E::TABLE.name,
                    })?;
                    self = self.set_or_validate_policy_column(policy.column, user_id)?;
                }
                RlsPolicyKind::Tenant => {
                    let tenant_id = context.tenant_id.clone().ok_or(RlsError::MissingTenant {
                        table: E::TABLE.name,
                    })?;
                    self = self.set_or_validate_policy_column(policy.column, tenant_id)?;
                }
                RlsPolicyKind::Custom => {
                    if let Some(name) = policy.custom {
                        predicates.predicate(E::TABLE.name, name, RlsOperation::Insert, context)?;
                    }
                }
                RlsPolicyKind::Public | RlsPolicyKind::Rbac => {}
            }
        }

        self.rls_applied = true;
        Ok(self)
    }

    fn set_or_validate_policy_column(
        mut self,
        column: Option<&'static str>,
        value: Value,
    ) -> Result<Self, RlsError> {
        let Some(column) = column else {
            return Ok(self);
        };
        validate_policy_value_type(E::TABLE, column, &value)?;

        if let Some(position) = self.columns.iter().position(|existing| *existing == column) {
            if self.values[position] == value {
                Ok(self)
            } else {
                Err(RlsError::Forbidden {
                    table: E::TABLE.name,
                })
            }
        } else {
            self.columns.push(column);
            self.values.push(value);
            Ok(self)
        }
    }

    pub fn lint(&self) -> Vec<QueryLint> {
        let mut lints = Vec::new();
        push_unscoped_rls_lint::<E>(&mut lints, self.rls_applied, self.unscoped_rls_reason);
        lints
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

    pub fn to_statement_checked(self) -> Result<Statement, QueryCheckError> {
        let lints = self.lint();
        if lints
            .iter()
            .all(|lint| lint.severity() == QueryLintSeverity::Warning)
        {
            Ok(self.to_statement())
        } else {
            Err(QueryCheckError { lints })
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
    rls_applied: bool,
    unscoped_rls_reason: Option<&'static str>,
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
            rls_applied: false,
            unscoped_rls_reason: None,
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
        self.filter = Some(match self.filter {
            Some(current) => current.and(filter),
            None => filter,
        });
        self
    }

    pub fn and_where(mut self, filter: Expr) -> Self {
        self.filter = Some(match self.filter {
            Some(current) => current.and(filter),
            None => filter,
        });
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

    pub fn allow_unscoped_rls(mut self, reason: &'static str) -> Self {
        self.unscoped_rls_reason = Some(reason);
        self
    }

    pub fn apply_rls(
        mut self,
        context: &AccessContext,
        predicates: &impl CustomPredicateProvider,
    ) -> Result<Self, RlsError> {
        require_rls_operation_coverage(E::TABLE, RlsOperation::Update)?;
        for policy in E::TABLE
            .rls
            .iter()
            .filter(|policy| policy_applies(policy, RlsOperation::Update))
        {
            authorize_policy(E::TABLE.name, policy, context)?;
            if matches!(policy.kind, RlsPolicyKind::Owner | RlsPolicyKind::Tenant) {
                if let Some(column) = policy.column {
                    if self
                        .assignments
                        .iter()
                        .any(|(assigned, _)| *assigned == column)
                    {
                        return Err(RlsError::Forbidden {
                            table: E::TABLE.name,
                        });
                    }
                }
            }
            if let Some(predicate) =
                predicate_for_policy::<E>(policy, RlsOperation::Update, context, predicates)?
            {
                self = self.and_where(predicate);
            }
        }

        self.rls_applied = true;
        Ok(self)
    }

    pub fn lint(&self) -> Vec<QueryLint> {
        let mut lints = Vec::new();

        push_unscoped_rls_lint::<E>(&mut lints, self.rls_applied, self.unscoped_rls_reason);

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

    pub fn to_statement_checked(self) -> Result<Statement, QueryCheckError> {
        let lints = self.lint();
        if lints
            .iter()
            .all(|lint| lint.severity() == QueryLintSeverity::Warning)
        {
            Ok(self.to_statement())
        } else {
            Err(QueryCheckError { lints })
        }
    }
}

#[derive(Debug, Clone)]
pub struct Delete<E> {
    filter: Option<Expr>,
    allow_full_table_scan: bool,
    allow_broad_write: bool,
    rls_applied: bool,
    unscoped_rls_reason: Option<&'static str>,
    _entity: PhantomData<E>,
}

impl<E: Entity> Delete<E> {
    pub(crate) fn new() -> Self {
        Self {
            filter: None,
            allow_full_table_scan: false,
            allow_broad_write: false,
            rls_applied: false,
            unscoped_rls_reason: None,
            _entity: PhantomData,
        }
    }

    pub fn where_(mut self, filter: Expr) -> Self {
        self.filter = Some(match self.filter {
            Some(current) => current.and(filter),
            None => filter,
        });
        self
    }

    pub fn and_where(mut self, filter: Expr) -> Self {
        self.filter = Some(match self.filter {
            Some(current) => current.and(filter),
            None => filter,
        });
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

    pub fn allow_unscoped_rls(mut self, reason: &'static str) -> Self {
        self.unscoped_rls_reason = Some(reason);
        self
    }

    pub fn apply_rls(
        mut self,
        context: &AccessContext,
        predicates: &impl CustomPredicateProvider,
    ) -> Result<Self, RlsError> {
        require_rls_operation_coverage(E::TABLE, RlsOperation::Delete)?;
        for policy in E::TABLE
            .rls
            .iter()
            .filter(|policy| policy_applies(policy, RlsOperation::Delete))
        {
            authorize_policy(E::TABLE.name, policy, context)?;
            if let Some(predicate) =
                predicate_for_policy::<E>(policy, RlsOperation::Delete, context, predicates)?
            {
                self = self.and_where(predicate);
            }
        }

        self.rls_applied = true;
        Ok(self)
    }

    pub fn lint(&self) -> Vec<QueryLint> {
        let mut lints = Vec::new();

        push_unscoped_rls_lint::<E>(&mut lints, self.rls_applied, self.unscoped_rls_reason);

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

    pub fn to_statement_checked(self) -> Result<Statement, QueryCheckError> {
        let lints = self.lint();
        if lints
            .iter()
            .all(|lint| lint.severity() == QueryLintSeverity::Warning)
        {
            Ok(self.to_statement())
        } else {
            Err(QueryCheckError { lints })
        }
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

fn push_unscoped_rls_lint<E: Entity>(
    lints: &mut Vec<QueryLint>,
    rls_applied: bool,
    unscoped_rls_reason: Option<&'static str>,
) {
    if table_has_protected_rls(E::TABLE) && !rls_applied && unscoped_rls_reason.is_none() {
        lints.push(QueryLint::UnscopedRls {
            table: E::TABLE.name,
        });
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

fn predicate_for_policy<E: Entity>(
    policy: &super::RlsPolicyDef,
    operation: RlsOperation,
    context: &AccessContext,
    predicates: &impl CustomPredicateProvider,
) -> Result<Option<Expr>, RlsError> {
    match policy.kind {
        RlsPolicyKind::Owner => {
            let user_id = context.user_id.clone().ok_or(RlsError::MissingUser {
                table: E::TABLE.name,
            })?;
            if let Some(column) = policy.column {
                validate_policy_value_type(E::TABLE, column, &user_id)?;
                Ok(Some(policy_column_predicate(
                    E::TABLE.name,
                    column,
                    user_id,
                )))
            } else {
                Ok(None)
            }
        }
        RlsPolicyKind::Tenant => {
            let tenant_id = context.tenant_id.clone().ok_or(RlsError::MissingTenant {
                table: E::TABLE.name,
            })?;
            if let Some(column) = policy.column {
                validate_policy_value_type(E::TABLE, column, &tenant_id)?;
                Ok(Some(policy_column_predicate(
                    E::TABLE.name,
                    column,
                    tenant_id,
                )))
            } else {
                Ok(None)
            }
        }
        RlsPolicyKind::Custom => {
            let Some(name) = policy.custom else {
                return Ok(None);
            };
            Ok(Some(predicates.predicate(
                E::TABLE.name,
                name,
                operation,
                context,
            )?))
        }
        RlsPolicyKind::Public | RlsPolicyKind::Rbac => Ok(None),
    }
}

fn policy_column_predicate(table: &'static str, column: &'static str, value: Value) -> Expr {
    Expr {
        sql: format!("{} = ?", super::column::qualified_column(table, column)),
        binds: vec![value],
        columns: vec![ColumnRef {
            table,
            name: column,
        }],
    }
}
