use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};

use crate::casing::{pluralize, to_pascal_case, to_screaming_snake_case, to_snake_case};
use crate::cli::GenerateRouteArgs;
use crate::entity_introspect::{self, EntityField};
use crate::rustfile;

pub fn run(args: GenerateRouteArgs) -> Result<()> {
    let project_dir = args.path.unwrap_or_else(|| PathBuf::from("."));
    let concept = to_snake_case(&args.entity);
    if concept.is_empty() {
        bail!(
            "entity name `{}` must contain at least one letter",
            args.entity
        );
    }

    let context = pluralize(&concept);
    let struct_name = format!("{}Row", to_pascal_case(&concept));

    let context_dir = project_dir.join("src").join(&context);
    let model_rs_path = context_dir.join("model.rs");

    if !model_rs_path.exists() {
        bail!(
            "{} doesn't exist; run `comet generate entity {}` first",
            model_rs_path.display(),
            args.entity
        );
    }

    let fields = entity_introspect::read_entity_fields(&model_rs_path, &struct_name)?;
    let primary_key = fields
        .iter()
        .find(|field| field.is_primary_key)
        .with_context(|| format!("`{struct_name}` has no `#[nebula(primary_key)]` field"))?
        .clone();
    let creatable_fields = fields
        .iter()
        .filter(|field| !field.is_primary_key && !field.is_auto)
        .cloned()
        .collect::<Vec<_>>();

    let new_struct_name = format!("New{}", strip_row_suffix(&struct_name));
    let new_struct_marker = format!("struct {new_struct_name} ");
    let new_struct_block = render_new_struct(&new_struct_name, &creatable_fields);
    rustfile::append_block_if_missing(&model_rs_path, "", &new_struct_marker, &new_struct_block)
        .context("appending the create-request struct to model.rs")?;

    let routes_rs_path = context_dir.join("routes.rs");
    let error_rs_path = context_dir.join("error.rs");
    let mod_rs_path = context_dir.join("mod.rs");

    if routes_rs_path.exists() {
        bail!("{} already exists", routes_rs_path.display());
    }

    fs::write(&error_rs_path, render_error_rs())
        .with_context(|| format!("writing {}", error_rs_path.display()))?;
    fs::write(
        &routes_rs_path,
        render_routes_rs(
            &context,
            &concept,
            &struct_name,
            &new_struct_name,
            &primary_key,
            &fields,
            &creatable_fields,
            &args.db_binding,
        ),
    )
    .with_context(|| format!("writing {}", routes_rs_path.display()))?;

    rustfile::ensure_module_declared(&mod_rs_path, "routes")?;
    rustfile::ensure_module_declared(&mod_rs_path, "error")?;

    println!("Wrote {}", routes_rs_path.display());
    println!("Wrote {}", error_rs_path.display());
    println!();
    println!("Next steps — wire these into src/app.rs:");
    println!(
        "  use crate::{context}::routes::{{list_{context}, get_{concept}, create_{concept}, update_{concept}, delete_{concept}}};"
    );
    println!(
        "  // add list_{context}, get_{concept}, create_{concept}, update_{concept}, delete_{concept} to the routes![...] list"
    );

    Ok(())
}

fn strip_row_suffix(struct_name: &str) -> &str {
    struct_name.strip_suffix("Row").unwrap_or(struct_name)
}

fn render_new_struct(new_struct_name: &str, creatable_fields: &[EntityField]) -> String {
    let mut block = String::new();
    block.push_str("#[derive(Debug, Clone, Deserialize)]\n");
    block.push_str("#[serde(crate = \"rocket::serde\")]\n");
    block.push_str(&format!("pub struct {new_struct_name} {{\n"));
    for field in creatable_fields {
        block.push_str(&format!("    pub {}: {},\n", field.name, field.rust_type));
    }
    block.push('}');
    block
}

fn render_error_rs() -> String {
    "use rocket::http::Status;\n\
     use rocket::response::Responder;\n\
     use rocket::serde::json::Json;\n\
     use rocket::serde::Serialize;\n\
     use rocket::Request;\n\
     \n\
     #[derive(Debug)]\n\
     pub enum ApiError {\n\
     \x20   NotFound,\n\
     \x20   BadRequest(String),\n\
     \x20   Worker(worker::Error),\n\
     }\n\
     \n\
     impl From<worker::Error> for ApiError {\n\
     \x20   fn from(error: worker::Error) -> Self {\n\
     \x20       ApiError::Worker(error)\n\
     \x20   }\n\
     }\n\
     \n\
     #[derive(Serialize)]\n\
     #[serde(crate = \"rocket::serde\")]\n\
     struct ErrorBody {\n\
     \x20   error: String,\n\
     }\n\
     \n\
     impl<'r> Responder<'r, 'static> for ApiError {\n\
     \x20   fn respond_to(self, request: &'r Request<'_>) -> rocket::response::Result<'static> {\n\
     \x20       let (status, message) = match self {\n\
     \x20           ApiError::NotFound => (Status::NotFound, \"not found\".to_string()),\n\
     \x20           ApiError::BadRequest(message) => (Status::BadRequest, message),\n\
     \x20           ApiError::Worker(error) => (Status::InternalServerError, error.to_string()),\n\
     \x20       };\n\
     \n\
     \x20       Json(ErrorBody { error: message })\n\
     \x20           .respond_to(request)\n\
     \x20           .map(|mut response| {\n\
     \x20               response.set_status(status);\n\
     \x20               response\n\
     \x20           })\n\
     \x20   }\n\
     }\n\
     \n\
     pub type ApiResult<T> = Result<T, ApiError>;\n"
        .to_owned()
}

#[allow(clippy::too_many_arguments)]
fn render_routes_rs(
    context: &str,
    concept: &str,
    struct_name: &str,
    new_struct_name: &str,
    primary_key: &EntityField,
    all_fields: &[EntityField],
    creatable_fields: &[EntityField],
    db_binding: &str,
) -> String {
    let columns_const = format!("{}_COLUMNS", context.to_uppercase());
    let columns = all_fields
        .iter()
        .map(|field| format!("\"{}\"", field.name))
        .collect::<Vec<_>>()
        .join(", ");

    let set_lines = |target: &str| {
        creatable_fields
            .iter()
            .map(|field| {
                format!(
                    "        .set({struct_name}::{}, {target}.{})\n",
                    to_screaming_snake_case(&field.name),
                    field.name
                )
            })
            .collect::<String>()
    };

    let pk_upper = to_screaming_snake_case(&primary_key.name);
    let pk_type = &primary_key.rust_type;

    let mut model_imports = [struct_name, new_struct_name];
    model_imports.sort();
    let model_imports = model_imports.join(", ");

    format!(
        "use comet::cloudflare::{{BindingName, D1}};\n\
         use comet::nebula::Entity;\n\
         use rocket::http::Status;\n\
         use rocket::serde::json::Json;\n\
         \n\
         use crate::{context}::error::{{ApiError, ApiResult}};\n\
         use crate::{context}::model::{{{model_imports}}};\n\
         \n\
         const {columns_const}: &[&str] = &[{columns}];\n\
         \n\
         pub struct DB;\n\
         \n\
         impl BindingName for DB {{\n\
         \x20   const NAME: &'static str = \"{db_binding}\";\n\
         }}\n\
         \n\
         #[get(\"/{context}\")]\n\
         pub async fn list_{context}(db: D1<DB>) -> ApiResult<Json<Vec<{struct_name}>>> {{\n\
         \x20   let rows = {struct_name}::select()\n\
         \x20       .order_by({struct_name}::{pk_upper}.asc())\n\
         \x20       .limit(50)\n\
         \x20       .to_statement()\n\
         \x20       .fetch_all_d1(&db)\n\
         \x20       .await\n\
         \x20       .map_err(ApiError::from)?\n\
         \x20       .results::<{struct_name}>()\n\
         \x20       .map_err(ApiError::from)?;\n\
         \n\
         \x20   Ok(Json(rows))\n\
         }}\n\
         \n\
         #[get(\"/{context}/<id>\")]\n\
         pub async fn get_{concept}(id: {pk_type}, db: D1<DB>) -> ApiResult<Json<{struct_name}>> {{\n\
         \x20   let row = {struct_name}::select()\n\
         \x20       .where_({struct_name}::{pk_upper}.eq(id))\n\
         \x20       .to_statement()\n\
         \x20       .fetch_optional_d1::<{struct_name}>(&db)\n\
         \x20       .await\n\
         \x20       .map_err(ApiError::from)?\n\
         \x20       .ok_or(ApiError::NotFound)?;\n\
         \n\
         \x20   Ok(Json(row))\n\
         }}\n\
         \n\
         #[post(\"/{context}\", data = \"<new_{concept}>\")]\n\
         pub async fn create_{concept}(new_{concept}: Json<{new_struct_name}>, db: D1<DB>) -> ApiResult<Json<{struct_name}>> {{\n\
         \x20   let new_{concept} = new_{concept}.into_inner();\n\
         \x20   let row = {struct_name}::insert()\n\
         {create_set_lines}\
         \x20       .returning({columns_const}.iter().copied())\n\
         \x20       .to_statement()\n\
         \x20       .fetch_one_d1::<{struct_name}>(&db)\n\
         \x20       .await\n\
         \x20       .map_err(ApiError::from)?;\n\
         \n\
         \x20   Ok(Json(row))\n\
         }}\n\
         \n\
         #[put(\"/{context}/<id>\", data = \"<update_{concept}>\")]\n\
         pub async fn update_{concept}(\n\
         \x20   id: {pk_type},\n\
         \x20   update_{concept}: Json<{new_struct_name}>,\n\
         \x20   db: D1<DB>,\n\
         ) -> ApiResult<Json<{struct_name}>> {{\n\
         \x20   let update_{concept} = update_{concept}.into_inner();\n\
         \x20   let row = {struct_name}::update()\n\
         {update_set_lines}\
         \x20       .where_({struct_name}::{pk_upper}.eq(id))\n\
         \x20       .returning({columns_const}.iter().copied())\n\
         \x20       .to_statement()\n\
         \x20       .fetch_optional_d1::<{struct_name}>(&db)\n\
         \x20       .await\n\
         \x20       .map_err(ApiError::from)?\n\
         \x20       .ok_or(ApiError::NotFound)?;\n\
         \n\
         \x20   Ok(Json(row))\n\
         }}\n\
         \n\
         #[delete(\"/{context}/<id>\")]\n\
         pub async fn delete_{concept}(id: {pk_type}, db: D1<DB>) -> ApiResult<Status> {{\n\
         \x20   {struct_name}::delete()\n\
         \x20       .where_({struct_name}::{pk_upper}.eq(id))\n\
         \x20       .to_statement()\n\
         \x20       .execute_d1(&db)\n\
         \x20       .await\n\
         \x20       .map_err(ApiError::from)?;\n\
         \n\
         \x20   Ok(Status::NoContent)\n\
         }}\n",
        create_set_lines = set_lines(&format!("new_{concept}")),
        update_set_lines = set_lines(&format!("update_{concept}")),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::GenerateEntityArgs;
    use crate::commands::generate::entity;

    #[test]
    fn generates_crud_routes_from_a_discovered_entity() {
        let dir = tempfile::tempdir().unwrap();

        entity::run(GenerateEntityArgs {
            name: "Board".into(),
            fields: vec![
                "title:string".into(),
                "org_id:i64:foreign_key=orgs.id,index".into(),
            ],
            table: None,
            path: Some(dir.path().to_path_buf()),
        })
        .unwrap();

        run(GenerateRouteArgs {
            entity: "Board".into(),
            db_binding: "DB".into(),
            path: Some(dir.path().to_path_buf()),
        })
        .unwrap();

        let model = fs::read_to_string(dir.path().join("src/boards/model.rs")).unwrap();
        assert!(model.contains("pub struct NewBoard {"));
        assert!(model.contains("pub title: String,"));
        assert!(model.contains("pub org_id: i64,"));
        assert!(!model.contains("NewBoard {\n    pub id"));

        let routes = fs::read_to_string(dir.path().join("src/boards/routes.rs")).unwrap();
        assert!(routes.contains("pub async fn list_boards(db: D1<DB>)"));
        assert!(routes.contains("pub async fn get_board(id: i32, db: D1<DB>)"));
        assert!(
            routes.contains("pub async fn create_board(new_board: Json<NewBoard>, db: D1<DB>)")
        );
        assert!(routes.contains("pub async fn update_board("));
        assert!(routes.contains("pub async fn delete_board(id: i32, db: D1<DB>)"));
        assert!(routes.contains(".set(BoardRow::TITLE, new_board.title)"));
        assert!(routes.contains(".set(BoardRow::ORG_ID, new_board.org_id)"));
        assert!(
            routes.contains("const BOARDS_COLUMNS: &[&str] = &[\"id\", \"title\", \"org_id\"];")
        );

        let error = fs::read_to_string(dir.path().join("src/boards/error.rs")).unwrap();
        assert!(error.contains("pub enum ApiError"));

        let mod_rs = fs::read_to_string(dir.path().join("src/boards/mod.rs")).unwrap();
        assert!(mod_rs.contains("pub mod error;"));
        assert!(mod_rs.contains("pub mod model;"));
        assert!(mod_rs.contains("pub mod routes;"));
    }

    #[test]
    fn requires_the_entity_to_exist_first() {
        let dir = tempfile::tempdir().unwrap();

        let error = run(GenerateRouteArgs {
            entity: "Board".into(),
            db_binding: "DB".into(),
            path: Some(dir.path().to_path_buf()),
        })
        .unwrap_err();

        assert!(error.to_string().contains("generate entity Board"));
    }

    #[test]
    fn refuses_to_overwrite_existing_routes() {
        let dir = tempfile::tempdir().unwrap();
        entity::run(GenerateEntityArgs {
            name: "Board".into(),
            fields: Vec::new(),
            table: None,
            path: Some(dir.path().to_path_buf()),
        })
        .unwrap();

        let args = || GenerateRouteArgs {
            entity: "Board".into(),
            db_binding: "DB".into(),
            path: Some(dir.path().to_path_buf()),
        };
        run(args()).unwrap();
        let error = run(args()).unwrap_err();

        assert!(error.to_string().contains("already exists"));
    }
}
