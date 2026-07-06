use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result};

/// Ensures `mod_rs_path` declares `pub mod <module_name>;`, alongside
/// whatever `pub mod` declarations are already there. Regenerates the whole
/// file from the resulting set (sorted, so output is deterministic) rather
/// than appending a line — these files only ever contain `pub mod` lines, so
/// this stays simple and is idempotent against re-running the same command.
pub fn ensure_module_declared(mod_rs_path: &Path, module_name: &str) -> Result<()> {
    let mut modules = read_pub_mod_declarations(mod_rs_path)?;
    modules.insert(module_name.to_owned());

    let contents = modules
        .iter()
        .map(|module| format!("pub mod {module};\n"))
        .collect::<String>();

    if let Some(parent) = mod_rs_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("creating directory {}", parent.display()))?;
    }
    fs::write(mod_rs_path, contents).with_context(|| format!("writing {}", mod_rs_path.display()))
}

fn read_pub_mod_declarations(path: &Path) -> Result<BTreeSet<String>> {
    if !path.exists() {
        return Ok(BTreeSet::new());
    }

    let text = fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    Ok(text
        .lines()
        .filter_map(|line| {
            let name = line.trim().strip_prefix("pub mod ")?;
            name.strip_suffix(';').map(str::to_owned)
        })
        .collect())
}

/// Appends `block` to `path` (creating it with `header` first if it doesn't
/// exist yet) unless `marker` is already present in the file's contents, in
/// which case this is a no-op. Returns whether it wrote anything.
pub fn append_block_if_missing(
    path: &Path,
    header: &str,
    marker: &str,
    block: &str,
) -> Result<bool> {
    let existing = if path.exists() {
        fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?
    } else {
        String::new()
    };

    if existing.contains(marker) {
        return Ok(false);
    }

    let updated = if existing.is_empty() {
        format!("{header}\n{block}\n")
    } else {
        format!("{existing}\n{block}\n")
    };

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("creating directory {}", parent.display()))?;
    }
    fs::write(path, updated).with_context(|| format!("writing {}", path.display()))?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ensure_module_declared_creates_a_fresh_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mod.rs");

        ensure_module_declared(&path, "model").unwrap();

        assert_eq!(fs::read_to_string(&path).unwrap(), "pub mod model;\n");
    }

    #[test]
    fn ensure_module_declared_adds_to_existing_declarations_sorted() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mod.rs");
        fs::write(&path, "pub mod model;\n").unwrap();

        ensure_module_declared(&path, "error").unwrap();
        ensure_module_declared(&path, "routes").unwrap();

        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "pub mod error;\npub mod model;\npub mod routes;\n"
        );
    }

    #[test]
    fn ensure_module_declared_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mod.rs");

        ensure_module_declared(&path, "model").unwrap();
        ensure_module_declared(&path, "model").unwrap();

        assert_eq!(fs::read_to_string(&path).unwrap(), "pub mod model;\n");
    }

    #[test]
    fn append_block_if_missing_creates_with_header() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("model.rs");

        let wrote = append_block_if_missing(
            &path,
            "use rocket::serde::Deserialize;",
            "struct Foo",
            "pub struct Foo {}",
        )
        .unwrap();

        assert!(wrote);
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "use rocket::serde::Deserialize;\npub struct Foo {}\n"
        );
    }

    #[test]
    fn append_block_if_missing_skips_when_marker_present() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("model.rs");
        fs::write(&path, "pub struct Foo {}\n").unwrap();

        let wrote =
            append_block_if_missing(&path, "unused", "struct Foo", "pub struct Foo {}").unwrap();

        assert!(!wrote);
        assert_eq!(fs::read_to_string(&path).unwrap(), "pub struct Foo {}\n");
    }
}
