//! Placeholder MCP server for 2D DXF drawings (Rust scaffold).
//!
//! Purpose of this scaffold:
//!
//! 1. Pin the tool surface and response-envelope vocabulary now, so agent
//!    prompts remain stable while the implementation lands.
//! 2. Mirror the Python servers' AI-to-AI rules: sandboxed paths, compact
//!    summaries, paginated queries, structured errors - never raw dumps.
//!
//! Roadmap (in order):
//!
//! - `dxf_summary` - schema/version, $INSUNITS, layer table counts, entity
//!   counts by type, extents/bounding box (parse with the `dxf` crate).
//! - `dxf_query_entities` - paginated by layer/type/name regex.
//! - `dxf_extract_text` - text annotations (agents frequently need
//!   dimension and label text before geometry).
//! - `dxf_export_slice` - first *mutating* tool: clip by bbox to a **new**
//!   output file under `<root>/outputs` and return a structured diff.

use rmcp::handler::server::wrapper::Parameters;
use rmcp::{schemars, tool, tool_router};

/// Placeholder server state (will hold the sandbox root and DXF parser pool).
#[derive(Clone, Default)]
pub struct DxfServer;

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct DxfSummaryParams {
    /// Path to the DXF file, relative to the workspace root
    /// (MCP_CAD_WORKSPACE_ROOT). Traversal and symlink escapes are rejected.
    pub dxf_path: String,
}

#[tool_router(server_handler)]
impl DxfServer {
    #[tool(
        description = "Summarize a 2D DXF drawing: units, layers, entity counts by type, and extents. Compact alternative to loading full drawing geometry (currently a scaffold: returns not_implemented)."
    )]
    fn dxf_summary(
        &self,
        Parameters(DxfSummaryParams { dxf_path }): Parameters<DxfSummaryParams>,
    ) -> String {
        serde_json::json!({
            "ok": false,
            "error": {
                "code": "not_implemented",
                "message": "mcp-dxf is a scaffold package. dxf_summary will parse the drawing (layers, entity counts, extents) using the Rust `dxf` crate.",
                "retryable": false
            },
            "meta": { "tool": "dxf_summary", "requested_path": dxf_path }
        })
        .to_string()
    }
}
