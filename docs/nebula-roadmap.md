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

## Intended Shape

The long-term API should come from a derive macro:

```rust
#[derive(nebula::Entity)]
#[nebula(table = "tasks")]
pub struct Task {
    #[nebula(primary_key, auto)]
    pub id: i64,

    #[nebula(index)]
    pub title: String,

    #[nebula(index)]
    pub done: bool,

    pub created_at: String,
}
```

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

The first implementation is deliberately lower-level: it exposes schema
metadata, typed columns, SQL values, and query builders. The derive macro and
D1 execution adapter are separate tasks in
[`nebula-implementation-tracker.md`](nebula-implementation-tracker.md).

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

The MVP currently lives behind the `nebula` feature in the main crate to avoid
workspace churn. Macro and CLI work should revisit that packaging decision.

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
- Typed `Column<T>` values.
- `Value` bind model independent of Worker/D1 types.
- `Select`, `Insert`, `Update`, and `Delete` builders.
- `nebula-d1` execution helpers for D1 prepared statements, batch execution,
  and typed result fetching.
- Example task routes backed by Nebula against local D1.
- Unit tests for deterministic SQL and bind ordering.

Next:

- Derive macro package plan.
- Migration manifest format.
- Query lint API and optimization hints.
- SQL generation and example performance benchmarks.
