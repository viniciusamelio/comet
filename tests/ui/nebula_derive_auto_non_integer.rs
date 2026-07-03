#[derive(comet::nebula::Entity)]
#[nebula(table = "users")]
struct User {
    #[nebula(primary_key, auto)]
    id: String,
}

fn main() {}
