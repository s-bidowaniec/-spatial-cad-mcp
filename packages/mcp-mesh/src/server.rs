//! MCP server for 2D/3D mesh inspection, topology, and element quality QA
//! (pure Rust, sandboxed, AI-to-AI envelopes).
//!
//! Supported formats: SMS `.2dm`, Wavefront `.obj`, ASCII & binary `.stl`.
//!
//! Implemented read-only tools:
//! - `mesh_summary` — format, vertex/face/triangle/quad counts, 3D bbox,
//!   watertightness, boundary/non-manifold edges, Euler characteristic, and
//!   aggregate element QA metrics.
//! - `mesh_query_parts` — paginated part/group/material zone summaries.
//! - `mesh_query_quality` — paginated worst or failing elements filtered by
//!   angle thresholds, neighbor area ratio, validity flags, or 2D/3D bbox.

use crate::mesh::{
    BBox3D, FaceQuality, MeshFormat, PartSummary, QualitySummary, TopologySummary, analyze_mesh,
    load_mesh_file,
};
use cad_envelope::{ToolError, paginate, resolve_input_file, run_envelope};
use cad_sandbox::WorkspaceSandbox;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::{schemars, tool, tool_router};
use serde::Serialize;
use serde_json::Map;

#[derive(Clone)]
pub struct MeshServer {
    sandbox: WorkspaceSandbox,
}

impl MeshServer {
    pub fn new(sandbox: WorkspaceSandbox) -> Self {
        Self { sandbox }
    }
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct MeshSummaryParams {
    /// Path to the mesh file (.2dm, .obj, .stl), relative to MCP_CAD_WORKSPACE_ROOT.
    /// Traversal ('..') and symlink escapes outside the workspace are rejected.
    pub mesh_path: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct MeshQueryPartsParams {
    /// Path to the mesh file (.2dm, .obj, .stl), relative to MCP_CAD_WORKSPACE_ROOT.
    pub mesh_path: String,
    /// Page size (default 50, hard cap 200).
    #[serde(default)]
    pub limit: Option<i64>,
    /// Zero-based pagination offset (default 0).
    #[serde(default)]
    pub offset: Option<i64>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct MeshQueryQualityParams {
    /// Path to the mesh file (.2dm, .obj, .stl), relative to MCP_CAD_WORKSPACE_ROOT.
    pub mesh_path: String,
    /// Return only faces with minimum interior angle strictly below this threshold in degrees (e.g. 15.0).
    #[serde(default)]
    pub min_angle_below: Option<f64>,
    /// Return only faces with maximum interior angle strictly above this threshold in degrees (e.g. 150.0).
    #[serde(default)]
    pub max_angle_above: Option<f64>,
    /// Return only faces whose worst shared-edge neighbor area ratio exceeds this threshold (e.g. 4.0).
    #[serde(default)]
    pub max_neighbor_ratio_above: Option<f64>,
    /// If true, return only invalid faces (degenerate, non-convex quad, clockwise XY, or touching a non-manifold edge).
    #[serde(default)]
    pub only_invalid: Option<bool>,
    /// Optional 2D XY bounding box filter [min_x, min_y, max_x, max_y].
    #[serde(default)]
    pub bbox: Option<[f64; 4]>,
    /// Page size (default 50, hard cap 200).
    #[serde(default)]
    pub limit: Option<i64>,
    /// Zero-based pagination offset (default 0).
    #[serde(default)]
    pub offset: Option<i64>,
}

#[derive(Debug, Serialize)]
struct MeshSummaryData {
    mesh_path: String,
    format: MeshFormat,
    vertex_count: usize,
    face_count: usize,
    triangle_count: usize,
    quad_count: usize,
    part_count: usize,
    bbox: Option<BBox3D>,
    topology: TopologySummary,
    quality: QualitySummary,
    top_parts: Vec<PartSummary>,
}

#[derive(Debug, Serialize)]
struct MeshPartsData {
    mesh_path: String,
    parts: Vec<PartSummary>,
}

#[derive(Debug, Serialize)]
struct MeshQualityData {
    mesh_path: String,
    faces: Vec<FaceQuality>,
}

#[tool_router(server_handler)]
impl MeshServer {
    #[tool(
        description = "Read-only orientation summary of a 2D/3D mesh (.2dm, .obj, .stl): vertex/triangle/quad counts, 3D bounding box, watertightness, boundary/non-manifold edges, Euler characteristic, valence, and aggregate element quality QA. Call this first before querying parts or individual failing faces."
    )]
    pub fn mesh_summary(
        &self,
        Parameters(MeshSummaryParams { mesh_path }): Parameters<MeshSummaryParams>,
    ) -> String {
        run_envelope("mesh_summary", || {
            let resolved = resolve_input_file(&self.sandbox, &mesh_path)?;
            let mesh = load_mesh_file(&resolved)?;
            let analysis = analyze_mesh(&mesh);

            let part_count = analysis.parts.len();
            let top_parts = analysis.parts.into_iter().take(25).collect();

            Ok((
                MeshSummaryData {
                    mesh_path,
                    format: analysis.format,
                    vertex_count: analysis.vertex_count,
                    face_count: analysis.face_count,
                    triangle_count: analysis.triangle_count,
                    quad_count: analysis.quad_count,
                    part_count,
                    bbox: analysis.bbox,
                    topology: analysis.topology,
                    quality: analysis.quality,
                    top_parts,
                },
                None,
            ))
        })
    }

    #[tool(
        description = "Read-only paginated listing of parts, groups, or material zones in a mesh (.2dm materials, .obj objects/groups, .stl solids) with per-part face counts and 3D bounding boxes. Hard page cap: 200."
    )]
    pub fn mesh_query_parts(
        &self,
        Parameters(params): Parameters<MeshQueryPartsParams>,
    ) -> String {
        run_envelope("mesh_query_parts", || {
            let resolved = resolve_input_file(&self.sandbox, &params.mesh_path)?;
            let mesh = load_mesh_file(&resolved)?;
            let analysis = analyze_mesh(&mesh);

            let page = paginate(&analysis.parts, params.limit, params.offset)?;
            let mut meta = Map::new();
            meta.insert("pagination".to_string(), page.meta_value());
            Ok((
                MeshPartsData {
                    mesh_path: params.mesh_path,
                    parts: page.items,
                },
                Some(meta),
            ))
        })
    }

    #[tool(
        description = "Read-only paginated element quality QA query for a mesh (.2dm, .obj, .stl). Filter by min_angle_below, max_angle_above, max_neighbor_ratio_above, only_invalid, or 2D bbox [min_x, min_y, max_x, max_y]. Results are sorted deterministically worst-first (invalid faces first, then smallest min_angle_deg). Hard page cap: 200."
    )]
    pub fn mesh_query_quality(
        &self,
        Parameters(params): Parameters<MeshQueryQualityParams>,
    ) -> String {
        run_envelope("mesh_query_quality", || {
            if let Some(b) = params.bbox
                && (!b.iter().all(|v| v.is_finite()) || b[0] > b[2] || b[1] > b[3])
            {
                return Err(ToolError::new(
                    "invalid_input",
                    "bbox must be finite [min_x, min_y, max_x, max_y] with min <= max.",
                ));
            }
            let resolved = resolve_input_file(&self.sandbox, &params.mesh_path)?;
            let mesh = load_mesh_file(&resolved)?;
            let analysis = analyze_mesh(&mesh);

            let only_invalid = params.only_invalid.unwrap_or(false);
            let mut filtered: Vec<FaceQuality> = analysis
                .face_qualities
                .into_iter()
                .filter(|f| {
                    let is_invalid =
                        f.degenerate || f.nonconvex || f.clockwise_xy || f.non_manifold;
                    if only_invalid && !is_invalid {
                        return false;
                    }
                    if let Some(thr) = params.min_angle_below
                        && f.min_angle_deg >= thr
                    {
                        return false;
                    }
                    if let Some(thr) = params.max_angle_above
                        && f.max_angle_deg <= thr
                    {
                        return false;
                    }
                    if let Some(thr) = params.max_neighbor_ratio_above
                        && f.worst_neighbor_area_ratio.unwrap_or(0.0) <= thr
                    {
                        return false;
                    }
                    if let Some(b) = params.bbox
                        && (f.bbox.max[0] < b[0]
                            || f.bbox.min[0] > b[2]
                            || f.bbox.max[1] < b[1]
                            || f.bbox.min[1] > b[3])
                    {
                        return false;
                    }
                    true
                })
                .collect();

            // Deterministic worst-first ordering: invalid faces first, then smallest min angle, then face_id
            filtered.sort_by(|a, b| {
                let a_bad = a.degenerate || a.nonconvex || a.non_manifold || a.clockwise_xy;
                let b_bad = b.degenerate || b.nonconvex || b.non_manifold || b.clockwise_xy;
                b_bad
                    .cmp(&a_bad)
                    .then_with(|| a.min_angle_deg.total_cmp(&b.min_angle_deg))
                    .then_with(|| a.face_id.cmp(&b.face_id))
            });

            let page = paginate(&filtered, params.limit, params.offset)?;
            let mut meta = Map::new();
            meta.insert("pagination".to_string(), page.meta_value());
            Ok((
                MeshQualityData {
                    mesh_path: params.mesh_path,
                    faces: page.items,
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

    fn make_workspace(tag: &str) -> (std::path::PathBuf, MeshServer) {
        let dir = std::env::temp_dir().join(format!("mcp-mesh-test-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let mesh_2dm = "\
MESH2D
ND 1 0.0 0.0 10.0
ND 2 10.0 0.0 10.0
ND 3 5.0 0.1 10.0
ND 4 10.0 10.0 10.0
ND 5 0.0 10.0 10.0
E3T 1 1 2 3 1
E4Q 2 1 3 4 5 2
";
        fs::write(dir.join("test.2dm"), mesh_2dm).unwrap();
        let sandbox = WorkspaceSandbox::from_root(&dir).unwrap();
        (dir, MeshServer::new(sandbox))
    }

    #[test]
    fn summary_parts_and_quality_qa_work_end_to_end() {
        let (dir, srv) = make_workspace("e2e");

        let sum: Value = serde_json::from_str(&srv.mesh_summary(Parameters(MeshSummaryParams {
            mesh_path: "test.2dm".to_string(),
        })))
        .unwrap();
        assert_eq!(sum["ok"], true);
        assert_eq!(sum["data"]["vertex_count"], 5);
        assert_eq!(sum["data"]["face_count"], 2);
        assert_eq!(sum["data"]["part_count"], 2);

        let parts: Value =
            serde_json::from_str(&srv.mesh_query_parts(Parameters(MeshQueryPartsParams {
                mesh_path: "test.2dm".to_string(),
                limit: Some(10),
                offset: Some(0),
            })))
            .unwrap();
        assert_eq!(parts["ok"], true);
        assert_eq!(parts["data"]["parts"].as_array().unwrap().len(), 2);

        let qa: Value =
            serde_json::from_str(&srv.mesh_query_quality(Parameters(MeshQueryQualityParams {
                mesh_path: "test.2dm".to_string(),
                min_angle_below: Some(5.0),
                max_angle_above: None,
                max_neighbor_ratio_above: None,
                only_invalid: None,
                bbox: None,
                limit: Some(10),
                offset: Some(0),
            })))
            .unwrap();
        assert_eq!(qa["ok"], true);
        // Sliver triangle 1 has min angle ~1.15 deg
        assert_eq!(qa["data"]["faces"][0]["face_id"], 1);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn rejects_unsupported_extension_and_traversal() {
        let (dir, srv) = make_workspace("reject");
        fs::write(dir.join("bad.xyz"), "1 2 3").unwrap();

        let unsup: Value = serde_json::from_str(&srv.mesh_summary(Parameters(MeshSummaryParams {
            mesh_path: "bad.xyz".to_string(),
        })))
        .unwrap();
        assert_eq!(unsup["ok"], false);
        assert_eq!(unsup["error"]["code"], "unsupported_format");

        let trav: Value = serde_json::from_str(&srv.mesh_summary(Parameters(MeshSummaryParams {
            mesh_path: "../outside.obj".to_string(),
        })))
        .unwrap();
        assert_eq!(trav["ok"], false);
        assert_eq!(trav["error"]["code"], "path_traversal");

        let _ = fs::remove_dir_all(&dir);
    }
}
