use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::cli::AuthInitArgs;
use crate::snapshot;

const AUTH_MIGRATION_SQL: &str = include_str!("../../../comet-auth/migrations/0001_comet_auth.sql");

pub fn init(args: AuthInitArgs) -> Result<()> {
    let project_dir = args.path.unwrap_or_else(|| PathBuf::from("."));
    let db_binding = validate_binding(&args.db_binding, "--db-binding")?;
    let kv_binding = validate_binding(&args.kv_binding, "--kv-binding")?;
    let migrations_dir = project_dir.join("migrations");

    fs::create_dir_all(&migrations_dir)
        .with_context(|| format!("creating {}", migrations_dir.display()))?;

    if auth_migration_exists(&migrations_dir)? {
        bail!(
            "a Comet Auth migration already exists in {}; refusing to add a duplicate",
            migrations_dir.display()
        );
    }

    let sequence = snapshot::next_migration_sequence(&migrations_dir)?;
    let path = migrations_dir.join(format!("{sequence:04}_comet_auth.sql"));
    fs::write(&path, AUTH_MIGRATION_SQL).with_context(|| format!("writing {}", path.display()))?;

    println!("Wrote {}", path.display());
    println!();
    println!("Add the runtime dependency if it is not present:");
    println!(
        "  comet-auth = {{ git = \"https://github.com/viniciusamelio/comet\", default-features = false, features = [\"cloudflare\"] }}"
    );
    println!();
    println!(
        "Ensure wrangler.jsonc has a D1 binding named `{db_binding}` and a KV namespace named `{kv_binding}`."
    );
    println!("Mount auth in Rocket with:");
    println!("  .attach(comet_auth::Auth::<{db_binding}, {kv_binding}>::fairing(auth_config))");
    println!("  .mount(\"/auth\", comet_auth::routes::<{db_binding}, {kv_binding}>())");

    Ok(())
}

fn auth_migration_exists(migrations_dir: &Path) -> Result<bool> {
    if !migrations_dir.exists() {
        return Ok(false);
    }

    for entry in fs::read_dir(migrations_dir)
        .with_context(|| format!("reading {}", migrations_dir.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|extension| extension.to_str()) != Some("sql") {
            continue;
        }

        let sql =
            fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
        if sql.contains("comet_auth_users") || sql.contains("comet_auth_sessions") {
            return Ok(true);
        }
    }

    Ok(false)
}

fn validate_binding(value: &str, label: &str) -> Result<String> {
    let trimmed = value.trim();

    if trimmed.is_empty() {
        bail!("{label} must not be blank");
    }

    let mut chars = trimmed.chars();
    let starts_valid = chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_');
    let rest_valid = chars.all(|c| c.is_ascii_alphanumeric() || c == '_');

    if !starts_valid || !rest_valid {
        bail!(
            "{label} `{trimmed}` must start with an ASCII letter or `_` and contain only ASCII letters, digits, or `_`"
        );
    }

    Ok(trimmed.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_auth_migration_at_next_sequence() {
        let dir = tempfile::tempdir().unwrap();
        let migrations_dir = dir.path().join("migrations");
        fs::create_dir_all(&migrations_dir).unwrap();
        fs::write(
            migrations_dir.join("0001_init.sql"),
            "create table tasks(id integer);",
        )
        .unwrap();

        init(AuthInitArgs {
            path: Some(dir.path().to_path_buf()),
            db_binding: "DB".to_owned(),
            kv_binding: "AUTH_KV".to_owned(),
        })
        .unwrap();

        let migration = fs::read_to_string(migrations_dir.join("0002_comet_auth.sql")).unwrap();
        assert!(migration.contains("comet_auth_users"));
        assert!(migration.contains("comet_auth_sessions"));
    }

    #[test]
    fn refuses_duplicate_auth_migration() {
        let dir = tempfile::tempdir().unwrap();
        let migrations_dir = dir.path().join("migrations");
        fs::create_dir_all(&migrations_dir).unwrap();
        fs::write(
            migrations_dir.join("0001_comet_auth.sql"),
            AUTH_MIGRATION_SQL,
        )
        .unwrap();

        let error = init(AuthInitArgs {
            path: Some(dir.path().to_path_buf()),
            db_binding: "DB".to_owned(),
            kv_binding: "AUTH_KV".to_owned(),
        })
        .unwrap_err();

        assert!(error.to_string().contains("already exists"));
    }
}
