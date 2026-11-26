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
    HdmFormat, HdmSummary, ProfilePayload, SeriesPoint, SeriesStats, export_timeseries_csv,
    query_profiles, query_timeseries, summarize_hdm,
};
use cad_envelope::{ToolError, paginate, resolve_input_file, run_envelope};
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

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct HdmQueryProfilesParams {
    /// Path to the HDM file (.slf, .res, .inp), relative to MCP_CAD_WORKSPACE_ROOT.
    pub hdm_path: String,
    /// Variable name for TELEMAC Selafin line profiles (e.g. "WATER DEPTH"). Omit for SWMM conduit profiles.
    #[serde(default)]
    pub variable: Option<String>,
    /// Optional 0-based time step index for TELEMAC Selafin (defaults to the final time step).
    #[serde(default)]
    pub time_step: Option<usize>,
    /// Section start [x0, y0] (required for TELEMAC Selafin line profiles).
    #[serde(default)]
    pub start: Option<[f64; 2]>,
    /// Section end [x1, y1] (required for TELEMAC Selafin line profiles).
    #[serde(default)]
    pub end: Option<[f64; 2]>,
    /// Number of samples along a Selafin section line (default 50, range 2..=200).
    #[serde(default)]
    pub samples: Option<usize>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct HdmExportSliceParams {
    /// Path to the source HDM file (.slf, .res, .inp), relative to MCP_CAD_WORKSPACE_ROOT.
    pub hdm_path: String,
    /// Variable name (for TELEMAC Selafin) or time-series name (for SWMM .inp).
    pub series_name: String,
    /// Plain output CSV file name (e.g. "station_depth.csv") written under `<workspace>/outputs/`. Must not already exist.
    pub output_name: String,
    /// Optional 0-based mesh node index (for TELEMAC Selafin).
    #[serde(default)]
    pub node_index: Option<usize>,
    /// Optional [x, y] coordinates to snap to the nearest mesh node (for TELEMAC Selafin).
    #[serde(default)]
    pub xy: Option<[f64; 2]>,
}

#[derive(Debug, Serialize)]
struct HdmProfileData {
    hdm_path: String,
    #[serde(flatten)]
    profile: ProfilePayload,
}

#[derive(Debug, Serialize)]
struct HdmExportData {
    hdm_path: String,
    format: HdmFormat,
    series_name: String,
    output_path: String,
    rows_written: usize,
    stats: SeriesStats,
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

#[tool_router]
impl HdmServer {
    #[tool(
        description = "Read-only orientation summary of an open-standard hydrodynamic model (.slf/.res TELEMAC Selafin, .inp EPA SWMM, .2dm SMS mesh): node/element counts, spatial extent, time window, and available variables or time series. Call this first before querying time series.",
        annotations(
            title = "HDM summary",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
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
        description = "Read-only paginated time-series query for a hydrodynamic model: extract a variable at a mesh node (`node_index` or nearest `xy`) from TELEMAC Selafin (.slf/.res), or a named `[TIMESERIES]` table from EPA SWMM (.inp), with min/max/mean summary stats. Hard page cap: 200.",
        annotations(
            title = "HDM query timeseries",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    pub fn hdm_query_timeseries(
        &self,
        Parameters(params): Parameters<HdmQueryTimeseriesParams>,
    ) -> String {
        run_envelope("hdm_query_timeseries", || {
            let resolved = resolve_input_file(&self.sandbox, &params.hdm_path)?;
            let extracted =
                query_timeseries(&resolved, &params.series_name, params.node_index, params.xy)?;
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

    #[tool(
        description = "Read-only profile extractor: interpolates a TELEMAC Selafin (.slf/.res) variable along a 2D section line (start -> end, 2..=200 samples) at a time step, or returns EPA SWMM (.inp) conduit longitudinal profiles with invert elevations and slopes.",
        annotations(
            title = "HDM query profiles",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    pub fn hdm_query_profiles(
        &self,
        Parameters(params): Parameters<HdmQueryProfilesParams>,
    ) -> String {
        run_envelope("hdm_query_profiles", || {
            let n = params.samples.unwrap_or(50);
            if !(2..=200).contains(&n) {
                return Err(ToolError::new(
                    "invalid_input",
                    "samples must be between 2 and 200.",
                ));
            }
            let resolved = resolve_input_file(&self.sandbox, &params.hdm_path)?;
            let profile = query_profiles(
                &resolved,
                params.variable.as_deref(),
                params.time_step,
                params.start,
                params.end,
                n,
            )?;
            Ok((
                HdmProfileData {
                    hdm_path: params.hdm_path,
                    profile,
                },
                None,
            ))
        })
    }

    #[tool(
        description = "Mutating tool: export a TELEMAC Selafin node/XY time series or EPA SWMM `[TIMESERIES]` to a NEW CSV file under `<workspace>/outputs/<output_name>`. Never modifies inputs and refuses to overwrite existing outputs.",
        annotations(
            title = "HDM export slice",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    pub fn hdm_export_slice(&self, Parameters(params): Parameters<HdmExportSliceParams>) -> String {
        run_envelope("hdm_export_slice", || {
            let resolved_in = resolve_input_file(&self.sandbox, &params.hdm_path)?;
            let resolved_out = self
                .sandbox
                .resolve_output(&params.output_name)
                .map_err(ToolError::from_sandbox)?;
            let res = export_timeseries_csv(
                &resolved_in,
                &params.series_name,
                params.node_index,
                params.xy,
                &resolved_out,
            )?;
            Ok((
                HdmExportData {
                    hdm_path: params.hdm_path,
                    format: res.format,
                    series_name: res.series_name,
                    output_path: format!("outputs/{}", params.output_name),
                    rows_written: res.rows_written,
                    stats: res.stats,
                },
                None,
            ))
        })
    }
}

#[rmcp::tool_handler(
    instructions = "Read-only HDM model inspection tools plus an output-isolated CSV export. Start with hdm_summary, then hdm_query_timeseries or hdm_query_profiles. The mutator hdm_export_slice writes a NEW CSV under <workspace>/outputs and never modifies inputs. Every tool returns a JSON envelope {ok, data, meta} or {ok:false, error:{code, message, retryable}}; paths are relative to the workspace root and sandboxed."
)]
impl rmcp::handler::server::ServerHandler for HdmServer {}

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
        let inp = "[TITLE]\nDemo\n[OPTIONS]\nFLOW_UNITS CMS\n[JUNCTIONS]\nJ1 10 1\nJ2 9 1\n[CONDUITS]\nC1 J1 J2 100 0.013\n[TIMESERIES]\nRAIN 0:00 1.2\nRAIN 1:00 3.4\n";
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

        let prof: Value =
            serde_json::from_str(&srv.hdm_query_profiles(Parameters(HdmQueryProfilesParams {
                hdm_path: "net.inp".to_string(),
                variable: None,
                time_step: None,
                start: None,
                end: None,
                samples: None,
            })))
            .unwrap();
        assert_eq!(prof["ok"], true);
        assert_eq!(prof["data"]["kind"], "swmm_conduits");
        assert_eq!(prof["data"]["conduits"][0]["slope"], 0.01);

        let exp: Value =
            serde_json::from_str(&srv.hdm_export_slice(Parameters(HdmExportSliceParams {
                hdm_path: "net.inp".to_string(),
                series_name: "RAIN".to_string(),
                output_name: "rain.csv".to_string(),
                node_index: None,
                xy: None,
            })))
            .unwrap();
        assert_eq!(exp["ok"], true);
        assert_eq!(exp["data"]["rows_written"], 2);
        assert!(dir.join("outputs/rain.csv").is_file());

        let bad: Value = serde_json::from_str(&srv.hdm_summary(Parameters(HdmSummaryParams {
            hdm_path: "../outside.inp".to_string(),
        })))
        .unwrap();
        assert_eq!(bad["ok"], false);
        assert_eq!(bad["error"]["code"], "path_traversal");

        let _ = fs::remove_dir_all(&dir);
    }
}
