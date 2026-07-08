use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};

use crate::cli::RpcManifestArgs;
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
