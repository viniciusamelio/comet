#[derive(comet::nebula::Entity)]
#[nebula(table = "tasks")]
struct Task {
    id: i64,
    #[nebula(foreign_key = "users")]
    user_id: i64,
}

fn main() {}
