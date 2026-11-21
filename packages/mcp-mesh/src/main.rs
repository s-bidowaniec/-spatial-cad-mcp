//! Entry point for the mcp-mesh server: serve over stdio.
//!
//! Run inside the MCP client config:
//!
//! ```json
//! { "mcpServers": { "mcp-mesh": {
//!     "command": "cargo", "args": ["run", "-p", "mcp-mesh"],
//!     "env": { "MCP_CAD_WORKSPACE_ROOT": "/abs/path/to/workspace" } } } }
//! ```

mod server;

use anyhow::Result;
use rmcp::ServiceExt;
use rmcp::transport::stdio;

#[tokio::main]
async fn main() -> Result<()> {
    let service = server::MeshServer.serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}
