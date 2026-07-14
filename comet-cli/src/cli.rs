use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};

#[derive(Parser)]
#[command(
    name = "comet",
    version,
    about = "Scaffold, generate, migrate, and test Comet + Nebula projects."
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Scaffold a new Comet + Nebula + Cloudflare Worker project.
    New(NewArgs),
    /// Generate entities or route modules in an existing project.
    #[command(subcommand)]
    Generate(GenerateCommand),
    /// Generate or inspect Nebula migrations.
    #[command(subcommand)]
    Migrate(MigrateCommand),
    /// Scaffold Comet Auth migrations and Cloudflare binding hints.
    #[command(subcommand)]
    Auth(AuthCommand),
    /// Inspect Nebula RLS coverage.
    #[command(subcommand)]
    Rls(RlsCommand),
    /// Inspect routes and generate typed RPC contracts/clients.
    #[command(subcommand)]
    Rpc(RpcCommand),
    /// Run the project's test/release gate.
    #[command(subcommand)]
    Test(TestCommand),
}

#[derive(Subcommand)]
pub enum AuthCommand {
    /// Add the Comet Auth runtime migration to a project.
    Init(AuthInitArgs),
}

#[derive(Subcommand)]
pub enum RlsCommand {
    /// Show RLS coverage for discovered Nebula entities.
    Status(RlsStatusArgs),
}

#[derive(Subcommand)]
pub enum RpcCommand {
    /// Emit the discovered RPC manifest as JSON.
    Manifest(RpcManifestArgs),
    /// Generate a typed RPC client from discovered routes.
    Generate(RpcGenerateArgs),
}

#[derive(Args)]
pub struct NewArgs {
    /// Project name; also used as the crate name and D1 database name.
    pub name: String,

    /// Directory to create the project in. Defaults to `./<name>`.
    #[arg(long)]
    pub path: Option<PathBuf>,

    /// Wrangler D1 binding name used in `wrangler.jsonc` and route code.
    #[arg(long, default_value = "DB")]
    pub db_binding: String,
}

#[derive(Subcommand)]
pub enum GenerateCommand {
    /// Generate a `#[derive(Entity)]` struct skeleton.
    Entity(GenerateEntityArgs),
    /// Generate a CRUD route module bound to an existing entity.
    Route(GenerateRouteArgs),
}

#[derive(Args)]
pub struct GenerateEntityArgs {
    /// Singular concept name, e.g. `Board` (generates `BoardRow` in `src/boards/model.rs`).
    pub name: String,

    /// A field as `name:type[:attr[,attr]...]`, e.g. `title:string` or
    /// `org_id:i64:foreign_key=orgs.id,index`. Repeatable. An `id` primary
    /// key is added automatically unless one is already given.
    #[arg(long = "field", value_name = "SPEC")]
    pub fields: Vec<String>,

    /// Table name override. Defaults to the pluralized snake_case of `name`.
    #[arg(long)]
    pub table: Option<String>,

    /// Project directory. Defaults to the current directory.
    #[arg(long)]
    pub path: Option<PathBuf>,
}

#[derive(Args)]
pub struct GenerateRouteArgs {
    /// Entity concept name, e.g. `Board` (looks for `BoardRow` in `src/boards/model.rs`).
    pub entity: String,

    /// Wrangler D1 binding name used in the generated route code.
    #[arg(long, default_value = "DB")]
    pub db_binding: String,

    /// Project directory. Defaults to the current directory.
    #[arg(long)]
    pub path: Option<PathBuf>,
}

#[derive(Subcommand)]
pub enum MigrateCommand {
    /// Generate the first migration from the current entities.
    Init(MigrateInitArgs),
    /// Diff current entities against the last migration and generate a new one.
    Generate(MigrateGenerateArgs),
    /// Show pending schema changes against the last migration.
    Status(MigrateStatusArgs),
}

#[derive(Args)]
pub struct MigrateInitArgs {
    /// Project directory to inspect. Defaults to the current directory.
    #[arg(long)]
    pub path: Option<PathBuf>,
}

#[derive(Args)]
pub struct MigrateGenerateArgs {
    /// Migration name, used to build its file name (e.g. `add_done` -> `0002_add_done.sql`).
    pub name: String,

    /// Project directory to inspect. Defaults to the current directory.
    #[arg(long)]
    pub path: Option<PathBuf>,
}

#[derive(Args)]
pub struct MigrateStatusArgs {
    /// Project directory to inspect. Defaults to the current directory.
    #[arg(long)]
    pub path: Option<PathBuf>,
}

#[derive(Args)]
pub struct AuthInitArgs {
    /// Project directory. Defaults to the current directory.
    #[arg(long)]
    pub path: Option<PathBuf>,

    /// Wrangler D1 binding used for durable auth tables.
    #[arg(long, default_value = "DB")]
    pub db_binding: String,

    /// Wrangler KV binding used for OAuth state and optional session cache.
    #[arg(long, default_value = "AUTH_KV")]
    pub kv_binding: String,

    /// Also add the RBAC authorization migration.
    #[arg(long)]
    pub with_rbac: bool,
}

#[derive(Args)]
pub struct RlsStatusArgs {
    /// Project directory to inspect. Defaults to the current directory.
    #[arg(long)]
    pub path: Option<PathBuf>,

    /// Exit non-zero when any table has missing or incomplete RLS coverage.
    #[arg(long)]
    pub strict: bool,

    /// Emit machine-readable JSON.
    #[arg(long)]
    pub json: bool,

    /// Declare a custom predicate name available for all operations.
    #[arg(long = "custom-predicate", value_name = "NAME")]
    pub custom_predicates: Vec<String>,

    /// Declare a custom predicate for specific operations, e.g. can_archive:update,delete.
    #[arg(long = "custom-predicate-rule", value_name = "NAME:OPS")]
    pub custom_predicate_rules: Vec<String>,
}

#[derive(Args)]
pub struct RpcManifestArgs {
    /// Project directory to inspect. Defaults to the current directory.
    #[arg(long)]
    pub path: Option<PathBuf>,

    /// File to write. Defaults to stdout.
    #[arg(long)]
    pub out: Option<PathBuf>,
}

#[derive(Args)]
pub struct RpcGenerateArgs {
    /// Project directory to inspect. Defaults to the current directory.
    #[arg(long)]
    pub path: Option<PathBuf>,

    /// Client language to generate.
    #[arg(long)]
    pub lang: RpcLanguage,

    /// File to write. Defaults to stdout.
    #[arg(long)]
    pub out: Option<PathBuf>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum RpcLanguage {
    Ts,
    Dart,
    Rust,
}

#[derive(Subcommand)]
pub enum TestCommand {
    /// `cargo fmt --check` + `cargo test --lib`.
    Unit(TestArgs),
    /// The project's `npm run test:integration` script.
    Integration(TestArgs),
    /// The project's `npm run test:perf` script.
    Perf(TestArgs),
    /// Unit, then integration, then perf, stopping at the first failure.
    All(TestArgs),
}

#[derive(Args)]
pub struct TestArgs {
    /// Project directory to run tests in. Defaults to the current directory.
    #[arg(long)]
    pub path: Option<PathBuf>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rpc_generate_requires_supported_language() {
        let result = Cli::try_parse_from([
            "comet",
            "rpc",
            "generate",
            "--lang",
            "go",
            "--out",
            "client.go",
        ]);
        let Err(error) = result else {
            panic!("expected invalid rpc language to fail");
        };

        assert_eq!(error.kind(), clap::error::ErrorKind::InvalidValue);
    }
}
