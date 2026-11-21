//! Entry point for the mcp-dxf server: serve over stdio.
//!
//! Run inside the MCP client config:
//!
//! ```json
//! { "mcpServers": { "mcp-dxf": {
//!     "command": "cargo", "args": ["run", "-p", "mcp-dxf"],
//!     "env": { "MCP_CAD_WORKSPACE_ROOT": "/abs/path/to/workspace" } } } }
//! ```

mod server;

use anyhow::Result;
use rmcp::ServiceExt;
use rmcp::transport::stdio;

#[tokio::main]
async fn main() -> Result<()> {
    let service = server::DxfServer.serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}
