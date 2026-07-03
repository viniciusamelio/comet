use comet::nebula::{BelongsTo, HasMany, belongs_to, has_many};
use rocket::serde::{Deserialize, Serialize};

/// A task as returned to API clients.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(crate = "rocket::serde")]
pub struct Task {
    pub id: i32,
    pub title: String,
    pub done: bool,
    pub created_at: String,
}

/// The shape of a task row as it comes back from D1.
///
/// D1/SQLite has no boolean storage class, so `done` round-trips as an
/// integer. This type exists to isolate that quirk from the `Task` type we
/// actually serve to clients.
#[derive(Debug, Clone, Deserialize, comet::nebula::Entity)]
#[nebula(table = "tasks")]
#[serde(crate = "rocket::serde")]
pub struct TaskRow {
    #[nebula(primary_key, auto, unique, index)]
    pub id: i32,
    pub title: String,
    #[nebula(default = "0")]
    pub done: i32,
    #[nebula(default = "datetime('now')")]
    pub created_at: String,
}

impl From<TaskRow> for Task {
    fn from(row: TaskRow) -> Self {
        Task {
            id: row.id,
            title: row.title,
            done: row.done != 0,
            created_at: row.created_at,
        }
    }
}

#[derive(Debug, Clone, Deserialize, comet::nebula::Entity)]
#[nebula(table = "orgs")]
#[serde(crate = "rocket::serde")]
pub struct OrgRow {
    #[nebula(primary_key, auto, unique, index)]
    pub id: i32,
    pub name: String,
}

#[derive(Debug, Clone, Deserialize, comet::nebula::Entity)]
#[nebula(table = "users")]
#[serde(crate = "rocket::serde")]
pub struct UserRow {
    #[nebula(primary_key, auto, unique, index)]
    pub id: i32,
    #[nebula(foreign_key = "orgs.id", index)]
    pub org_id: i32,
    pub email: String,
}

impl UserRow {
    pub const ORG: BelongsTo<UserRow, OrgRow, i32> = belongs_to(Self::ORG_ID, OrgRow::ID);
}

#[derive(Debug, Clone, Deserialize, comet::nebula::Entity)]
#[nebula(table = "boards")]
#[serde(crate = "rocket::serde")]
pub struct BoardRow {
    #[nebula(primary_key, auto, unique, index)]
    pub id: i32,
    #[nebula(foreign_key = "orgs.id", index)]
    pub org_id: i32,
    pub title: String,
}

impl BoardRow {
    pub const ORG: BelongsTo<BoardRow, OrgRow, i32> = belongs_to(Self::ORG_ID, OrgRow::ID);
    pub const TASKS: HasMany<BoardRow, BoardTaskRow, i32> =
        has_many(Self::ID, BoardTaskRow::BOARD_ID);
}

#[derive(Debug, Clone, Deserialize, comet::nebula::Entity)]
#[nebula(table = "board_tasks")]
#[serde(crate = "rocket::serde")]
pub struct BoardTaskRow {
    #[nebula(primary_key, auto, unique, index)]
    pub id: i32,
    #[nebula(foreign_key = "boards.id", index)]
    pub board_id: i32,
    #[nebula(foreign_key = "tasks.id", index)]
    pub task_id: i32,
    #[nebula(foreign_key = "users.id", index)]
    pub assignee_user_id: i32,
}

impl BoardTaskRow {
    pub const BOARD: BelongsTo<BoardTaskRow, BoardRow, i32> =
        belongs_to(Self::BOARD_ID, BoardRow::ID);
    pub const TASK: BelongsTo<BoardTaskRow, TaskRow, i32> =
        belongs_to(Self::TASK_ID, TaskRow::ID);
    pub const ASSIGNEE: BelongsTo<BoardTaskRow, UserRow, i32> =
        belongs_to(Self::ASSIGNEE_USER_ID, UserRow::ID);
}

/// Body accepted by `POST /tasks`.
#[derive(Debug, Clone, Deserialize)]
#[serde(crate = "rocket::serde")]
pub struct NewTask {
    pub title: String,
}

impl NewTask {
    /// Trims the title and rejects blank input.
    pub fn validated_title(&self) -> Result<&str, &'static str> {
        let title = self.title.trim();
        if title.is_empty() {
            Err("title must not be blank")
        } else {
            Ok(title)
        }
    }
}

/// The kind of lifecycle event recorded for a task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(crate = "rocket::serde", rename_all = "snake_case")]
pub enum TaskEventKind {
    Created,
    Completed,
}

impl TaskEventKind {
    pub fn as_str(self) -> &'static str {
        match self {
            TaskEventKind::Created => "created",
            TaskEventKind::Completed => "completed",
        }
    }
}

/// The message shape published to the `TASK_EVENTS` queue and consumed by
/// the queue handler.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(crate = "rocket::serde")]
pub struct TaskEvent {
    pub task_id: i32,
    pub kind: TaskEventKind,
}

#[cfg(test)]
mod tests {
    use super::*;
    use comet::nebula::{Entity as _, SchemaLint, SchemaManifest, Value};
    use rocket::serde::json;

    #[test]
    fn task_row_maps_integer_done_to_bool() {
        let row = TaskRow {
            id: 1,
            title: "write tests".into(),
            done: 1,
            created_at: "2026-07-01T00:00:00Z".into(),
        };

        let task: Task = row.into();

        assert!(task.done);
        assert_eq!(task.id, 1);
        assert_eq!(task.title, "write tests");
    }

    #[test]
    fn task_row_zero_is_not_done() {
        let row = TaskRow {
            id: 2,
            title: "pending".into(),
            done: 0,
            created_at: "2026-07-01T00:00:00Z".into(),
        };

        let task: Task = row.into();

        assert!(!task.done);
    }

    #[test]
    fn task_serializes_done_as_json_boolean() {
        let task = Task {
            id: 7,
            title: "ship it".into(),
            done: true,
            created_at: "2026-07-01T00:00:00Z".into(),
        };

        let json = json::to_string(&task).unwrap();

        assert!(json.contains(r#""done":true"#), "got: {json}");
    }

    #[test]
    fn new_task_rejects_blank_title() {
        let blank = NewTask {
            title: "   ".into(),
        };

        assert_eq!(blank.validated_title(), Err("title must not be blank"));
    }

    #[test]
    fn new_task_trims_title() {
        let padded = NewTask {
            title: "  buy milk  ".into(),
        };

        assert_eq!(padded.validated_title(), Ok("buy milk"));
    }

    #[test]
    fn new_task_deserializes_from_json_body() {
        let parsed: NewTask = json::from_str(r#"{"title":"read a book"}"#).unwrap();

        assert_eq!(parsed.title, "read a book");
    }

    #[test]
    fn task_event_round_trips_through_json() {
        let event = TaskEvent {
            task_id: 42,
            kind: TaskEventKind::Completed,
        };

        let encoded = json::to_string(&event).unwrap();
        let decoded: TaskEvent = json::from_str(&encoded).unwrap();

        assert_eq!(decoded.task_id, 42);
        assert_eq!(decoded.kind, TaskEventKind::Completed);
        assert!(encoded.contains(r#""kind":"completed""#), "got: {encoded}");
    }

    #[test]
    fn relationship_metadata_is_indexed_and_deterministic() {
        let manifest = SchemaManifest::new([
            OrgRow::TABLE,
            UserRow::TABLE,
            BoardRow::TABLE,
            TaskRow::TABLE,
            BoardTaskRow::TABLE,
        ]);

        assert_eq!(manifest.lint(), Vec::<SchemaLint>::new());
        assert_eq!(UserRow::TABLE.foreign_keys[0].references_table, "orgs");
        assert_eq!(BoardRow::TABLE.foreign_keys[0].references_table, "orgs");
        assert_eq!(BoardTaskRow::TABLE.foreign_keys.len(), 3);
    }

    #[test]
    fn relationship_helpers_build_explicit_selects() {
        let org_statement = BoardRow::ORG.select_parent(3).to_statement();
        assert_eq!(
            org_statement.sql,
            "SELECT \"id\", \"name\" FROM \"orgs\" WHERE \"orgs\".\"id\" = ? LIMIT ?"
        );
        assert_eq!(
            org_statement.binds,
            vec![Value::Integer(3), Value::Integer(1)]
        );

        let board_tasks_statement = BoardRow::TASKS
            .select_children(9)
            .order_by(BoardTaskRow::ID.asc())
            .limit(25)
            .to_statement();
        assert_eq!(
            board_tasks_statement.sql,
            "SELECT \"id\", \"board_id\", \"task_id\", \"assignee_user_id\" FROM \"board_tasks\" \
             WHERE \"board_tasks\".\"board_id\" = ? ORDER BY \"board_tasks\".\"id\" ASC LIMIT ?"
        );
        assert_eq!(
            board_tasks_statement.binds,
            vec![Value::Integer(9), Value::Integer(25)]
        );
    }
}
