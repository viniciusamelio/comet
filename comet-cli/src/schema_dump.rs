use std::fs;
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};
use comet::nebula::schema::SchemaSnapshot;
use toml::Value;

use crate::discover::DiscoveredEntity;

/// Compiles and runs a throwaway crate that reads the *real*,
/// derive-generated `Entity::TABLE` for each discovered entity and prints it
/// as JSON, then parses that JSON back into a [`SchemaSnapshot`].
///
/// The throwaway crate must resolve to the *exact same* `comet` package
/// instance the target project uses — not a second copy — or `TaskRow`'s
/// `Entity` impl in one `comet` and `SchemaManifest` in another would be
/// incompatible types despite being structurally identical. So rather than
/// declaring a fresh `comet` dependency, this copies the target project's own
/// `[dependencies].comet` entry verbatim (same git rev / path / version) and
/// only adds the `nebula-schema` feature to it, guaranteeing Cargo resolves
/// it as a single, unified instance.
pub fn dump_schema(project_dir: &Path, entities: &[DiscoveredEntity]) -> Result<SchemaSnapshot> {
    if entities.is_empty() {
        bail!(
            "no `#[derive(Entity)]` structs found under {}",
            project_dir.join("src").display()
        );
    }

    let project_dir = project_dir
        .canonicalize()
        .with_context(|| format!("resolving {}", project_dir.display()))?;
    let cargo_toml_path = project_dir.join("Cargo.toml");
    let cargo_toml_text = fs::read_to_string(&cargo_toml_path)
        .with_context(|| format!("reading {}", cargo_toml_path.display()))?;
    let root: Value = toml::from_str(&cargo_toml_text)
        .with_context(|| format!("parsing {}", cargo_toml_path.display()))?;

    let package_name = root
        .get("package")
        .and_then(|package| package.get("name"))
        .and_then(Value::as_str)
        .with_context(|| format!("{} has no [package].name", cargo_toml_path.display()))?
        .to_owned();
    let crate_name = package_name.replace('-', "_");

    let comet_dependency = root
        .get("dependencies")
        .and_then(|deps| deps.get("comet"))
        .with_context(|| {
            format!(
                "{} has no [dependencies].comet entry",
                cargo_toml_path.display()
            )
        })?;
    let mut comet_dependency_with_schema = add_nebula_schema_feature(comet_dependency)?;
    resolve_relative_path(&mut comet_dependency_with_schema, &project_dir)?;

    let temp_dir = tempfile::tempdir().context("creating schema-dump temp directory")?;
    let dump_manifest =
        build_dump_manifest(&package_name, &project_dir, &comet_dependency_with_schema)?;
    fs::write(temp_dir.path().join("Cargo.toml"), dump_manifest)
        .context("writing schema-dump Cargo.toml")?;
    fs::create_dir_all(temp_dir.path().join("src")).context("creating schema-dump src/")?;
    fs::write(
        temp_dir.path().join("src/main.rs"),
        render_main_rs(entities, &crate_name),
    )
    .context("writing schema-dump main.rs")?;

    let output = Command::new("cargo")
        .arg("run")
        .arg("--quiet")
        .current_dir(temp_dir.path())
        .output()
        .context("running cargo for the schema-dump crate")?;

    if !output.status.success() {
        bail!(
            "schema dump failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let json = String::from_utf8(output.stdout).context("schema dump produced non-UTF8 output")?;
    serde_json::from_str(&json).context("parsing schema dump JSON")
}

/// Returns a copy of `comet_dependency` (a version string or a table) as a
/// table with `"nebula-schema"` added to its `features` list, leaving every
/// other key (`git`, `rev`, `branch`, `path`, `version`, ...) untouched.
fn add_nebula_schema_feature(comet_dependency: &Value) -> Result<Value> {
    let mut table = match comet_dependency {
        Value::String(version) => {
            let mut table = toml::Table::new();
            table.insert("version".to_owned(), Value::String(version.clone()));
            table
        }
        Value::Table(table) => table.clone(),
        other => bail!("unsupported `comet` dependency format in Cargo.toml: {other:?}"),
    };

    let features = table
        .entry("features")
        .or_insert_with(|| Value::Array(Vec::new()));
    let Value::Array(features) = features else {
        bail!("`comet` dependency's `features` key must be an array");
    };
    if !features
        .iter()
        .any(|feature| feature.as_str() == Some("nebula-schema"))
    {
        features.push(Value::String("nebula-schema".to_owned()));
    }

    Ok(Value::Table(table))
}

/// If `dependency` has a `path` key with a relative path, rewrites it to an
/// absolute path resolved against `base_dir` (the target project's own
/// directory). The throwaway crate lives elsewhere (a temp directory), so a
/// path copied verbatim — e.g. `path = "../.."` — would otherwise resolve
/// relative to the wrong location.
fn resolve_relative_path(dependency: &mut Value, base_dir: &Path) -> Result<()> {
    let Value::Table(table) = dependency else {
        return Ok(());
    };
    let Some(Value::String(path_str)) = table.get("path").cloned() else {
        return Ok(());
    };

    let path = Path::new(&path_str);
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        base_dir.join(path)
    };
    let absolute = absolute
        .canonicalize()
        .with_context(|| format!("resolving comet path dependency {}", absolute.display()))?;

    table.insert(
        "path".to_owned(),
        Value::String(absolute.display().to_string()),
    );
    Ok(())
}

fn build_dump_manifest(
    target_package_name: &str,
    target_project_dir: &Path,
    comet_dependency: &Value,
) -> Result<String> {
    let mut target_dependency = toml::Table::new();
    target_dependency.insert(
        "path".to_owned(),
        Value::String(target_project_dir.display().to_string()),
    );

    let mut dependencies = toml::Table::new();
    dependencies.insert(
        target_package_name.to_owned(),
        Value::Table(target_dependency),
    );
    dependencies.insert("comet".to_owned(), comet_dependency.clone());
    dependencies.insert("serde_json".to_owned(), Value::String("1".to_owned()));

    let mut package = toml::Table::new();
    package.insert(
        "name".to_owned(),
        Value::String("comet-schema-dump".to_owned()),
    );
    package.insert("version".to_owned(), Value::String("0.0.0".to_owned()));
    package.insert("edition".to_owned(), Value::String("2021".to_owned()));
    package.insert("publish".to_owned(), Value::Boolean(false));

    let mut root = toml::Table::new();
    root.insert("package".to_owned(), Value::Table(package));
    root.insert("dependencies".to_owned(), Value::Table(dependencies));

    toml::to_string_pretty(&Value::Table(root)).context("rendering schema-dump Cargo.toml")
}

fn render_main_rs(entities: &[DiscoveredEntity], crate_name: &str) -> String {
    let table_exprs = entities
        .iter()
        .map(|entity| {
            format!(
                "        <{path} as ::comet::nebula::Entity>::TABLE,",
                path = entity.qualified_path(crate_name)
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        "fn main() {{\n\
         \x20   let manifest = ::comet::nebula::SchemaManifest::from_entities([\n\
         {table_exprs}\n\
         \x20   ]);\n\
         \x20   let snapshot = ::comet::nebula::schema::SchemaSnapshot::from_manifest(&manifest);\n\
         \x20   print!(\"{{}}\", ::serde_json::to_string(&snapshot).unwrap());\n\
         }}\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_a_relative_path_against_the_target_project_dir() {
        let base_dir = tempfile::tempdir().unwrap();
        let target = base_dir.path().join("../..").canonicalize().unwrap();

        let mut dependency = Value::Table({
            let mut table = toml::Table::new();
            table.insert("path".to_owned(), Value::String("../..".to_owned()));
            table
        });

        resolve_relative_path(&mut dependency, base_dir.path()).unwrap();

        let Value::Table(table) = dependency else {
            panic!("expected a table");
        };
        assert_eq!(
            table.get("path").and_then(Value::as_str),
            Some(target.display().to_string().as_str())
        );
    }

    #[test]
    fn leaves_an_absolute_path_dependency_untouched() {
        let absolute = tempfile::tempdir().unwrap();
        let absolute_path = absolute.path().canonicalize().unwrap();

        let mut dependency = Value::Table({
            let mut table = toml::Table::new();
            table.insert(
                "path".to_owned(),
                Value::String(absolute_path.display().to_string()),
            );
            table
        });

        resolve_relative_path(&mut dependency, Path::new("/irrelevant")).unwrap();

        let Value::Table(table) = dependency else {
            panic!("expected a table");
        };
        assert_eq!(
            table.get("path").and_then(Value::as_str),
            Some(absolute_path.display().to_string().as_str())
        );
    }

    #[test]
    fn adds_nebula_schema_feature_to_a_table_dependency() {
        let mut original = toml::Table::new();
        original.insert(
            "git".to_owned(),
            Value::String("https://example.com/comet".to_owned()),
        );
        original.insert(
            "features".to_owned(),
            Value::Array(vec![Value::String("cloudflare".to_owned())]),
        );

        let updated = add_nebula_schema_feature(&Value::Table(original)).unwrap();

        let Value::Table(table) = updated else {
            panic!("expected a table");
        };
        assert_eq!(
            table.get("git").and_then(Value::as_str),
            Some("https://example.com/comet")
        );
        let Some(Value::Array(features)) = table.get("features") else {
            panic!("expected features array");
        };
        let feature_names = features
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>();
        assert_eq!(feature_names, vec!["cloudflare", "nebula-schema"]);
    }

    #[test]
    fn adds_nebula_schema_feature_to_a_version_string_dependency() {
        let updated = add_nebula_schema_feature(&Value::String("1.0".to_owned())).unwrap();

        let Value::Table(table) = updated else {
            panic!("expected a table");
        };
        assert_eq!(table.get("version").and_then(Value::as_str), Some("1.0"));
        let Some(Value::Array(features)) = table.get("features") else {
            panic!("expected features array");
        };
        assert_eq!(
            features
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>(),
            vec!["nebula-schema"]
        );
    }

    #[test]
    fn does_not_duplicate_an_already_requested_feature() {
        let mut original = toml::Table::new();
        original.insert(
            "features".to_owned(),
            Value::Array(vec![Value::String("nebula-schema".to_owned())]),
        );

        let updated = add_nebula_schema_feature(&Value::Table(original)).unwrap();

        let Value::Table(table) = updated else {
            panic!("expected a table");
        };
        let Some(Value::Array(features)) = table.get("features") else {
            panic!("expected features array");
        };
        assert_eq!(features.len(), 1);
    }

    #[test]
    fn renders_fully_qualified_table_expressions() {
        let entities = vec![
            DiscoveredEntity {
                module_path: vec!["tasks".to_owned(), "model".to_owned()],
                struct_name: "TaskRow".to_owned(),
            },
            DiscoveredEntity {
                module_path: vec!["orgs".to_owned()],
                struct_name: "OrgRow".to_owned(),
            },
        ];

        let main_rs = render_main_rs(&entities, "my_app");

        assert!(
            main_rs.contains("<my_app::tasks::model::TaskRow as ::comet::nebula::Entity>::TABLE")
        );
        assert!(main_rs.contains("<my_app::orgs::OrgRow as ::comet::nebula::Entity>::TABLE"));
        assert!(main_rs.contains("::comet::nebula::schema::SchemaSnapshot::from_manifest"));
    }
}
