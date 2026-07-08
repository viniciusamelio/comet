use rocket::serde::Deserialize;

#[derive(Debug, Clone, Deserialize, comet::nebula::Entity)]
#[nebula(table = "orgs")]
#[nebula(rls(public))]
#[serde(crate = "rocket::serde")]
pub struct OrgRow {
    #[nebula(primary_key, auto, unique, index)]
    pub id: i32,
    pub name: String,
}
