#!/usr/bin/env bash
# End-to-end test against the real `comet` binary: scaffolds a project,
# points its `comet`/`rocket` dependencies at this repo checkout (git deps
# would need a public remote and wouldn't reflect uncommitted core changes),
# then drives it through the full migrate/generate/test command surface and
# checks the result actually compiles and is `cargo fmt`-clean. This is the
# manual verification cycle from `docs/comet-cli-tracker.md` (see the
# Comet CLI Release Gate section), scripted so CI can run it.
#
# Requires: rustup with the wasm32-unknown-unknown target, python3 (used for
# small structured edits to generated Rust files, same as `sed` elsewhere in
# this repo's test scripts but without the escaping headaches of matching
# Rust source).
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
CLI_DIR="$REPO_ROOT/comet-cli"
FIXTURE_PARENT="$(mktemp -d)"
FIXTURE="$FIXTURE_PARENT/fixture"

cleanup() {
  rm -rf "$FIXTURE_PARENT"
}
trap cleanup EXIT

echo "Building comet-cli..."
cargo build --manifest-path "$CLI_DIR/Cargo.toml" --quiet
COMET="$CLI_DIR/target/debug/comet"

echo "Scaffolding a fixture project..."
"$COMET" new fixture --path "$FIXTURE" --db-binding DB >/dev/null

python3 - "$FIXTURE/Cargo.toml" "$REPO_ROOT" <<'PY'
import sys
path, repo_root = sys.argv[1], sys.argv[2]
content = open(path).read()
content = content.replace(
    'comet = { git = "https://github.com/viniciusamelio/comet"',
    f'comet = {{ path = "{repo_root}"',
)
content = content.replace(
    'rocket = { git = "https://github.com/viniciusamelio/comet"',
    f'rocket = {{ path = "{repo_root}/vendor/rocket/core/lib"',
)
open(path, "w").write(content)
PY

echo "Running migrate init..."
"$COMET" migrate init --path "$FIXTURE" >/dev/null
test -f "$FIXTURE/migrations/0001_init.sql"
test -f "$FIXTURE/migrations/.comet-schema.json"

echo "Adding a nullable column and checking migrate status detects it..."
python3 - "$FIXTURE/src/tasks/model.rs" <<'PY'
import sys
path = sys.argv[1]
content = open(path).read()
marker = '    #[nebula(default = "datetime(\'now\')")]\n    pub created_at: String,\n}'
assert content.count(marker) == 1, "TaskRow shape changed; update this fixture edit"
content = content.replace(
    marker,
    marker[:-1] + '    #[nebula(nullable)]\n    pub notes: String,\n}',
)
open(path, "w").write(content)
PY

status_output="$("$COMET" migrate status --path "$FIXTURE")"
echo "$status_output" | grep -q 'ALTER TABLE "tasks" ADD COLUMN "notes"'

echo "Running migrate generate..."
"$COMET" migrate generate add_notes --path "$FIXTURE" >/dev/null
test -f "$FIXTURE/migrations/0002_add_notes.sql"
grep -q 'ADD COLUMN "notes"' "$FIXTURE/migrations/0002_add_notes.sql"

status_output="$("$COMET" migrate status --path "$FIXTURE")"
echo "$status_output" | grep -q "up to date"

echo "Checking a destructive change is blocked, not silently applied..."
python3 - "$FIXTURE/src/tasks/model.rs" <<'PY'
import sys
path = sys.argv[1]
content = open(path).read()
marker = "    pub title: String,\n    #[nebula(default = \"0\")]"
assert marker in content, "TaskRow shape changed; update this fixture edit"
content = content.replace(
    marker,
    "    #[nebula(nullable)]\n    pub title: String,\n    #[nebula(default = \"0\")]",
)
open(path, "w").write(content)
PY

if "$COMET" migrate generate change_title --path "$FIXTURE" >/tmp/comet-e2e-blocker.log 2>&1; then
  echo "not ok - migrate generate should have refused a destructive change"
  cat /tmp/comet-e2e-blocker.log
  exit 1
fi
grep -q "change column" /tmp/comet-e2e-blocker.log
test ! -f "$FIXTURE/migrations/0003_change_title.sql"
rm -f /tmp/comet-e2e-blocker.log

# Revert the blocked change so the fixture keeps compiling for later steps.
python3 - "$FIXTURE/src/tasks/model.rs" <<'PY'
import sys
path = sys.argv[1]
content = open(path).read()
content = content.replace(
    "    #[nebula(nullable)]\n    pub title: String,\n    #[nebula(default = \"0\")]",
    "    pub title: String,\n    #[nebula(default = \"0\")]",
)
open(path, "w").write(content)
PY

echo "Generating a new entity and its CRUD routes..."
"$COMET" generate entity Board \
  --field "title:string" \
  --field "org_id:i64:foreign_key=orgs.id,index" \
  --path "$FIXTURE" >/dev/null
test -f "$FIXTURE/src/boards/model.rs"

"$COMET" generate route Board --db-binding DB --path "$FIXTURE" >/dev/null
test -f "$FIXTURE/src/boards/routes.rs"
test -f "$FIXTURE/src/boards/error.rs"

echo "Wiring the generated module into lib.rs/app.rs..."
python3 - "$FIXTURE/src/lib.rs" <<'PY'
import sys
path = sys.argv[1]
content = open(path).read()
content = content.replace("pub mod tasks;\n", "pub mod boards;\npub mod tasks;\n")
open(path, "w").write(content)
PY

python3 - "$FIXTURE/src/app.rs" <<'PY'
import sys
path = sys.argv[1]
content = open(path).read()
content = content.replace(
    "use crate::tasks::routes::{complete_task, create_task, get_task, list_tasks};\n",
    "use crate::boards::routes::{create_board, delete_board, get_board, list_boards, update_board};\n"
    "use crate::tasks::routes::{complete_task, create_task, get_task, list_tasks};\n",
)
content = content.replace(
    "routes![index, list_tasks, get_task, create_task, complete_task],",
    "routes![\n"
    "            index,\n"
    "            list_tasks,\n"
    "            get_task,\n"
    "            create_task,\n"
    "            complete_task,\n"
    "            list_boards,\n"
    "            get_board,\n"
    "            create_board,\n"
    "            update_board,\n"
    "            delete_board\n"
    "        ],",
)
open(path, "w").write(content)
PY

echo "Checking the generated project actually compiles for wasm32..."
(
  cd "$FIXTURE"
  RUSTC="$(rustup which rustc)" cargo check --target wasm32-unknown-unknown --quiet
  cargo fmt --check
)

echo "Running comet test unit..."
"$COMET" test unit --path "$FIXTURE" >/dev/null

echo "Checking comet test integration fails loudly on a script the scaffold doesn't define..."
if "$COMET" test integration --path "$FIXTURE" >/tmp/comet-e2e-integration.log 2>&1; then
  echo "not ok - test integration should have failed (no test:integration script)"
  cat /tmp/comet-e2e-integration.log
  exit 1
fi
grep -qi "missing script" /tmp/comet-e2e-integration.log
rm -f /tmp/comet-e2e-integration.log

echo "ok - comet-cli end-to-end cycle passed"
