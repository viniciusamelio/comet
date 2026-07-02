# Nebula Implementation Tracker

This file is the coordination source for Nebula, the D1-first ORM planned for
Comet. Keep it current when starting, handing off, or finishing work.

Status values:

- `done`: implemented and verified.
- `in-progress`: actively being changed in the current branch.
- `blocked`: cannot proceed without an external decision or dependency.
- `pending`: scoped but not started.

Owner values are free-form. Use `unassigned` when no agent owns the task.

## Snapshot

Last updated: 2026-07-02

| Area | Status | Notes |
| --- | --- | --- |
| Product shape and constraints | done | Roadmap documents D1-first scope, module packaging for the MVP, and no runtime migrations. |
| Core schema/query model | done | Feature-gated MVP supports schema metadata, SQL values, and deterministic select/insert/update/delete builders. |
| D1 execution adapter | done | Statement and explicit batch execution helpers compile behind `nebula-d1`; Worker integration executes Nebula queries against local D1. |
| Entity derive macros | pending | Requires a proc-macro crate or workspace split. |
| Migration generation | pending | Should run from CLI/build tooling, not Worker request runtime. |
| Query optimization hints | pending | Should flag missing indexes and dangerous scans before production. |
| Comet example integration | done | Task routes use Nebula for D1 reads/writes while preserving queue behavior and integration coverage. |
| Performance validation | pending | Must compare against current hand-written SQL and Comet perf baseline. |

## Task List

### A. Product Definition And Coordination

Goal: define Nebula's boundaries before adding ORM surface area.

| ID | Task | Status | Owner | Target files | Done when |
| --- | --- | --- | --- | --- | --- |
| A1 | Add Nebula multi-agent tracker | done | Codex 2026-07-02 | `docs/nebula-implementation-tracker.md` | Work is split into task IDs, statuses, owners, target files, and completion criteria. |
| A2 | Write Nebula design roadmap | done | Codex 2026-07-02 | `docs/nebula-roadmap.md` | Roadmap documents D1-first scope, non-goals, DX examples, migration policy, and performance constraints. |
| A3 | Decide crate/module packaging | done | Codex 2026-07-02 | `Cargo.toml`, docs | Decision is recorded: in-crate feature-gated module for MVP; macro/CLI tasks should revisit workspace packaging. |
| A4 | Document runtime migration policy | done | Codex 2026-07-02 | docs | Docs explicitly say migrations are generated/applied outside Worker request handling. |

### B. Core Schema And SQL Model

Goal: create a small compile-time-friendly core that can represent entities,
columns, typed values, filters, and generated SQL without touching D1.

| ID | Task | Status | Owner | Target files | Done when |
| --- | --- | --- | --- | --- | --- |
| B1 | Add `nebula` feature and module gate | done | Codex 2026-07-02 | `Cargo.toml`, `src/lib.rs`, `src/nebula.rs` | Comet builds unchanged by default; `cargo test --features nebula` compiles the new module. |
| B2 | Define schema metadata types | done | Codex 2026-07-02 | `src/nebula.rs` | `Entity`, `ColumnDef`, `TableDef`, SQL types, indexes, and primary-key metadata exist. |
| B3 | Define SQL value/bind model | done | Codex 2026-07-02 | `src/nebula.rs` | Values cover D1/SQLite primitives without requiring Worker types in core. |
| B4 | Implement select query builder | done | Codex 2026-07-02 | `src/nebula.rs` | Builder emits deterministic SQL and bind values for filters, ordering, limit, and offset. |
| B5 | Implement insert/update/delete builders | done | Codex 2026-07-02 | `src/nebula.rs` | Builders generate deterministic SQL and preserve bind order. |
| B6 | Add core unit tests | done | Codex 2026-07-02 | `src/nebula.rs` | Tests assert SQL strings and bind vectors for representative queries. |

### C. D1 Execution Adapter

Goal: execute Nebula statements through Cloudflare D1 with minimal overhead and
clear D1 cost semantics.

| ID | Task | Status | Owner | Target files | Done when |
| --- | --- | --- | --- | --- | --- |
| C1 | Add `nebula-d1` feature | done | Codex 2026-07-02 | `Cargo.toml`, `src/nebula.rs` or `src/nebula/d1.rs` | D1 integration compiles only when `cloudflare-d1` is available. |
| C2 | Map Nebula values to JS/D1 bind values | done | Codex 2026-07-02 | D1 adapter tests | `Null`, integer, real, text, bool, and blob values bind correctly at the API level; runtime binding coverage remains in C5. |
| C3 | Implement `fetch_all`, `fetch_one`, `fetch_optional`, and `execute` | done | Codex 2026-07-02 | D1 adapter | Query builders can execute through `worker::D1Database`; `comet::cloudflare::D1<B>` works through deref coercion. |
| C4 | Preserve D1 batch semantics | done | Codex 2026-07-02 | D1 adapter | Multi-statement API uses D1 batch explicitly and documents transaction behavior. |
| C5 | Add Worker integration coverage | done | Codex 2026-07-02 | example tests | At least one route executes a Nebula query against local D1 through `wrangler dev`. |

### D. Entity Derive And DX

Goal: make entity mapping ergonomic without adding runtime reflection.

| ID | Task | Status | Owner | Target files | Done when |
| --- | --- | --- | --- | --- | --- |
| D1 | Design derive attribute syntax | pending | unassigned | docs, macro crate | Attributes cover table name, primary keys, indexes, uniqueness, nullability, defaults, and column rename. |
| D2 | Create proc-macro crate plan | pending | unassigned | `Cargo.toml`, workspace docs | Workspace/package split is documented before adding macro crate files. |
| D3 | Implement `#[derive(Entity)]` MVP | pending | unassigned | macro crate | Structs generate `Entity` metadata and typed `Column<T>` constants. |
| D4 | Improve compile errors | pending | unassigned | macro crate tests | Invalid primary keys, unsupported field types, and duplicate column names produce actionable errors. |
| D5 | Add compile-fail tests | pending | unassigned | macro crate tests | Macro diagnostics are covered by UI tests or equivalent. |

### E. Migration Generation

Goal: generate and validate D1/SQLite migrations outside request runtime.

| ID | Task | Status | Owner | Target files | Done when |
| --- | --- | --- | --- | --- | --- |
| E1 | Define schema manifest format | pending | unassigned | docs, CLI/core | Entity metadata can be serialized to a deterministic manifest. |
| E2 | Generate initial `CREATE TABLE` SQL | pending | unassigned | CLI/core | CLI can generate an initial migration from entity metadata. |
| E3 | Implement safe schema diff MVP | pending | unassigned | CLI/core | Add-table, add-nullable/defaulted-column, and add-index diffs are generated; destructive changes are blocked. |
| E4 | Integrate with Wrangler migrations layout | pending | unassigned | example, docs | Generated files land in a `migrations/` layout compatible with `wrangler d1 migrations apply`. |
| E5 | Add migration tests | pending | unassigned | tests | Snapshot tests cover deterministic migration SQL. |

### F. Query Optimization And Safety

Goal: help users avoid expensive D1 scans while keeping runtime fast.

| ID | Task | Status | Owner | Target files | Done when |
| --- | --- | --- | --- | --- | --- |
| F1 | Track indexed columns in metadata | pending | unassigned | core/macro | Query validation can tell whether filters/orderings use indexed columns. |
| F2 | Add query lint API | pending | unassigned | core/CLI | Lints flag missing limits, unindexed filters, unindexed orderings, and broad deletes/updates. |
| F3 | Add explicit escape hatches | pending | unassigned | core docs | Users can acknowledge intentional scans/destructive statements in code. |
| F4 | Document D1 cost model implications | pending | unassigned | docs | Docs connect lints to D1 rows read/written and index usage. |

### G. Example App Integration

Goal: prove Nebula improves the Comet example without hiding D1 behavior.

| ID | Task | Status | Owner | Target files | Done when |
| --- | --- | --- | --- | --- | --- |
| G1 | Model existing task schema as Nebula entities | done | Codex 2026-07-02 | `examples/cloudflare-worker/src/model.rs` | Task rows have Nebula entity metadata without changing route behavior. |
| G2 | Replace list/get task SQL | done | Codex 2026-07-02 | `examples/cloudflare-worker/src/routes.rs` | Read routes use Nebula and integration tests still pass. |
| G3 | Replace create/complete task SQL | done | Codex 2026-07-02 | example routes | Write routes use Nebula and queue behavior remains unchanged. |
| G4 | Keep raw SQL escape hatch documented | done | Codex 2026-07-02 | example README, docs | Example shows how to drop to hand-written SQL for complex cases. |

### H. Performance And Release Gates

Goal: ensure Nebula does not compromise Comet's request/sec or cold path.

| ID | Task | Status | Owner | Target files | Done when |
| --- | --- | --- | --- | --- | --- |
| H1 | Add SQL generation benchmarks | pending | unassigned | benches | Query builder overhead is measured separately from D1/workerd. |
| H2 | Add example perf comparison | pending | unassigned | example perf scripts | `/tasks` is measured before/after Nebula under `wrangler dev`. |
| H3 | Audit feature-gated binary impact | pending | unassigned | Cargo/features docs | Comet without `nebula` has no Nebula code in the build. |
| H4 | Define MVP release criteria | pending | unassigned | tracker/docs | MVP requires docs, tests, integration coverage, and no significant perf regression. |

## Verification Commands

Use these commands after touching the corresponding areas:

```sh
cargo fmt --check
cargo test --features nebula
cargo test --no-default-features --features nebula
cargo test --features cloudflare,cloudflare-d1,nebula,nebula-d1
```

For D1/runtime integration:

```sh
cargo check --manifest-path examples/cloudflare-worker/Cargo.toml
cd examples/cloudflare-worker && npm run test:integration
cd examples/cloudflare-worker && npm run test:perf
```
