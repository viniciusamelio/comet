use comet::nebula::{belongs_to, has_many, BelongsTo, HasMany};
use rocket::serde::Deserialize;

use crate::boards::board_task::BoardTaskRow;
use crate::orgs::model::OrgRow;

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
