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
