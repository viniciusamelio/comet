#![cfg(feature = "nebula")]

use comet::nebula::Entity as _;

#[derive(comet::nebula::Entity)]
#[nebula(table = "tasks")]
#[allow(dead_code)]
struct TaskRow {
    #[nebula(primary_key, auto)]
    id: i64,
    #[nebula(foreign_key = "boards.id", index)]
    board_id: i64,
    title: String,
    #[nebula(default = "0", index)]
    done: bool,
    #[nebula(rename = "created_at", default = "datetime('now')")]
    created: String,
}

#[test]
fn derive_entity_generates_metadata_and_columns() {
    assert_eq!(TaskRow::TABLE.name, "tasks");
    assert_eq!(TaskRow::TABLE.indexes, &[]);
    assert_eq!(TaskRow::TABLE.rls, &[]);
    assert_eq!(TaskRow::ID.name(), "id");
    assert_eq!(TaskRow::BOARD_ID.name(), "board_id");
    assert_eq!(TaskRow::TITLE.name(), "title");
    assert_eq!(TaskRow::DONE.name(), "done");
    assert_eq!(TaskRow::CREATED.name(), "created_at");

    assert_eq!(TaskRow::TABLE.columns.len(), 5);
    assert_eq!(TaskRow::TABLE.columns[0].name, "id");
    assert_eq!(
        TaskRow::TABLE.columns[0].sql_type,
        comet::nebula::SqlType::Integer
    );
    assert!(TaskRow::TABLE.columns[0].primary_key);
    assert!(TaskRow::TABLE.columns[0].auto_increment);

    assert_eq!(TaskRow::TABLE.columns[1].name, "board_id");
    assert!(TaskRow::TABLE.columns[1].indexed);
    assert_eq!(TaskRow::TABLE.foreign_keys.len(), 1);
    assert_eq!(TaskRow::TABLE.foreign_keys[0].columns, &["board_id"]);
    assert_eq!(TaskRow::TABLE.foreign_keys[0].references_table, "boards");
    assert_eq!(TaskRow::TABLE.foreign_keys[0].references_columns, &["id"]);

    assert_eq!(TaskRow::TABLE.columns[3].name, "done");
    assert_eq!(
        TaskRow::TABLE.columns[3].sql_type,
        comet::nebula::SqlType::Boolean
    );
    assert!(TaskRow::TABLE.columns[3].indexed);
    assert_eq!(TaskRow::TABLE.columns[3].default_sql, Some("0"));

    assert_eq!(TaskRow::TABLE.columns[4].name, "created_at");
    assert_eq!(
        TaskRow::TABLE.columns[4].default_sql,
        Some("datetime('now')")
    );
}

#[derive(comet::nebula::Entity)]
#[nebula(table = "boards")]
#[nebula(rls(owner = "user_id"))]
#[nebula(rls(select, permission = "boards:read"))]
#[nebula(rls(update, any(role = "admin", permission = "boards:write")))]
#[nebula(rls(delete, custom = "can_delete_board"))]
#[allow(dead_code)]
struct SecuredBoard {
    #[nebula(primary_key, auto)]
    id: i64,
    #[nebula(index)]
    user_id: String,
    title: String,
}

#[test]
fn derive_entity_generates_rls_metadata() {
    use comet::nebula::{RlsMatchMode, RlsOperation, RlsPolicyKind};

    let policies = SecuredBoard::TABLE.rls;

    assert_eq!(policies.len(), 4);
    assert_eq!(policies[0].kind, RlsPolicyKind::Owner);
    assert_eq!(policies[0].column, Some("user_id"));
    assert_eq!(policies[1].operations, &[RlsOperation::Select]);
    assert_eq!(policies[1].authorization.permissions, &["boards:read"]);
    assert_eq!(policies[2].operations, &[RlsOperation::Update]);
    assert_eq!(policies[2].authorization.mode, RlsMatchMode::Any);
    assert_eq!(policies[2].authorization.roles, &["admin"]);
    assert_eq!(policies[3].kind, RlsPolicyKind::Custom);
    assert_eq!(policies[3].custom, Some("can_delete_board"));
}

#[test]
fn derive_entity_works_with_query_builders() {
    let statement = TaskRow::select()
        .where_(TaskRow::DONE.eq(false))
        .order_by(TaskRow::ID.asc())
        .limit(10)
        .to_statement();

    assert_eq!(
        statement.sql,
        "SELECT \"id\", \"board_id\", \"title\", \"done\", \"created_at\" FROM \"tasks\" \
         WHERE \"tasks\".\"done\" = ? ORDER BY \"tasks\".\"id\" ASC LIMIT ?"
    );
}
