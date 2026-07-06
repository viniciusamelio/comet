use comet::nebula::{belongs_to, BelongsTo};
use rocket::serde::Deserialize;

use crate::boards::model::BoardRow;
use crate::tasks::model::TaskRow;
use crate::users::model::UserRow;

/// The assignment of a task to a board, owned by a specific user.
///
/// Lives alongside `BoardRow` rather than under `tasks` or `users` because
/// it's a join entity whose identity is the board assignment itself, not a
/// property of any single one of the three tables it references.
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
    pub const TASK: BelongsTo<BoardTaskRow, TaskRow, i32> = belongs_to(Self::TASK_ID, TaskRow::ID);
    pub const ASSIGNEE: BelongsTo<BoardTaskRow, UserRow, i32> =
        belongs_to(Self::ASSIGNEE_USER_ID, UserRow::ID);
}

#[cfg(test)]
mod tests {
    use super::*;
    use comet::nebula::{Entity as _, SchemaLint, SchemaManifest, Value};

    use crate::orgs::model::OrgRow;

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
