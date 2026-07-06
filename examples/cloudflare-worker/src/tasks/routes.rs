use comet::cloudflare::{BindingName, QueueBinding, D1};
use comet::nebula::Entity;
use rocket::serde::json::Json;

use crate::tasks::error::{ApiError, ApiResult};
use crate::tasks::model::{NewTask, Task, TaskEvent, TaskEventKind, TaskRow};

const TASK_COLUMNS: &[&str] = &["id", "title", "done", "created_at"];

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

#[get("/tasks")]
pub async fn list_tasks(db: D1<DB>) -> ApiResult<Json<Vec<Task>>> {
    let rows = TaskRow::select()
        .order_by(TaskRow::ID.asc())
        .to_statement()
        .fetch_all_d1(&db)
        .await
        .map_err(ApiError::from)?
        .results::<TaskRow>()
        .map_err(ApiError::from)?;

    Ok(Json(rows.into_iter().map(Task::from).collect()))
}

#[get("/tasks/<id>")]
pub async fn get_task(id: i32, db: D1<DB>) -> ApiResult<Json<Task>> {
    let row = TaskRow::select()
        .where_(TaskRow::ID.eq(id))
        .to_statement()
        .fetch_optional_d1::<TaskRow>(&db)
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

    let row = TaskRow::insert()
        .set(TaskRow::TITLE, title)
        .returning(TASK_COLUMNS.iter().copied())
        .to_statement()
        .fetch_one_d1::<TaskRow>(&db)
        .await
        .map_err(ApiError::from)?;

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
    let row = TaskRow::update()
        .set(TaskRow::DONE, 1)
        .where_(TaskRow::ID.eq(id))
        .returning(TASK_COLUMNS.iter().copied())
        .to_statement()
        .fetch_optional_d1::<TaskRow>(&db)
        .await
        .map_err(ApiError::from)?
        .ok_or(ApiError::NotFound)?;

    let task: Task = row.into();
    publish_task_event(&queue, task.id, TaskEventKind::Completed).await?;

    Ok(Json(task))
}
