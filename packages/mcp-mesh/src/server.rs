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

use crate::cad3d::{
    CutPlaneReport, MeshToDxfReport, Plane3D, VolumesIntersectReport, cut_mesh_with_plane,
    export_mesh_to_dxf, overlay_volumes_and_intersect,
};
use crate::mesh::{
    BBox3D, ElevationSample, FaceQuality, MeshDiffSummary, MeshFlipReport, MeshFormat, PartSummary,
    QualitySummary, SectionSample, SectionStats, TopologySummary, analyze_mesh, diff_meshes,
    improve_mesh_flips, load_mesh_file, sample_elevations, sample_section_line, write_bbox_slice,
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

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct MeshSampleElevationParams {
    /// Path to the mesh file (.2dm, .obj, .stl), relative to MCP_CAD_WORKSPACE_ROOT.
    pub mesh_path: String,
    /// List of 2D query points [[x, y], ...] (max 200 points per call).
    pub points: Vec<[f64; 2]>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct MeshSectionLineParams {
    /// Path to the mesh file (.2dm, .obj, .stl), relative to MCP_CAD_WORKSPACE_ROOT.
    pub mesh_path: String,
    /// Section start point [x0, y0].
    pub start: [f64; 2],
    /// Section end point [x1, y1].
    pub end: [f64; 2],
    /// Number of evenly spaced stations along the section (default 50, range 2..=200).
    #[serde(default)]
    pub samples: Option<usize>,
}

#[derive(Debug, Serialize)]
struct MeshSampleElevationData {
    mesh_path: String,
    hit_count: usize,
    samples: Vec<ElevationSample>,
}

#[derive(Debug, Serialize)]
struct MeshSectionLineData {
    mesh_path: String,
    start: [f64; 2],
    end: [f64; 2],
    stats: SectionStats,
    profile: Vec<SectionSample>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct MeshDiffParams {
    /// Path to the baseline ("before") mesh (.2dm, .obj, .stl), relative to MCP_CAD_WORKSPACE_ROOT.
    pub before_path: String,
    /// Path to the updated ("after") mesh (.2dm, .obj, .stl), relative to MCP_CAD_WORKSPACE_ROOT.
    pub after_path: String,
    /// XY coordinate matching tolerance for vertex elevation comparison (default 0.001).
    #[serde(default)]
    pub xy_tolerance: Option<f64>,
}

#[derive(Debug, Serialize)]
struct MeshDiffData {
    before_path: String,
    after_path: String,
    diff: MeshDiffSummary,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct MeshImproveFlipsParams {
    /// Path to the source mesh (.2dm, .obj, .stl), relative to MCP_CAD_WORKSPACE_ROOT.
    pub mesh_path: String,
    /// Plain output file name (e.g. "improved.2dm" or "improved.obj") written under `<workspace>/outputs/`. Must not already exist.
    pub output_name: String,
    /// Target maximum neighbor element height ratio before triggering diagonal flips (default 1.5, must be > 1.0).
    #[serde(default)]
    pub height_ratio_limit: Option<f64>,
    /// Minimum interior angle in degrees enforced for flipped triangles (default 20.0, range 1.0..=55.0).
    #[serde(default)]
    pub min_angle_deg: Option<f64>,
    /// Minimum allowed length of a newly created flipped diagonal edge (default 0.0).
    #[serde(default)]
    pub min_edge_length: Option<f64>,
    /// Maximum greedy flip passes (default 5, range 1..=20).
    #[serde(default)]
    pub max_rounds: Option<usize>,
}

#[derive(Debug, Serialize)]
struct MeshImproveFlipsData {
    mesh_path: String,
    output_path: String,
    height_ratio_limit: f64,
    min_angle_deg: f64,
    report: MeshFlipReport,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct MeshCutPlaneParams {
    /// Path to the source surface mesh (.2dm, .obj, .stl), relative to MCP_CAD_WORKSPACE_ROOT.
    pub mesh_path: String,
    /// Plain output DXF file name (e.g. "contour_105.dxf") written under `<workspace>/outputs/`. Must not already exist.
    pub output_name: String,
    /// Horizontal plane elevation Z (e.g. 105.0 for a contour / waterline). Mutually exclusive with `origin`/`normal` or `points`.
    #[serde(default)]
    pub z_level: Option<f64>,
    /// Plane reference point [x, y, z] (requires `normal`).
    #[serde(default)]
    pub origin: Option<[f64; 3]>,
    /// Plane normal vector [nx, ny, nz] (requires `origin`).
    #[serde(default)]
    pub normal: Option<[f64; 3]>,
    /// Three non-collinear 3D points [[x,y,z], [x,y,z], [x,y,z]] defining an arbitrary plane.
    #[serde(default)]
    pub points: Option<[[f64; 3]; 3]>,
    /// Optional DXF output layer name (default "CUT_PLANE").
    #[serde(default)]
    pub layer_name: Option<String>,
}

#[derive(Debug, Serialize)]
struct MeshCutPlaneData {
    mesh_path: String,
    output_path: String,
    report: CutPlaneReport,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct MeshVolumesIntersectParams {
    /// Path to surface A (existing / baseline mesh: .2dm, .obj, .stl), relative to MCP_CAD_WORKSPACE_ROOT.
    pub surface_a_path: String,
    /// Path to surface B (design / target mesh: fill where B > A, cut where B < A), relative to MCP_CAD_WORKSPACE_ROOT.
    pub surface_b_path: String,
    /// Height difference tolerance below which A and B are treated as coplanar/equal (default 0.005).
    #[serde(default)]
    pub height_tolerance: Option<f64>,
    /// Optional output DXF filename under `<workspace>/outputs/` to write 3D zero-lines (where zB = zA). Must not already exist.
    #[serde(default)]
    pub output_dxf_name: Option<String>,
}

#[derive(Debug, Serialize)]
struct MeshVolumesIntersectData {
    surface_a_path: String,
    surface_b_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    output_path: Option<String>,
    report: VolumesIntersectReport,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct MeshToDxfParams {
    /// Path to the source surface mesh (.2dm, .obj, .stl), relative to MCP_CAD_WORKSPACE_ROOT.
    pub mesh_path: String,
    /// Plain output DXF file name (e.g. "terrain.dxf") written under `<workspace>/outputs/`. Must not already exist.
    pub output_name: String,
    /// If true (default), also write closed 3D boundary polylines on layer "BOUNDARY".
    #[serde(default)]
    pub include_boundary: Option<bool>,
}

#[derive(Debug, Serialize)]
struct MeshToDxfData {
    mesh_path: String,
    output_path: String,
    report: MeshToDxfReport,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct MeshSliceBboxParams {
    /// Path to the source mesh (.2dm, .obj, .stl), relative to MCP_CAD_WORKSPACE_ROOT.
    pub mesh_path: String,
    /// 2D XY bounding box [min_x, min_y, max_x, max_y] selecting intersecting faces.
    pub bbox: [f64; 4],
    /// Plain output file name (e.g. "slice.obj" or "slice.2dm") written under `<workspace>/outputs/`. Must not already exist.
    pub output_name: String,
}

#[derive(Debug, Serialize)]
struct MeshSliceData {
    mesh_path: String,
    output_path: String,
    bbox: [f64; 4],
    before_vertices: usize,
    after_vertices: usize,
    before_faces: usize,
    after_faces: usize,
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

    #[tool(
        description = "Read-only barycentric 2D point-in-face elevation sampler for a surface mesh (.2dm, .obj, .stl). Accepts up to 200 [x, y] points per call and returns interpolated Z, face_id, and part (or z: null outside the surface)."
    )]
    pub fn mesh_sample_elevation(
        &self,
        Parameters(params): Parameters<MeshSampleElevationParams>,
    ) -> String {
        run_envelope("mesh_sample_elevation", || {
            if params.points.is_empty() || params.points.len() > 200 {
                return Err(ToolError::new(
                    "invalid_input",
                    "points must contain between 1 and 200 [x, y] pairs.",
                ));
            }
            if params
                .points
                .iter()
                .any(|p| !p[0].is_finite() || !p[1].is_finite())
            {
                return Err(ToolError::new(
                    "invalid_input",
                    "All query coordinates must be finite numbers.",
                ));
            }
            let resolved = resolve_input_file(&self.sandbox, &params.mesh_path)?;
            let mesh = load_mesh_file(&resolved)?;
            let samples = sample_elevations(&mesh, &params.points);
            let hit_count = samples.iter().filter(|s| s.z.is_some()).count();
            Ok((
                MeshSampleElevationData {
                    mesh_path: params.mesh_path,
                    hit_count,
                    samples,
                },
                None,
            ))
        })
    }

    #[tool(
        description = "Read-only vertical cross-section sampler along a 2D line segment (start -> end) across a 3D surface mesh (.2dm, .obj, .stl). Returns station-elevation samples (2..=200) plus length and min/max/mean Z stats."
    )]
    pub fn mesh_section_line(
        &self,
        Parameters(params): Parameters<MeshSectionLineParams>,
    ) -> String {
        run_envelope("mesh_section_line", || {
            if !params.start.iter().all(|v| v.is_finite())
                || !params.end.iter().all(|v| v.is_finite())
            {
                return Err(ToolError::new(
                    "invalid_input",
                    "start and end must be finite [x, y] coordinates.",
                ));
            }
            let n = params.samples.unwrap_or(50);
            if !(2..=200).contains(&n) {
                return Err(ToolError::new(
                    "invalid_input",
                    "samples must be between 2 and 200.",
                ));
            }
            let resolved = resolve_input_file(&self.sandbox, &params.mesh_path)?;
            let mesh = load_mesh_file(&resolved)?;
            let (stats, profile) = sample_section_line(&mesh, params.start, params.end, n);
            Ok((
                MeshSectionLineData {
                    mesh_path: params.mesh_path,
                    start: params.start,
                    end: params.end,
                    stats,
                    profile,
                },
                None,
            ))
        })
    }

    #[tool(
        description = "Mutating tool: clip faces intersecting a 2D XY bbox [min_x, min_y, max_x, max_y] into a NEW `.obj` or `.2dm` file under `<workspace>/outputs/<output_name>`. Never modifies the input mesh and refuses to overwrite existing outputs."
    )]
    pub fn mesh_slice_bbox(
        &self,
        Parameters(params): Parameters<MeshSliceBboxParams>,
    ) -> String {
        run_envelope("mesh_slice_bbox", || {
            if !params.bbox.iter().all(|v| v.is_finite())
                || params.bbox[0] > params.bbox[2]
                || params.bbox[1] > params.bbox[3]
            {
                return Err(ToolError::new(
                    "invalid_input",
                    "bbox must be finite [min_x, min_y, max_x, max_y] with min <= max.",
                ));
            }
            let resolved_in = resolve_input_file(&self.sandbox, &params.mesh_path)?;
            let resolved_out = self
                .sandbox
                .resolve_output(&params.output_name)
                .map_err(ToolError::from_sandbox)?;
            let mesh = load_mesh_file(&resolved_in)?;
            let before_vertices = mesh.vertices.len();
            let before_faces = mesh.faces.len();
            let (after_vertices, after_faces) =
                write_bbox_slice(&mesh, params.bbox, &resolved_out)?;
            Ok((
                MeshSliceData {
                    mesh_path: params.mesh_path,
                    output_path: format!("outputs/{}", params.output_name),
                    bbox: params.bbox,
                    before_vertices,
                    after_vertices,
                    before_faces,
                    after_faces,
                },
                None,
            ))
        })
    }

    #[tool(
        description = "Read-only structured diff between two surface meshes (before_path vs after_path): vertex/face count deltas, XY-matched elevation change stats (min/max/mean_abs dz), and added/removed/modified face IDs."
    )]
    pub fn mesh_diff(&self, Parameters(params): Parameters<MeshDiffParams>) -> String {
        run_envelope("mesh_diff", || {
            let tol = params.xy_tolerance.unwrap_or(1e-3);
            if !tol.is_finite() || tol <= 0.0 {
                return Err(ToolError::new(
                    "invalid_input",
                    "xy_tolerance must be a positive finite number.",
                ));
            }
            let p_before = resolve_input_file(&self.sandbox, &params.before_path)?;
            let p_after = resolve_input_file(&self.sandbox, &params.after_path)?;
            let m_before = load_mesh_file(&p_before)?;
            let m_after = load_mesh_file(&p_after)?;
            let diff = diff_meshes(&m_before, &m_after, tol);
            Ok((
                MeshDiffData {
                    before_path: params.before_path,
                    after_path: params.after_path,
                    diff,
                },
                None,
            ))
        })
    }

    #[tool(
        description = "Mutating tool: greedy diagonal edge-flipping on interior triangle pairs to reduce neighbor height-ratio spikes while preserving outer & material/part boundaries, convexity, and min angle. Writes a NEW `.2dm` or `.obj` under `<workspace>/outputs/<output_name>` with a before/after QA report."
    )]
    pub fn mesh_improve_flips(
        &self,
        Parameters(params): Parameters<MeshImproveFlipsParams>,
    ) -> String {
        run_envelope("mesh_improve_flips", || {
            let limit = params.height_ratio_limit.unwrap_or(1.5);
            let min_ang = params.min_angle_deg.unwrap_or(20.0);
            let min_edge = params.min_edge_length.unwrap_or(0.0);
            let rounds = params.max_rounds.unwrap_or(5);
            if !limit.is_finite() || limit <= 1.0 {
                return Err(ToolError::new(
                    "invalid_input",
                    "height_ratio_limit must be finite and strictly greater than 1.0.",
                ));
            }
            if !min_ang.is_finite() || !(1.0..=55.0).contains(&min_ang) {
                return Err(ToolError::new(
                    "invalid_input",
                    "min_angle_deg must be between 1.0 and 55.0.",
                ));
            }
            if !min_edge.is_finite() || min_edge < 0.0 {
                return Err(ToolError::new(
                    "invalid_input",
                    "min_edge_length must be finite and >= 0.0.",
                ));
            }
            if !(1..=20).contains(&rounds) {
                return Err(ToolError::new(
                    "invalid_input",
                    "max_rounds must be between 1 and 20.",
                ));
            }
            let resolved_in = resolve_input_file(&self.sandbox, &params.mesh_path)?;
            let resolved_out = self
                .sandbox
                .resolve_output(&params.output_name)
                .map_err(ToolError::from_sandbox)?;
            let mesh = load_mesh_file(&resolved_in)?;
            let report =
                improve_mesh_flips(&mesh, limit, min_ang, min_edge, rounds, &resolved_out)?;
            Ok((
                MeshImproveFlipsData {
                    mesh_path: params.mesh_path,
                    output_path: format!("outputs/{}", params.output_name),
                    height_ratio_limit: limit,
                    min_angle_deg: min_ang,
                    report,
                },
                None,
            ))
        })
    }

    #[tool(
        description = "Mutating tool: cut a 3D surface mesh (.2dm, .obj, .stl) with an arbitrary plane (horizontal `z_level`, `origin`+`normal`, or 3 `points`), chaining exact edge crossings into continuous 3D DXF polylines under `<workspace>/outputs/<output_name>`."
    )]
    pub fn mesh_cut_plane(
        &self,
        Parameters(params): Parameters<MeshCutPlaneParams>,
    ) -> String {
        run_envelope("mesh_cut_plane", || {
            // spatial-cad-review[SC-03]: Reject mixed or incomplete plane definitions.
            let plane = if let Some(z) = params.z_level {
                if !z.is_finite() {
                    return Err(ToolError::new("invalid_input", "z_level must be finite."));
                }
                Plane3D::horizontal(z)
            } else if let (Some(o), Some(n)) = (params.origin, params.normal) {
                if !o.iter().all(|v| v.is_finite()) || !n.iter().all(|v| v.is_finite()) {
                    return Err(ToolError::new(
                        "invalid_input",
                        "origin and normal must be finite 3D vectors.",
                    ));
                }
                Plane3D::from_point_normal(o, n)?
            } else if let Some(pts) = params.points {
                if !pts.iter().flatten().all(|v| v.is_finite()) {
                    return Err(ToolError::new(
                        "invalid_input",
                        "All three plane points must be finite.",
                    ));
                }
                Plane3D::from_three_points(pts[0], pts[1], pts[2])?
            } else {
                return Err(ToolError::new(
                    "invalid_input",
                    "Specify either `z_level`, `origin` + `normal`, or 3 `points`.",
                ));
            };
            let resolved_in = resolve_input_file(&self.sandbox, &params.mesh_path)?;
            let resolved_out = self
                .sandbox
                .resolve_output(&params.output_name)
                .map_err(ToolError::from_sandbox)?;
            let mesh = load_mesh_file(&resolved_in)?;
            let layer = params.layer_name.as_deref().unwrap_or("CUT_PLANE");
            let report = cut_mesh_with_plane(&mesh, plane, layer, &resolved_out)?;
            Ok((
                MeshCutPlaneData {
                    mesh_path: params.mesh_path,
                    output_path: format!("outputs/{}", params.output_name),
                    report,
                },
                None,
            ))
        })
    }

    #[tool(
        description = "Exact non-rasterized cut/fill volume & surface-intersection overlay between two 3D surfaces (A = existing, B = design). Computes exact fill/cut/net volumes, areas, dz range, and zero-lines (zB = zA). When `output_dxf_name` is set, writes 3D zero-line polylines to `<workspace>/outputs/<output_dxf_name>`."
    )]
    pub fn mesh_volumes_intersect(
        &self,
        Parameters(params): Parameters<MeshVolumesIntersectParams>,
    ) -> String {
        run_envelope("mesh_volumes_intersect", || {
            let tol = params.height_tolerance.unwrap_or(0.005);
            if !tol.is_finite() || tol < 0.0 {
                return Err(ToolError::new(
                    "invalid_input",
                    "height_tolerance must be finite and >= 0.0.",
                ));
            }
            let pa = resolve_input_file(&self.sandbox, &params.surface_a_path)?;
            let pb = resolve_input_file(&self.sandbox, &params.surface_b_path)?;
            let resolved_out = match params.output_dxf_name.as_deref() {
                Some(name) => Some(
                    self.sandbox
                        .resolve_output(name)
                        .map_err(ToolError::from_sandbox)?,
                ),
                None => None,
            };
            let ma = load_mesh_file(&pa)?;
            let mb = load_mesh_file(&pb)?;
            let report =
                overlay_volumes_and_intersect(&ma, &mb, tol, resolved_out.as_deref())?;
            Ok((
                MeshVolumesIntersectData {
                    surface_a_path: params.surface_a_path,
                    surface_b_path: params.surface_b_path,
                    output_path: params.output_dxf_name.map(|n| format!("outputs/{n}")),
                    report,
                },
                None,
            ))
        })
    }

    #[tool(
        description = "Mutating tool: export a 3D surface mesh (.2dm, .obj, .stl) to an ASCII DXF file under `<workspace>/outputs/<output_name>` with 3DFACE entities grouped by part/material layer and optional 3D boundary polylines."
    )]
    pub fn mesh_to_dxf(&self, Parameters(params): Parameters<MeshToDxfParams>) -> String {
        run_envelope("mesh_to_dxf", || {
            let resolved_in = resolve_input_file(&self.sandbox, &params.mesh_path)?;
            let resolved_out = self
                .sandbox
                .resolve_output(&params.output_name)
                .map_err(ToolError::from_sandbox)?;
            let mesh = load_mesh_file(&resolved_in)?;
            let report = export_mesh_to_dxf(
                &mesh,
                params.include_boundary.unwrap_or(true),
                &resolved_out,
            )?;
            Ok((
                MeshToDxfData {
                    mesh_path: params.mesh_path,
                    output_path: format!("outputs/{}", params.output_name),
                    report,
                },
                None,
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

        let sliced: Value =
            serde_json::from_str(&srv.mesh_slice_bbox(Parameters(MeshSliceBboxParams {
                mesh_path: "test.2dm".to_string(),
                bbox: [0.0, 0.0, 10.0, 0.5],
                output_name: "clip.2dm".to_string(),
            })))
            .unwrap();
        assert_eq!(sliced["ok"], true);
        assert_eq!(sliced["data"]["before_faces"], 2);
        assert!(dir.join("outputs/clip.2dm").is_file());

        let dup: Value =
            serde_json::from_str(&srv.mesh_slice_bbox(Parameters(MeshSliceBboxParams {
                mesh_path: "test.2dm".to_string(),
                bbox: [0.0, 0.0, 10.0, 0.5],
                output_name: "clip.2dm".to_string(),
            })))
            .unwrap();
        assert_eq!(dup["ok"], false);
        assert_eq!(dup["error"]["code"], "output_exists");

        let elev: Value = serde_json::from_str(&srv.mesh_sample_elevation(Parameters(
            MeshSampleElevationParams {
                mesh_path: "test.2dm".to_string(),
                points: vec![[5.0, 5.0], [500.0, 500.0]],
            },
        )))
        .unwrap();
        assert_eq!(elev["ok"], true);
        assert_eq!(elev["data"]["hit_count"], 1);
        assert_eq!(elev["data"]["samples"][0]["z"], 10.0);
        assert!(elev["data"]["samples"][1]["z"].is_null());

        let sec: Value =
            serde_json::from_str(&srv.mesh_section_line(Parameters(MeshSectionLineParams {
                mesh_path: "test.2dm".to_string(),
                start: [1.0, 5.0],
                end: [9.0, 5.0],
                samples: Some(5),
            })))
            .unwrap();
        assert_eq!(sec["ok"], true);
        // Stations at x=1,3,5,7 lie inside E4Q; x=9 is past edge (5,0.1)-(10,10)
        assert_eq!(sec["data"]["stats"]["hit_count"], 4);
        assert_eq!(sec["data"]["stats"]["min_z"], 10.0);
        assert_eq!(sec["data"]["stats"]["max_z"], 10.0);
        assert!(sec["data"]["stations"][4]["z"].is_null());

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

    #[test]
    // spatial-cad-review[SC-07]: Add stdio coverage with independently reopened exports.
    fn diff_and_improve_flips_work_end_to_end() {
        let (dir, srv) = make_workspace("diff-flip");
        // Asymmetric convex quad (0,0)-(10,0)-(6,4)-(0,4) split along long diagonal (0,0)-(6,4)
        // creates two triangles with unequal heights; flipping to (10,0)-(0,4) or testing a
        // 3-triangle patch with a sharp height-ratio jump triggers a flip.
        let before_2dm = "\
MESH2D
ND 1 0.0 0.0 10.0
ND 2 10.0 0.0 10.0
ND 3 10.0 6.0 10.0
ND 4 0.0 6.0 10.0
ND 5 3.0 3.0 10.0
E3T 1 1 2 5 1
E3T 2 2 3 5 1
E3T 3 3 4 5 1
E3T 4 4 1 5 1
";
        // Same topology, node 5 raised by +1.5 m
        let after_2dm = "\
MESH2D
ND 1 0.0 0.0 10.0
ND 2 10.0 0.0 10.0
ND 3 10.0 6.0 10.0
ND 4 0.0 6.0 10.0
ND 5 3.0 3.0 11.5
E3T 1 1 2 5 1
E3T 2 2 3 5 1
E3T 3 3 4 5 1
E3T 4 4 1 5 1
";
        fs::write(dir.join("before.2dm"), before_2dm).unwrap();
        fs::write(dir.join("after.2dm"), after_2dm).unwrap();

        let d: Value = serde_json::from_str(&srv.mesh_diff(Parameters(MeshDiffParams {
            before_path: "before.2dm".to_string(),
            after_path: "after.2dm".to_string(),
            xy_tolerance: None,
        })))
        .unwrap();
        assert_eq!(d["ok"], true);
        assert_eq!(d["data"]["diff"]["matched_xy_vertices"], 5);
        assert_eq!(d["data"]["diff"]["changed_z_vertices"], 1);
        assert_eq!(d["data"]["diff"]["max_dz"], 1.5);
        assert_eq!(d["data"]["diff"]["modified_faces"], 4);

        // Two-triangle strip next to a large triangle: splitting the convex quad (0,0)-(4,0)-(4,4)-(0,4)
        // along (0,0)-(4,4) vs (4,0)-(0,4) next to big triangle (0,0)-(0,4)-(-10,2)
        let flip_src = "\
MESH2D
ND 1 0.0 0.0 0.0
ND 2 6.0 0.0 0.0
ND 3 6.0 6.0 0.0
ND 4 0.0 6.0 0.0
ND 5 1.5 3.0 0.0
E3T 1 1 2 3 1
E3T 2 1 3 4 1
E3T 3 4 5 1 1
";
        fs::write(dir.join("flip_in.2dm"), flip_src).unwrap();
        let fl: Value = serde_json::from_str(&srv.mesh_improve_flips(Parameters(
            MeshImproveFlipsParams {
                mesh_path: "flip_in.2dm".to_string(),
                output_name: "flipped.2dm".to_string(),
                height_ratio_limit: Some(1.3),
                min_angle_deg: Some(15.0),
                min_edge_length: None,
                max_rounds: Some(5),
            },
        )))
        .unwrap();
        assert_eq!(fl["ok"], true);
        assert!(dir.join("outputs/flipped.2dm").is_file());

        // Refuses overwrite
        let dup: Value = serde_json::from_str(&srv.mesh_improve_flips(Parameters(
            MeshImproveFlipsParams {
                mesh_path: "flip_in.2dm".to_string(),
                output_name: "flipped.2dm".to_string(),
                height_ratio_limit: Some(1.3),
                min_angle_deg: Some(15.0),
                min_edge_length: None,
                max_rounds: Some(5),
            },
        )))
        .unwrap();
        assert_eq!(dup["ok"], false);
        assert_eq!(dup["error"]["code"], "output_exists");

        // Exact cut-plane, volumes/zero-line overlay, and mesh_to_dxf
        // Surface A: flat plane at z=10 on [0,10]x[0,10] (area = 100)
        // Surface B: tilted plane z = 8 + 0.4*x (z=8 at x=0, z=12 at x=10; crosses z=10 at x=5)
        // Expected: fill_area = 50, cut_area = 50, fill_vol = 50*1.0 = 50, cut_vol = 50*1.0 = 50, net = 0
        // Zero line along x=5, y=0..10 (length = 10)
        let surf_a = "\
MESH2D
ND 1 0.0 0.0 10.0
ND 2 10.0 0.0 10.0
ND 3 10.0 10.0 10.0
ND 4 0.0 10.0 10.0
E4Q 1 1 2 3 4 1
";
        let surf_b = "\
MESH2D
ND 1 0.0 0.0 8.0
ND 2 10.0 0.0 12.0
ND 3 10.0 10.0 12.0
ND 4 0.0 10.0 8.0
E4Q 1 1 2 3 4 1
";
        fs::write(dir.join("surf_a.2dm"), surf_a).unwrap();
        fs::write(dir.join("surf_b.2dm"), surf_b).unwrap();

        let cut: Value = serde_json::from_str(&srv.mesh_cut_plane(Parameters(MeshCutPlaneParams {
            mesh_path: "surf_b.2dm".to_string(),
            output_name: "contour_10.dxf".to_string(),
            z_level: Some(10.0),
            origin: None,
            normal: None,
            points: None,
            layer_name: Some("CONTOUR_10".to_string()),
        })))
        .unwrap();
        assert_eq!(cut["ok"], true);
        assert_eq!(cut["data"]["report"]["polyline_count"], 1);
        assert_eq!(cut["data"]["report"]["total_length_3d"], 10.0);
        assert!(dir.join("outputs/contour_10.dxf").is_file());

        let vol: Value = serde_json::from_str(&srv.mesh_volumes_intersect(Parameters(
            MeshVolumesIntersectParams {
                surface_a_path: "surf_a.2dm".to_string(),
                surface_b_path: "surf_b.2dm".to_string(),
                height_tolerance: Some(0.005),
                output_dxf_name: Some("zero_line.dxf".to_string()),
            },
        )))
        .unwrap();
        assert_eq!(vol["ok"], true);
        assert_eq!(vol["data"]["report"]["common_area"], 100.0);
        assert_eq!(vol["data"]["report"]["fill_area"], 50.0);
        assert_eq!(vol["data"]["report"]["cut_area"], 50.0);
        assert_eq!(vol["data"]["report"]["fill_volume"], 50.0);
        assert_eq!(vol["data"]["report"]["cut_volume"], 50.0);
        assert_eq!(vol["data"]["report"]["net_volume"], 0.0);
        assert_eq!(vol["data"]["report"]["zero_line_count"], 1);
        assert_eq!(vol["data"]["report"]["zero_line_length_2d"], 10.0);
        assert!(dir.join("outputs/zero_line.dxf").is_file());

        let dxf_exp: Value =
            serde_json::from_str(&srv.mesh_to_dxf(Parameters(MeshToDxfParams {
                mesh_path: "surf_a.2dm".to_string(),
                output_name: "surf_a.dxf".to_string(),
                include_boundary: Some(true),
            })))
            .unwrap();
        assert_eq!(dxf_exp["ok"], true);
        assert_eq!(dxf_exp["data"]["report"]["face_count"], 1);
        assert_eq!(dxf_exp["data"]["report"]["boundary_loops"], 1);
        assert!(dir.join("outputs/surf_a.dxf").is_file());

        let _ = fs::remove_dir_all(&dir);
    }
}
