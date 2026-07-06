use rocket::{Build, Rocket};
use worker::{Context, Env};

use crate::assets::routes::{get_asset, put_asset};
use crate::demo::routes::{echo, index, stream_demo, websocket_echo};
use crate::tasks::routes::{complete_task, create_task, get_task, list_tasks};

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
