use comet::cloudflare::{BindingName, D1, QueueBinding};
use rocket::serde::json::Json;
use rocket::{Build, Rocket};
use wasm_bindgen::JsValue;
use worker::Env;

use crate::error::{ApiError, ApiResult};
use crate::model::{NewTask, Task, TaskEvent, TaskEventKind, TaskRow};

const TASKS_QUERY: &str = "SELECT id, title, done, created_at FROM tasks ORDER BY id";
const TASK_BY_ID_QUERY: &str = "SELECT id, title, done, created_at FROM tasks WHERE id = ?1";
const INSERT_TASK_QUERY: &str =
    "INSERT INTO tasks (title) VALUES (?1) RETURNING id, title, done, created_at";
const COMPLETE_TASK_QUERY: &str =
    "UPDATE tasks SET done = 1 WHERE id = ?1 RETURNING id, title, done, created_at";

pub struct DB;

impl BindingName for DB {
    const NAME: &'static str = "DB";
}

pub struct TaskEvents;

impl BindingName for TaskEvents {
    const NAME: &'static str = "TASK_EVENTS";
}

async fn publish_task_event(
    queue: &QueueBinding<TaskEvents>,
    task_id: i32,
    kind: TaskEventKind,
) -> ApiResult<()> {
    queue
        .send(TaskEvent { task_id, kind })
        .await
        .map_err(ApiError::from)
}

#[get("/")]
pub fn index() -> &'static str {
    "hello from Rocket on Cloudflare Workers\n"
}

#[post("/echo", data = "<body>")]
pub fn echo(body: String) -> String {
    body
}

/// Proves comet's response streaming actually streams: each chunk is only
/// produced after a real, Workers-native delay (`worker::Delay`, backed by
/// `setTimeout`, not a tokio timer that wouldn't run under Workers). If
/// comet buffered the whole body before responding, a client would see all
/// chunks arrive at once after ~1.2s; streamed, they arrive ~400ms apart.
#[get("/stream")]
pub fn stream_demo() -> rocket::response::stream::ByteStream<impl rocket::futures::stream::Stream<Item = Vec<u8>>> {
    let raw = rocket::response::stream::stream! {
        for chunk in 0..3u8 {
            yield vec![b'0' + chunk; 4096];
            worker::Delay::from(std::time::Duration::from_millis(400)).await;
        }
    };

    rocket::response::stream::ByteStream(comet::cloudflare::local_stream(raw))
}

#[get("/tasks")]
pub async fn list_tasks(db: D1<DB>) -> ApiResult<Json<Vec<Task>>> {
    comet::cloudflare::local(async move {
        let rows = db
            .prepare(TASKS_QUERY)
            .all()
            .await
            .map_err(ApiError::from)?
            .results::<TaskRow>()
            .map_err(ApiError::from)?;

        Ok(Json(rows.into_iter().map(Task::from).collect()))
    })
    .await
}

#[get("/tasks/<id>")]
pub async fn get_task(id: i32, db: D1<DB>) -> ApiResult<Json<Task>> {
    comet::cloudflare::local(async move {
        let row = db
            .prepare(TASK_BY_ID_QUERY)
            .bind(&[JsValue::from(id)])
            .map_err(ApiError::from)?
            .first::<TaskRow>(None)
            .await
            .map_err(ApiError::from)?
            .ok_or(ApiError::NotFound)?;

        Ok(Json(Task::from(row)))
    })
    .await
}

#[post("/tasks", data = "<new_task>")]
pub async fn create_task(
    new_task: Json<NewTask>,
    db: D1<DB>,
    queue: QueueBinding<TaskEvents>,
) -> ApiResult<Json<Task>> {
    comet::cloudflare::local(async move {
        let title = new_task
            .validated_title()
            .map_err(|message| ApiError::BadRequest(message.to_string()))?;

        let row = db
            .prepare(INSERT_TASK_QUERY)
            .bind(&[JsValue::from(title)])
            .map_err(ApiError::from)?
            .first::<TaskRow>(None)
            .await
            .map_err(ApiError::from)?
            .ok_or_else(|| ApiError::BadRequest("insert did not return a row".to_string()))?;

        let task: Task = row.into();
        publish_task_event(&queue, task.id, TaskEventKind::Created).await?;

        Ok(Json(task))
    })
    .await
}

#[post("/tasks/<id>/complete")]
pub async fn complete_task(
    id: i32,
    db: D1<DB>,
    queue: QueueBinding<TaskEvents>,
) -> ApiResult<Json<Task>> {
    comet::cloudflare::local(async move {
        let row = db
            .prepare(COMPLETE_TASK_QUERY)
            .bind(&[JsValue::from(id)])
            .map_err(ApiError::from)?
            .first::<TaskRow>(None)
            .await
            .map_err(ApiError::from)?
            .ok_or(ApiError::NotFound)?;

        let task: Task = row.into();
        publish_task_event(&queue, task.id, TaskEventKind::Completed).await?;

        Ok(Json(task))
    })
    .await
}

pub fn rocket(env: Env) -> Rocket<Build> {
    use rocket::data::{Limits, ToByteUnit};

    let limits = Limits::default().limit("string", 25.megabytes());
    let config = rocket::Config {
        limits,
        ..rocket::Config::default()
    };

    rocket::custom(config).manage(env).mount(
        "/",
        routes![
            index,
            echo,
            stream_demo,
            list_tasks,
            get_task,
            create_task,
            complete_task
        ],
    )
}
