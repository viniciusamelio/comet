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

#[rocket::async_test]
async fn injected_required_guard_rejects_anonymous_requests() {
    let rocket = rocket::build()
        .attach(Auth::<(), ()>::fairing(AuthConfig::default()))
        .mount("/", rocket::routes![protected, admin]);
    let client = Client::tracked(rocket).await.unwrap();

    assert_eq!(
        client.get("/protected").dispatch().await.status(),
        Status::Unauthorized
    );
    assert_eq!(
        client.get("/admin").dispatch().await.status(),
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
