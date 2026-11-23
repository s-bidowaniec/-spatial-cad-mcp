//! MCP server for GIS vector datasets (GeoPackage via `rusqlite`, native GeoJSON;
//! pure Rust, sandboxed, AI-to-AI envelopes).
//!
//! Implemented tools:
//! - `gis_summary` (read-only) — format, CRS/EPSG, layers, geometry types,
//!   feature counts, extents, and CRS-vs-coordinate sanity warnings.
//! - `gis_extract_schema` (read-only) — per-layer attribute schema with null rates.
//! - `gis_query_features` (read-only) — paginated feature summaries filtered by
//!   layer, bbox, and attributes (geometries summarized, never dumped raw).
//! - `gis_export_bbox` (mutating) — clips features intersecting a 2D bbox into
//!   a new `.geojson` file under `<root>/outputs` and returns a structured diff.

use crate::dataset::{
    FeatureSummary, GisFormat, LayerSchema, LayerSummary, PointMatchSummary, export_bbox_geojson,
    extract_schema, query_features, query_features_by_point, summarize_dataset,
};
use cad_envelope::{
    MAX_VALUE_CHARS, ToolError, paginate, resolve_input_file, run_envelope, truncate_value,
};
use cad_sandbox::WorkspaceSandbox;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::{schemars, tool, tool_router};
use serde::Serialize;
use serde_json::{Map, Value, json};

#[derive(Clone)]
pub struct GisServer {
    sandbox: WorkspaceSandbox,
}

impl GisServer {
    pub fn new(sandbox: WorkspaceSandbox) -> Self {
        Self { sandbox }
    }
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct GisSummaryParams {
    /// Path to the GIS dataset (.gpkg, .geojson, .json), relative to MCP_CAD_WORKSPACE_ROOT.
    pub dataset_path: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct GisExtractSchemaParams {
    /// Path to the GIS dataset (.gpkg, .geojson, .json), relative to MCP_CAD_WORKSPACE_ROOT.
    pub dataset_path: String,
    /// Optional layer name (required only if you want a single layer from a multi-layer GeoPackage).
    #[serde(default)]
    pub layer: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct GisQueryFeaturesParams {
    /// Path to the GIS dataset (.gpkg, .geojson, .json), relative to MCP_CAD_WORKSPACE_ROOT.
    pub dataset_path: String,
    /// Optional layer name (defaults to the first feature layer).
    #[serde(default)]
    pub layer: Option<String>,
    /// Optional 2D bounding box filter [min_x, min_y, max_x, max_y].
    #[serde(default)]
    pub bbox: Option<[f64; 4]>,
    /// Optional attribute name to filter on.
    #[serde(default)]
    pub attribute_name: Option<String>,
    /// Optional case-insensitive substring filter on `attribute_name` (or any attribute if omitted).
    #[serde(default)]
    pub attribute_contains: Option<String>,
    /// Page size (default 50, hard cap 200).
    #[serde(default)]
    pub limit: Option<i64>,
    /// Zero-based pagination offset (default 0).
    #[serde(default)]
    pub offset: Option<i64>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct GisQueryPointParams {
    /// Path to the GIS dataset (.gpkg, .geojson, .json), relative to MCP_CAD_WORKSPACE_ROOT.
    pub dataset_path: String,
    /// Query point [x, y] in the dataset's coordinate system.
    pub xy: [f64; 2],
    /// Search radius around `xy` in dataset units (default 0.0 = exact containment / hit only).
    #[serde(default)]
    pub radius: Option<f64>,
    /// Optional layer name (defaults to the first feature layer).
    #[serde(default)]
    pub layer: Option<String>,
    /// Page size (default 50, hard cap 200).
    #[serde(default)]
    pub limit: Option<i64>,
    /// Zero-based pagination offset (default 0).
    #[serde(default)]
    pub offset: Option<i64>,
}

#[derive(Debug, Serialize)]
struct GisQueryPointData {
    dataset_path: String,
    layer: String,
    xy: [f64; 2],
    radius: f64,
    matches: Vec<PointMatchSummary>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct GisExportBboxParams {
    /// Path to the source GIS dataset (.gpkg, .geojson), relative to MCP_CAD_WORKSPACE_ROOT.
    pub dataset_path: String,
    /// 2D bounding box [min_x, min_y, max_x, max_y] selecting intersecting features.
    pub bbox: [f64; 4],
    /// Plain output file name (e.g. "clip.geojson") written under `<workspace>/outputs/`. Must not already exist.
    pub output_name: String,
    /// Optional layer name (defaults to the first feature layer).
    #[serde(default)]
    pub layer: Option<String>,
}

#[derive(Debug, Serialize)]
struct GisSummaryData {
    dataset_path: String,
    format: GisFormat,
    layer_count: usize,
    layers: Vec<LayerSummary>,
}

#[derive(Debug, Serialize)]
struct GisSchemaData {
    dataset_path: String,
    schemas: Vec<LayerSchema>,
}

#[derive(Debug, Serialize)]
struct GisQueryData {
    dataset_path: String,
    layer: String,
    features: Vec<FeatureSummary>,
}

#[derive(Debug, Serialize)]
struct GisExportData {
    dataset_path: String,
    layer: String,
    output_path: String,
    bbox: [f64; 4],
    before_features: usize,
    after_features: usize,
}

fn validate_bbox(b: [f64; 4]) -> Result<(), ToolError> {
    if !b.iter().all(|v| v.is_finite()) || b[0] > b[2] || b[1] > b[3] {
        return Err(ToolError::new(
            "invalid_input",
            "bbox must be finite [min_x, min_y, max_x, max_y] with min <= max.",
        ));
    }
    Ok(())
}

#[tool_router(server_handler)]
impl GisServer {
    #[tool(
        description = "Read-only orientation summary of a GIS vector dataset (.gpkg, .geojson): format, layers, CRS/EPSG, geometry types, feature counts, extents, and CRS-vs-coordinate sanity warnings. Call this first before querying features."
    )]
    pub fn gis_summary(
        &self,
        Parameters(GisSummaryParams { dataset_path }): Parameters<GisSummaryParams>,
    ) -> String {
        run_envelope("gis_summary", || {
            let resolved = resolve_input_file(&self.sandbox, &dataset_path)?;
            let (format, layers) = summarize_dataset(&resolved)?;
            let layer_count = layers.len();
            Ok((
                GisSummaryData {
                    dataset_path,
                    format,
                    layer_count,
                    layers,
                },
                None,
            ))
        })
    }

    #[tool(
        description = "Read-only attribute schema inspector for a GIS dataset (.gpkg, .geojson): field names, data types, null counts, and null rates per layer."
    )]
    pub fn gis_extract_schema(
        &self,
        Parameters(params): Parameters<GisExtractSchemaParams>,
    ) -> String {
        run_envelope("gis_extract_schema", || {
            let resolved = resolve_input_file(&self.sandbox, &params.dataset_path)?;
            let schemas = extract_schema(&resolved, params.layer.as_deref())?;
            Ok((
                GisSchemaData {
                    dataset_path: params.dataset_path,
                    schemas,
                },
                None,
            ))
        })
    }

    #[tool(
        description = "Read-only paginated feature query for a GIS dataset (.gpkg, .geojson) filtered by layer, 2D bbox [min_x, min_y, max_x, max_y], and attribute substring. Geometries are summarized (type, vertex count, bbox) rather than dumped raw. Hard page cap: 200."
    )]
    pub fn gis_query_features(
        &self,
        Parameters(params): Parameters<GisQueryFeaturesParams>,
    ) -> String {
        run_envelope("gis_query_features", || {
            if let Some(b) = params.bbox {
                validate_bbox(b)?;
            }
            let resolved = resolve_input_file(&self.sandbox, &params.dataset_path)?;
            // spatial-cad-review[SC-04]: Materialize bounded pages and measure truncation on those rows.
            let (layer_name, mut features) = query_features(
                &resolved,
                params.layer.as_deref(),
                params.bbox,
                params.attribute_name.as_deref(),
                params.attribute_contains.as_deref(),
            )?;

            let mut truncated_values = 0usize;
            for f in &mut features {
                for val in f.properties.values_mut() {
                    if let Value::String(s) = val {
                        let (clipped, was) = truncate_value(s, MAX_VALUE_CHARS);
                        if was {
                            truncated_values += 1;
                            *s = clipped;
                        }
                    }
                }
            }

            let page = paginate(&features, params.limit, params.offset)?;
            let mut meta = Map::new();
            meta.insert("pagination".to_string(), page.meta_value());
            if truncated_values > 0 {
                meta.insert("truncated_values".to_string(), json!(truncated_values));
            }
            Ok((
                GisQueryData {
                    dataset_path: params.dataset_path,
                    layer: layer_name,
                    features: page.items,
                },
                Some(meta),
            ))
        })
    }

    #[tool(
        description = "Mutating tool: export features intersecting a 2D bbox [min_x, min_y, max_x, max_y] to a NEW GeoJSON file under `<workspace>/outputs/<output_name>`. Never modifies the source dataset and refuses to overwrite existing outputs."
    )]
    pub fn gis_export_bbox(
        &self,
        Parameters(params): Parameters<GisExportBboxParams>,
    ) -> String {
        run_envelope("gis_export_bbox", || {
            validate_bbox(params.bbox)?;
            let resolved_in = resolve_input_file(&self.sandbox, &params.dataset_path)?;
            let resolved_out = self
                .sandbox
                .resolve_output(&params.output_name)
                .map_err(ToolError::from_sandbox)?;

            let stats = export_bbox_geojson(
                &resolved_in,
                params.layer.as_deref(),
                params.bbox,
                &resolved_out,
            )?;
            let rel_out = format!("outputs/{}", params.output_name);
            Ok((
                GisExportData {
                    dataset_path: params.dataset_path,
                    layer: stats.layer,
                    output_path: rel_out,
                    bbox: params.bbox,
                    before_features: stats.before_features,
                    after_features: stats.after_features,
                },
                None,
            ))
        })
    }

    #[tool(
        description = "Read-only spatial point query on a GIS dataset (.gpkg, .geojson): finds features containing [x, y] (exact point-in-polygon with hole rings) or within `radius` (sorted by containment, then distance). Paginated (hard cap 200)."
    )]
    pub fn gis_query_point(
        &self,
        Parameters(params): Parameters<GisQueryPointParams>,
    ) -> String {
        run_envelope("gis_query_point", || {
            if !params.xy.iter().all(|v| v.is_finite()) {
                return Err(ToolError::new(
                    "invalid_input",
                    "xy must be finite [x, y] coordinates.",
                ));
            }
            let radius = params.radius.unwrap_or(0.0);
            if !radius.is_finite() || radius < 0.0 {
                return Err(ToolError::new(
                    "invalid_input",
                    "radius must be a finite number >= 0.0.",
                ));
            }
            let resolved = resolve_input_file(&self.sandbox, &params.dataset_path)?;
            let (layer_name, mut matches) = query_features_by_point(
                &resolved,
                params.layer.as_deref(),
                params.xy,
                radius,
            )?;

            let mut truncated_values = 0usize;
            for m in &mut matches {
                for val in m.properties.values_mut() {
                    if let Value::String(s) = val {
                        let (clipped, was) = truncate_value(s, MAX_VALUE_CHARS);
                        if was {
                            truncated_values += 1;
                            *s = clipped;
                        }
                    }
                }
            }

            let page = paginate(&matches, params.limit, params.offset)?;
            let mut meta = Map::new();
            meta.insert("pagination".to_string(), page.meta_value());
            if truncated_values > 0 {
                meta.insert("truncated_values".to_string(), json!(truncated_values));
            }
            Ok((
                GisQueryPointData {
                    dataset_path: params.dataset_path,
                    layer: layer_name,
                    xy: params.xy,
                    radius,
                    matches: page.items,
                },
                Some(meta),
            ))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn geojson_summary_schema_query_and_export_work() {
        let dir = std::env::temp_dir().join(format!("mcp-gis-srv-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let gj = json!({
            "type": "FeatureCollection",
            "name": "parcels",
            "features": [
                {"type": "Feature", "id": 1, "geometry": {"type": "Point", "coordinates": [10.0, 50.0]}, "properties": {"kind": "bridge", "note": null}},
                {"type": "Feature", "id": 2, "geometry": {"type": "Point", "coordinates": [20.0, 60.0]}, "properties": {"kind": "road", "note": "ok"}}
            ]
        });
        fs::write(dir.join("data.geojson"), gj.to_string()).unwrap();

        let srv = GisServer::new(WorkspaceSandbox::from_root(&dir).unwrap());
        let sum: Value = serde_json::from_str(&srv.gis_summary(Parameters(GisSummaryParams {
            dataset_path: "data.geojson".to_string(),
        })))
        .unwrap();
        assert_eq!(sum["ok"], true);
        assert_eq!(sum["data"]["layers"][0]["feature_count"], 2);

        let sch: Value =
            serde_json::from_str(&srv.gis_extract_schema(Parameters(GisExtractSchemaParams {
                dataset_path: "data.geojson".to_string(),
                layer: None,
            })))
            .unwrap();
        assert_eq!(sch["ok"], true);
        assert_eq!(sch["data"]["schemas"][0]["fields"].as_array().unwrap().len(), 2);

        let q: Value =
            serde_json::from_str(&srv.gis_query_features(Parameters(GisQueryFeaturesParams {
                dataset_path: "data.geojson".to_string(),
                layer: None,
                bbox: None,
                attribute_name: Some("kind".to_string()),
                attribute_contains: Some("brid".to_string()),
                limit: Some(10),
                offset: Some(0),
            })))
            .unwrap();
        assert_eq!(q["ok"], true);
        assert_eq!(q["data"]["features"].as_array().unwrap().len(), 1);

        let exp: Value =
            serde_json::from_str(&srv.gis_export_bbox(Parameters(GisExportBboxParams {
                dataset_path: "data.geojson".to_string(),
                bbox: [9.0, 49.0, 11.0, 51.0],
                output_name: "sliced.geojson".to_string(),
                layer: None,
            })))
            .unwrap();
        assert_eq!(exp["ok"], true);
        assert_eq!(exp["data"]["after_features"], 1);
        assert!(dir.join("outputs/sliced.geojson").is_file());

        // Refuses overwrite
        let dup: Value =
            serde_json::from_str(&srv.gis_export_bbox(Parameters(GisExportBboxParams {
                dataset_path: "data.geojson".to_string(),
                bbox: [9.0, 49.0, 11.0, 51.0],
                output_name: "sliced.geojson".to_string(),
                layer: None,
            })))
            .unwrap();
        assert_eq!(dup["ok"], false);
        assert_eq!(dup["error"]["code"], "output_exists");

        // Spatial point-in-polygon (with hole ring) and radius search
        let poly_gj = json!({
            "type": "FeatureCollection",
            "name": "zones",
            "features": [
                {
                    "type": "Feature",
                    "id": 10,
                    "geometry": {
                        "type": "Polygon",
                        "coordinates": [
                            [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0], [0.0, 0.0]],
                            [[4.0, 4.0], [6.0, 4.0], [6.0, 6.0], [4.0, 6.0], [4.0, 4.0]]
                        ]
                    },
                    "properties": {"zone": "ring"}
                },
                {
                    "type": "Feature",
                    "id": 20,
                    "geometry": {"type": "Point", "coordinates": [12.0, 5.0]},
                    "properties": {"zone": "marker"}
                }
            ]
        });
        fs::write(dir.join("zones.geojson"), poly_gj.to_string()).unwrap();

        // Point inside exterior ring & outside hole -> contains=true, distance=0
        let hit: Value = serde_json::from_str(&srv.gis_query_point(Parameters(
            GisQueryPointParams {
                dataset_path: "zones.geojson".to_string(),
                xy: [2.0, 2.0],
                radius: Some(0.0),
                layer: None,
                limit: Some(10),
                offset: Some(0),
            },
        )))
        .unwrap();
        assert_eq!(hit["ok"], true);
        assert_eq!(hit["data"]["matches"].as_array().unwrap().len(), 1);
        assert_eq!(hit["data"]["matches"][0]["fid"], 10);
        assert_eq!(hit["data"]["matches"][0]["contains"], true);

        // Point inside hole -> NOT contained at radius=0
        let hole: Value = serde_json::from_str(&srv.gis_query_point(Parameters(
            GisQueryPointParams {
                dataset_path: "zones.geojson".to_string(),
                xy: [5.0, 5.0],
                radius: Some(0.0),
                layer: None,
                limit: Some(10),
                offset: Some(0),
            },
        )))
        .unwrap();
        assert_eq!(hole["ok"], true);
        assert_eq!(hole["data"]["matches"].as_array().unwrap().len(), 0);

        // Radius search near (11.5, 5.0) with radius=2.0 hits both marker (d=0.5) and polygon edge (d=1.5)
        let near: Value = serde_json::from_str(&srv.gis_query_point(Parameters(
            GisQueryPointParams {
                dataset_path: "zones.geojson".to_string(),
                xy: [11.5, 5.0],
                radius: Some(2.0),
                layer: None,
                limit: Some(10),
                offset: Some(0),
            },
        )))
        .unwrap();
        assert_eq!(near["ok"], true);
        assert_eq!(near["data"]["matches"].as_array().unwrap().len(), 2);
        assert_eq!(near["data"]["matches"][0]["fid"], 20);
        assert_eq!(near["data"]["matches"][1]["fid"], 10);

        let _ = fs::remove_dir_all(&dir);
    }
}
