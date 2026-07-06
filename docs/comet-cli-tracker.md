# Comet CLI Implementation Tracker

This file is the coordination source for the Comet CLI: a developer tool that
scaffolds Comet projects, generates Nebula entities and routes, drives
migration generation, and orchestrates the test/release gate. Keep it current
when starting, handing off, or finishing work.

Status values:

- `done`: implemented and verified.
- `in-progress`: actively being changed in the current branch.
- `blocked`: cannot proceed without an external decision or dependency.
- `pending`: scoped but not started.

Owner values are free-form. Use `unassigned` when no agent owns the task.

## Snapshot

Last updated: 2026-07-03

| Area | Status | Notes |
| --- | --- | --- |
| Product shape and packaging | done | A1-A4 all done: decisions recorded, `comet-cli` crate building, dependencies chosen, command surface and layout contract documented in `docs/comet-cli-roadmap.md`. |
| Project scaffolding (`comet new`) | done | `comet new` implemented and verified end-to-end (B1-B4 done). |
| Entity discovery and schema snapshot model | done | C1-C4 all done (owned manifest, `syn` discovery, schema-dump runner, persisted snapshot at `migrations/.comet-schema.json` with real diffing). |
| Migration generation (`comet migrate`) | done | `migrate init`, `migrate generate <name>`, and `migrate status` are all real and verified end-to-end (D1-D5), including the destructive-change/blocker path refusing to write. |
| Entity and route generation (`comet generate`) | done | `generate entity <Name>` and `generate route <Entity>` are both real and verified end-to-end, including a full compile of the generated CRUD module. |
| Test orchestration (`comet test`) | done | `test unit/integration/perf/all` all implemented and verified end-to-end, including the stop-at-first-failure behavior of `all`. |
| Documentation and release | done | G1-G3 done: README, tracker cross-link, and release gate all written. `cargo publish` itself stays blocked on the vendored-Rocket-fork distribution problem `comet` already has (not something this CLI can fix on its own). |

## Open Decisions

Resolved 2026-07-03:

1. **Binary name and distribution — resolved.** Standalone `comet` binary
   (not a `cargo-comet` subcommand).
2. **Crate location — resolved.** `comet-cli` lives in this repo, outside the
   Cargo workspace (path-based, same precedent as `comet-macros`), and
   releases together with the `comet` crate — same version number, same
   release commit/tag, so a given `comet-cli` version always pairs with the
   `comet` core version it was built and tested against.
3. **Entity discovery mechanism — resolved.** `syn`-based static source
   scanning for `#[derive(Entity)]` structs, run at dev-time only (never
   ships in the compiled Worker), consistent with Nebula's "no runtime
   reflection" design goal.
4. **Schema snapshot format — resolved, see C1 below.** Implemented as an
   owned/serializable mirror of `TableDef` that leaks its strings to
   `'static` on demand and hands off to the existing, already-tested
   `SchemaManifest::diff` — no duplication of the diff engine.

## Task List

### A. Product Definition And Packaging

Goal: fix the CLI's shape before any scaffolding or codegen is written.

| ID | Task | Status | Owner | Target files | Done when |
| --- | --- | --- | --- | --- | --- |
| A1 | Record command surface | done | Claude 2026-07-03 | `docs/comet-cli-roadmap.md` | `comet new`, `comet generate entity`, `comet generate route`, `comet migrate init/generate/status`, and `comet test unit/integration/perf/all` are documented with flags and exit-code conventions, written after all five commands were implemented and verified (so it documents actual behavior, not aspiration). |
| A2 | Decide crate/binary packaging | done | Claude 2026-07-03 | `Cargo.toml`, docs | Decision recorded per Open Decision 2: `comet-cli` binary crate, path dependency, not a workspace member, released in lockstep with `comet`. |
| A3 | Choose CLI dependencies | done | Claude 2026-07-03 | `comet-cli/Cargo.toml` | `clap` (derive) + `anyhow` chosen for argument parsing/errors; templating uses `include_str!` + placeholder substitution, no template-engine dependency; `tempfile` added as a dev-dependency for scaffold tests. `syn`/`quote` deliberately not added yet — scoped to C2 (entity discovery) when that work starts. |
| A4 | Define generated-project layout contract | done | Claude 2026-07-03 | `docs/comet-cli-roadmap.md` | Contract mirrors `examples/cloudflare-worker` (`Cargo.toml`, `wrangler.jsonc`, `package.json`, `src/app.rs` + one module per context (`src/<context>/{mod,model,routes,error}.rs`), `migrations/`) so the example stays the reference fixture for scaffolding tests. Documents the two load-bearing rules the layout depends on (entity-bearing contexts must be `pub mod`; `migrations/` is never scaffolded statically) and why. Per-context module layout adopted 2026-07-03, replacing an earlier single `model.rs`/`routes.rs`/`error.rs` design in both the example and the CLI template — see G5 in `nebula-implementation-tracker.md`. |

### B. Project Scaffolding (`comet new`)

Goal: produce a buildable Comet + Nebula + Cloudflare Worker project from one
command, using `examples/cloudflare-worker` as the golden template.

| ID | Task | Status | Owner | Target files | Done when |
| --- | --- | --- | --- | --- | --- |
| B1 | Embed project templates | done | Claude 2026-07-03 | `comet-cli/templates/` | `Cargo.toml`, `wrangler.jsonc`, `package.json`, `README.md`, `.gitignore`, `src/lib.rs`, `src/entry.rs`, `src/app.rs` (mounts routes only), and `src/tasks/{mod,model,routes,error}.rs` templates exist, embedded via `include_str!`, with `{{project_name}}`/`{{db_binding}}` placeholder tokens. Trimmed to a single `Task` entity + CRUD (no queue/R2/websocket) relative to `examples/cloudflare-worker`, but mirrors its per-context module split (2026-07-03) so a future `comet generate entity/route <Name>` can generate a same-shaped `src/<name>/` module. Cargo.toml template uses the git-dependency form for `comet`/`rocket` documented in the root README (comet isn't on crates.io yet), not a version dependency. Removed the originally-shipped `migrations/0001_init.sql.tmpl` (D1): it collided with `comet migrate init`'s own numbering/content, so migrations are now generated exclusively through `comet migrate init`, never scaffolded statically. |
| B2 | Implement `comet new <name>` | done | Claude 2026-07-03 | `comet-cli/src/commands/new.rs` | `comet new <name> [--path] [--db-binding]` creates the directory, renders all templates, refuses to overwrite an existing directory, validates name/binding as safe identifiers (rejects blank input and path-like/`..` strings), and prints next steps (now including `comet migrate init`). |
| B3 | Add scaffold-freshness test | done | Claude 2026-07-03 | manual verification 2026-07-03 | Manually scaffolded a project via `comet new`, pointed its `comet`/`rocket` deps at this repo's local paths, and ran `RUSTC="$(rustup which rustc)" cargo check --target wasm32-unknown-unknown` — clean pass, no errors, only pre-existing vendored-Rocket warnings. Re-verified after the per-context template split (same result), and `cargo fmt --check` also passes on the generated project. Not yet automated as a `comet-cli` test (needs a way to point the template at a local path in-test without shipping that as the default template) — follow-up left for whoever picks up B3 automation. |
| B4 | Add CLI integration tests for `new` | done | Claude 2026-07-03 | `comet-cli/src/commands/new.rs` (`#[cfg(test)] mod tests`) | Covers generated file contents/substitution, refusal to overwrite an existing directory, and rejection of blank/path-like (`../escape`) names. `cargo test` in `comet-cli/` passes (4 tests); `cargo fmt --check` clean. |

### C. Entity Discovery And Schema Snapshot Model

Goal: give the CLI a way to know "what entities exist" and "what schema state
was last migrated" without requiring runtime reflection in the Worker.

| ID | Task | Status | Owner | Target files | Done when |
| --- | --- | --- | --- | --- | --- |
| C1 | Add owned/serializable manifest types | done | Claude 2026-07-03 | `src/nebula.rs`, `Cargo.toml` | `nebula-schema` feature added (`dep:serde` with `derive`). `schema` submodule in `src/nebula.rs` provides `OwnedColumnDef`/`OwnedIndexDef`/`OwnedForeignKeyDef`/`OwnedTableDef`/`SchemaSnapshot` with `From<&TableDef>`-style conversions and a `leak`-based `SchemaSnapshot::to_manifest()` that hands off to the existing `SchemaManifest::diff` unchanged. Covered by JSON round-trip, leak-equivalence, and diff-reuse tests; `cargo test --no-default-features --features nebula-schema` (33 passed) and full feature matrix (`nebula`, `nebula-schema`, `cloudflare,cloudflare-d1,nebula,nebula-d1`, default) all green; `cargo fmt --check` clean. |
| C2 | Implement `syn`-based entity discovery | done | Claude 2026-07-03 | `comet-cli/src/discover.rs`, `comet-cli/Cargo.toml` | Recursively scans a source directory (sorted traversal for determinism), parses each `.rs` file with `syn::parse_file`, and matches `#[derive(..., Entity)]` structs by the derive path's last segment (so `Entity`, `nebula::Entity`, `comet::nebula::Entity` all match). Computes each struct's module path from its file path (`mod.rs`/`lib.rs`/`main.rs` contribute to the parent module; other files nest one level). Deliberately does **not** parse `#[nebula(...)]` attributes — that logic already exists in `comet-macros`; duplicating it here risks the CLI and the derive macro disagreeing about a struct's schema. Instead, discovery only resolves *which* structs exist and their qualified path, for C3 to reference in a generated file that reads the real, derive-generated `TABLE` const. Also required making entity-bearing context modules `pub mod` (not just `pub` items in a private `mod`) in both `examples/cloudflare-worker/src/lib.rs` and `comet-cli/templates/src_lib.rs.tmpl`, since a separate schema-dump binary can only see a crate's public API — this incidentally also cleared 7 pre-existing `dead_code` warnings in the example (the relationship-only entities are now reachable, not just constructed-nowhere `pub` items). 7 tests passing in `comet-cli`; `cargo fmt --check` clean; re-verified the example (`cargo test --lib`, `cargo check --target wasm32-unknown-unknown`) and a freshly scaffolded CLI project both still green. |
| C3 | Generate a schema-dump runner | done | Claude 2026-07-03 | `comet-cli/src/schema_dump.rs`, `comet-cli/Cargo.toml` | Resolved the cross-crate type-identity risk flagged when this task was scoped: the throwaway crate does **not** declare a fresh `comet` dependency (which risked a second, incompatible package instance). Instead it copies the target project's own `[dependencies].comet` entry verbatim from its `Cargo.toml` (parsed with the new `toml` dependency), adds `"nebula-schema"` to its `features` array, and — critically, found only by testing against the example's `path = "../.."` — resolves any relative `path` to an absolute path against the target project's own directory before writing it into the temp crate's manifest (a path copied verbatim would otherwise resolve against the *temp* directory instead). The temp crate's `main.rs` builds `SchemaManifest::from_entities([<TaskRow as ::comet::nebula::Entity>::TABLE, ...])` (fully-qualified, no imports needed) and prints a `SchemaSnapshot` as JSON via its own `serde_json` dependency (comet-cli added `comet` with `nebula-schema` only, plus `serde_json`/`toml`, as real — not dev — dependencies). Wired into a new `comet migrate status` command (prints discovered entities + current schema; the diff itself is C4/D) so this was verified end-to-end, not just unit-tested: ran against both a freshly scaffolded project and the full `examples/cloudflare-worker` (5 entities, 3 of them relationship-only, one `path = "../.."` dependency) and got correct table/column output both times. 13 tests passing; `cargo fmt --check` clean. |
| C4 | Persist and compare snapshots | done | Claude 2026-07-03 | `comet-cli/src/snapshot.rs` | `snapshot_path()`, `load_snapshot()` (returns `None` if uninitialized), `write_snapshot()`, and `next_migration_sequence()` (scans `migrations/` for the highest `NNNN_*.sql` prefix) all exist and are unit-tested (round-trip, missing-file, sequence numbering). Wired into `comet migrate status`, which now does a real diff — see D. |

### D. Migration Generation (`comet migrate`)

Goal: turn the diff between "current code" and "last migrated snapshot" into
a Wrangler-compatible migration file, reusing the existing safe-diff core.

| ID | Task | Status | Owner | Target files | Done when |
| --- | --- | --- | --- | --- | --- |
| D1 | Implement `comet migrate init` | done | Claude 2026-07-03 | `comet-cli/src/commands/migrate.rs`, `comet-cli/src/cli.rs` | Refuses to run if a snapshot already exists (points at `migrate generate` instead). Otherwise dumps the current schema, builds a `MigrationPlan` from `SchemaManifest::initial_migration()`, writes `migrations/0001_init.sql` via the existing `MigrationPlan::write_sql_file`, and saves the snapshot. Also removed the previously scaffolded static `migrations/0001_init.sql.tmpl` from `comet new` (B1) — it collided with this command's own numbering: a fresh project would get a hand-written `0001_init.sql` from the template *and* a code-generated one from `migrate init`, using plain `CREATE TABLE` (no `IF NOT EXISTS`) that would fail against the table the template's migration already created. `comet new`'s template and printed next-steps now point at `comet migrate init` as the single source of truth for the first migration. |
| D2 | Implement `comet migrate generate <name>` | done | Claude 2026-07-03 | same | Loads the persisted snapshot (bails with a clear message if `migrate init` hasn't run), diffs it against the current schema via the existing `SchemaManifest::diff`, and on a safe non-empty plan writes a numbered/slugged migration file and updates the snapshot. Snapshot is only overwritten after a successful write. |
| D3 | Surface blockers as CLI errors | done | Claude 2026-07-03 | same | `describe_blocker()` renders each `MigrationBlocker` variant as a human-readable sentence (e.g. "change column `tasks.title` (SQLite can't alter a column in place; write this migration by hand)"). `migrate generate` prints all blockers and exits non-zero without writing anything when the plan isn't safe; `migrate status` prints the same descriptions without exiting non-zero (it's a read-only report). |
| D4 | Add `--dry-run` | pending | unassigned | same | The generated SQL and blocker list print to stdout without touching the filesystem or the stored snapshot. Not yet implemented — `migrate status` currently covers the "preview without writing" use case, but doesn't show the exact file name/sequence a `generate` call would use. |
| D5 | Add migration command integration tests | done | Claude 2026-07-03 | manual verification 2026-07-03 (not yet an automated `comet-cli` test — same limitation as B3: needs a way to point a fixture project's `comet` dependency at a local path in-test) | Ran the full cycle against a freshly scaffolded project: `migrate init` (wrote `0001_init.sql` + snapshot) → added a nullable column → `migrate status` showed the pending `ALTER TABLE ... ADD COLUMN` → `migrate generate add_notes` wrote `0002_add_notes.sql` and updated the snapshot → `migrate status` reported up to date. Then made an existing column nullable (a `ChangeColumn` blocker): `migrate status` reported the blocker, and `migrate generate` exited 1, printed the blocker, and wrote no file (confirmed via directory listing and exit code, not just stdout). |

### E. Entity And Route Generation (`comet generate`)

Goal: scaffold new entities and CRUD route modules without hand-rewriting
existing files in fragile ways.

| ID | Task | Status | Owner | Target files | Done when |
| --- | --- | --- | --- | --- | --- |
| E1 | Decide route-wiring strategy | done | Claude 2026-07-03 | docs, `examples/cloudflare-worker/src/{app.rs,tasks/}`, `comet-cli/templates/` | Recorded decision, now implemented in both the example and the CLI template: each context gets its own module (`src/<context>/{mod,model,routes,error}.rs`); `src/app.rs` only imports each context's route functions and mounts them via `routes![...]`. The CLI does not rewrite `app.rs`'s `routes![...]` call for `comet generate route` — it creates the new `src/<name>/` module and prints the one `mount()`/`routes![]` line the developer adds by hand. Chosen over AST-rewriting existing files, which is fragile against manual edits. |
| E2 | Implement `comet generate entity <Name>` | done | Claude 2026-07-03 | `comet-cli/src/commands/generate/entity.rs`, `casing.rs`, `fieldspec.rs`, `rustfile.rs` | `comet generate entity <Name> [--field name:type[:attr,...]]... [--table] [--path]` derives context (`pluralize(snake_case(name))`) and struct name (`PascalCase(name) + "Row"`), auto-adds an `id: i32` primary key unless a field is already named `id` or flagged `primary_key` (a real bug caught and fixed during review — the original check only matched the name `id`, so a custom-named primary key would have silently gotten a second one), and appends the struct to `src/<context>/model.rs` (creating `mod.rs` with `pub mod model;` for a new context). Never rewrites `src/lib.rs`; prints the `pub mod <context>;` line to add only when it's actually missing. `--field` supports type aliases (`string`, `i32`/`int`, `i64`/`bigint`, `f64`/`float`, `bool` → `i32` with a comment — see note below, `bytes`/`blob` → `Vec<u8>`) and attrs (`primary_key`, `auto`, `unique`, `index`, `nullable`, `default=`, `rename=`, `foreign_key=table.column`). `bool` is deliberately mapped to `i32`, not Rust `bool`: D1/SQLite has no boolean storage class and a `bool`-typed field can fail to deserialize a raw `0`/`1` row, the same constraint `examples/cloudflare-worker`'s `TaskRow::done` documents. |
| E3 | Implement `comet generate route <Entity>` | done | Claude 2026-07-03 | `comet-cli/src/commands/generate/route.rs`, `entity_introspect.rs` | Reads the target entity's fields back out of its already-generated `model.rs` with `syn` (`entity_introspect.rs`, reusing C2's `has_entity_derive`) instead of asking the caller to redeclare the shape — keeps a single source of truth. Determines the primary-key column and the "creatable" fields (everything not `primary_key`/`auto`), generates a companion `New<Entity>` input struct (appended to `model.rs`), and writes `list/get/create/update/delete` handlers plus `error.rs` to `src/<context>/`. Refuses to overwrite an existing `routes.rs`. Bails with a `generate entity` pointer if the entity doesn't exist yet. Prints the `use` + `routes![...]` lines for `src/app.rs` rather than rewriting it, per E1. |
| E4 | Add generator golden-file tests | done | Claude 2026-07-03 | `comet-cli/src/commands/generate/{entity,route}.rs` (`#[cfg(test)]`), manual verification 2026-07-03 | 7 unit tests cover scaffolding, duplicate refusal, explicit-id/table overrides, and the primary-key-name bug above. End-to-end, not just unit-tested: scaffolded a project, ran `generate entity Board --field title:string --field org_id:i64:foreign_key=orgs.id,index`, then `generate route Board`, manually wired the two printed lines into `lib.rs`/`app.rs`, and confirmed `cargo check --target wasm32-unknown-unknown` and `cargo fmt --check` both pass clean on the generated `boards` module. No separate golden-file fixtures committed (`comet-cli/tests/generate.rs`) — same gap as B3/D5: needs a way to point a fixture project's `comet` dependency at a local path in an automated test. |

### F. Test Orchestration (`comet test`)

Goal: make the existing, already-documented Nebula MVP release gate a single
command instead of a checklist a human has to run by hand.

| ID | Task | Status | Owner | Target files | Done when |
| --- | --- | --- | --- | --- | --- |
| F1 | Implement `comet test unit` | done | Claude 2026-07-03 | `comet-cli/src/commands/test.rs` | Runs `cargo fmt --check` then `cargo test --lib` directly (not through `npm`, so unit tests don't need a Node toolchain). Rescoped from the original wording (`--features nebula`, `--no-default-features --features nebula`): those are `comet`-repo-specific flags from the Nebula release gate, meaningless as a blanket default for an arbitrary downstream project — a scaffolded project's own `Cargo.toml` already pins whatever features it needs, so a plain `cargo test --lib` is the right generic command. |
| F2 | Implement `comet test integration` | done | Claude 2026-07-03 | same | Runs the project's own `npm run test:integration` (streaming output live via `Command::status`, not captured) rather than reimplementing `wrangler dev` orchestration — that's exactly what `examples/cloudflare-worker/tests/integration.sh` already does, and the CLI shouldn't duplicate it generically. `comet new`'s scaffolded `package.json` doesn't define this script (a fresh project has nothing to integration-test yet), so running this today fails loudly with npm's own "Missing script" error — verified end-to-end, not just described. |
| F3 | Implement `comet test perf` | done | Claude 2026-07-03 | same | Same shape as F2, running `npm run test:perf`. |
| F4 | Implement `comet test all` | done | Claude 2026-07-03 | same | Composes F1→F2→F3, stopping at the first failure. Verified end-to-end against a freshly scaffolded project: `unit` ran and passed (0 tests, fmt clean), `integration` failed with npm's missing-script error and returned exit code 1, and `perf` was confirmed never invoked (grepped the captured output for `test:perf`: zero occurrences). |

### G. Documentation And Release

Goal: make the CLI installable and discoverable once the areas above reach
MVP.

| ID | Task | Status | Owner | Target files | Done when |
| --- | --- | --- | --- | --- | --- |
| G1 | Write `comet-cli` README | done | Claude 2026-07-03 | `comet-cli/README.md` | Covers install (via git checkout + `cargo install --path`, matching `comet`'s own not-yet-on-crates.io status — not the aspirational `cargo install comet-cli` originally written here), quick start for all five capabilities (new/entity/route/migrate/test), and the raw-SQL/manual-wiring/hand-written-migration/non-generated-context escape hatches. |
| G2 | Cross-link trackers | done | Claude 2026-07-03 | `docs/nebula-implementation-tracker.md` | Task E4 ("a standalone CLI wrapper remains future work") updated to record that it shipped 2026-07-03 as `comet-cli`, pointing at this tracker. |
| G3 | Define CLI release gate | done | Claude 2026-07-03 | this file (see below) | Analogous "release gate" section added below, covering both crates (`comet-cli` itself, and a scaffolded fixture project). |

## Comet CLI Release Gate

Mirrors the "Nebula MVP release gate" in `docs/nebula-implementation-tracker.md`,
scoped to `comet-cli`:

```sh
# comet-cli itself
cd comet-cli
cargo fmt --check
cargo test

# A scaffolded fixture project exercises every command against real output,
# not just comet-cli's own unit tests. Point the fixture's `comet`/`rocket`
# dependencies at this repo's local paths first (git deps require a public
# remote and won't reflect uncommitted core changes):
comet new fixture --path /tmp/fixture
# edit /tmp/fixture/Cargo.toml: comet/rocket -> path = "<this repo>"/"<this repo>/vendor/rocket/core/lib"
comet migrate init --path /tmp/fixture
comet generate entity Board --field title:string --path /tmp/fixture
comet generate route Board --path /tmp/fixture
# wire the printed lines into /tmp/fixture/src/lib.rs and src/app.rs by hand
comet migrate generate add_boards --path /tmp/fixture
cd /tmp/fixture
RUSTC="$(rustup which rustc)" cargo check --target wasm32-unknown-unknown
cargo fmt --check
comet test unit --path /tmp/fixture
```

All of the above passed as of 2026-07-03 (see the `done` rows in areas B–F
for exactly what each step verified). Not yet automated as a single script
or CI job — every run so far has been manual, documented inline in this
tracker as it happened. `cargo publish --dry-run` is blocked on the same
issue blocking `comet` itself: the vendored Rocket fork has no public,
versioned home yet (see the root README and
`docs/rocket-worker-roadmap.md`), so packaging would silently swap in
unpatched Rocket. `cargo clippy` was not run as part of this gate — it's a
reasonable follow-up but wasn't part of any task's done-when criteria to
date.

## Suggested Build Order

C1 (core snapshot type) unblocks C3/C4, which unblock D. A and B can proceed
in parallel with C since scaffolding doesn't depend on schema discovery. E
depends on A (packaging/templating groundwork) and C2 (discovery) but not on
D. F depends on nothing but the existing example scripts and can be built
first if a quick win is wanted.

1. A (decisions) → B (scaffolding) in parallel with C1–C2 (core + discovery).
2. C3–C4 (schema dump + snapshot) → D (migration generation).
3. E (generation) once A and C2 land.
4. F (test orchestration) any time — lowest risk, no new core changes.
5. G once B–F reach MVP.
