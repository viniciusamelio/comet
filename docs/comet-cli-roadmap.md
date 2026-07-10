# Comet CLI Roadmap

`comet-cli` (binary name `comet`) scaffolds Comet + Nebula + Cloudflare
Worker projects, generates entities and CRUD routes, drives Nebula migration
generation, inspects routes for typed RPC clients, and orchestrates a
project's test/release gate. It lives in this
repo, outside the Cargo workspace (same precedent as `comet-macros`), and
releases in lockstep with the `comet` crate — a given `comet-cli` version
always pairs with the `comet` core version it was built and tested against.

This document is the command reference. See the source under `comet-cli/src/`
for the design decisions behind each command (entity discovery via `syn`,
the schema-snapshot format, the route-wiring strategy).

## Command reference

Every command accepts `--path <dir>` to operate on a project directory other
than the current one (`comet new` uses `--path` for where to create the
project instead, since there's nothing to operate *on* yet).

Exit-code convention: `0` on success, non-zero on any failure — including
partial/ambiguous states the CLI won't guess through (an unsafe schema
change, a missing baseline snapshot, a missing entity). Commands never
succeed silently when they didn't do what was asked; unimplemented command
surface (there is none as of this writing — see the tracker's Snapshot
table) fails loudly with a pointer to the tracker area, rather than
exiting 0 having done nothing.

### `comet new <name>`

Scaffolds a new project.

```
comet new <name> [--path <dir>] [--db-binding <NAME>]
```

- `<name>`: project name; also the crate name (via Cargo's hyphen→underscore
  rule) and the D1 database name. Must start with a letter and contain only
  ASCII letters, digits, `-`, or `_` (rejects blank input and path-like/`..`
  strings, since it also becomes a directory name).
- `--path`: directory to create the project in. Defaults to `./<name>`.
  Fails if the directory already exists.
- `--db-binding`: the Wrangler D1 binding name used in `wrangler.jsonc` and
  route code. Defaults to `DB`.

Prints the files it wrote and next steps (`comet migrate init`, `npm
install`, `npm run dev`). See "Generated project layout" below for exactly
what gets created.

### `comet generate entity <Name>`

Adds a `#[derive(Entity)]` struct to an existing project.

```
comet generate entity <Name> [--field <spec>]... [--table <name>] [--path <dir>]
```

- `<Name>`: singular concept name, e.g. `Board`. Generates `BoardRow` in
  `src/boards/model.rs` (context = `pluralize(snake_case(Name))`, struct =
  `PascalCase(Name) + "Row"`).
- `--field <spec>` (repeatable): `name:type[:attr[,attr]...]`. Grammar:
  - `type` is one of `string`/`text`, `i32`/`int`/`integer`,
    `i64`/`bigint`, `f64`/`float`/`real`, `bool`/`boolean`, `bytes`/`blob`.
    **`bool` maps to Rust `i32`, not `bool`** — D1/SQLite has no boolean
    storage class, and a `bool`-typed field can fail to deserialize a raw
    `0`/`1` row. This is the same constraint
    `examples/cloudflare-worker/src/tasks/model.rs`'s `TaskRow::done`
    documents; the generated field gets a comment noting it.
  - `attr` is a bare flag (`primary_key`, `auto`/`auto_increment`, `unique`,
    `index`/`indexed`, `nullable`) or `key=value`
    (`default=0`, `rename=foo`, `foreign_key=table.column`).
  - Example: `--field org_id:i64:foreign_key=orgs.id,index`.
- An `id: i32` primary key (`primary_key, auto, unique, index`) is added
  automatically unless a field named `id` or flagged `primary_key` is
  already given.
- `--table`: table name override. Defaults to the same pluralized
  snake_case used for the context/module name.

Never edits `src/lib.rs`. If the module needs `pub mod <context>;` added and
it isn't already there, the command prints that line instead of writing it —
consistent with `generate route` below never editing `src/app.rs`. Refuses
to run if the struct already exists (no overwriting).

### `comet generate route <Entity>`

Adds CRUD routes for an existing entity.

```
comet generate route <Entity> [--db-binding <NAME>] [--path <dir>]
```

- `<Entity>`: same concept name used with `generate entity`. Reads the
  entity's fields back out of its already-generated `model.rs` with `syn`
  (not redeclared on the command line), so the two can't drift apart.
- Generates `list_<context>`, `get_<concept>`, `create_<concept>`,
  `update_<concept>`, `delete_<concept>` in `src/<context>/routes.rs`, an
  `ApiError`/`ApiResult` pair in `src/<context>/error.rs`, and a companion
  `New<Entity>` input struct (every field except the primary key and any
  `auto` field) appended to `model.rs`.
- Fails if the entity doesn't exist yet (points at `generate entity`), or if
  `routes.rs` already exists (no overwriting).
- Never edits `src/app.rs`. Prints the `use` line and the route names to add
  to the `routes![...]` list instead.

### `comet migrate init`

Generates the first migration and the baseline schema snapshot.

```
comet migrate init [--path <dir>]
```

Fails if `migrations/.comet-schema.json` already exists (the project is
already initialized — use `generate` instead). Otherwise discovers entities,
reads their real schema by compiling and running a throwaway crate (see the
tracker's C3 for why), writes `migrations/0001_init.sql`, and saves the
snapshot.

### `comet migrate generate <name>`

Diffs the current entities against the last saved snapshot and writes a new
migration for the safe, additive changes found.

```
comet migrate generate <name> [--path <dir>]
```

- Fails if no snapshot exists yet (run `migrate init` first).
- If the diff includes anything unsafe or ambiguous (dropped/changed
  columns, changed indexes or foreign keys, an added column with no default
  on a non-nullable field) it prints each blocker as a human-readable
  sentence and **exits non-zero without writing anything** — these need a
  hand-written migration.
- If there's nothing to do, it says so and exits `0` without writing.
- Otherwise it writes `migrations/NNNN_<name>.sql` (sequence = one past the
  highest existing migration number) and updates the snapshot — only after
  the file write succeeds.

### `comet migrate status`

Read-only: prints the current schema and, if a baseline snapshot exists,
the pending changes (or blockers) against it. Never writes anything. Exits
`0` even when changes are pending — it's a report, not a gate; use `migrate
generate` to act on what it shows.

```
comet migrate status [--path <dir>]
```

### `comet rpc manifest`

Discovers Rocket route functions under `src/` and emits a machine-readable
RPC manifest.

```
comet rpc manifest [--path <dir>] [--out <file>]
```

The manifest includes each route's function name, module path, source file,
HTTP method, mounted path when it can be inferred from `rocket.mount(...)`,
path/query parameters, `Json<T>` request/response types, auth metadata, and
support classification:

- `json`: typed client generation supports this route.
- `raw`: route shape is recognized, but it uses non-JSON body/response data.
- `unsupported`: route is visible, but not currently safe to generate.

Routes guarded by `AuthSession`/`AuthorizedSession<T>` or
`#[comet_auth::requires_auth(...)]` are marked as authenticated. When a
project has a `Cargo.toml` and a `comet-auth` dependency, policy guards can
be compiled in a temporary crate so their roles, permissions, scopes,
resource, and authorization mode are included in the manifest.

### `comet rpc generate`

Generates a typed client for JSON-supported routes.

```
comet rpc generate --lang <ts|dart|rust> [--path <dir>] [--out <file>]
```

The generator walks the same manifest, discovers referenced public structs
and unit enums under `src/`, and emits client-side type declarations plus a
`CometClient`. Raw and unsupported routes are intentionally omitted from the
generated client.

Generated clients expect JSON HTTP endpoints and bearer-token auth when a
route is authenticated. The Rust client uses `reqwest`, `serde`,
`serde_json`, `thiserror`, and `percent-encoding`; the Dart client uses
`package:http/http.dart`; the TypeScript client only relies on `fetch`.

### `comet test unit` / `integration` / `perf` / `all`

```
comet test unit [--path <dir>]
comet test integration [--path <dir>]
comet test perf [--path <dir>]
comet test all [--path <dir>]
```

- `unit`: `cargo fmt --check` then `cargo test --lib`, run directly (not
  through `npm`) so unit tests don't need a Node toolchain.
- `integration`: the project's own `npm run test:integration` (e.g.
  `examples/cloudflare-worker/tests/integration.sh`, which drives `wrangler
  dev` itself — the CLI doesn't reimplement that).
- `perf`: the project's own `npm run test:perf`.
- `all`: `unit` → `integration` → `perf`, stopping at the first failure.

All four stream the underlying command's stdout/stderr live rather than
buffering it, and fail with the exact command line and exit status on a
non-zero exit — including npm's own "Missing script" error if a scaffolded
project hasn't defined `test:integration`/`test:perf` yet, which is the
honest state for a project fresh out of `comet new`.

## Generated project layout

`comet new` and `examples/cloudflare-worker` (the hand-maintained reference
app) share the same shape, so the example stays a valid fixture for
scaffolding-related tests:

```
<project>/
  Cargo.toml          # comet/rocket as git deps (or path, inside this repo)
  wrangler.jsonc       # D1 binding, build command
  package.json         # dev/deploy/check/test npm scripts
  README.md
  .gitignore
  src/
    lib.rs             # mod declarations only; entity-bearing contexts are `pub mod`
    entry.rs           # #[cfg(target_arch = "wasm32")] Worker glue
    app.rs             # mounts every context's routes; nothing else
    <context>/
      mod.rs           # pub mod {model, routes, error} — only what exists
      model.rs         # #[derive(Entity)] struct(s) + any New<X> input structs
      routes.rs         # CRUD handlers (only if `generate route` has run)
      error.rs          # ApiError/ApiResult (only if `generate route` has run)
  migrations/
    NNNN_<name>.sql     # created by `migrate init`/`generate`, never scaffolded statically
    .comet-schema.json  # snapshot `migrate generate`/`status` diff against
```

Two rules this layout depends on, both load-bearing for how the CLI works
rather than stylistic:

- **Entity-bearing context modules must be `pub mod`, not `mod`.** `comet
  migrate`'s schema-dump runner (tracker C3) compiles a separate crate that
  only sees the project's public API; a private `mod tasks;` would make
  `TaskRow` unreachable from outside and every `migrate`/`status` call would
  fail.
- **`migrations/` only ever contains CLI-generated files.** `comet new`
  deliberately does not scaffold a static `migrations/0001_init.sql` — an
  earlier version of the template did, and it collided with `migrate init`'s
  own numbering and content (the template's `CREATE TABLE IF NOT EXISTS`
  vs. the generated plain `CREATE TABLE`, both claiming sequence `0001`).
  Migrations are generated exclusively through `comet migrate`, so there's
  exactly one source of truth for what's been applied.

`examples/cloudflare-worker` has more than `comet new` scaffolds by default
(R2 assets, a queue-backed task-events demo, a WebSocket echo route, five
entities including relationship-only ones) — those are hand-added
demonstrations of Comet features beyond what a fresh project needs, not
something `comet new`/`generate` produce today.
