use comet::nebula::{belongs_to, BelongsTo};
use rocket::serde::Deserialize;

use crate::orgs::model::OrgRow;

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
