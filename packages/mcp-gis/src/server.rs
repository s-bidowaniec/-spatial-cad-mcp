//! Placeholder MCP server for GIS datasets (Rust scaffold).
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
//! - `gis_summary` - format detection (GeoPackage / GeoJSON / Shapefile / CSV),
//!   CRS/EPSG code, layer list with geometry types and feature counts,
//!   spatial extent per layer (GeoPackage via `rusqlite`, GeoJSON natively).
//! - `gis_query_features` - paginated features by layer, filtered by bbox
//!   and/or an attribute regex; geometries summarized, never dumped.
//! - `gis_extract_schema` - attribute schema per layer (name, type, null rate).
//! - `gis_export_bbox` - first *mutating* tool: clip by bbox to a **new**
//!   GeoJSON/GeoPackage file under `<root>/outputs` and return a diff.

use rmcp::handler::server::wrapper::Parameters;
use rmcp::{schemars, tool, tool_router};

/// Placeholder server state (will hold the sandbox root and dataset readers).
#[derive(Clone, Default)]
pub struct GisServer;

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct GisSummaryParams {
    /// Path to the GIS dataset, relative to the workspace root
    /// (MCP_CAD_WORKSPACE_ROOT). Traversal and symlink escapes are rejected.
    pub dataset_path: String,
}

#[tool_router(server_handler)]
impl GisServer {
    #[tool(
        description = "Summarize a GIS dataset (GeoPackage/GeoJSON/Shapefile): format, CRS, layers with geometry types, feature counts and extents. Compact alternative to loading full geometries (currently a scaffold: returns not_implemented)."
    )]
    fn gis_summary(
        &self,
        Parameters(GisSummaryParams { dataset_path }): Parameters<GisSummaryParams>,
    ) -> String {
        serde_json::json!({
            "ok": false,
            "error": {
                "code": "not_implemented",
                "message": "mcp-gis is a scaffold package. gis_summary will detect the format and report CRS, layers, geometry types, feature counts and extents (GeoPackage via rusqlite, GeoJSON natively).",
                "retryable": false
            },
            "meta": { "tool": "gis_summary", "requested_path": dataset_path }
        })
        .to_string()
    }
}
