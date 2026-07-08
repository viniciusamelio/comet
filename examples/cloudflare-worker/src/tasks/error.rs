use rocket::http::Status;
use rocket::response::Responder;
use rocket::serde::json::Json;
use rocket::serde::Serialize;
use rocket::Request;

use comet::nebula::RlsError;

#[derive(Debug)]
pub enum ApiError {
    NotFound,
    BadRequest(String),
    Rls(RlsError),
    Worker(worker::Error),
}

impl From<worker::Error> for ApiError {
    fn from(error: worker::Error) -> Self {
        ApiError::Worker(error)
    }
}

impl From<RlsError> for ApiError {
    fn from(error: RlsError) -> Self {
        ApiError::Rls(error)
    }
}

#[derive(Serialize)]
#[serde(crate = "rocket::serde")]
struct ErrorBody {
    error: String,
}

impl<'r> Responder<'r, 'static> for ApiError {
    fn respond_to(self, request: &'r Request<'_>) -> rocket::response::Result<'static> {
        let (status, message) = self.status_and_message();

        Json(ErrorBody { error: message })
            .respond_to(request)
            .map(|mut response| {
                response.set_status(status);
                response
            })
    }
}

impl ApiError {
    fn status_and_message(self) -> (Status, String) {
        match self {
            ApiError::NotFound => (Status::NotFound, "task not found".to_string()),
            ApiError::BadRequest(message) => (Status::BadRequest, message),
            ApiError::Rls(RlsError::MissingUser { .. } | RlsError::MissingTenant { .. }) => (
                Status::Unauthorized,
                "missing RLS access context".to_string(),
            ),
            ApiError::Rls(RlsError::Forbidden { .. }) => {
                (Status::Forbidden, "RLS policy denied access".to_string())
            }
            ApiError::Rls(RlsError::TypeMismatch {
                table,
                column,
                expected,
            }) => (
                Status::InternalServerError,
                format!(
                    "RLS value for `{table}.{column}` does not match expected {}",
                    expected.name()
                ),
            ),
            ApiError::Rls(RlsError::MissingCustomPredicate { table, name }) => (
                Status::InternalServerError,
                format!("missing RLS custom predicate `{name}` for table `{table}`"),
            ),
            ApiError::Worker(error) => (Status::InternalServerError, error.to_string()),
        }
    }
}

pub type ApiResult<T> = Result<T, ApiError>;

#[cfg(test)]
mod tests {
    use super::*;
    use comet::nebula::SqlType;
    use rocket::local::asynchronous::Client;

    #[test]
    fn rls_errors_map_to_http_statuses() {
        assert_eq!(
            ApiError::from(RlsError::MissingUser { table: "tasks" })
                .status_and_message()
                .0,
            Status::Unauthorized
        );
        assert_eq!(
            ApiError::from(RlsError::Forbidden { table: "tasks" })
                .status_and_message()
                .0,
            Status::Forbidden
        );
        assert_eq!(
            ApiError::from(RlsError::MissingCustomPredicate {
                table: "tasks",
                name: "can_update",
            })
            .status_and_message()
            .0,
            Status::InternalServerError
        );
        assert_eq!(
            ApiError::from(RlsError::TypeMismatch {
                table: "boards",
                column: "org_id",
                expected: SqlType::Integer,
            })
            .status_and_message()
            .0,
            Status::InternalServerError
        );
    }

    #[rocket::get("/missing-context")]
    fn missing_context() -> Result<&'static str, ApiError> {
        Err(ApiError::from(RlsError::MissingUser { table: "tasks" }))
    }

    #[rocket::get("/forbidden")]
    fn forbidden() -> Result<&'static str, ApiError> {
        Err(ApiError::from(RlsError::Forbidden { table: "tasks" }))
    }

    #[rocket::get("/missing-custom")]
    fn missing_custom() -> Result<&'static str, ApiError> {
        Err(ApiError::from(RlsError::MissingCustomPredicate {
            table: "tasks",
            name: "can_complete_task",
        }))
    }

    #[rocket::async_test]
    async fn rls_responder_sets_http_statuses() {
        let rocket = rocket::build().mount(
            "/",
            rocket::routes![missing_context, forbidden, missing_custom],
        );
        let client = Client::tracked(rocket).await.unwrap();

        assert_eq!(
            client.get("/missing-context").dispatch().await.status(),
            Status::Unauthorized
        );
        assert_eq!(
            client.get("/forbidden").dispatch().await.status(),
            Status::Forbidden
        );
        assert_eq!(
            client.get("/missing-custom").dispatch().await.status(),
            Status::InternalServerError
        );
    }
}
