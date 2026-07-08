use super::{
    AccessContext, BelongsTo, Column, ColumnDef, ColumnRef, CustomPredicateProvider,
    CustomPredicateRegistration, Entity, Expr, ForeignKeyDef, HasMany, IndexDef, MigrationBlocker,
    MigrationPlan, MigrationWriteError, QueryLint, QueryLintSeverity, RlsAuthorizationDef,
    RlsError, RlsMatchMode, RlsOperation, RlsPolicyDef, RlsPolicyKind, SchemaLint, SchemaManifest,
    SqlType, TableDef, Value, belongs_to, has_many,
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

const PUBLIC_RLS: &[RlsPolicyDef] = &[RlsPolicyDef::public()];

impl Entity for Task {
    const TABLE: TableDef = TableDef {
        name: "tasks",
        columns: TASK_COLUMNS,
        indexes: TASK_INDEXES,
        foreign_keys: &[],
        rls: PUBLIC_RLS,
    };
}

struct Board;

impl Board {
    const ID: Column<i64> = Column::new("boards", "id");
    const TASKS: HasMany<Board, Task, i64> = has_many(Self::ID, Task::ID);
}

const BOARD_COLUMNS: &[ColumnDef] = &[
    ColumnDef::new("id", SqlType::Integer),
    ColumnDef::new("name", SqlType::Text),
];

impl Entity for Board {
    const TABLE: TableDef = TableDef {
        name: "boards",
        columns: BOARD_COLUMNS,
        indexes: &[],
        foreign_keys: &[],
        rls: PUBLIC_RLS,
    };
}

impl Task {
    const BOARD: BelongsTo<Task, Board, i64> = belongs_to(Self::ID, Board::ID);
}

#[derive(Debug)]
struct SecureDoc;

impl SecureDoc {
    const ID: Column<i64> = Column::new("secure_docs", "id");
    const USER_ID: Column<String> = Column::new("secure_docs", "user_id");
    const STATUS: Column<String> = Column::new("secure_docs", "status");
}

const SECURE_DOC_COLUMNS: &[ColumnDef] = &[
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
    ColumnDef {
        name: "user_id",
        sql_type: SqlType::Text,
        nullable: false,
        primary_key: false,
        auto_increment: false,
        unique: false,
        indexed: true,
        default_sql: None,
    },
    ColumnDef::new("status", SqlType::Text),
];

const SECURE_DOC_RLS: &[RlsPolicyDef] = &[
    RlsPolicyDef {
        operations: &[],
        kind: RlsPolicyKind::Owner,
        column: Some("user_id"),
        authorization: RlsAuthorizationDef::empty(),
        custom: None,
    },
    RlsPolicyDef {
        operations: &[RlsOperation::Update],
        kind: RlsPolicyKind::Rbac,
        column: None,
        authorization: RlsAuthorizationDef {
            mode: RlsMatchMode::Any,
            roles: &["admin"],
            permissions: &["docs:write"],
            scopes: &[],
            resource: None,
        },
        custom: None,
    },
    RlsPolicyDef {
        operations: &[RlsOperation::Delete],
        kind: RlsPolicyKind::Custom,
        column: None,
        authorization: RlsAuthorizationDef::empty(),
        custom: Some("can_delete_doc"),
    },
];

impl Entity for SecureDoc {
    const TABLE: TableDef = TableDef {
        name: "secure_docs",
        columns: SECURE_DOC_COLUMNS,
        indexes: &[],
        foreign_keys: &[],
        rls: SECURE_DOC_RLS,
    };
}

#[derive(Debug)]
struct UpdateOnlyDoc;

impl UpdateOnlyDoc {
    const STATUS: Column<String> = Column::new("update_only_docs", "status");
}

const UPDATE_ONLY_DOC_COLUMNS: &[ColumnDef] = &[ColumnDef::new("status", SqlType::Text)];

const UPDATE_ONLY_DOC_RLS: &[RlsPolicyDef] = &[RlsPolicyDef {
    operations: &[RlsOperation::Update],
    kind: RlsPolicyKind::Rbac,
    column: None,
    authorization: RlsAuthorizationDef {
        mode: RlsMatchMode::All,
        roles: &[],
        permissions: &["docs:update"],
        scopes: &[],
        resource: None,
    },
    custom: None,
}];

impl Entity for UpdateOnlyDoc {
    const TABLE: TableDef = TableDef {
        name: "update_only_docs",
        columns: UPDATE_ONLY_DOC_COLUMNS,
        indexes: &[],
        foreign_keys: &[],
        rls: UPDATE_ONLY_DOC_RLS,
    };
}

#[derive(Debug)]
struct ResourceDoc;

impl ResourceDoc {
    const STATUS: Column<String> = Column::new("resource_docs", "status");
}

const RESOURCE_DOC_COLUMNS: &[ColumnDef] = &[ColumnDef::new("status", SqlType::Text)];

const RESOURCE_DOC_RLS: &[RlsPolicyDef] = &[RlsPolicyDef {
    operations: &[RlsOperation::Update],
    kind: RlsPolicyKind::Rbac,
    column: None,
    authorization: RlsAuthorizationDef {
        mode: RlsMatchMode::All,
        roles: &[],
        permissions: &["docs:update"],
        scopes: &[],
        resource: Some("doc:7"),
    },
    custom: None,
}];

impl Entity for ResourceDoc {
    const TABLE: TableDef = TableDef {
        name: "resource_docs",
        columns: RESOURCE_DOC_COLUMNS,
        indexes: &[],
        foreign_keys: &[],
        rls: RESOURCE_DOC_RLS,
    };
}

#[derive(Debug)]
struct TenantDoc;

impl TenantDoc {
    const ID: Column<i64> = Column::new("tenant_docs", "id");
    const ORG_ID: Column<i64> = Column::new("tenant_docs", "org_id");
}

#[derive(Debug)]
struct BoolDoc;

impl BoolDoc {
    const ID: Column<i64> = Column::new("bool_docs", "id");
    const ACTIVE: Column<bool> = Column::new("bool_docs", "active");
}

const BOOL_DOC_COLUMNS: &[ColumnDef] = &[
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
    ColumnDef {
        name: "active",
        sql_type: SqlType::Boolean,
        nullable: false,
        primary_key: false,
        auto_increment: false,
        unique: false,
        indexed: true,
        default_sql: None,
    },
];

const BOOL_DOC_RLS: &[RlsPolicyDef] = &[RlsPolicyDef {
    operations: &[],
    kind: RlsPolicyKind::Tenant,
    column: Some("active"),
    authorization: RlsAuthorizationDef::empty(),
    custom: None,
}];

impl Entity for BoolDoc {
    const TABLE: TableDef = TableDef {
        name: "bool_docs",
        columns: BOOL_DOC_COLUMNS,
        indexes: &[],
        foreign_keys: &[],
        rls: BOOL_DOC_RLS,
    };
}

const TENANT_DOC_COLUMNS: &[ColumnDef] = &[
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
    ColumnDef {
        name: "org_id",
        sql_type: SqlType::Integer,
        nullable: false,
        primary_key: false,
        auto_increment: false,
        unique: false,
        indexed: true,
        default_sql: None,
    },
];

const TENANT_DOC_RLS: &[RlsPolicyDef] = &[RlsPolicyDef {
    operations: &[],
    kind: RlsPolicyKind::Tenant,
    column: Some("org_id"),
    authorization: RlsAuthorizationDef::empty(),
    custom: None,
}];

impl Entity for TenantDoc {
    const TABLE: TableDef = TableDef {
        name: "tenant_docs",
        columns: TENANT_DOC_COLUMNS,
        indexes: &[],
        foreign_keys: &[],
        rls: TENANT_DOC_RLS,
    };
}

struct TestPredicates;

impl CustomPredicateProvider for TestPredicates {
    fn predicate(
        &self,
        _table: &'static str,
        name: &'static str,
        _operation: RlsOperation,
        _context: &AccessContext,
    ) -> Result<Expr, RlsError> {
        if name == "can_delete_doc" {
            Ok(SecureDoc::STATUS.eq("archived"))
        } else {
            Err(RlsError::MissingCustomPredicate {
                table: "secure_docs",
                name,
            })
        }
    }

    fn registered_predicate_rules(&self) -> &'static [CustomPredicateRegistration] {
        &[CustomPredicateRegistration {
            name: "can_delete_doc",
            operations: &[RlsOperation::Delete],
        }]
    }
}

struct EmptyPredicates;

impl CustomPredicateProvider for EmptyPredicates {
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

struct WrongOperationPredicates;

impl CustomPredicateProvider for WrongOperationPredicates {
    fn predicate(
        &self,
        table: &'static str,
        name: &'static str,
        _operation: RlsOperation,
        _context: &AccessContext,
    ) -> Result<Expr, RlsError> {
        Err(RlsError::MissingCustomPredicate { table, name })
    }

    fn registered_predicate_rules(&self) -> &'static [CustomPredicateRegistration] {
        &[CustomPredicateRegistration {
            name: "can_delete_doc",
            operations: &[RlsOperation::Update],
        }]
    }
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
fn raw_unscoped_statement_is_explicit() {
    let statement = super::Statement::raw_unscoped(
        "SELECT count(*) FROM secure_docs WHERE user_id = ?",
        [Value::Text("user_1".into())],
    );

    assert_eq!(
        statement.sql,
        "SELECT count(*) FROM secure_docs WHERE user_id = ?"
    );
    assert_eq!(statement.binds, vec![Value::Text("user_1".into())]);
}

#[test]
fn scoped_select_applies_owner_predicate() {
    let context = AccessContext::authenticated("user_1");
    let statement = SecureDoc::select_scoped(&context)
        .unwrap()
        .where_(SecureDoc::ID.eq(7))
        .order_by(SecureDoc::USER_ID.asc())
        .limit(1)
        .to_statement();

    assert_eq!(
        statement.sql,
        "SELECT \"id\", \"user_id\", \"status\" FROM \"secure_docs\" \
         WHERE (\"secure_docs\".\"user_id\" = ?) AND (\"secure_docs\".\"id\" = ?) \
         ORDER BY \"secure_docs\".\"user_id\" ASC LIMIT ?"
    );
    assert_eq!(
        statement.binds,
        vec![
            Value::Text("user_1".into()),
            Value::Integer(7),
            Value::Integer(1)
        ]
    );
}

#[test]
fn query_lints_flag_unscoped_rls_for_protected_tables() {
    assert_eq!(
        SecureDoc::select().limit(10).lint(),
        vec![QueryLint::UnscopedRls {
            table: "secure_docs"
        }]
    );
    assert_eq!(
        SecureDoc::update().set(SecureDoc::STATUS, "draft").lint(),
        vec![
            QueryLint::UnscopedRls {
                table: "secure_docs"
            },
            QueryLint::BroadUpdate,
        ]
    );
    assert_eq!(
        SecureDoc::insert().set(SecureDoc::STATUS, "draft").lint(),
        vec![QueryLint::UnscopedRls {
            table: "secure_docs"
        }]
    );
    assert_eq!(
        SecureDoc::delete().lint(),
        vec![
            QueryLint::UnscopedRls {
                table: "secure_docs"
            },
            QueryLint::BroadDelete,
        ]
    );
}

#[test]
fn query_lints_accept_scoped_or_explicitly_unscoped_rls() {
    let context = AccessContext::authenticated("user_1").with_permissions(["docs:write"]);

    assert_eq!(
        SecureDoc::select_scoped(&context).unwrap().limit(10).lint(),
        Vec::new()
    );
    assert_eq!(
        SecureDoc::insert_scoped(&context)
            .unwrap()
            .set(SecureDoc::STATUS, "draft")
            .lint(),
        Vec::new()
    );
    assert_eq!(
        SecureDoc::select()
            .allow_unscoped_rls("admin export")
            .allow_unbounded_select()
            .lint(),
        Vec::new()
    );
    assert_eq!(
        SecureDoc::insert()
            .allow_unscoped_rls("admin backfill")
            .set(SecureDoc::STATUS, "draft")
            .lint(),
        Vec::new()
    );
}

#[test]
fn scoped_insert_sets_owner_column() {
    let context = AccessContext::authenticated("user_1");
    let statement = SecureDoc::insert_scoped(&context)
        .unwrap()
        .set(SecureDoc::STATUS, "draft")
        .to_statement();

    assert_eq!(
        statement.sql,
        "INSERT INTO \"secure_docs\" (\"user_id\", \"status\") VALUES (?, ?)"
    );
    assert_eq!(
        statement.binds,
        vec![Value::Text("user_1".into()), Value::Text("draft".into())]
    );
}

#[test]
fn scoped_tenant_predicate_preserves_value_type() {
    let context = AccessContext::default().with_tenant_value(42_i64);
    let statement = TenantDoc::select_scoped(&context)
        .unwrap()
        .where_(TenantDoc::ORG_ID.eq(42_i64))
        .where_(TenantDoc::ID.eq(7_i64))
        .limit(1)
        .to_statement();

    assert_eq!(
        statement.sql,
        "SELECT \"id\", \"org_id\" FROM \"tenant_docs\" \
         WHERE ((\"tenant_docs\".\"org_id\" = ?) AND (\"tenant_docs\".\"org_id\" = ?)) \
         AND (\"tenant_docs\".\"id\" = ?) LIMIT ?"
    );
    assert_eq!(
        statement.binds,
        vec![
            Value::Integer(42),
            Value::Integer(42),
            Value::Integer(7),
            Value::Integer(1)
        ]
    );
}

#[test]
fn scoped_tenant_predicate_rejects_wrong_value_type() {
    let context = AccessContext::default().with_tenant("org_42");
    assert_eq!(
        TenantDoc::select_scoped(&context).unwrap_err(),
        RlsError::TypeMismatch {
            table: "tenant_docs",
            column: "org_id",
            expected: SqlType::Integer,
        }
    );
}

#[test]
fn boolean_rls_accepts_bool_and_zero_one_integer_values() {
    let bool_statement = BoolDoc::select_scoped(&AccessContext::default().with_tenant_value(true))
        .unwrap()
        .where_(BoolDoc::ACTIVE.eq(true))
        .where_(BoolDoc::ID.eq(1_i64))
        .limit(1)
        .to_statement();
    assert_eq!(
        bool_statement.sql,
        "SELECT \"id\", \"active\" FROM \"bool_docs\" \
         WHERE ((\"bool_docs\".\"active\" = ?) AND (\"bool_docs\".\"active\" = ?)) \
         AND (\"bool_docs\".\"id\" = ?) LIMIT ?"
    );
    assert_eq!(
        bool_statement.binds,
        vec![
            Value::Bool(true),
            Value::Bool(true),
            Value::Integer(1),
            Value::Integer(1)
        ]
    );

    assert!(BoolDoc::select_scoped(&AccessContext::default().with_tenant_value(1_i64)).is_ok());
    assert_eq!(
        BoolDoc::select_scoped(&AccessContext::default().with_tenant_value(2_i64)).unwrap_err(),
        RlsError::TypeMismatch {
            table: "bool_docs",
            column: "active",
            expected: SqlType::Boolean,
        }
    );
}

#[test]
fn scoped_update_enforces_rbac_per_operation() {
    let denied = SecureDoc::update_scoped(&AccessContext::authenticated("user_1"));
    assert_eq!(
        denied.unwrap_err(),
        RlsError::Forbidden {
            table: "secure_docs"
        }
    );

    let context = AccessContext::authenticated("user_1").with_permissions(["docs:write"]);
    let statement = SecureDoc::update_scoped(&context)
        .unwrap()
        .set(SecureDoc::STATUS, "published")
        .where_(SecureDoc::ID.eq(7))
        .to_statement();

    assert_eq!(
        statement.sql,
        "UPDATE \"secure_docs\" SET \"status\" = ? \
         WHERE (\"secure_docs\".\"user_id\" = ?) AND (\"secure_docs\".\"id\" = ?)"
    );
    assert_eq!(
        statement.binds,
        vec![
            Value::Text("published".into()),
            Value::Text("user_1".into()),
            Value::Integer(7)
        ]
    );
}

#[test]
fn scoped_builders_fail_closed_for_uncovered_operations() {
    let context = AccessContext::authenticated("user_1").with_permissions(["docs:update"]);

    assert_eq!(
        UpdateOnlyDoc::select_scoped(&context).unwrap_err(),
        RlsError::Forbidden {
            table: "update_only_docs"
        }
    );
    assert_eq!(
        UpdateOnlyDoc::insert_scoped(&context).unwrap_err(),
        RlsError::Forbidden {
            table: "update_only_docs"
        }
    );
    assert_eq!(
        UpdateOnlyDoc::delete_scoped(&context).unwrap_err(),
        RlsError::Forbidden {
            table: "update_only_docs"
        }
    );
    assert!(
        UpdateOnlyDoc::update_scoped(&context)
            .unwrap()
            .set(UpdateOnlyDoc::STATUS, "reviewed")
            .lint()
            .contains(&QueryLint::BroadUpdate)
    );
}

#[test]
fn scoped_rbac_requires_matching_resource_when_declared() {
    assert_eq!(
        ResourceDoc::update_scoped(
            &AccessContext::authenticated("user_1").with_permissions(["docs:update"])
        )
        .unwrap_err(),
        RlsError::Forbidden {
            table: "resource_docs"
        }
    );
    assert_eq!(
        ResourceDoc::update_scoped(
            &AccessContext::authenticated("user_1")
                .with_permissions(["docs:update"])
                .with_resource("doc:8")
        )
        .unwrap_err(),
        RlsError::Forbidden {
            table: "resource_docs"
        }
    );
    assert!(
        ResourceDoc::update_scoped(
            &AccessContext::authenticated("user_1")
                .with_permissions(["docs:update"])
                .with_resource("doc:7")
        )
        .unwrap()
        .set(ResourceDoc::STATUS, "reviewed")
        .lint()
        .contains(&QueryLint::BroadUpdate)
    );
}

#[test]
fn checked_statement_rejects_linted_queries() {
    let error = SecureDoc::select()
        .limit(10)
        .to_statement_checked()
        .unwrap_err();

    assert_eq!(
        error.lints,
        vec![QueryLint::UnscopedRls {
            table: "secure_docs"
        }]
    );
}

#[test]
fn checked_statement_allows_warning_lints() {
    let statement = Task::select().to_statement_checked().unwrap();

    assert_eq!(
        Task::select().lint()[0].severity(),
        QueryLintSeverity::Warning
    );
    assert_eq!(
        statement.sql,
        "SELECT \"id\", \"title\", \"done\", \"created_at\" FROM \"tasks\""
    );
}

#[test]
fn scoped_delete_composes_custom_predicate() {
    let context = AccessContext::authenticated("user_1");
    let statement = SecureDoc::delete_scoped_with(&context, &TestPredicates)
        .unwrap()
        .where_(SecureDoc::ID.eq(7))
        .to_statement();

    assert_eq!(
        statement.sql,
        "DELETE FROM \"secure_docs\" \
         WHERE ((\"secure_docs\".\"user_id\" = ?) AND (\"secure_docs\".\"status\" = ?)) \
         AND (\"secure_docs\".\"id\" = ?)"
    );
    assert_eq!(
        statement.binds,
        vec![
            Value::Text("user_1".into()),
            Value::Text("archived".into()),
            Value::Integer(7)
        ]
    );
}

#[test]
fn custom_predicate_registry_validates_entity_policies() {
    assert_eq!(
        SecureDoc::validate_custom_predicates_with(&EmptyPredicates).unwrap_err(),
        RlsError::MissingCustomPredicate {
            table: "secure_docs",
            name: "can_delete_doc",
        }
    );
    assert_eq!(
        SecureDoc::validate_custom_predicates_with(&TestPredicates),
        Ok(())
    );
    assert_eq!(
        SecureDoc::validate_custom_predicates_with(&WrongOperationPredicates).unwrap_err(),
        RlsError::MissingCustomPredicate {
            table: "secure_docs",
            name: "can_delete_doc",
        }
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
fn like_escaped_wraps_pattern_and_escapes_wildcards() {
    let expr = Task::TITLE.like_escaped("50%_off\\sale");

    assert_eq!(
        expr,
        Expr {
            sql: "\"tasks\".\"title\" LIKE ? ESCAPE '\\'".into(),
            binds: vec![Value::Text("%50\\%\\_off\\\\sale%".into())],
            columns: vec![ColumnRef {
                table: "tasks",
                name: "title",
            }],
        }
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
fn belongs_to_relation_builds_parent_lookup() {
    let statement = Task::BOARD.select_parent(42).to_statement();

    assert_eq!(
        statement.sql,
        "SELECT \"id\", \"name\" FROM \"boards\" WHERE \"boards\".\"id\" = ? LIMIT ?"
    );
    assert_eq!(statement.binds, vec![Value::Integer(42), Value::Integer(1)]);
}

#[test]
fn has_many_relation_builds_child_lookup() {
    let statement = Board::TASKS
        .select_children(7)
        .order_by(Task::ID.asc())
        .limit(50)
        .to_statement();

    assert_eq!(
        statement.sql,
        "SELECT \"id\", \"title\", \"done\", \"created_at\" FROM \"tasks\" \
         WHERE \"tasks\".\"id\" = ? ORDER BY \"tasks\".\"id\" ASC LIMIT ?"
    );
    assert_eq!(statement.binds, vec![Value::Integer(7), Value::Integer(50)]);
}

#[test]
fn relationship_helpers_have_scoped_variants() {
    let context = AccessContext::authenticated("user_1");
    let statement = Board::TASKS
        .select_children_scoped(7, &context)
        .unwrap()
        .order_by(Task::ID.asc())
        .limit(50)
        .to_statement();

    assert_eq!(
        statement.sql,
        "SELECT \"id\", \"title\", \"done\", \"created_at\" FROM \"tasks\" \
         WHERE \"tasks\".\"id\" = ? ORDER BY \"tasks\".\"id\" ASC LIMIT ?"
    );
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
        foreign_keys: &[],
        rls: PUBLIC_RLS,
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
fn initial_migration_generates_foreign_key_constraints() {
    const BOARD_COLUMNS: &[ColumnDef] = &[
        ColumnDef {
            name: "id",
            sql_type: SqlType::Integer,
            nullable: false,
            primary_key: true,
            auto_increment: false,
            unique: true,
            indexed: true,
            default_sql: None,
        },
        ColumnDef {
            name: "owner_id",
            sql_type: SqlType::Integer,
            nullable: false,
            primary_key: false,
            auto_increment: false,
            unique: false,
            indexed: true,
            default_sql: None,
        },
    ];
    const BOARD_FOREIGN_KEYS: &[ForeignKeyDef] = &[ForeignKeyDef {
        columns: &["owner_id"],
        references_table: "users",
        references_columns: &["id"],
    }];
    let manifest = SchemaManifest::new([TableDef {
        name: "boards",
        columns: BOARD_COLUMNS,
        indexes: &[],
        foreign_keys: BOARD_FOREIGN_KEYS,
        rls: PUBLIC_RLS,
    }]);

    assert_eq!(
        manifest.initial_migration(),
        vec![
            "CREATE TABLE \"boards\" (\"id\" INTEGER PRIMARY KEY, \"owner_id\" INTEGER NOT NULL, FOREIGN KEY (\"owner_id\") REFERENCES \"users\" (\"id\"))",
            "CREATE INDEX \"idx_boards_owner_id\" ON \"boards\" (\"owner_id\")",
        ]
    );
}

#[test]
fn schema_lints_flag_unindexed_foreign_keys() {
    const COMMENT_COLUMNS: &[ColumnDef] = &[
        ColumnDef::new("id", SqlType::Integer),
        ColumnDef::new("task_id", SqlType::Integer),
    ];
    const COMMENT_FOREIGN_KEYS: &[ForeignKeyDef] = &[ForeignKeyDef {
        columns: &["task_id"],
        references_table: "tasks",
        references_columns: &["id"],
    }];
    let manifest = SchemaManifest::new([TableDef {
        name: "comments",
        columns: COMMENT_COLUMNS,
        indexes: &[],
        foreign_keys: COMMENT_FOREIGN_KEYS,
        rls: PUBLIC_RLS,
    }]);

    assert_eq!(
        manifest.lint(),
        vec![SchemaLint::UnindexedForeignKey {
            table: "comments",
            column: "task_id",
        }]
    );
}

#[test]
fn schema_lints_accept_indexed_foreign_keys() {
    const COMMENT_COLUMNS: &[ColumnDef] = &[
        ColumnDef::new("id", SqlType::Integer),
        ColumnDef {
            name: "task_id",
            sql_type: SqlType::Integer,
            nullable: false,
            primary_key: false,
            auto_increment: false,
            unique: false,
            indexed: true,
            default_sql: None,
        },
    ];
    const COMMENT_FOREIGN_KEYS: &[ForeignKeyDef] = &[ForeignKeyDef {
        columns: &["task_id"],
        references_table: "tasks",
        references_columns: &["id"],
    }];
    let manifest = SchemaManifest::new([TableDef {
        name: "comments",
        columns: COMMENT_COLUMNS,
        indexes: &[],
        foreign_keys: COMMENT_FOREIGN_KEYS,
        rls: PUBLIC_RLS,
    }]);

    assert_eq!(manifest.lint(), Vec::new());
}

#[test]
fn schema_lints_flag_missing_rls() {
    const COMMENT_COLUMNS: &[ColumnDef] = &[ColumnDef::new("id", SqlType::Integer)];
    let manifest = SchemaManifest::new([TableDef {
        name: "comments",
        columns: COMMENT_COLUMNS,
        indexes: &[],
        foreign_keys: &[],
        rls: &[],
    }]);

    assert_eq!(
        manifest.lint(),
        vec![SchemaLint::MissingRls { table: "comments" }]
    );
}

#[test]
fn schema_lints_flag_unindexed_rls_columns() {
    const COMMENT_COLUMNS: &[ColumnDef] = &[
        ColumnDef::new("id", SqlType::Integer),
        ColumnDef::new("user_id", SqlType::Text),
    ];
    const COMMENT_RLS: &[RlsPolicyDef] = &[RlsPolicyDef {
        operations: &[],
        kind: RlsPolicyKind::Owner,
        column: Some("user_id"),
        authorization: RlsAuthorizationDef::empty(),
        custom: None,
    }];
    let manifest = SchemaManifest::new([TableDef {
        name: "comments",
        columns: COMMENT_COLUMNS,
        indexes: &[],
        foreign_keys: &[],
        rls: COMMENT_RLS,
    }]);

    assert_eq!(
        manifest.lint(),
        vec![SchemaLint::UnindexedRlsColumn {
            table: "comments",
            column: "user_id",
        }]
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
        foreign_keys: &[],
        rls: PUBLIC_RLS,
    }]);
    let desired = SchemaManifest::new([TableDef {
        name: "tasks",
        columns: DESIRED_COLUMNS,
        indexes: &[],
        foreign_keys: &[],
        rls: PUBLIC_RLS,
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
fn migration_diff_blocks_rls_changes_for_review() {
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
        ColumnDef {
            name: "user_id",
            sql_type: SqlType::Text,
            nullable: false,
            primary_key: false,
            auto_increment: false,
            unique: false,
            indexed: true,
            default_sql: None,
        },
    ];
    const OWNER_RLS: &[RlsPolicyDef] = &[RlsPolicyDef {
        operations: &[],
        kind: RlsPolicyKind::Owner,
        column: Some("user_id"),
        authorization: RlsAuthorizationDef::empty(),
        custom: None,
    }];
    let current = SchemaManifest::new([TableDef {
        name: "tasks",
        columns: COLUMNS,
        indexes: &[],
        foreign_keys: &[],
        rls: PUBLIC_RLS,
    }]);
    let desired = SchemaManifest::new([TableDef {
        name: "tasks",
        columns: COLUMNS,
        indexes: &[],
        foreign_keys: &[],
        rls: OWNER_RLS,
    }]);

    let plan = current.diff(&desired);

    assert_eq!(
        plan.blockers,
        vec![MigrationBlocker::ChangeRls {
            table: "tasks".into(),
        }]
    );
    assert_eq!(plan.statements, Vec::<String>::new());
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
        foreign_keys: &[],
        rls: PUBLIC_RLS,
    }]);
    let desired = SchemaManifest::new([TableDef {
        name: "tasks",
        columns: DESIRED_COLUMNS,
        indexes: &[],
        foreign_keys: &[],
        rls: PUBLIC_RLS,
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
fn migration_diff_blocks_foreign_key_changes_on_existing_tables() {
    const COLUMNS: &[ColumnDef] = &[
        ColumnDef::new("id", SqlType::Integer),
        ColumnDef {
            name: "task_id",
            sql_type: SqlType::Integer,
            nullable: false,
            primary_key: false,
            auto_increment: false,
            unique: false,
            indexed: true,
            default_sql: None,
        },
    ];
    const FOREIGN_KEYS: &[ForeignKeyDef] = &[ForeignKeyDef {
        columns: &["task_id"],
        references_table: "tasks",
        references_columns: &["id"],
    }];
    let current = SchemaManifest::new([TableDef {
        name: "comments",
        columns: COLUMNS,
        indexes: &[],
        foreign_keys: &[],
        rls: PUBLIC_RLS,
    }]);
    let desired = SchemaManifest::new([TableDef {
        name: "comments",
        columns: COLUMNS,
        indexes: &[],
        foreign_keys: FOREIGN_KEYS,
        rls: PUBLIC_RLS,
    }]);

    let plan = current.diff(&desired);

    assert!(!plan.is_safe());
    assert_eq!(
        plan.blockers,
        vec![MigrationBlocker::AddForeignKey {
            table: "comments".into(),
            columns: vec!["task_id".into()],
        }]
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
