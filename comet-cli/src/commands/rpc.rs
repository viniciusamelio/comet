use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};

use crate::cli::{RpcGenerateArgs, RpcLanguage, RpcManifestArgs};
use crate::rpc;

pub fn manifest(args: RpcManifestArgs) -> Result<()> {
    let project_dir = args.path.unwrap_or_else(|| PathBuf::from("."));
    let manifest = rpc::discover_manifest(&project_dir)?;
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
    let output = match args.lang {
        RpcLanguage::Ts => {
            let types = rpc::discover_typescript_types(&project_dir, &manifest)?;
            rpc::generate_typescript_client_with_types(&manifest, &types)
        }
        RpcLanguage::Dart => {
            let types = rpc::discover_typescript_types(&project_dir, &manifest)?;
            rpc::generate_dart_client_with_types(&manifest, &types)
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
