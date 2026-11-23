//! MCP server for 2D/3D DXF drawings (pure Rust, sandboxed, AI-to-AI envelopes).
//!
//! Implemented read-only tools:
//! - `dxf_summary` — DXF release, `$INSUNITS`, layer & entity counts, 3D extents,
//!   Z=0 audit, coordinate convention hint, and LibreDWG mesh repair warnings.
//! - `dxf_query_entities` — paginated compact entity listings filtered by layer,
//!   type, elevation presence, and 2D bounding box.
//! - `dxf_extract_text` — paginated cleaned `TEXT`/`MTEXT`/`ATTRIB` annotations.

use crate::parser::{
    BBox3D, EntityRecord, LayerDetail, TextRecord, export_dxf_slice, parse_dxf_file,
};
use cad_envelope::{
    MAX_VALUE_CHARS, ToolError, paginate, resolve_input_file, run_envelope, truncate_value,
};
use cad_sandbox::WorkspaceSandbox;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::{schemars, tool, tool_router};
use serde::Serialize;
use serde_json::{Map, json};

#[derive(Clone)]
pub struct DxfServer {
    sandbox: WorkspaceSandbox,
}

impl DxfServer {
    pub fn new(sandbox: WorkspaceSandbox) -> Self {
        Self { sandbox }
    }
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct DxfSummaryParams {
    /// Path to the ASCII DXF file, relative to MCP_CAD_WORKSPACE_ROOT.
    /// Traversal ('..') and symlink escapes outside the workspace are rejected.
    pub dxf_path: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct DxfQueryEntitiesParams {
    /// Path to the ASCII DXF file, relative to MCP_CAD_WORKSPACE_ROOT.
    pub dxf_path: String,
    /// Optional exact layer name filter (case-insensitive).
    #[serde(default)]
    pub layer: Option<String>,
    /// Optional DXF entity type filter (e.g. "LINE", "LWPOLYLINE", "3DFACE", "INSERT").
    #[serde(default)]
    pub entity_type: Option<String>,
    /// Optional filter on whether the entity carries non-zero 3D elevation (true = 3D, false = 2D/Z=0).
    #[serde(default)]
    pub has_z: Option<bool>,
    /// Optional 2D bounding box filter [min_x, min_y, max_x, max_y] returning entities whose bbox intersects it.
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
pub struct DxfExtractTextParams {
    /// Path to the ASCII DXF file, relative to MCP_CAD_WORKSPACE_ROOT.
    pub dxf_path: String,
    /// Optional exact layer name filter (case-insensitive).
    #[serde(default)]
    pub layer: Option<String>,
    /// Optional substring filter applied to cleaned annotation text.
    #[serde(default)]
    pub contains: Option<String>,
    /// Whether `contains` matching is case-sensitive (default false).
    #[serde(default)]
    pub case_sensitive: Option<bool>,
    /// Page size (default 50, hard cap 200).
    #[serde(default)]
    pub limit: Option<i64>,
    /// Zero-based pagination offset (default 0).
    #[serde(default)]
    pub offset: Option<i64>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct DxfQueryLayersParams {
    /// Path to the ASCII DXF file, relative to MCP_CAD_WORKSPACE_ROOT.
    pub dxf_path: String,
    /// Page size (default 50, hard cap 200).
    #[serde(default)]
    pub limit: Option<i64>,
    /// Zero-based pagination offset (default 0).
    #[serde(default)]
    pub offset: Option<i64>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct DxfExportSliceParams {
    /// Path to the source ASCII DXF file, relative to MCP_CAD_WORKSPACE_ROOT.
    pub dxf_path: String,
    /// 2D bounding box [min_x, min_y, max_x, max_y] selecting intersecting entities.
    pub bbox: [f64; 4],
    /// Plain output file name (e.g. "clip.dxf") written under `<workspace>/outputs/`. Must not already exist.
    pub output_name: String,
    /// Optional exact layer name filter (case-insensitive).
    #[serde(default)]
    pub layer: Option<String>,
    /// Optional DXF entity type filter (e.g. "LINE", "LWPOLYLINE", "3DFACE").
    #[serde(default)]
    pub entity_type: Option<String>,
}

#[derive(Debug, Serialize)]
struct QueryLayersData {
    dxf_path: String,
    layers: Vec<LayerDetail>,
}

#[derive(Debug, Serialize)]
struct ExportSliceData {
    dxf_path: String,
    output_path: String,
    bbox: [f64; 4],
    before_entities: usize,
    after_entities: usize,
    kept_layers: usize,
    extents: Option<BBox3D>,
}

#[derive(Debug, Serialize)]
struct NameCount {
    name: String,
    count: usize,
}

#[derive(Debug, Serialize)]
struct DxfSummaryData {
    dxf_path: String,
    acad_version: Option<String>,
    acad_release: Option<String>,
    insunits_code: Option<i32>,
    insunits: String,
    total_entities: usize,
    total_texts: usize,
    total_layers: usize,
    insert_count: usize,
    mesh_counts_repaired: usize,
    extents: Option<crate::parser::BBox3D>,
    header_extents: Option<crate::parser::BBox3D>,
    z_audit: crate::parser::ZAudit,
    coord_convention: Option<crate::parser::CoordConvention>,
    entity_types: Vec<NameCount>,
    layers: Vec<NameCount>,
    warnings: Vec<String>,
}

#[derive(Debug, Serialize)]
struct QueryEntitiesData {
    dxf_path: String,
    entities: Vec<EntityRecord>,
}

#[derive(Debug, Serialize)]
struct ExtractTextData {
    dxf_path: String,
    texts: Vec<TextRecord>,
}

#[tool_router(server_handler)]
impl DxfServer {
    #[tool(
        description = "Read-only orientation summary of an ASCII DXF drawing: version, $INSUNITS, top layers, entity counts by type, 3D extents, Z=0 audit, coordinate convention hint, and LibreDWG repair notes. Call this first before querying entities or text."
    )]
    pub fn dxf_summary(
        &self,
        Parameters(DxfSummaryParams { dxf_path }): Parameters<DxfSummaryParams>,
    ) -> String {
        run_envelope("dxf_summary", || {
            let resolved = resolve_input_file(&self.sandbox, &dxf_path)?;
            let doc = parse_dxf_file(&resolved)?;

            let entity_types = doc
                .entity_type_counts()
                .into_iter()
                .take(30)
                .map(|(name, count)| NameCount { name, count })
                .collect();
            let all_layers = doc.layer_counts();
            let total_layers = all_layers.len();
            let layers = all_layers
                .into_iter()
                .take(50)
                .map(|(name, count)| NameCount { name, count })
                .collect();

            let data = DxfSummaryData {
                dxf_path,
                acad_version: doc.acad_version,
                acad_release: doc.acad_release,
                insunits_code: doc.insunits_code,
                insunits: doc.insunits_name,
                total_entities: doc.entities.len(),
                total_texts: doc.texts.len(),
                total_layers,
                insert_count: doc.insert_count,
                mesh_counts_repaired: doc.mesh_counts_repaired,
                extents: doc.extents,
                header_extents: doc.header_extents,
                z_audit: doc.z_audit,
                coord_convention: doc.coord_convention,
                entity_types,
                layers,
                warnings: doc.warnings,
            };
            Ok((data, None))
        })
    }

    #[tool(
        description = "Read-only paginated query of DXF entities filtered by layer, entity_type, has_z, or 2D bbox [min_x, min_y, max_x, max_y]. Returns compact metadata (handle, type, layer, vertex count, 3D bbox, length) without dumping raw coordinates. Hard page cap: 200."
    )]
    pub fn dxf_query_entities(
        &self,
        Parameters(params): Parameters<DxfQueryEntitiesParams>,
    ) -> String {
        run_envelope("dxf_query_entities", || {
            if let Some(b) = params.bbox
                && (!b.iter().all(|v| v.is_finite()) || b[0] > b[2] || b[1] > b[3])
            {
                return Err(ToolError::new(
                    "invalid_input",
                    "bbox must be finite [min_x, min_y, max_x, max_y] with min <= max.",
                ));
            }
            let resolved = resolve_input_file(&self.sandbox, &params.dxf_path)?;
            let doc = parse_dxf_file(&resolved)?;

            let layer_lc = params.layer.as_deref().map(str::to_ascii_lowercase);
            let type_uc = params.entity_type.as_deref().map(str::to_ascii_uppercase);

            let filtered: Vec<EntityRecord> = doc
                .entities
                .into_iter()
                .filter(|e| {
                    if let Some(ref want_layer) = layer_lc
                        && e.layer.to_ascii_lowercase() != *want_layer
                    {
                        return false;
                    }
                    if let Some(ref want_type) = type_uc
                        && e.entity_type != *want_type
                    {
                        return false;
                    }
                    if let Some(want_z) = params.has_z
                        && e.has_z != want_z
                    {
                        return false;
                    }
                    if let Some(b) = params.bbox {
                        let Some(eb) = e.bbox else {
                            return false;
                        };
                        if eb.max[0] < b[0]
                            || eb.min[0] > b[2]
                            || eb.max[1] < b[1]
                            || eb.min[1] > b[3]
                        {
                            return false;
                        }
                    }
                    true
                })
                .collect();

            let page = paginate(&filtered, params.limit, params.offset)?;
            let mut meta = Map::new();
            meta.insert("pagination".to_string(), page.meta_value());
            Ok((
                QueryEntitiesData {
                    dxf_path: params.dxf_path,
                    entities: page.items,
                },
                Some(meta),
            ))
        })
    }

    #[tool(
        description = "Read-only paginated extraction of TEXT, MTEXT, and ATTRIB annotations from a DXF drawing with AutoCAD control codes stripped. Supports layer and substring filters. Hard page cap: 200."
    )]
    pub fn dxf_extract_text(
        &self,
        Parameters(params): Parameters<DxfExtractTextParams>,
    ) -> String {
        run_envelope("dxf_extract_text", || {
            let resolved = resolve_input_file(&self.sandbox, &params.dxf_path)?;
            let doc = parse_dxf_file(&resolved)?;

            let layer_lc = params.layer.as_deref().map(str::to_ascii_lowercase);
            let case_sensitive = params.case_sensitive.unwrap_or(false);
            let needle_lc = params
                .contains
                .as_deref()
                .map(|s| if case_sensitive { s.to_string() } else { s.to_ascii_lowercase() });

            let mut truncated_values = 0usize;
            let filtered: Vec<TextRecord> = doc
                .texts
                .into_iter()
                .filter(|t| {
                    if let Some(ref want_layer) = layer_lc
                        && t.layer.to_ascii_lowercase() != *want_layer
                    {
                        return false;
                    }
                    if let Some(ref needle) = needle_lc {
                        let hay = if case_sensitive {
                            t.text.clone()
                        } else {
                            t.text.to_ascii_lowercase()
                        };
                        if !hay.contains(needle.as_str()) {
                            return false;
                        }
                    }
                    true
                })
                .map(|mut t| {
                    let (clipped, was_clipped) = truncate_value(&t.text, MAX_VALUE_CHARS);
                    if was_clipped {
                        truncated_values += 1;
                    }
                    t.text = clipped;
                    t
                })
                .collect();

            let page = paginate(&filtered, params.limit, params.offset)?;
            let mut meta = Map::new();
            meta.insert("pagination".to_string(), page.meta_value());
            if truncated_values > 0 {
                meta.insert("truncated_values".to_string(), json!(truncated_values));
            }
            Ok((
                ExtractTextData {
                    dxf_path: params.dxf_path,
                    texts: page.items,
                },
                Some(meta),
            ))
        })
    }

    #[tool(
        description = "Read-only paginated breakdown of DXF layers with per-layer entity counts, type histogram, Z=0 count, and 3D bounding box. Hard page cap: 200."
    )]
    pub fn dxf_query_layers(
        &self,
        Parameters(params): Parameters<DxfQueryLayersParams>,
    ) -> String {
        run_envelope("dxf_query_layers", || {
            let resolved = resolve_input_file(&self.sandbox, &params.dxf_path)?;
            let doc = parse_dxf_file(&resolved)?;
            let details = doc.layer_details();
            let page = paginate(&details, params.limit, params.offset)?;
            let mut meta = Map::new();
            meta.insert("pagination".to_string(), page.meta_value());
            Ok((
                QueryLayersData {
                    dxf_path: params.dxf_path,
                    layers: page.items,
                },
                Some(meta),
            ))
        })
    }

    #[tool(
        description = "Mutating tool: extract entities intersecting a 2D bbox [min_x, min_y, max_x, max_y] (with optional layer/entity_type filters) into a NEW ASCII DXF under `<workspace>/outputs/<output_name>`. Preserves original entity geometry verbatim, never modifies inputs, and refuses to overwrite existing outputs."
    )]
    pub fn dxf_export_slice(
        &self,
        Parameters(params): Parameters<DxfExportSliceParams>,
    ) -> String {
        run_envelope("dxf_export_slice", || {
            if !params.bbox.iter().all(|v| v.is_finite())
                || params.bbox[0] > params.bbox[2]
                || params.bbox[1] > params.bbox[3]
            {
                return Err(ToolError::new(
                    "invalid_input",
                    "bbox must be finite [min_x, min_y, max_x, max_y] with min <= max.",
                ));
            }
            let resolved_in = resolve_input_file(&self.sandbox, &params.dxf_path)?;
            let resolved_out = self
                .sandbox
                .resolve_output(&params.output_name)
                .map_err(ToolError::from_sandbox)?;
            let stats = export_dxf_slice(
                &resolved_in,
                params.bbox,
                params.layer.as_deref(),
                params.entity_type.as_deref(),
                &resolved_out,
            )?;
            Ok((
                ExportSliceData {
                    dxf_path: params.dxf_path,
                    output_path: format!("outputs/{}", params.output_name),
                    bbox: params.bbox,
                    before_entities: stats.before_entities,
                    after_entities: stats.after_entities,
                    kept_layers: stats.kept_layers,
                    extents: stats.extents,
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

    fn make_workspace(tag: &str) -> (std::path::PathBuf, DxfServer) {
        let dir = std::env::temp_dir().join(format!("mcp-dxf-test-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let dxf = "\
0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\nAC1024\n9\n$INSUNITS\n70\n6\n0\nENDSEC\n\
0\nSECTION\n2\nENTITIES\n\
0\nLINE\n5\n1\n8\nROAD\n10\n10.0\n20\n20.0\n30\n5.0\n11\n30.0\n21\n20.0\n31\n5.5\n\
0\nTEXT\n5\n2\n8\nNOTES\n10\n15.0\n20\n25.0\n30\n0.0\n40\n2.0\n1\nStation 0+100 %%p0.01\n\
0\nENDSEC\n0\nEOF\n";
        fs::write(dir.join("sample.dxf"), dxf).unwrap();
        let sandbox = WorkspaceSandbox::from_root(&dir).unwrap();
        (dir, DxfServer::new(sandbox))
    }

    #[test]
    fn summary_query_and_text_work_end_to_end() {
        let (dir, srv) = make_workspace("e2e");

        let sum: Value = serde_json::from_str(&srv.dxf_summary(Parameters(DxfSummaryParams {
            dxf_path: "sample.dxf".to_string(),
        })))
        .unwrap();
        assert_eq!(sum["ok"], true);
        assert_eq!(sum["data"]["total_entities"], 2);
        assert_eq!(sum["data"]["insunits"], "meters");

        let q: Value =
            serde_json::from_str(&srv.dxf_query_entities(Parameters(DxfQueryEntitiesParams {
                dxf_path: "sample.dxf".to_string(),
                layer: Some("road".to_string()),
                entity_type: None,
                has_z: Some(true),
                bbox: Some([0.0, 0.0, 50.0, 50.0]),
                limit: Some(500),
                offset: Some(0),
            })))
            .unwrap();
        assert_eq!(q["ok"], true);
        assert_eq!(q["data"]["entities"].as_array().unwrap().len(), 1);
        assert_eq!(q["meta"]["pagination"]["limit_clamped_to"], 200);

        let txt: Value =
            serde_json::from_str(&srv.dxf_extract_text(Parameters(DxfExtractTextParams {
                dxf_path: "sample.dxf".to_string(),
                layer: None,
                contains: Some("station".to_string()),
                case_sensitive: Some(false),
                limit: None,
                offset: None,
            })))
            .unwrap();
        assert_eq!(txt["ok"], true);
        assert_eq!(txt["data"]["texts"][0]["text"], "Station 0+100 ±0.01");

        let layers: Value =
            serde_json::from_str(&srv.dxf_query_layers(Parameters(DxfQueryLayersParams {
                dxf_path: "sample.dxf".to_string(),
                limit: Some(10),
                offset: Some(0),
            })))
            .unwrap();
        assert_eq!(layers["ok"], true);
        assert_eq!(layers["data"]["layers"].as_array().unwrap().len(), 2);

        let sliced: Value =
            serde_json::from_str(&srv.dxf_export_slice(Parameters(DxfExportSliceParams {
                dxf_path: "sample.dxf".to_string(),
                bbox: [0.0, 0.0, 50.0, 22.0],
                output_name: "road_only.dxf".to_string(),
                layer: Some("ROAD".to_string()),
                entity_type: None,
            })))
            .unwrap();
        assert_eq!(sliced["ok"], true);
        assert_eq!(sliced["data"]["before_entities"], 2);
        assert_eq!(sliced["data"]["after_entities"], 1);
        assert!(dir.join("outputs/road_only.dxf").is_file());

        // Verify exported DXF parses cleanly back through dxf_summary
        let resumm: Value =
            serde_json::from_str(&srv.dxf_summary(Parameters(DxfSummaryParams {
                dxf_path: "outputs/road_only.dxf".to_string(),
            })))
            .unwrap();
        assert_eq!(resumm["ok"], true);
        assert_eq!(resumm["data"]["total_entities"], 1);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn rejects_traversal_and_missing_file() {
        let (dir, srv) = make_workspace("reject");

        let bad: Value = serde_json::from_str(&srv.dxf_summary(Parameters(DxfSummaryParams {
            dxf_path: "../secret.dxf".to_string(),
        })))
        .unwrap();
        assert_eq!(bad["ok"], false);
        assert_eq!(bad["error"]["code"], "path_traversal");

        let missing: Value = serde_json::from_str(&srv.dxf_summary(Parameters(DxfSummaryParams {
            dxf_path: "nope.dxf".to_string(),
        })))
        .unwrap();
        assert_eq!(missing["ok"], false);
        assert_eq!(missing["error"]["code"], "file_not_found");

        let _ = fs::remove_dir_all(&dir);
    }
}
