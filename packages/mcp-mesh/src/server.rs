//! Placeholder MCP server for 3D mesh/spatial data (Rust scaffold).
//!
//! Scope (Python `trimesh`/`shapely` equivalents, rebuilt natively in Rust):
//!
//! - `mesh_summary` - vertex/edge/face counts, bounding box, watertightness,
//!   euler characteristics (fast pure-Rust mesh formats: OBJ, STL, glTF).
//! - `mesh_query_parts` - paginated scene/part listing for multi-part files.
//! - `mesh_slice_bbox` - *mutating*: extract a sub-model to a **new** output
//!   file under `<root>/outputs`, returning a structured diff/summary.
//!
//! All tools resolve paths through `cad-sandbox` (MCP_CAD_WORKSPACE_ROOT)
//! and speak the shared response envelopes documented in `docs/tool-design.md`.

use rmcp::handler::server::wrapper::Parameters;
use rmcp::{schemars, tool, tool_router};

/// Placeholder server state (will hold the sandbox root and mesh loaders).
#[derive(Clone, Default)]
pub struct MeshServer;

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct MeshSummaryParams {
    /// Path to the mesh file (OBJ, STL, glTF), relative to the workspace root
    /// (MCP_CAD_WORKSPACE_ROOT). Traversal and symlink escapes are rejected.
    pub mesh_path: String,
}

#[tool_router(server_handler)]
impl MeshServer {
    #[tool(
        description = "Summarize a 3D mesh: vertex/face counts, bounding box, watertightness. Compact alternative to loading full geometry (currently a scaffold: returns not_implemented)."
    )]
    fn mesh_summary(
        &self,
        Parameters(MeshSummaryParams { mesh_path }): Parameters<MeshSummaryParams>,
    ) -> String {
        serde_json::json!({
            "ok": false,
            "error": {
                "code": "not_implemented",
                "message": "mcp-mesh is a scaffold package. mesh_summary will report vertex/face counts, bounding box and watertightness (OBJ/STL/glTF loaders in pure Rust).",
                "retryable": false
            },
            "meta": { "tool": "mesh_summary", "requested_path": mesh_path }
        })
        .to_string()
    }
}
