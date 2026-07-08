#[derive(comet::nebula::Entity)]
#[nebula(table = "tasks")]
#[nebula(rls(select, public))]
struct Task {
    #[nebula(primary_key)]
    id: i64,
}

fn main() {}
