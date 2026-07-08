use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use comet::nebula::schema::SchemaSnapshot;

/// Where the last-applied schema snapshot lives, alongside the generated
/// migration files it corresponds to.
pub fn snapshot_path(project_dir: &Path) -> PathBuf {
    project_dir.join("migrations").join(".comet-schema.json")
}

/// Returns `None` if no snapshot has been written yet (the project hasn't
/// run `comet migrate init`).
pub fn load_snapshot(path: &Path) -> Result<Option<SchemaSnapshot>> {
    if !path.exists() {
        return Ok(None);
    }

    let text = fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let snapshot =
        serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
    Ok(Some(snapshot))
}

pub fn write_snapshot(path: &Path, snapshot: &SchemaSnapshot) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("creating directory {}", parent.display()))?;
    }

    let text = serde_json::to_string_pretty(snapshot).context("serializing schema snapshot")?;
    fs::write(path, text).with_context(|| format!("writing {}", path.display()))
}

/// The sequence number the next migration file should use: one past the
/// highest `NNNN_*.sql` file already in `migrations_dir`, or `1` if the
/// directory doesn't exist or has none yet.
pub fn next_migration_sequence(migrations_dir: &Path) -> Result<u32> {
    if !migrations_dir.exists() {
        return Ok(1);
    }

    let mut max_sequence = 0u32;
    for entry in fs::read_dir(migrations_dir)
        .with_context(|| format!("reading {}", migrations_dir.display()))?
    {
        let entry = entry?;
        let file_name = entry.file_name();
        if let Some(sequence) = parse_migration_sequence(&file_name.to_string_lossy()) {
            max_sequence = max_sequence.max(sequence);
        }
    }

    Ok(max_sequence + 1)
}

fn parse_migration_sequence(file_name: &str) -> Option<u32> {
    let digits = file_name.get(0..4)?;
    if !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    if file_name.as_bytes().get(4) != Some(&b'_') {
        return None;
    }
    digits.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use comet::nebula::schema::OwnedTableDef;

    #[test]
    fn load_snapshot_returns_none_when_missing() {
        let dir = tempfile::tempdir().unwrap();
        let path = snapshot_path(dir.path());

        assert_eq!(load_snapshot(&path).unwrap(), None);
    }

    #[test]
    fn write_then_load_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let path = snapshot_path(dir.path());
        let snapshot = SchemaSnapshot {
            tables: vec![OwnedTableDef {
                name: "tasks".to_owned(),
                columns: Vec::new(),
                indexes: Vec::new(),
                foreign_keys: Vec::new(),
                rls: Vec::new(),
            }],
        };

        write_snapshot(&path, &snapshot).unwrap();
        let loaded = load_snapshot(&path).unwrap();

        assert_eq!(loaded, Some(snapshot));
    }

    #[test]
    fn next_migration_sequence_is_one_when_directory_is_missing() {
        let dir = tempfile::tempdir().unwrap();
        let migrations_dir = dir.path().join("migrations");

        assert_eq!(next_migration_sequence(&migrations_dir).unwrap(), 1);
    }

    #[test]
    fn next_migration_sequence_follows_the_highest_existing_number() {
        let dir = tempfile::tempdir().unwrap();
        let migrations_dir = dir.path().join("migrations");
        fs::create_dir_all(&migrations_dir).unwrap();
        fs::write(migrations_dir.join("0001_init.sql"), "").unwrap();
        fs::write(migrations_dir.join("0003_add_done.sql"), "").unwrap();
        fs::write(migrations_dir.join(".comet-schema.json"), "{}").unwrap();

        assert_eq!(next_migration_sequence(&migrations_dir).unwrap(), 4);
    }
}
