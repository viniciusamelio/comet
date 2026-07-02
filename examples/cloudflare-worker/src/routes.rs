use comet::cloudflare::{
    BindingName, D1, QueueBinding, R2Bucket, R2Object, WebSocketResponse, WebSocketUpgrade,
};
use rocket::data::Capped;
use rocket::futures::StreamExt;
use rocket::http::Status;
use rocket::serde::json::Json;
use rocket::{Build, Rocket};
use std::path::PathBuf;
use wasm_bindgen::JsValue;
use worker::{Context, Env, WebsocketEvent};

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

pub struct Assets;

impl BindingName for Assets {
    const NAME: &'static str = "ASSETS";
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

#[get("/ws/echo")]
pub async fn websocket_echo(ws: WebSocketUpgrade) -> WebSocketResponse {
    ws.accept(|socket| async move {
        let mut events = socket.events()?;
        while let Some(event) = events.next().await {
            match event? {
                WebsocketEvent::Message(message) => {
                    if let Some(text) = message.text() {
                        socket.send_with_str(text)?;
                    } else if let Some(bytes) = message.bytes() {
                        socket.send_with_bytes(bytes)?;
                    }
                }
                WebsocketEvent::Close(_) => break,
            }
        }

        Ok(())
    })
}

#[put("/assets/<key..>", data = "<body>")]
pub async fn put_asset(
    key: PathBuf,
    body: Capped<Vec<u8>>,
    bucket: R2Bucket<Assets>,
) -> Result<Status, Status> {
    if !body.is_complete() {
        return Err(Status::PayloadTooLarge);
    }

    bucket
        .put(asset_key(key), body.value)
        .execute()
        .await
        .map_err(|_| Status::InternalServerError)?;

    Ok(Status::Created)
}

#[get("/assets/<key..>")]
pub async fn get_asset(key: PathBuf, bucket: R2Bucket<Assets>) -> Option<R2Object> {
    R2Object::get(&bucket, asset_key(key)).await.ok().flatten()
}

fn asset_key(key: PathBuf) -> String {
    key.to_string_lossy().replace('\\', "/")
}

#[get("/tasks")]
pub async fn list_tasks(db: D1<DB>) -> ApiResult<Json<Vec<Task>>> {
    let rows = db
        .prepare(TASKS_QUERY)
        .all()
        .await
        .map_err(ApiError::from)?
        .results::<TaskRow>()
        .map_err(ApiError::from)?;

    Ok(Json(rows.into_iter().map(Task::from).collect()))
}

#[get("/tasks/<id>")]
pub async fn get_task(id: i32, db: D1<DB>) -> ApiResult<Json<Task>> {
    let row = db
        .prepare(TASK_BY_ID_QUERY)
        .bind(&[JsValue::from(id)])
        .map_err(ApiError::from)?
        .first::<TaskRow>(None)
        .await
        .map_err(ApiError::from)?
        .ok_or(ApiError::NotFound)?;

    Ok(Json(Task::from(row)))
}

#[post("/tasks", data = "<new_task>")]
pub async fn create_task(
    new_task: Json<NewTask>,
    db: D1<DB>,
    queue: QueueBinding<TaskEvents>,
) -> ApiResult<Json<Task>> {
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
}

#[post("/tasks/<id>/complete")]
pub async fn complete_task(
    id: i32,
    db: D1<DB>,
    queue: QueueBinding<TaskEvents>,
) -> ApiResult<Json<Task>> {
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
}

pub fn rocket(env: Env, _ctx: Context) -> Rocket<Build> {
    use rocket::data::{Limits, ToByteUnit};

    let limits = Limits::default()
        .limit("string", 25.megabytes())
        .limit("bytes", 25.megabytes());
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
            websocket_echo,
            put_asset,
            get_asset,
            list_tasks,
            get_task,
            create_task,
            complete_task
        ],
    )
}
