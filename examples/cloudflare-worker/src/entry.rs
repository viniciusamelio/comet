//! Wasm-only glue between the Cloudflare Worker runtime and the Rocket app.
use wasm_bindgen::JsValue;
use worker::{event, Context, Env, MessageBatch, Request, Response, Result};

use crate::model::TaskEvent;
use crate::routes::rocket;

const RECORD_TASK_EVENT_QUERY: &str = "INSERT INTO task_events (task_id, kind) VALUES (?1, ?2)";

#[event(fetch)]
pub async fn main(req: Request, env: Env, ctx: Context) -> Result<Response> {
    console_error_panic_hook::set_once();

    comet::cloudflare::fetch(req, env, ctx, rocket).await
}

/// Consumes `TaskEvent` messages published by the API routes and records
/// each one in the `task_events` table, proving the queue round-trip end to
/// end rather than just accepting a fire-and-forget send.
#[event(queue)]
pub async fn queue(batch: MessageBatch<TaskEvent>, env: Env, _ctx: Context) -> Result<()> {
    let db = env.d1("DB")?;

    for message in batch.messages()? {
        let event = message.into_body();
        db.prepare(RECORD_TASK_EVENT_QUERY)
            .bind(&[JsValue::from(event.task_id), JsValue::from(event.kind.as_str())])?
            .run()
            .await?;
    }

    batch.ack_all();
    Ok(())
}
