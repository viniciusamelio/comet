use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use comet::nebula::schema::SchemaSnapshot;
use comet::nebula::{MigrationBlocker, MigrationPlan};

use crate::cli::{MigrateGenerateArgs, MigrateInitArgs, MigrateStatusArgs};
use crate::{discover, schema_dump, snapshot};

/// Generates the first migration from the project's current entities and
/// saves the baseline schema snapshot that later `migrate generate`/`status`
/// calls diff against.
pub fn init(args: MigrateInitArgs) -> Result<()> {
    let project_dir = args.path.unwrap_or_else(|| PathBuf::from("."));
    let snapshot_path = snapshot::snapshot_path(&project_dir);

    if snapshot_path.exists() {
        bail!(
            "{} already exists; this project is already initialized. Use `comet migrate generate` for schema changes.",
            snapshot_path.display()
        );
    }

    let current = dump_current_schema(&project_dir)?;
    let statements = current.clone().to_manifest().initial_migration();
    let plan = MigrationPlan {
        statements,
        blockers: Vec::new(),
    };

    let migrations_dir = project_dir.join("migrations");
    let sequence = snapshot::next_migration_sequence(&migrations_dir)?;
    let path = plan
        .write_sql_file(&migrations_dir, sequence, "init")
        .map_err(|error| anyhow::anyhow!("writing initial migration file: {error:?}"))?;

    snapshot::write_snapshot(&snapshot_path, &current)?;

    println!("Wrote {}", path.display());
    println!("Saved schema snapshot to {}", snapshot_path.display());

    Ok(())
}

/// Diffs the project's current entities against the last saved snapshot and
/// writes a new migration file for the safe, additive changes found.
pub fn generate(args: MigrateGenerateArgs) -> Result<()> {
    let project_dir = args.path.unwrap_or_else(|| PathBuf::from("."));
    let snapshot_path = snapshot::snapshot_path(&project_dir);

    let Some(persisted) = snapshot::load_snapshot(&snapshot_path)? else {
        bail!(
            "no schema snapshot found at {}; run `comet migrate init` first",
            snapshot_path.display()
        );
    };

    let current = dump_current_schema(&project_dir)?;
    let plan = persisted.to_manifest().diff(&current.clone().to_manifest());

    if !plan.is_safe() {
        println!("Cannot generate a migration automatically:");
        for blocker in &plan.blockers {
            println!("  - {}", describe_blocker(blocker));
        }
        bail!("resolve the blockers above, or write this migration by hand");
    }

    if plan.statements.is_empty() {
        println!("Schema is up to date; no migration generated.");
        return Ok(());
    }

    let migrations_dir = project_dir.join("migrations");
    let sequence = snapshot::next_migration_sequence(&migrations_dir)?;
    let path = plan
        .write_sql_file(&migrations_dir, sequence, &args.name)
        .map_err(|error| anyhow::anyhow!("writing migration file: {error:?}"))?;

    snapshot::write_snapshot(&snapshot_path, &current)?;

    println!("Wrote {}", path.display());
    println!("Updated schema snapshot at {}", snapshot_path.display());

    Ok(())
}

/// Reports pending schema changes against the last saved snapshot, without
/// writing anything.
pub fn status(args: MigrateStatusArgs) -> Result<()> {
    let project_dir = args.path.unwrap_or_else(|| PathBuf::from("."));
    let current = dump_current_schema(&project_dir)?;

    println!("Current schema ({} table(s)):", current.tables.len());
    for table in &current.tables {
        println!("  - {} ({} column(s))", table.name, table.columns.len());
    }
    println!();

    let snapshot_path = snapshot::snapshot_path(&project_dir);
    let Some(persisted) = snapshot::load_snapshot(&snapshot_path)? else {
        println!(
            "No baseline snapshot found at {}. Run `comet migrate init` to create one.",
            snapshot_path.display()
        );
        return Ok(());
    };

    let plan = persisted.to_manifest().diff(&current.to_manifest());

    if plan.statements.is_empty() && plan.blockers.is_empty() {
        println!("Schema is up to date with {}.", snapshot_path.display());
        return Ok(());
    }

    if !plan.statements.is_empty() {
        println!("Pending changes (run `comet migrate generate <name>` to write them):");
        for statement in &plan.statements {
            println!("  {statement}");
        }
    }

    if !plan.blockers.is_empty() {
        println!("Blockers (need a hand-written migration):");
        for blocker in &plan.blockers {
            println!("  - {}", describe_blocker(blocker));
        }
    }

    Ok(())
}

fn dump_current_schema(project_dir: &Path) -> Result<SchemaSnapshot> {
    let src_dir = project_dir.join("src");
    let entities = discover::discover_entities(&src_dir)
        .with_context(|| format!("discovering entities under {}", src_dir.display()))?;
    schema_dump::dump_schema(project_dir, &entities)
}

fn describe_blocker(blocker: &MigrationBlocker) -> String {
    match blocker {
        MigrationBlocker::DropTable { table } => {
            format!("drop table `{table}` (destructive; write this migration by hand)")
        }
        MigrationBlocker::DropColumn { table, column } => {
            format!("drop column `{table}.{column}` (destructive; write this migration by hand)")
        }
        MigrationBlocker::ChangeColumn { table, column } => format!(
            "change column `{table}.{column}` (SQLite can't alter a column in place; write this migration by hand)"
        ),
        MigrationBlocker::UnsafeAddColumn { table, column } => format!(
            "add column `{table}.{column}` without a default (existing rows would violate NOT NULL; add a default or make it nullable)"
        ),
        MigrationBlocker::DropIndex { table, index } => {
            format!("drop index `{index}` on `{table}` (write this migration by hand)")
        }
        MigrationBlocker::ChangeIndex { table, index } => {
            format!("change index `{index}` on `{table}` (write this migration by hand)")
        }
        MigrationBlocker::AddForeignKey { table, columns } => format!(
            "add foreign key on `{table}` ({}) to an existing table (SQLite can't add foreign keys in place; write this migration by hand)",
            columns.join(", ")
        ),
        MigrationBlocker::DropForeignKey { table, columns } => format!(
            "drop foreign key on `{table}` ({}) (write this migration by hand)",
            columns.join(", ")
        ),
        MigrationBlocker::ChangeForeignKey { table, columns } => format!(
            "change foreign key on `{table}` ({}) (write this migration by hand)",
            columns.join(", ")
        ),
        MigrationBlocker::ChangeRls { table } => {
            format!("change RLS policies on `{table}` (review the authorization change by hand)")
        }
    }
}
