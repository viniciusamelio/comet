#[derive(comet::nebula::Entity)]
#[nebula(table = "tasks")]
struct TaskRow(i64, String);

fn main() {}
