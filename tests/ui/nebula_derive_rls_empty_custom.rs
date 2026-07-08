#[derive(comet::nebula::Entity)]
#[nebula(table = "tasks")]
#[nebula(rls(delete, custom = ""))]
struct Task {
    #[nebula(primary_key)]
    id: i64,
}

fn main() {}
