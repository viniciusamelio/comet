use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::cli::TestArgs;

/// `cargo fmt --check` + `cargo test --lib`, run directly (not through
/// `npm`) so unit tests don't need a Node toolchain just to run.
pub fn unit(args: TestArgs) -> Result<()> {
    let project_dir = args.path.unwrap_or_else(|| PathBuf::from("."));
    run_step(&project_dir, "cargo", &["fmt", "--check"])?;
    run_step(&project_dir, "cargo", &["test", "--lib"])
}

/// The project's own `npm run test:integration` script (e.g.
/// `examples/cloudflare-worker/tests/integration.sh`, which drives
/// `wrangler dev` itself) — the CLI doesn't reimplement that orchestration,
/// it just runs whatever the project defines.
pub fn integration(args: TestArgs) -> Result<()> {
    let project_dir = args.path.unwrap_or_else(|| PathBuf::from("."));
    run_step(&project_dir, "npm", &["run", "test:integration"])
}

/// The project's own `npm run test:perf` script.
pub fn perf(args: TestArgs) -> Result<()> {
    let project_dir = args.path.unwrap_or_else(|| PathBuf::from("."));
    run_step(&project_dir, "npm", &["run", "test:perf"])
}

/// Unit, then integration, then perf — the same order as the "Nebula MVP
/// release gate" documented in `docs/nebula-implementation-tracker.md`,
/// stopping at the first stage that fails.
pub fn all(args: TestArgs) -> Result<()> {
    let project_dir = args.path.unwrap_or_else(|| PathBuf::from("."));
    unit(TestArgs {
        path: Some(project_dir.clone()),
    })?;
    integration(TestArgs {
        path: Some(project_dir.clone()),
    })?;
    perf(TestArgs {
        path: Some(project_dir),
    })
}

/// Runs `program args...` in `project_dir`, streaming its stdout/stderr
/// straight through (not captured) so long-running steps like `cargo test`
/// or `wrangler dev` show live progress instead of a buffered dump at the
/// end. Fails loudly, including the exact command, on a non-zero exit.
fn run_step(project_dir: &Path, program: &str, args: &[&str]) -> Result<()> {
    let command_line = format!("{program} {}", args.join(" "));
    println!("$ {command_line}");

    let status = Command::new(program)
        .args(args)
        .current_dir(project_dir)
        .status()
        .with_context(|| format!("running `{command_line}`"))?;

    if !status.success() {
        bail!("`{command_line}` failed ({status})");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_step_succeeds_on_a_zero_exit() {
        run_step(Path::new("."), "true", &[]).unwrap();
    }

    #[test]
    fn run_step_fails_on_a_nonzero_exit() {
        let error = run_step(Path::new("."), "false", &[]).unwrap_err();
        assert!(error.to_string().contains("false"));
    }

    #[test]
    fn run_step_fails_with_context_when_the_program_is_missing() {
        let error = run_step(Path::new("."), "definitely-not-a-real-program", &[]).unwrap_err();
        assert!(error.to_string().contains("definitely-not-a-real-program"));
    }
}
