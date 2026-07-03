#[derive(comet::nebula::Entity)]
#[nebula(table = "tasks")]
struct TaskRow {
    #[nebula(rename = "title")]
    title: String,
    #[nebula(rename = "title")]
    name: String,
}

fn main() {}
