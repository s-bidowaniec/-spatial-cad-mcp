//! MCP server for open-standard hydrodynamic model files (pure Rust, sandboxed,
//! AI-to-AI envelopes).
//!
//! Supported formats:
//! - TELEMAC Selafin (`.slf`, `.res`) — SERAFIN / SERAFIND binary meshes & time-step results
//! - EPA SWMM (`.inp`) — urban drainage network topology, options, & `[TIMESERIES]`
//! - SMS `.2dm` — 2D unstructured hydrodynamic meshes
//!
//! Implemented read-only tools:
//! - `hdm_summary` — format, title, node/element counts, spatial extent,
//!   simulation time window, and available variables/time series.
//! - `hdm_query_timeseries` — paginated time series extraction (by variable +
//!   `node_index`/`xy` for Selafin, or series name for SWMM `.inp`) with
//!   summary statistics (`min`/`max`/`mean`).

use crate::hdm::{
    HdmFormat, HdmSummary, SeriesPoint, SeriesStats, query_timeseries, summarize_hdm,
};
use cad_envelope::{paginate, resolve_input_file, run_envelope};
use cad_sandbox::WorkspaceSandbox;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::{schemars, tool, tool_router};
use serde::Serialize;
use serde_json::Map;

#[derive(Clone)]
pub struct HdmServer {
    sandbox: WorkspaceSandbox,
}

impl HdmServer {
    pub fn new(sandbox: WorkspaceSandbox) -> Self {
        Self { sandbox }
    }
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct HdmSummaryParams {
    /// Path to the HDM file (.slf, .res, .inp, .2dm), relative to MCP_CAD_WORKSPACE_ROOT.
    pub hdm_path: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct HdmQueryTimeseriesParams {
    /// Path to the HDM file (.slf, .res, .inp), relative to MCP_CAD_WORKSPACE_ROOT.
    pub hdm_path: String,
    /// Variable name (for TELEMAC Selafin, e.g. "WATER DEPTH") or time-series name (for SWMM .inp).
    pub series_name: String,
    /// Optional 0-based mesh node index (for TELEMAC Selafin).
    #[serde(default)]
    pub node_index: Option<usize>,
    /// Optional [x, y] coordinates to snap to the nearest mesh node (for TELEMAC Selafin).
    #[serde(default)]
    pub xy: Option<[f64; 2]>,
    /// Page size (default 50, hard cap 200).
    #[serde(default)]
    pub limit: Option<i64>,
    /// Zero-based pagination offset (default 0).
    #[serde(default)]
    pub offset: Option<i64>,
}

#[derive(Debug, Serialize)]
struct HdmSummaryData {
    hdm_path: String,
    #[serde(flatten)]
    summary: HdmSummary,
}

#[derive(Debug, Serialize)]
struct HdmTimeseriesData {
    hdm_path: String,
    format: HdmFormat,
    series_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    unit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    resolved_node_index: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    distance_to_xy: Option<f64>,
    stats: SeriesStats,
    points: Vec<SeriesPoint>,
}

#[tool_router(server_handler)]
impl HdmServer {
    #[tool(
        description = "Read-only orientation summary of an open-standard hydrodynamic model (.slf/.res TELEMAC Selafin, .inp EPA SWMM, .2dm SMS mesh): node/element counts, spatial extent, time window, and available variables or time series. Call this first before querying time series."
    )]
    pub fn hdm_summary(
        &self,
        Parameters(HdmSummaryParams { hdm_path }): Parameters<HdmSummaryParams>,
    ) -> String {
        run_envelope("hdm_summary", || {
            let resolved = resolve_input_file(&self.sandbox, &hdm_path)?;
            let summary = summarize_hdm(&resolved)?;
            Ok((HdmSummaryData { hdm_path, summary }, None))
        })
    }

    #[tool(
        description = "Read-only paginated time-series query for a hydrodynamic model: extract a variable at a mesh node (`node_index` or nearest `xy`) from TELEMAC Selafin (.slf/.res), or a named `[TIMESERIES]` table from EPA SWMM (.inp), with min/max/mean summary stats. Hard page cap: 200."
    )]
    pub fn hdm_query_timeseries(
        &self,
        Parameters(params): Parameters<HdmQueryTimeseriesParams>,
    ) -> String {
        run_envelope("hdm_query_timeseries", || {
            let resolved = resolve_input_file(&self.sandbox, &params.hdm_path)?;
            let extracted = query_timeseries(
                &resolved,
                &params.series_name,
                params.node_index,
                params.xy,
            )?;
            let page = paginate(&extracted.points, params.limit, params.offset)?;
            let mut meta = Map::new();
            meta.insert("pagination".to_string(), page.meta_value());
            Ok((
                HdmTimeseriesData {
                    hdm_path: params.hdm_path,
                    format: extracted.format,
                    series_name: extracted.series_name,
                    unit: extracted.unit,
                    resolved_node_index: extracted.resolved_node_index,
                    distance_to_xy: extracted.distance_to_xy,
                    stats: extracted.stats,
                    points: page.items,
                },
                Some(meta),
            ))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use std::fs;

    #[test]
    fn swmm_summary_and_timeseries_via_server() {
        let dir = std::env::temp_dir().join(format!("mcp-hdm-srv-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let inp = "[TITLE]\nDemo\n[OPTIONS]\nFLOW_UNITS CMS\n[JUNCTIONS]\nJ1 10 1\n[TIMESERIES]\nRAIN 0:00 1.2\nRAIN 1:00 3.4\n";
        fs::write(dir.join("net.inp"), inp).unwrap();

        let srv = HdmServer::new(WorkspaceSandbox::from_root(&dir).unwrap());
        let sum: Value = serde_json::from_str(&srv.hdm_summary(Parameters(HdmSummaryParams {
            hdm_path: "net.inp".to_string(),
        })))
        .unwrap();
        assert_eq!(sum["ok"], true);
        assert_eq!(sum["data"]["format"], "swmm_inp");

        let ts: Value = serde_json::from_str(&srv.hdm_query_timeseries(Parameters(
            HdmQueryTimeseriesParams {
                hdm_path: "net.inp".to_string(),
                series_name: "RAIN".to_string(),
                node_index: None,
                xy: None,
                limit: Some(10),
                offset: Some(0),
            },
        )))
        .unwrap();
        assert_eq!(ts["ok"], true);
        assert_eq!(ts["data"]["stats"]["count"], 2);

        let bad: Value = serde_json::from_str(&srv.hdm_summary(Parameters(HdmSummaryParams {
            hdm_path: "../outside.inp".to_string(),
        })))
        .unwrap();
        assert_eq!(bad["ok"], false);
        assert_eq!(bad["error"]["code"], "path_traversal");

        let _ = fs::remove_dir_all(&dir);
    }
}
