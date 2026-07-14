use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};

use crate::cli::{RpcGenerateArgs, RpcLanguage, RpcManifestArgs};
use crate::rpc;

pub fn manifest(args: RpcManifestArgs) -> Result<()> {
    let project_dir = args.path.unwrap_or_else(|| PathBuf::from("."));
    let manifest = rpc::discover_manifest(&project_dir)?;
    if manifest.routes.is_empty() {
        bail!(
            "no Rocket route attributes found under {}; RPC manifest generation needs at least one #[get]/#[post]/#[put]/#[delete]/#[patch] route",
            project_dir.join("src").display()
        );
    }
    let json = serde_json::to_string_pretty(&manifest).context("serializing RPC manifest")?;

    match args.out {
        Some(path) => {
            if let Some(parent) = path.parent()
                && !parent.as_os_str().is_empty()
            {
                fs::create_dir_all(parent)
                    .with_context(|| format!("creating directory {}", parent.display()))?;
            }
            fs::write(&path, json).with_context(|| format!("writing {}", path.display()))?;
        }
        None => println!("{json}"),
    }

    Ok(())
}

pub fn generate(args: RpcGenerateArgs) -> Result<()> {
    let project_dir = args.path.unwrap_or_else(|| PathBuf::from("."));
    let manifest = rpc::discover_manifest(&project_dir)?;
    if !manifest
        .routes
        .iter()
        .any(|route| route.support == rpc::RpcRouteSupport::Json)
    {
        bail!(
            "no JSON RPC routes found under {}; client generation only supports routes with Json<T> inputs or outputs",
            project_dir.join("src").display()
        );
    }
    let output = match args.lang {
        RpcLanguage::Ts => {
            let types = rpc::discover_typescript_types(&project_dir, &manifest)?;
            rpc::generate_typescript_client_with_types(&manifest, &types)
        }
        RpcLanguage::Dart => {
            let types = rpc::discover_typescript_types(&project_dir, &manifest)?;
            rpc::generate_dart_client_with_types(&manifest, &types)
        }
        RpcLanguage::Rust => {
            let types = rpc::discover_typescript_types(&project_dir, &manifest)?;
            rpc::generate_rust_client_with_types(&manifest, &types)
        }
    };

    match args.out {
        Some(path) => write_output(&path, output),
        None => {
            println!("{output}");
            Ok(())
        }
    }
}

fn write_output(path: &PathBuf, output: String) -> Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)
            .with_context(|| format!("creating directory {}", parent.display()))?;
    }

    fs::write(path, output).with_context(|| format!("writing {}", path.display()))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::cli::{RpcGenerateArgs, RpcLanguage};

    #[test]
    fn manifest_errors_when_no_routes_are_found() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();

        let error = manifest(RpcManifestArgs {
            path: Some(dir.path().to_path_buf()),
            out: Some(dir.path().join("rpc.json")),
        })
        .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("no Rocket route attributes found")
        );
    }

    #[test]
    fn generate_errors_when_no_json_routes_are_found() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::write(
            dir.path().join("src/routes.rs"),
            r#"
            #[get("/")]
            pub fn index() -> &'static str {
                "ok"
            }
            "#,
        )
        .unwrap();

        let error = generate(RpcGenerateArgs {
            path: Some(dir.path().to_path_buf()),
            lang: RpcLanguage::Ts,
            out: Some(dir.path().join("client.ts")),
        })
        .unwrap_err();

        assert!(error.to_string().contains("no JSON RPC routes found"));
    }
}
