# Nebula Roadmap

Nebula is a D1-first ORM for Comet. Its goal is to give Rocket-on-Workers apps
an ergonomic data layer without adding hidden request-time cost to Comet.

## Goals

- Keep route code close to Rocket's normal ergonomics.
- Make D1 the first-class backend, using SQLite-compatible SQL directly.
- Generate deterministic SQL with explicit bind values.
- Generate migrations outside Worker request handling.
- Keep the Comet hot path free of Nebula unless the `nebula` feature is
  enabled.
- Preserve a raw SQL escape hatch for complex queries.

## Non-Goals For The MVP

- Runtime schema synchronization.
- Automatic production migrations from inside a Worker request.
- Cross-database portability before the D1/SQLite path is strong.
- A fully general relational mapper with implicit joins and lazy loading.
- Query planning that hides D1's rows-read/rows-written cost model.

## Entity Derive

Nebula exposes `#[derive(comet::nebula::Entity)]` behind the `nebula` feature.
The derive macro lives in the `comet-macros` proc-macro crate and is reexported
from `comet::nebula`.

```rust
#[derive(comet::nebula::Entity)]
#[nebula(table = "tasks")]
pub struct Task {
    #[nebula(primary_key, auto)]
    pub id: i64,

    #[nebula(index)]
    pub title: String,

    #[nebula(index)]
    pub done: bool,

    #[nebula(foreign_key = "boards.id", index)]
    pub board_id: i64,

    pub created_at: String,
}
```

Supported MVP attributes:

- Struct: `#[nebula(table = "tasks")]`
- Struct: `#[nebula(crate = "::my_crate")]` for macro path overrides
- Field: `#[nebula(primary_key)]`
- Field: `#[nebula(auto)]` or `#[nebula(auto_increment)]`
- Field: `#[nebula(index)]` or `#[nebula(indexed)]`
- Field: `#[nebula(unique)]`
- Field: `#[nebula(nullable)]` or `#[nebula(nullable = true)]`
- Field: `#[nebula(default = "0")]`
- Field: `#[nebula(rename = "created_at")]`
- Field: `#[nebula(foreign_key = "boards.id")]`

The derive supports structs with named fields. It generates typed
`Column<T>` constants using upper-snake field names and an `Entity`
implementation with deterministic `TableDef` metadata. The MVP emits compile
errors for tuple/unit structs, duplicate column names, multiple primary keys,
unsupported field types, and invalid foreign-key syntax.

Foreign keys are schema metadata first. A field like
`#[nebula(foreign_key = "boards.id", index)]` generates a local column plus a
`FOREIGN KEY ("board_id") REFERENCES "boards" ("id")` constraint in initial
migrations. Nebula lints foreign-key columns that are not indexed because D1
relationship lookups and cascading-style application queries should not depend
on table scans.

Relationship helpers are explicit query-builder shortcuts, not lazy loading.
Apps can define constants such as:

```rust
impl BoardTask {
    pub const BOARD: comet::nebula::BelongsTo<BoardTask, Board, i64> =
        comet::nebula::belongs_to(Self::BOARD_ID, Board::ID);
}

impl Board {
    pub const TASKS: comet::nebula::HasMany<Board, BoardTask, i64> =
        comet::nebula::has_many(Self::ID, BoardTask::BOARD_ID);
}
```

`BoardTask::BOARD.select_parent(board_id)` returns a normal `Select<Board>`;
`Board::TASKS.select_children(board_id)` returns a normal `Select<BoardTask>`.
Callers still choose limits, ordering, execution, and raw SQL when a join or a
hand-tuned query is better.

Routes should read like ordinary Comet/Rocket code:

```rust
#[get("/tasks")]
async fn list(db: comet::cloudflare::D1<DB>) -> Result<Json<Vec<Task>>, ApiError> {
    let tasks = Task::select()
        .where_(Task::DONE.eq(false))
        .order_by(Task::CREATED_AT.desc())
        .limit(50)
        .fetch_all(&db)
        .await?;

    Ok(Json(tasks))
}
```

The lower-level metadata APIs remain public so generated and hand-written
entities can coexist.

## Architecture

Nebula should split conceptually into these layers:

- Core schema/query model: entity metadata, column metadata, SQL values,
  expressions, and deterministic statement generation.
- D1 adapter: converts Nebula statements into `worker::D1Database` prepared
  statements and binds.
- Derive macros: generate entity metadata and typed column constants.
- Migration tooling: generate and diff schema manifests into Wrangler-compatible
  migration files.
- Lints/optimization hints: warn about missing limits, unindexed filters,
  unindexed orderings, and broad writes.

The runtime/core MVP lives behind the `nebula` feature in the main crate. The
derive macro lives in `comet-macros` as a path dependency outside the Cargo
workspace so standard project commands do not accidentally format or test the
vendored Rocket crates.

## Migration Policy

Nebula should not run migrations automatically from request handlers.

The intended workflow:

1. Entity metadata is collected from code.
2. A deterministic schema manifest is written.
3. A CLI/build tool compares manifests.
4. SQL migration files are generated under a Wrangler-compatible `migrations/`
   directory.
5. The application applies migrations with `wrangler d1 migrations apply`.

Destructive changes should be blocked by default. The first safe diff set is:

- create table
- add nullable column
- add column with default
- create index
- create unique index
- foreign-key constraints on newly-created tables

## Migration Core

The MVP migration core lives in `comet::nebula` and is intentionally independent
from Wrangler, D1 bindings, and Worker request runtime.

It exposes:

- `SchemaManifest::new(...)` for deterministic table manifests.
- `SchemaManifest::to_manifest_string()` for snapshotting schema state.
- `SchemaManifest::initial_migration()` for `CREATE TABLE` and index SQL.
- `SchemaManifest::diff(...)` for safe additive migration plans.
- `MigrationPlan::is_safe()` plus `MigrationBlocker` values for changes that
  require human review.
- `MigrationPlan::to_sql_file_contents()` for semicolon-terminated SQL files.
- `MigrationPlan::migration_file_name(...)` for deterministic
  `0001_slug.sql` names.
- `MigrationPlan::write_sql_file(...)` for native tooling that writes files
  under a Wrangler-compatible migrations directory.

The safe diff MVP generates:

- missing tables
- nullable columns
- columns with defaults
- missing indexes
- missing unique indexes

The diff blocks:

- dropped tables
- dropped columns
- changed columns
- non-null columns without defaults
- changed or dropped indexes
- added, changed, or dropped foreign keys on existing tables

The core writer produces Wrangler-compatible numbered migration files, for
example:

```text
migrations/0001_initial.sql
migrations/0002_add_task_done.sql
```

Those files are then applied with Wrangler's normal D1 migration flow, not from
inside a Worker request. A future CLI/build wrapper should call the core writer
after collecting entity metadata from application code.

## D1 Performance Constraints

D1 pricing and performance are shaped by rows read, rows written, statement
shape, and indexes. Nebula should make those costs more visible, not hide them.

Design constraints:

- Prefer prepared statements with explicit bind values.
- Preserve bind order deterministically.
- Encourage `LIMIT` on list queries.
- Track indexed columns in metadata.
- Warn when filters/orderings use columns without indexes.
- Keep generated SQL inspectable through `to_statement()`.
- Keep raw SQL available for queries that do not fit the builder.

## Query Lints

Nebula query builders expose `lint()` so applications and future CLI tooling can
spot expensive query shapes before they are shipped. The lint API is advisory:
it does not change generated SQL and it does not run during D1 execution unless
the application explicitly calls it.

The MVP lints cover:

- selects without `LIMIT`
- filters on columns that are not primary keys, unique, explicitly indexed, or
  the left-most column of an index
- orderings on unindexed columns
- updates and deletes without a `WHERE` clause

Intentional cases should be explicit in code:

- `allow_unbounded_select()` documents an intentionally unbounded read.
- `allow_full_table_scan()` documents an intentional scan or unindexed sort.
- `allow_broad_write()` documents an intentional table-wide update/delete.

These lints map directly to D1 cost risks: unbounded reads and unindexed
filters/orderings can increase rows read, while broad writes can increase rows
written and make accidental destructive changes easier.

## Raw SQL Escape Hatch

Nebula statements are intentionally plain SQL plus bind values. Apps should keep
using `worker::D1Database::prepare()` directly for queries that need SQLite/D1
features outside the builder surface, such as recursive CTEs, FTS, specialized
aggregates, or hand-tuned query plans.

The preferred rule is pragmatic:

- Use Nebula builders for common entity CRUD and simple indexed lookups.
- Use raw prepared SQL when the query shape is clearer, faster, or not yet
  represented by Nebula.
- Keep raw SQL parameterized; do not build SQL by concatenating user input.
- Keep migrations as the source of truth until Nebula's migration generator is
  implemented.

## Current MVP

Implemented:

- `nebula` feature gate.
- `Entity`, `TableDef`, `ColumnDef`, `IndexDef`, and `SqlType`.
- `#[derive(comet::nebula::Entity)]` for named structs.
- Typed `Column<T>` values.
- `Value` bind model independent of Worker/D1 types.
- `Select`, `Insert`, `Update`, and `Delete` builders.
- `nebula-d1` execution helpers for D1 prepared statements, batch execution,
  and typed result fetching.
- Migration manifests, initial migration SQL, and safe additive schema diffs.
- Native migration SQL file writer for Wrangler-compatible `migrations/`
  directories.
- Example task routes backed by Nebula against local D1.
- Example relationship entities for orgs, users, boards, board tasks, and task
  assignees.
- SQL-generation benchmarks and `wrangler dev` performance smoke coverage for
  the D1-backed example route.
- Query lints for missing limits, unindexed filters/orderings, and broad writes.
- Schema lints for unindexed foreign keys.
- Explicit `BelongsTo` and `HasMany` helpers that return `Select` builders.
- Unit tests for deterministic SQL and bind ordering.

Next:

- CLI/build wrapper that discovers entities and calls the migration writer.
