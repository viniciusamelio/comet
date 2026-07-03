use std::time::SystemTime;

#[derive(comet::nebula::Entity)]
#[nebula(table = "tasks")]
struct TaskRow {
    id: i64,
    created_at: SystemTime,
}

fn main() {}
