#[derive(comet::nebula::Entity)]
#[nebula(table = "tasks")]
struct TaskRow {
    #[nebula(primary_key)]
    id: i64,
    #[nebula(primary_key)]
    external_id: String,
}

fn main() {}
