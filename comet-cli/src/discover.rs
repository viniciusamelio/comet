use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use syn::Item;

/// A `#[derive(Entity)]` struct found by scanning source files, identified
/// by its module path relative to the crate root (e.g. `["tasks", "model"]`
/// for a `TaskRow` declared in `src/tasks/model.rs`).
///
/// This deliberately does *not* parse `#[nebula(...)]` attributes (primary
/// keys, indexes, foreign keys, defaults...) — that parsing already exists,
/// battle-tested, in `comet-macros`. Duplicating it here would risk the CLI
/// and the derive macro disagreeing about what a struct's schema is.
/// Instead, discovery only needs enough to *reference* the struct from a
/// generated Rust file, which then reads the struct's real,
/// derive-generated `Entity::TABLE` by compiling and running (see the
/// schema-dump runner planned in `docs/comet-cli-tracker.md`, area C3).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct DiscoveredEntity {
    pub module_path: Vec<String>,
    pub struct_name: String,
}

impl DiscoveredEntity {
    /// The fully qualified path to this entity from outside the crate, e.g.
    /// `my_app::tasks::model::TaskRow` for `crate_name = "my_app"`.
    pub fn qualified_path(&self, crate_name: &str) -> String {
        let mut segments = vec![crate_name.to_owned()];
        segments.extend(self.module_path.iter().cloned());
        segments.push(self.struct_name.clone());
        segments.join("::")
    }
}

/// Recursively scans `src_dir` for `#[derive(Entity)]` structs (matching by
/// the derive path's last segment, so `Entity`, `nebula::Entity`, and
/// `comet::nebula::Entity` are all recognized regardless of how the caller
/// imported it).
pub fn discover_entities(src_dir: &Path) -> Result<Vec<DiscoveredEntity>> {
    let mut entities = Vec::new();
    visit_dir(src_dir, &[], &mut entities)?;
    entities.sort();
    Ok(entities)
}

fn visit_dir(
    dir: &Path,
    module_path: &[String],
    entities: &mut Vec<DiscoveredEntity>,
) -> Result<()> {
    let mut dir_entries = fs::read_dir(dir)
        .with_context(|| format!("reading directory {}", dir.display()))?
        .collect::<std::io::Result<Vec<_>>>()
        .with_context(|| format!("reading directory {}", dir.display()))?;
    dir_entries.sort_by_key(|entry| entry.file_name());

    for entry in dir_entries {
        let path = entry.path();
        let file_type = entry
            .file_type()
            .with_context(|| format!("reading file type of {}", path.display()))?;

        if file_type.is_dir() {
            let mut nested_path = module_path.to_vec();
            nested_path.push(entry.file_name().to_string_lossy().into_owned());
            visit_dir(&path, &nested_path, entities)?;
            continue;
        }

        if path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
            continue;
        }

        visit_file(&path, module_path, entities)?;
    }

    Ok(())
}

fn visit_file(
    path: &Path,
    module_path: &[String],
    entities: &mut Vec<DiscoveredEntity>,
) -> Result<()> {
    let stem = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or_default();

    // `mod.rs`, `lib.rs`, and `main.rs` declare items *at* `module_path`
    // rather than in a nested module named after the file.
    let file_module_path = match stem {
        "mod" | "lib" | "main" => module_path.to_vec(),
        _ => {
            let mut nested = module_path.to_vec();
            nested.push(stem.to_owned());
            nested
        }
    };

    let source = fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let file = syn::parse_file(&source).with_context(|| format!("parsing {}", path.display()))?;

    for item in &file.items {
        if let Item::Struct(item_struct) = item
            && has_entity_derive(&item_struct.attrs)
        {
            entities.push(DiscoveredEntity {
                module_path: file_module_path.clone(),
                struct_name: item_struct.ident.to_string(),
            });
        }
    }

    Ok(())
}

pub(crate) fn has_entity_derive(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| {
        if !attr.path().is_ident("derive") {
            return false;
        }

        let Ok(paths) = attr.parse_args_with(
            syn::punctuated::Punctuated::<syn::Path, syn::Token![,]>::parse_terminated,
        ) else {
            return false;
        };

        paths.iter().any(|path| {
            path.segments
                .last()
                .is_some_and(|segment| segment.ident == "Entity")
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, relative: &str, contents: &str) {
        let path = dir.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    #[test]
    fn finds_entity_structs_across_nested_modules() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path();

        write(src, "lib.rs", "mod tasks;\nmod orgs;\n");
        write(src, "tasks/mod.rs", "pub mod model;\n");
        write(
            src,
            "tasks/model.rs",
            r#"
            #[derive(Debug, Clone, comet::nebula::Entity)]
            #[nebula(table = "tasks")]
            pub struct TaskRow {
                pub id: i32,
            }

            #[derive(Debug, Clone)]
            pub struct NotAnEntity {
                pub id: i32,
            }
            "#,
        );
        write(
            src,
            "orgs/mod.rs",
            r#"
            use comet::nebula::Entity;

            #[derive(Debug, Clone, Entity)]
            #[nebula(table = "orgs")]
            pub struct OrgRow {
                pub id: i32,
            }
            "#,
        );

        let mut entities = discover_entities(src).unwrap();
        entities.sort();

        assert_eq!(
            entities,
            vec![
                DiscoveredEntity {
                    module_path: vec!["orgs".to_owned()],
                    struct_name: "OrgRow".to_owned(),
                },
                DiscoveredEntity {
                    module_path: vec!["tasks".to_owned(), "model".to_owned()],
                    struct_name: "TaskRow".to_owned(),
                },
            ]
        );
    }

    #[test]
    fn qualified_path_prefixes_the_crate_name() {
        let entity = DiscoveredEntity {
            module_path: vec!["tasks".to_owned(), "model".to_owned()],
            struct_name: "TaskRow".to_owned(),
        };

        assert_eq!(
            entity.qualified_path("my_app"),
            "my_app::tasks::model::TaskRow"
        );
    }

    #[test]
    fn fails_loudly_on_unparseable_source_files() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path();

        write(src, "broken.rs", "this is not valid rust {{{");

        let error = discover_entities(src).unwrap_err();
        assert!(error.to_string().contains("parsing"));
    }
}
