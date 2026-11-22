//! Placeholder MCP server for hydrodynamic model data (Rust scaffold).
//!
//! HDM = Hydrodynamic Modeling: 1D/2D hydraulic and hydrologic simulation
//! results (flood mapping, channel flow, urban drainage).
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
//! - `hdm_summary` - model type detection (HEC-RAS .hdf / SWMM .inp+.out /
//!   TELEMAC Selafin .slf / Delft3D netCDF), mesh/breakline counts, plan and
//!   simulation time window, available result datasets (HDF via `hdf5` crate,
//!   netCDF via the `netcdf` crate, Selafin natively).
//! - `hdm_query_timeseries` - paginated time series at monitoring stations or
//!   mesh cells (e.g. stage/flow/velocity), with min/max/mean per series.
//! - `hdm_query_profiles` - cross-section profiles (1D) or depth grid stats
//!   per time step (2D).
//! - `hdm_export_slice` - first *mutating* tool: extract a time/spatial slice
//!   to CSV or netCDF in `<root>/outputs` and return a structured diff.

use rmcp::handler::server::wrapper::Parameters;
use rmcp::{schemars, tool, tool_router};

/// Placeholder server state (will hold the sandbox root and dataset readers).
#[derive(Clone, Default)]
pub struct HdmServer;

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct HdmSummaryParams {
    /// Path to the model/results file, relative to the workspace root
    /// (MCP_CAD_WORKSPACE_ROOT). Traversal and symlink escapes are rejected.
    pub hdm_path: String,
}

#[tool_router(server_handler)]
impl HdmServer {
    #[tool(
        description = "Summarize a hydrodynamic model (HEC-RAS/SWMM/TELEMAC/Delft3D): plan, time window, mesh and available result datasets. Compact alternative to loading raw simulation output (currently a scaffold: returns not_implemented)."
    )]
    fn hdm_summary(
        &self,
        Parameters(HdmSummaryParams { hdm_path }): Parameters<HdmSummaryParams>,
    ) -> String {
        serde_json::json!({
            "ok": false,
            "error": {
                "code": "not_implemented",
                "message": "mcp-hdm is a scaffold package. hdm_summary will detect the model type (HEC-RAS HDF, SWMM, TELEMAC Selafin, Delft3D netCDF) and report plan, time window, mesh and result datasets.",
                "retryable": false
            },
            "meta": { "tool": "hdm_summary", "requested_path": hdm_path }
        })
        .to_string()
    }
}
