use std::path::PathBuf;

use comet::cloudflare::{BindingName, R2Bucket, R2Object};
use rocket::data::Capped;
use rocket::http::Status;

pub struct Assets;

impl BindingName for Assets {
    const NAME: &'static str = "ASSETS";
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
