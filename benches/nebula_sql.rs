use comet::nebula::{Column, ColumnDef, Entity, IndexDef, SqlType, TableDef};
use criterion::{Criterion, black_box, criterion_group, criterion_main};

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
        name: "created_at",
        sql_type: SqlType::Text,
        nullable: false,
        primary_key: false,
        auto_increment: false,
        unique: false,
        indexed: true,
        default_sql: Some("datetime('now')"),
    },
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

fn nebula_sql_benches(c: &mut Criterion) {
    c.bench_function("nebula_select_filtered", |b| {
        b.iter(|| {
            let statement = Task::select()
                .where_(Task::DONE.eq(black_box(false)))
                .and_where(Task::TITLE.like(black_box("%docs%")))
                .order_by(Task::CREATED_AT.desc())
                .limit(black_box(50))
                .offset(black_box(10))
                .to_statement();

            black_box(statement);
        });
    });

    c.bench_function("nebula_insert_returning", |b| {
        b.iter(|| {
            let statement = Task::insert()
                .set(Task::TITLE, black_box("write benchmarks"))
                .set(Task::DONE, black_box(false))
                .returning(["id", "title", "done", "created_at"])
                .to_statement();

            black_box(statement);
        });
    });

    c.bench_function("nebula_update_returning", |b| {
        b.iter(|| {
            let statement = Task::update()
                .set(Task::DONE, black_box(true))
                .where_(Task::ID.eq(black_box(42)))
                .returning(["id", "title", "done", "created_at"])
                .to_statement();

            black_box(statement);
        });
    });
}

criterion_group!(benches, nebula_sql_benches);
criterion_main!(benches);
