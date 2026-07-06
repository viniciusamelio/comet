use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};
use syn::{Item, Meta};

use crate::discover::has_entity_derive;

/// A field read back from an already-generated `#[derive(Entity)]` struct,
/// used by `comet generate route` to build CRUD handlers without needing
/// the caller to redeclare the entity's shape.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntityField {
    pub name: String,
    pub rust_type: String,
    pub is_primary_key: bool,
    pub is_auto: bool,
}

/// Reads `struct_name`'s fields out of `model_rs_path` by parsing it with
/// `syn` — the same approach discovery (C2) uses, for the same reason:
/// reusing the real, already-generated source instead of asking the caller
/// to describe the entity a second time keeps the two from drifting apart.
pub fn read_entity_fields(model_rs_path: &Path, struct_name: &str) -> Result<Vec<EntityField>> {
    let source = fs::read_to_string(model_rs_path)
        .with_context(|| format!("reading {}", model_rs_path.display()))?;
    let file =
        syn::parse_file(&source).with_context(|| format!("parsing {}", model_rs_path.display()))?;

    for item in &file.items {
        let Item::Struct(item_struct) = item else {
            continue;
        };
        if item_struct.ident != struct_name {
            continue;
        }
        if !has_entity_derive(&item_struct.attrs) {
            bail!(
                "`{struct_name}` in {} doesn't derive `Entity`",
                model_rs_path.display()
            );
        }

        return item_struct
            .fields
            .iter()
            .map(|field| {
                let name = field
                    .ident
                    .as_ref()
                    .with_context(|| format!("struct `{struct_name}` has an unnamed field"))?
                    .to_string();
                let ty = &field.ty;
                let rust_type = quote::quote!(#ty).to_string();
                let (is_primary_key, is_auto) = nebula_flags(&field.attrs);

                Ok(EntityField {
                    name,
                    rust_type,
                    is_primary_key,
                    is_auto,
                })
            })
            .collect();
    }

    bail!(
        "struct `{struct_name}` not found in {}",
        model_rs_path.display()
    )
}

fn nebula_flags(attrs: &[syn::Attribute]) -> (bool, bool) {
    let mut is_primary_key = false;
    let mut is_auto = false;

    for attr in attrs {
        if !attr.path().is_ident("nebula") {
            continue;
        }

        let Ok(metas) = attr
            .parse_args_with(syn::punctuated::Punctuated::<Meta, syn::Token![,]>::parse_terminated)
        else {
            continue;
        };

        for meta in metas {
            if let Meta::Path(path) = meta {
                if path.is_ident("primary_key") {
                    is_primary_key = true;
                } else if path.is_ident("auto") || path.is_ident("auto_increment") {
                    is_auto = true;
                }
            }
        }
    }

    (is_primary_key, is_auto)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, contents: &str) -> std::path::PathBuf {
        let path = dir.join("model.rs");
        fs::write(&path, contents).unwrap();
        path
    }

    #[test]
    fn reads_fields_and_flags() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(
            dir.path(),
            r#"
            #[derive(Debug, Clone, Serialize, Deserialize, comet::nebula::Entity)]
            #[nebula(table = "tasks")]
            pub struct TaskRow {
                #[nebula(primary_key, auto, unique, index)]
                pub id: i32,
                pub title: String,
                #[nebula(foreign_key = "orgs.id", index)]
                pub org_id: i64,
            }
            "#,
        );

        let fields = read_entity_fields(&path, "TaskRow").unwrap();

        assert_eq!(
            fields,
            vec![
                EntityField {
                    name: "id".into(),
                    rust_type: "i32".into(),
                    is_primary_key: true,
                    is_auto: true,
                },
                EntityField {
                    name: "title".into(),
                    rust_type: "String".into(),
                    is_primary_key: false,
                    is_auto: false,
                },
                EntityField {
                    name: "org_id".into(),
                    rust_type: "i64".into(),
                    is_primary_key: false,
                    is_auto: false,
                },
            ]
        );
    }

    #[test]
    fn errors_when_struct_is_missing() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(dir.path(), "pub struct Other {}\n");

        let error = read_entity_fields(&path, "TaskRow").unwrap_err();
        assert!(error.to_string().contains("not found"));
    }

    #[test]
    fn errors_when_struct_does_not_derive_entity() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(
            dir.path(),
            "#[derive(Debug, Clone)]\npub struct TaskRow { pub id: i32 }\n",
        );

        let error = read_entity_fields(&path, "TaskRow").unwrap_err();
        assert!(error.to_string().contains("doesn't derive"));
    }
}
