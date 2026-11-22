//! Entry point for the mcp-gis server: serve over stdio.
//!
//! Run inside the MCP client config:
//!
//! ```json
//! { "mcpServers": { "mcp-gis": {
//!     "command": "cargo", "args": ["run", "-p", "mcp-gis"],
//!     "env": { "MCP_CAD_WORKSPACE_ROOT": "/abs/path/to/workspace" } } } }
//! ```

mod server;

use anyhow::Result;
use rmcp::ServiceExt;
use rmcp::transport::stdio;

#[tokio::main]
async fn main() -> Result<()> {
    let service = server::GisServer.serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}
