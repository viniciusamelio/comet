use comet_auth::{Auth, AuthConfig};
use rocket::http::Status;
use rocket::local::asynchronous::Client;

#[comet_auth::requires_auth]
#[rocket::get("/protected")]
async fn protected() -> &'static str {
    "secret"
}

#[comet_auth::requires_auth(optional)]
#[rocket::get("/optional")]
async fn optional() -> &'static str {
    "ok"
}

#[comet_auth::requires_auth(scope = "admin")]
#[rocket::get("/admin")]
async fn admin() -> &'static str {
    "admin"
}

#[comet_auth::requires_auth(role = "admin", permission = "boards:write")]
#[rocket::get("/rbac")]
async fn rbac() -> &'static str {
    "rbac"
}

#[comet_auth::requires_auth(any(role = "admin", permission = "boards:read"), resource = "org:demo")]
#[rocket::get("/rbac-any")]
async fn rbac_any() -> &'static str {
    "rbac-any"
}

#[rocket::async_test]
async fn injected_required_guard_rejects_anonymous_requests() {
    let rocket = rocket::build()
        .attach(Auth::<(), ()>::fairing(AuthConfig::default()))
        .mount("/", rocket::routes![protected, admin, rbac, rbac_any]);
    let client = Client::tracked(rocket).await.unwrap();

    assert_eq!(
        client.get("/protected").dispatch().await.status(),
        Status::Unauthorized
    );
    assert_eq!(
        client.get("/admin").dispatch().await.status(),
        Status::Unauthorized
    );
    assert_eq!(
        client.get("/rbac").dispatch().await.status(),
        Status::Unauthorized
    );
    assert_eq!(
        client.get("/rbac-any").dispatch().await.status(),
        Status::Unauthorized
    );
}

#[rocket::async_test]
async fn injected_optional_guard_allows_anonymous_requests() {
    let rocket = rocket::build()
        .attach(Auth::<(), ()>::fairing(AuthConfig::default()))
        .mount("/", rocket::routes![optional]);
    let client = Client::tracked(rocket).await.unwrap();

    let response = client.get("/optional").dispatch().await;
    assert_eq!(response.status(), Status::Ok);
    assert_eq!(response.into_string().await.as_deref(), Some("ok"));
}
