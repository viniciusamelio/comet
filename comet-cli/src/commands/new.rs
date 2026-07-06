use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::cli::NewArgs;

struct TemplateVars {
    project_name: String,
    db_binding: String,
}

pub fn run(args: NewArgs) -> Result<()> {
    let project_name = validate_identifier(&args.name, "project name")?;
    let db_binding = validate_identifier(&args.db_binding, "--db-binding")?;
    let root = args.path.unwrap_or_else(|| PathBuf::from(&project_name));

    if root.exists() {
        bail!(
            "`{}` already exists; choose a different name or --path",
            root.display()
        );
    }

    let vars = TemplateVars {
        project_name,
        db_binding,
    };

    write_rendered(
        &root,
        "Cargo.toml",
        include_str!("../../templates/Cargo.toml.tmpl"),
        &vars,
    )?;
    write_rendered(
        &root,
        "wrangler.jsonc",
        include_str!("../../templates/wrangler.jsonc.tmpl"),
        &vars,
    )?;
    write_rendered(
        &root,
        "package.json",
        include_str!("../../templates/package.json.tmpl"),
        &vars,
    )?;
    write_rendered(
        &root,
        "README.md",
        include_str!("../../templates/README.md.tmpl"),
        &vars,
    )?;
    write_rendered(
        &root,
        ".gitignore",
        include_str!("../../templates/gitignore.tmpl"),
        &vars,
    )?;
    write_rendered(
        &root,
        "src/lib.rs",
        include_str!("../../templates/src_lib.rs.tmpl"),
        &vars,
    )?;
    write_rendered(
        &root,
        "src/entry.rs",
        include_str!("../../templates/src_entry.rs.tmpl"),
        &vars,
    )?;
    write_rendered(
        &root,
        "src/app.rs",
        include_str!("../../templates/src_app.rs.tmpl"),
        &vars,
    )?;
    write_rendered(
        &root,
        "src/tasks/mod.rs",
        include_str!("../../templates/tasks_mod.rs.tmpl"),
        &vars,
    )?;
    write_rendered(
        &root,
        "src/tasks/model.rs",
        include_str!("../../templates/tasks_model.rs.tmpl"),
        &vars,
    )?;
    write_rendered(
        &root,
        "src/tasks/routes.rs",
        include_str!("../../templates/tasks_routes.rs.tmpl"),
        &vars,
    )?;
    write_rendered(
        &root,
        "src/tasks/error.rs",
        include_str!("../../templates/tasks_error.rs.tmpl"),
        &vars,
    )?;

    println!(
        "Created `{}` in {}",
        vars_project_name(&root),
        root.display()
    );
    println!();
    println!("Next steps:");
    println!("  cd {}", root.display());
    println!("  comet migrate init");
    println!("  npm install");
    println!("  npm run dev");

    Ok(())
}

fn vars_project_name(root: &Path) -> String {
    root.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| root.display().to_string())
}

fn render(template: &str, vars: &TemplateVars) -> String {
    template
        .replace("{{project_name}}", &vars.project_name)
        .replace("{{db_binding}}", &vars.db_binding)
}

fn write_rendered(root: &Path, relative: &str, template: &str, vars: &TemplateVars) -> Result<()> {
    let path = root.join(relative);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("creating directory {}", parent.display()))?;
    }
    fs::write(&path, render(template, vars))
        .with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}

/// Accepts non-empty names built from ASCII letters, digits, `-`, and `_`,
/// starting with a letter. This is stricter than Cargo package-name rules
/// need to be, but it's a hard rule that also rejects path separators and
/// `..`, which matters since `project_name` and `db_binding` both end up in
/// file paths (`root`) or generated source/config, never in SQL.
fn validate_identifier(value: &str, label: &str) -> Result<String> {
    let trimmed = value.trim();

    if trimmed.is_empty() {
        bail!("{label} must not be blank");
    }

    let mut chars = trimmed.chars();
    let starts_with_letter = chars.next().is_some_and(|c| c.is_ascii_alphabetic());
    let rest_is_valid = chars.all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');

    if !starts_with_letter || !rest_is_valid {
        bail!(
            "{label} `{trimmed}` must start with a letter and contain only ASCII letters, digits, `-`, or `_`"
        );
    }

    Ok(trimmed.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scaffolds_expected_files_with_substitutions() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("my_app");

        run(NewArgs {
            name: "my_app".into(),
            path: Some(root.clone()),
            db_binding: "DB".into(),
        })
        .unwrap();

        let cargo_toml = fs::read_to_string(root.join("Cargo.toml")).unwrap();
        assert!(cargo_toml.contains("name = \"my_app\""));

        let wrangler = fs::read_to_string(root.join("wrangler.jsonc")).unwrap();
        assert!(wrangler.contains("\"name\": \"my_app\""));
        assert!(wrangler.contains("\"binding\": \"DB\""));

        let routes = fs::read_to_string(root.join("src/tasks/routes.rs")).unwrap();
        assert!(routes.contains("const NAME: &'static str = \"DB\";"));

        assert!(root.join("src/app.rs").is_file());
        assert!(root.join("src/entry.rs").is_file());
        assert!(root.join("src/tasks/mod.rs").is_file());
        assert!(root.join("src/tasks/model.rs").is_file());
        assert!(root.join("src/tasks/error.rs").is_file());

        // Migrations are generated by `comet migrate init`, not scaffolded
        // statically — a pre-written `0001_init.sql` would collide with the
        // first real migration `migrate init` writes, since both would
        // create the same table.
        assert!(!root.join("migrations").exists());
    }

    #[test]
    fn refuses_to_overwrite_an_existing_directory() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("taken");
        fs::create_dir(&root).unwrap();

        let error = run(NewArgs {
            name: "taken".into(),
            path: Some(root),
            db_binding: "DB".into(),
        })
        .unwrap_err();

        assert!(error.to_string().contains("already exists"));
    }

    #[test]
    fn rejects_path_like_project_names() {
        let error = validate_identifier("../escape", "project name").unwrap_err();
        assert!(error.to_string().contains("must start with a letter"));
    }

    #[test]
    fn rejects_blank_project_names() {
        let error = validate_identifier("   ", "project name").unwrap_err();
        assert!(error.to_string().contains("must not be blank"));
    }
}
