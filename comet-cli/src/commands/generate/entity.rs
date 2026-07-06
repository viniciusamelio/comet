use std::fs;
use std::path::PathBuf;

use anyhow::{Result, bail};

use crate::casing::{pluralize, to_pascal_case, to_snake_case};
use crate::cli::GenerateEntityArgs;
use crate::fieldspec::{FieldSpec, parse_field};
use crate::rustfile;

pub fn run(args: GenerateEntityArgs) -> Result<()> {
    let project_dir = args.path.unwrap_or_else(|| PathBuf::from("."));
    let concept = to_snake_case(&args.name);
    if concept.is_empty() {
        bail!(
            "entity name `{}` must contain at least one letter",
            args.name
        );
    }

    let context = pluralize(&concept);
    let struct_name = format!("{}Row", to_pascal_case(&concept));
    let table = args.table.unwrap_or_else(|| context.clone());

    let mut fields = args
        .fields
        .iter()
        .map(|spec| parse_field(spec))
        .collect::<Result<Vec<_>>>()?;

    if !fields
        .iter()
        .any(|field| field.name == "id" || field.is_primary_key())
    {
        fields.insert(0, implicit_id_field());
    }

    let context_dir = project_dir.join("src").join(&context);
    let model_rs_path = context_dir.join("model.rs");
    let mod_rs_path = context_dir.join("mod.rs");

    let marker = format!("struct {struct_name} ");
    let block = render_struct(&struct_name, &table, &fields);

    let wrote = rustfile::append_block_if_missing(
        &model_rs_path,
        "use rocket::serde::{Deserialize, Serialize};",
        &marker,
        &block,
    )?;
    if !wrote {
        bail!(
            "`{struct_name}` already exists in {}",
            model_rs_path.display()
        );
    }

    rustfile::ensure_module_declared(&mod_rs_path, "model")?;

    let lib_rs_path = project_dir.join("src").join("lib.rs");
    let needs_lib_wiring = match fs::read_to_string(&lib_rs_path) {
        Ok(contents) => !contents.contains(&format!("mod {context}")),
        Err(_) => true,
    };

    println!("Wrote {}", model_rs_path.display());
    println!();
    println!("Next steps:");
    if needs_lib_wiring {
        println!("  Add `pub mod {context};` to src/lib.rs");
    }
    println!(
        "  Run `comet generate route {}` to scaffold CRUD routes.",
        args.name
    );

    Ok(())
}

fn implicit_id_field() -> FieldSpec {
    FieldSpec {
        name: "id".to_owned(),
        rust_type: "i32".to_owned(),
        attrs: vec![
            "primary_key".to_owned(),
            "auto".to_owned(),
            "unique".to_owned(),
            "index".to_owned(),
        ],
        comment: None,
    }
}

fn render_struct(struct_name: &str, table: &str, fields: &[FieldSpec]) -> String {
    let mut block = String::new();
    block.push_str("#[derive(Debug, Clone, Serialize, Deserialize, comet::nebula::Entity)]\n");
    block.push_str(&format!("#[nebula(table = \"{table}\")]\n"));
    block.push_str("#[serde(crate = \"rocket::serde\")]\n");
    block.push_str(&format!("pub struct {struct_name} {{\n"));

    for field in fields {
        if let Some(comment) = field.comment {
            block.push_str(&format!("    // {comment}\n"));
        }
        if !field.attrs.is_empty() {
            block.push_str(&format!("    #[nebula({})]\n", field.attrs.join(", ")));
        }
        block.push_str(&format!("    pub {}: {},\n", field.name, field.rust_type));
    }

    block.push('}');
    block
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scaffolds_a_new_context_with_an_implicit_id() {
        let dir = tempfile::tempdir().unwrap();

        run(GenerateEntityArgs {
            name: "Board".into(),
            fields: vec![
                "title:string".into(),
                "org_id:i64:foreign_key=orgs.id,index".into(),
            ],
            table: None,
            path: Some(dir.path().to_path_buf()),
        })
        .unwrap();

        let model = fs::read_to_string(dir.path().join("src/boards/model.rs")).unwrap();
        assert!(model.contains("pub struct BoardRow {"));
        assert!(model.contains("#[nebula(table = \"boards\")]"));
        assert!(model.contains("#[nebula(primary_key, auto, unique, index)]"));
        assert!(model.contains("pub id: i32,"));
        assert!(model.contains("pub title: String,"));
        assert!(model.contains("#[nebula(foreign_key = \"orgs.id\", index)]"));
        assert!(model.contains("pub org_id: i64,"));

        let mod_rs = fs::read_to_string(dir.path().join("src/boards/mod.rs")).unwrap();
        assert_eq!(mod_rs, "pub mod model;\n");
    }

    #[test]
    fn refuses_to_duplicate_an_existing_entity() {
        let dir = tempfile::tempdir().unwrap();
        let args = || GenerateEntityArgs {
            name: "Board".into(),
            fields: Vec::new(),
            table: None,
            path: Some(dir.path().to_path_buf()),
        };

        run(args()).unwrap();
        let error = run(args()).unwrap_err();

        assert!(error.to_string().contains("already exists"));
    }

    #[test]
    fn respects_an_explicit_id_field_and_table_override() {
        let dir = tempfile::tempdir().unwrap();

        run(GenerateEntityArgs {
            name: "Board".into(),
            fields: vec!["id:i64:primary_key,auto".into()],
            table: Some("custom_boards".into()),
            path: Some(dir.path().to_path_buf()),
        })
        .unwrap();

        let model = fs::read_to_string(dir.path().join("src/boards/model.rs")).unwrap();
        assert!(model.contains("#[nebula(table = \"custom_boards\")]"));
        assert!(model.contains("pub id: i64,"));
        assert_eq!(model.matches("pub id:").count(), 1);
    }

    #[test]
    fn does_not_add_an_implicit_id_when_another_field_is_the_primary_key() {
        let dir = tempfile::tempdir().unwrap();

        run(GenerateEntityArgs {
            name: "Board".into(),
            fields: vec!["board_key:string:primary_key,unique".into()],
            table: None,
            path: Some(dir.path().to_path_buf()),
        })
        .unwrap();

        let model = fs::read_to_string(dir.path().join("src/boards/model.rs")).unwrap();
        assert!(!model.contains("pub id:"));
        assert!(model.contains("pub board_key: String,"));
    }
}
