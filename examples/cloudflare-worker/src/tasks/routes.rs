use comet::cloudflare::{BindingName, QueueBinding, D1};
use comet::nebula::{
    AccessContext, CustomPredicateProvider, CustomPredicateRegistration, Entity, Expr, RlsError,
    RlsOperation,
};
use comet_auth::{
    AuthSession, AuthorizationMode, AuthorizationRequirement, AuthorizedSession,
    NebulaAccessContextExt, RequiredAuthorization,
};
use rocket::serde::json::Json;

use crate::tasks::error::{ApiError, ApiResult};
use crate::tasks::model::{NewTask, Task, TaskEvent, TaskEventKind, TaskRow};

const TASK_COLUMNS: &[&str] = &["id", "user_id", "title", "done", "created_at"];

pub struct DB;

impl BindingName for DB {
    const NAME: &'static str = "DB";
}

pub struct TaskEvents;

impl BindingName for TaskEvents {
    const NAME: &'static str = "TASK_EVENTS";
}

pub struct TaskWritePolicy;

impl RequiredAuthorization for TaskWritePolicy {
    const REQUIREMENT: AuthorizationRequirement = AuthorizationRequirement::with_mode_and_resource(
        AuthorizationMode::All,
        &[],
        &["tasks:write"],
        &[],
        None,
    );
}

struct CompleteTaskPredicates;

impl CustomPredicateProvider for CompleteTaskPredicates {
    fn predicate(
        &self,
        table: &'static str,
        name: &'static str,
        _operation: RlsOperation,
        _context: &AccessContext,
    ) -> Result<Expr, RlsError> {
        if table == TaskRow::TABLE.name && name == "can_complete_task" {
            Ok(TaskRow::DONE.eq(0))
        } else {
            Err(RlsError::MissingCustomPredicate { table, name })
        }
    }

    fn registered_predicate_rules(&self) -> &'static [CustomPredicateRegistration] {
        &[CustomPredicateRegistration {
            name: "can_complete_task",
            operations: &[RlsOperation::Update],
        }]
    }
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
pub async fn list_tasks(session: AuthSession, db: D1<DB>) -> ApiResult<Json<Vec<Task>>> {
    let context = session.to_nebula_access_context();
    let rows = TaskRow::select_scoped(&context)
        .map_err(ApiError::from)?
        .order_by(TaskRow::ID.asc())
        .limit(100)
        .to_statement()
        .fetch_all_d1(&db)
        .await
        .map_err(ApiError::from)?
        .results::<TaskRow>()
        .map_err(ApiError::from)?;

    Ok(Json(rows.into_iter().map(Task::from).collect()))
}

#[get("/tasks/<id>")]
pub async fn get_task(id: i32, session: AuthSession, db: D1<DB>) -> ApiResult<Json<Task>> {
    let context = session.to_nebula_access_context();
    let row = TaskRow::select_scoped(&context)
        .map_err(ApiError::from)?
        .where_(TaskRow::ID.eq(id))
        .limit(1)
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
    session: AuthSession,
    db: D1<DB>,
    queue: QueueBinding<TaskEvents>,
) -> ApiResult<Json<Task>> {
    let context = session.to_nebula_access_context();
    let title = new_task
        .validated_title()
        .map_err(|message| ApiError::BadRequest(message.to_string()))?;

    let row = TaskRow::insert_scoped(&context)
        .map_err(ApiError::from)?
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
    session: AuthorizedSession<TaskWritePolicy>,
    db: D1<DB>,
    queue: QueueBinding<TaskEvents>,
) -> ApiResult<Json<Task>> {
    let context = session.to_nebula_access_context();
    TaskRow::validate_custom_predicates_with(&CompleteTaskPredicates).map_err(ApiError::from)?;
    let row = TaskRow::update_scoped_with(&context, &CompleteTaskPredicates)
        .map_err(ApiError::from)?
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
