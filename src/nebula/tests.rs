use super::{
    BelongsTo, Column, ColumnDef, ColumnRef, Entity, Expr, ForeignKeyDef, HasMany, IndexDef,
    MigrationBlocker, MigrationPlan, MigrationWriteError, QueryLint, SchemaLint, SchemaManifest,
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

impl Entity for Task {
    const TABLE: TableDef = TableDef {
        name: "tasks",
        columns: TASK_COLUMNS,
        indexes: TASK_INDEXES,
        foreign_keys: &[],
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
    };
}

impl Task {
    const BOARD: BelongsTo<Task, Board, i64> = belongs_to(Self::ID, Board::ID);
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
    }]);

    assert_eq!(manifest.lint(), Vec::new());
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
    }]);
    let desired = SchemaManifest::new([TableDef {
        name: "tasks",
        columns: DESIRED_COLUMNS,
        indexes: &[],
        foreign_keys: &[],
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
        foreign_keys: &[],
    }]);
    let desired = SchemaManifest::new([TableDef {
        name: "tasks",
        columns: DESIRED_COLUMNS,
        indexes: &[],
        foreign_keys: &[],
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
    }]);
    let desired = SchemaManifest::new([TableDef {
        name: "comments",
        columns: COLUMNS,
        indexes: &[],
        foreign_keys: FOREIGN_KEYS,
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
