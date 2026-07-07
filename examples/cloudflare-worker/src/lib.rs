#[macro_use]
extern crate rocket;

mod app;
mod assets;
mod demo;

// Entity-bearing contexts are `pub` (not just `pub` items in a private
// module) so a generated schema-dump binary — a separate compilation unit
// that only sees the crate's public API — can reach `crate_name::tasks::model::TaskRow`
// and friends to read their real, derive-generated `TableDef`s.
pub mod boards;
pub mod orgs;
pub mod tasks;
pub mod users;

mod entry;
