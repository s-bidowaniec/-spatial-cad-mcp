//! `cadctl`: fast, dependency-light CLI helpers for AI agents working with
//! CAD files inside a sandboxed workspace.
//!
//! Unlike the MCP servers, this is a plain CLI: it prints compact JSON to
//! stdout and diagnostics to stderr. Tool-chaining agents and human
//! developers can use it for quick orientation before (or instead of)
//! spinning up a server.

use std::path::PathBuf;

use anyhow::Result;
use cad_sandbox::WorkspaceSandbox;
use clap::Parser;
use serde::Serialize;

mod ifc;

#[derive(Parser)]
#[command(
    name = "cadctl",
    version,
    about = "Sandboxed CLI helpers for CAD/BIM workflows (IFC inspection, sandbox checks)"
)]
enum Cli {
    /// Inspect an IFC (ISO 10303-21) file: header schema and entity class counts.
    Inspect {
        /// Path to the .ifc file.
        path: PathBuf,
        /// How many top entity classes to report.
        #[arg(long, default_value_t = 20)]
        top: usize,
    },
    /// Check whether a path stays inside a workspace root (sandbox dry-run).
    SandboxCheck {
        /// Workspace root (defaults to MCP_CAD_WORKSPACE_ROOT when omitted).
        #[arg(long)]
        root: Option<String>,
        /// Relative (or absolute) path to validate.
        path: String,
    },
}

#[derive(Serialize)]
struct SandboxCheckResult {
    ok: bool,
    code: Option<String>,
    message: Option<String>,
    resolved_path: Option<String>,
}

fn main() -> Result<()> {
    match Cli::parse() {
        Cli::Inspect { path, top } => {
            // spatial-cad-review[SC-01]: Enforce the workspace contract for inspection.
            let summary = ifc::inspect(&path, top)?;
            println!("{}", serde_json::to_string(&summary)?);
        }
        Cli::SandboxCheck { root, path } => {
            let sandbox = match root {
                Some(root) => WorkspaceSandbox::from_root(&root),
                None => WorkspaceSandbox::from_env(),
            };
            let result = match sandbox.and_then(|sandbox| sandbox.resolve(&path)) {
                Ok(resolved) => SandboxCheckResult {
                    ok: true,
                    code: None,
                    message: None,
                    resolved_path: Some(resolved.display().to_string()),
                },
                Err(err) => SandboxCheckResult {
                    ok: false,
                    code: Some(err.code.to_string()),
                    message: Some(err.message),
                    resolved_path: None,
                },
            };
            println!("{}", serde_json::to_string(&result)?);
        }
    }
    Ok(())
}
