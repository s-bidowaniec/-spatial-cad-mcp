#![forbid(unsafe_code)]
//! Entry point for the `mcp-dxf` server: serve over stdio.
//!
//! Refuses to start without `MCP_CAD_WORKSPACE_ROOT` set to an existing directory.

mod cad3d;
mod edit;
mod parser;
mod server;

use anyhow::Result;
use cad_sandbox::WorkspaceSandbox;
use rmcp::ServiceExt;
use rmcp::transport::stdio;

#[tokio::main]
async fn main() -> Result<()> {
    let sandbox = WorkspaceSandbox::from_env().map_err(|e| anyhow::anyhow!("{e}"))?;
    let service = server::DxfServer::new(sandbox).serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}
