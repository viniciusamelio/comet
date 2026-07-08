mod casing;
mod cli;
mod commands;
mod discover;
mod entity_introspect;
mod fieldspec;
mod rpc;
mod rustfile;
mod schema_dump;
mod snapshot;

use clap::Parser;
use cli::{
    AuthCommand, Command, GenerateCommand, MigrateCommand, RlsCommand, RpcCommand, TestCommand,
};

fn main() -> anyhow::Result<()> {
    let cli = cli::Cli::parse();

    match cli.command {
        Command::New(args) => commands::new::run(args),
        Command::Generate(GenerateCommand::Entity(args)) => commands::generate::entity::run(args),
        Command::Generate(GenerateCommand::Route(args)) => commands::generate::route::run(args),
        Command::Migrate(MigrateCommand::Init(args)) => commands::migrate::init(args),
        Command::Migrate(MigrateCommand::Generate(args)) => commands::migrate::generate(args),
        Command::Migrate(MigrateCommand::Status(args)) => commands::migrate::status(args),
        Command::Auth(AuthCommand::Init(args)) => commands::auth::init(args),
        Command::Rls(RlsCommand::Status(args)) => commands::rls::status(args),
        Command::Rpc(RpcCommand::Manifest(args)) => commands::rpc::manifest(args),
        Command::Test(TestCommand::Unit(args)) => commands::test::unit(args),
        Command::Test(TestCommand::Integration(args)) => commands::test::integration(args),
        Command::Test(TestCommand::Perf(args)) => commands::test::perf(args),
        Command::Test(TestCommand::All(args)) => commands::test::all(args),
    }
}
