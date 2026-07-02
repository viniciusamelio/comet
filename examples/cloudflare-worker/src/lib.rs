#[macro_use]
extern crate rocket;

mod error;
mod model;
mod routes;

#[cfg(target_arch = "wasm32")]
mod entry;
