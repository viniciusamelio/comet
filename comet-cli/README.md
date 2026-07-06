# comet-cli

Scaffold Comet + Nebula + Cloudflare Worker projects, generate entities and
CRUD routes, drive Nebula migration generation, and run a project's
test/release gate — all from one binary (`comet`).

Full command reference: [`docs/comet-cli-roadmap.md`](../docs/comet-cli-roadmap.md).
Implementation status and design notes: [`docs/comet-cli-tracker.md`](../docs/comet-cli-tracker.md).

## Install

`comet-cli` isn't published to crates.io yet — same situation as the `comet`
crate itself (see the root [README](../README.md#using-this-in-another-project)).
Until then, `cargo install` can build it straight from the git repo, no
manual clone needed — Cargo resolves the `comet = { path = ".." }`
dependency correctly because `--git` clones the whole repository, not just
the `comet-cli` subdirectory:

```sh
cargo install --git https://github.com/viniciusamelio/comet comet-cli
```

This installs a `comet` binary on your `PATH` (`cargo install` prints the
directory if it isn't already there — typically `~/.cargo/bin`). Verify with
`comet --help`.

Prefer a local build from a checkout instead (e.g. to test uncommitted
changes)? `cd comet-cli && cargo install --path .` works the same way.

## Quick start

```sh
# Scaffold a new project
comet new my_app
cd my_app

# Generate the first migration + schema snapshot
comet migrate init
npm install

# Add an entity and its CRUD routes
comet generate entity Board --field title:string --field org_id:i64:foreign_key=orgs.id,index
comet generate route Board
# ...then add the two printed lines to src/lib.rs and src/app.rs

# Pick up the schema change
comet migrate generate add_boards

# Run the project's tests
comet test unit
```

`comet new`'s generated `Cargo.toml` depends on `comet`/`rocket` via git, for
the same reason described in the root README: the patched Rocket fork isn't
published anywhere else yet.

## Escape hatches

Every command this CLI generates is meant to be edited by hand afterward —
none of it is meant to be the only way to write the code it produces:

- **Raw SQL.** `comet::cloudflare::D1<B>` derefs to `worker::D1Database`, so
  any route can drop to `db.prepare(sql).bind(...)` for a query the Nebula
  builder API doesn't fit. Keep it parameterized — never build SQL by
  concatenating request input. See `docs/nebula-roadmap.md`.
- **Manual route/module wiring.** `comet generate route` never edits
  `src/app.rs`, and `comet generate entity` never edits `src/lib.rs` — both
  print the one or two lines you add by hand. This is deliberate (see
  `comet-cli-tracker.md`, task E1): rewriting a file you're also hand-editing
  is exactly the kind of codegen that breaks the first time someone doesn't
  follow the generator's assumptions.
- **Hand-written migrations.** `comet migrate generate` refuses to guess
  through destructive or ambiguous schema changes (dropped/changed columns,
  changed indexes or foreign keys, a new non-nullable column with no
  default). It prints each blocker and exits non-zero; write the migration
  SQL by hand in that case, then update `migrations/.comet-schema.json`'s
  corresponding table to match once applied.
- **Non-generated contexts.** Nothing requires every module to be
  CLI-generated — `examples/cloudflare-worker` hand-writes several entities
  and demo routes (R2, WebSocket, queue) alongside a `comet new`-shaped
  `tasks/` context. Generate what's repetitive, write the rest by hand.
