use rocket::{Build, Rocket};
use worker::{Context, Env};

use crate::assets::routes::{get_asset, put_asset};
use crate::boards::routes::list_org_boards;
use crate::demo::routes::{
    echo, index, private_admin, private_me, private_reviewer, stream_demo, websocket_echo,
};
use crate::tasks::routes::{complete_task, create_task, get_task, list_tasks, DB};

struct AuthKv;

impl comet::cloudflare::BindingName for AuthKv {
    const NAME: &'static str = "AUTH_KV";
}

#[allow(unused)]
pub fn rocket(env: Env, _ctx: Context) -> Rocket<Build> {
    use rocket::data::{Limits, ToByteUnit};

    let limits = Limits::default()
        .limit("string", 25.megabytes())
        .limit("bytes", 25.megabytes());
    let config = rocket::Config {
        limits,
        ..rocket::Config::default()
    };
    let auth_base_url = env
        .var("COMET_AUTH_BASE_URL")
        .map(|value| value.to_string())
        .or_else(|_| {
            env.secret("COMET_AUTH_BASE_URL")
                .map(|value| value.to_string())
        })
        .unwrap_or_else(|_| "http://localhost:8787".to_owned());
    let auth_config = comet_auth::AuthConfig::from_env()
        .base_url(auth_base_url)
        .provider(
            comet_auth::providers::Google::from_env()
                .web_client_id_env("GOOGLE_WEB_CLIENT_ID")
                .web_client_secret_env("GOOGLE_WEB_CLIENT_SECRET")
                .native_client_id_env("GOOGLE_IOS_CLIENT_ID")
                .native_client_id_env("GOOGLE_ANDROID_CLIENT_ID"),
        )
        .provider(
            comet_auth::providers::Apple::from_env()
                .service_id_env("APPLE_SERVICE_ID")
                .team_id_env("APPLE_TEAM_ID")
                .key_id_env("APPLE_KEY_ID")
                .private_key_pkcs8_pem_env("APPLE_PRIVATE_KEY_PKCS8_PEM")
                .native_audience_env("APPLE_IOS_BUNDLE_ID"),
        )
        .provider(
            comet_auth::providers::GitHub::from_env()
                .client_id_env("GITHUB_CLIENT_ID")
                .client_secret_env("GITHUB_CLIENT_SECRET"),
        );

    rocket::custom(config)
        .manage(env)
        .attach(comet_auth::Auth::<DB, AuthKv>::fairing(auth_config))
        .mount("/auth", comet_auth::routes::<DB, AuthKv>())
        .mount(
            "/",
            routes![
                index,
                echo,
                stream_demo,
                websocket_echo,
                put_asset,
                get_asset,
                list_org_boards,
                list_tasks,
                get_task,
                create_task,
                complete_task,
                private_admin,
                private_me,
                private_reviewer
            ],
        )
}
