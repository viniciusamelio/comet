use comet::cloudflare::D1;
use comet::nebula::Entity;
use comet_auth::{AuthSession, NebulaAccessContextExt};
use rocket::serde::json::Json;

use crate::boards::model::BoardRow;
use crate::tasks::error::{ApiError, ApiResult};
use crate::tasks::routes::DB;

#[get("/orgs/<org_id>/boards")]
pub async fn list_org_boards(
    org_id: i32,
    session: AuthSession,
    db: D1<DB>,
) -> ApiResult<Json<Vec<BoardRow>>> {
    let context = session.to_nebula_access_context().with_tenant_value(org_id);
    let rows = BoardRow::select_scoped(&context)
        .map_err(ApiError::from)?
        .order_by(BoardRow::ID.asc())
        .limit(100)
        .to_statement()
        .fetch_all_d1(&db)
        .await
        .map_err(ApiError::from)?
        .results::<BoardRow>()
        .map_err(ApiError::from)?;

    Ok(Json(rows))
}
