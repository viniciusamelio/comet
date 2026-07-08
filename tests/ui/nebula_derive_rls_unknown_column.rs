#[derive(comet::nebula::Entity)]
#[nebula(table = "tasks")]
#[nebula(rls(owner = "missing_user_id"))]
struct Task {
    #[nebula(primary_key)]
    id: i64,
    user_id: String,
}

fn main() {}
