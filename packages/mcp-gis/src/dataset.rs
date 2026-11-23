//! Pure-Rust GIS vector dataset reader for GeoPackage (`.gpkg` via `rusqlite`)
//! and GeoJSON (`.geojson`/`.json`), including GeoPackage binary WKB decoding,
//! attribute schema profiling, generic CRS-vs-coordinate sanity checks, and
//! bbox GeoJSON export.

use cad_envelope::ToolError;
use rusqlite::{Connection, OpenFlags, types::ValueRef};
use serde::Serialize;
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GisFormat {
    Geopackage,
    Geojson,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct BBox2D {
    pub min: [f64; 2],
    pub max: [f64; 2],
}

#[derive(Debug, Clone, Serialize)]
pub struct LayerSummary {
    pub layer: String,
    pub geometry_type: String,
    pub feature_count: usize,
    pub epsg: Option<i32>,
    pub srs_name: Option<String>,
    pub has_z: bool,
    pub bbox: Option<BBox2D>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FieldSchema {
    pub name: String,
    pub data_type: String,
    pub null_count: usize,
    pub null_rate: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct LayerSchema {
    pub layer: String,
    pub feature_count: usize,
    pub geometry_column: Option<String>,
    pub fields: Vec<FieldSchema>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FeatureSummary {
    pub fid: i64,
    pub layer: String,
    pub geometry_type: String,
    pub vertex_count: usize,
    pub bbox: Option<BBox2D>,
    pub properties: Map<String, Value>,
}

#[derive(Debug, Clone)]
pub struct DecodedGeometry {
    pub geom_type: String,
    pub vertex_count: usize,
    pub bbox: Option<BBox2D>,
    pub geojson: Value,
}

#[derive(Debug, Clone)]
pub struct ExportStats {
    pub before_features: usize,
    pub after_features: usize,
    pub layer: String,
}

fn round6(v: f64) -> f64 {
    (v * 1_000_000.0).round() / 1_000_000.0
}

fn round3(v: f64) -> f64 {
    (v * 1000.0).round() / 1000.0
}

fn quote_ident(ident: &str) -> String {
    format!("\"{}\"", ident.replace('"', "\"\""))
}

pub fn detect_format(path: &Path) -> Result<GisFormat, ToolError> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    match ext.as_str() {
        "gpkg" => Ok(GisFormat::Geopackage),
        "geojson" | "json" => Ok(GisFormat::Geojson),
        other => Err(ToolError::new(
            "unsupported_format",
            format!(
                "Unsupported GIS extension '.{other}'. Supported formats: .gpkg, .geojson, .json."
            ),
        )),
    }
}

/// Generic CRS vs coordinate magnitude sanity check (no regional hardcoding).
pub fn check_crs_sanity(epsg: Option<i32>, bbox: Option<BBox2D>) -> Vec<String> {
    let Some(b) = bbox else {
        return Vec::new();
    };
    let mid_x = (b.min[0] + b.max[0]) * 0.5;
    let mid_y = (b.min[1] + b.max[1]) * 0.5;
    let ax = mid_x.abs();
    let ay = mid_y.abs();
    let mut warnings = Vec::new();

    if let Some(code) = epsg {
        // Geographic CRS (e.g. WGS84 EPSG:4326 / ETRS89 EPSG:4258) must stay within [-180..180, -90..90]
        if matches!(code, 4326 | 4258 | 4269) && (ax > 180.0 || ay > 90.0) {
            warnings.push(format!(
                "Coordinates (mid=({:.2}, {:.2})) exceed geographic degrees range despite CRS EPSG:{code}.",
                mid_x, mid_y
            ));
        }
        // Projected metric CRS with tiny degree-sized coordinates
        if !matches!(code, 4326 | 4258 | 4269) && ax <= 180.0 && ay <= 90.0 && (ax > 0.0 || ay > 0.0)
        {
            warnings.push(format!(
                "Coordinates (mid=({:.4}, {:.4})) look geographic (degrees) or local, but declared CRS is projected EPSG:{code}.",
                mid_x, mid_y
            ));
        }
        // 8-digit zone-prefixed UTM easting tagged as a standard 6-digit UTM EPSG
        let digits = if ax >= 1.0 {
            (ax as u64).to_string().len()
        } else {
            0
        };
        if digits == 8 && ((25801..=25860).contains(&code) || (32601..=32660).contains(&code)) {
            warnings.push(format!(
                "8-digit easting ({:.0}) looks zone-prefixed, which mismatches standard 6-digit UTM EPSG:{code}.",
                mid_x
            ));
        }
        // 6-digit plain UTM easting tagged as Gauss-Kruger
        if digits == 6 && (31464..=31469).contains(&code) {
            warnings.push(format!(
                "6-digit easting ({:.0}) looks like unprefixed UTM, but declared CRS is Gauss-Kruger EPSG:{code}.",
                mid_x
            ));
        }
    }

    warnings
}

pub fn summarize_dataset(path: &Path) -> Result<(GisFormat, Vec<LayerSummary>), ToolError> {
    match detect_format(path)? {
        GisFormat::Geopackage => Ok((GisFormat::Geopackage, summarize_gpkg(path)?)),
        GisFormat::Geojson => Ok((GisFormat::Geojson, vec![summarize_geojson(path)?])),
    }
}

pub fn extract_schema(path: &Path, layer: Option<&str>) -> Result<Vec<LayerSchema>, ToolError> {
    match detect_format(path)? {
        GisFormat::Geopackage => schema_gpkg(path, layer),
        GisFormat::Geojson => Ok(vec![schema_geojson(path)?]),
    }
}

pub fn query_features(
    path: &Path,
    layer: Option<&str>,
    bbox: Option<[f64; 4]>,
    attr_name: Option<&str>,
    attr_contains: Option<&str>,
) -> Result<(String, Vec<FeatureSummary>), ToolError> {
    match detect_format(path)? {
        GisFormat::Geopackage => query_gpkg_features(path, layer, bbox, attr_name, attr_contains),
        GisFormat::Geojson => query_geojson_features(path, bbox, attr_name, attr_contains),
    }
}

// spatial-cad-review[SC-04]: Resolve the source CRS and export RFC 7946 coordinates.
pub fn export_bbox_geojson(
    path: &Path,
    layer: Option<&str>,
    bbox: [f64; 4],
    output_path: &Path,
) -> Result<ExportStats, ToolError> {
    let (layer_name, before_features, kept_features) = match detect_format(path)? {
        GisFormat::Geopackage => collect_gpkg_geojson_features(path, layer, bbox)?,
        GisFormat::Geojson => collect_geojson_filtered(path, bbox)?,
    };
    let after_features = kept_features.len();
    let fc = json!({
        "type": "FeatureCollection",
        "name": layer_name,
        "features": kept_features,
    });
    let body = serde_json::to_string_pretty(&fc).map_err(|e| {
        ToolError::new("internal_error", format!("Failed to serialize GeoJSON: {e}"))
    })?;
    std::fs::write(output_path, body).map_err(|e| {
        ToolError::new(
            "internal_error",
            format!("Failed to write GeoJSON output: {e}"),
        )
    })?;
    Ok(ExportStats {
        before_features,
        after_features,
        layer: layer_name,
    })
}

// ---------- GeoPackage (SQLite) ----------

fn open_gpkg(path: &Path) -> Result<Connection, ToolError> {
    Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|e| ToolError::new("cannot_open_gis", format!("Cannot open GeoPackage: {e}")))
}

#[derive(Debug, Clone)]
struct GpkgLayerMeta {
    table_name: String,
    geom_col: String,
    geom_type: String,
    srs_id: Option<i32>,
    epsg: Option<i32>,
    srs_name: Option<String>,
    has_z: bool,
    declared_bbox: Option<BBox2D>,
}

fn load_gpkg_layers(conn: &Connection) -> Result<Vec<GpkgLayerMeta>, ToolError> {
    let mut stmt = conn
        .prepare(
            "SELECT c.table_name,
                    gc.column_name,
                    gc.geometry_type_name,
                    COALESCE(gc.srs_id, c.srs_id),
                    s.organization,
                    s.organization_coordsys_id,
                    s.srs_name,
                    COALESCE(gc.z, 0),
                    c.min_x, c.min_y, c.max_x, c.max_y
             FROM gpkg_contents c
             LEFT JOIN gpkg_geometry_columns gc ON gc.table_name = c.table_name
             LEFT JOIN gpkg_spatial_ref_sys s ON s.srs_id = COALESCE(gc.srs_id, c.srs_id)
             WHERE c.data_type = 'features'
             ORDER BY c.table_name ASC",
        )
        .map_err(|e| {
            ToolError::new(
                "cannot_open_gis",
                format!("Invalid GeoPackage metadata tables: {e}"),
            )
        })?;

    let rows = stmt
        .query_map([], |r| {
            let table_name: String = r.get(0)?;
            let geom_col: Option<String> = r.get(1)?;
            let geom_type: Option<String> = r.get(2)?;
            let srs_id: Option<i32> = r.get(3)?;
            let org: Option<String> = r.get(4)?;
            let org_id: Option<i32> = r.get(5)?;
            let srs_name: Option<String> = r.get(6)?;
            let z_flag: i32 = r.get(7)?;
            let min_x: Option<f64> = r.get(8)?;
            let min_y: Option<f64> = r.get(9)?;
            let max_x: Option<f64> = r.get(10)?;
            let max_y: Option<f64> = r.get(11)?;

            let epsg = match (org.as_deref(), org_id) {
                (Some(o), Some(id)) if o.eq_ignore_ascii_case("EPSG") => Some(id),
                _ => srs_id.filter(|&id| id > 0),
            };
            let declared_bbox = match (min_x, min_y, max_x, max_y) {
                (Some(x0), Some(y0), Some(x1), Some(y1))
                    if x0.is_finite()
                        && y0.is_finite()
                        && x1.is_finite()
                        && y1.is_finite()
                        && x0 <= x1
                        && y0 <= y1 =>
                {
                    Some(BBox2D {
                        min: [round6(x0), round6(y0)],
                        max: [round6(x1), round6(y1)],
                    })
                }
                _ => None,
            };
            Ok(GpkgLayerMeta {
                table_name,
                geom_col: geom_col.unwrap_or_else(|| "geom".to_string()),
                geom_type: geom_type.unwrap_or_else(|| "GEOMETRY".to_string()),
                srs_id,
                epsg,
                srs_name,
                has_z: z_flag != 0,
                declared_bbox,
            })
        })
        .map_err(|e| ToolError::new("cannot_open_gis", format!("Failed reading gpkg_contents: {e}")))?;

    let mut out = Vec::new();
    for item in rows {
        out.push(item.map_err(|e| {
            ToolError::new("cannot_open_gis", format!("Row read error: {e}"))
        })?);
    }
    Ok(out)
}

fn resolve_gpkg_layer<'a>(
    layers: &'a [GpkgLayerMeta],
    requested: Option<&str>,
) -> Result<&'a GpkgLayerMeta, ToolError> {
    if layers.is_empty() {
        return Err(ToolError::new(
            "invalid_input",
            "GeoPackage contains no vector feature layers in gpkg_contents.",
        ));
    }
    match requested {
        Some(name) => layers
            .iter()
            .find(|l| l.table_name == name)
            .ok_or_else(|| {
                let avail: Vec<&str> = layers.iter().map(|l| l.table_name.as_str()).collect();
                ToolError::new(
                    "invalid_input",
                    format!(
                        "Layer '{name}' not found in GeoPackage. Available layers: {}",
                        avail.join(", ")
                    ),
                )
            }),
        None => Ok(&layers[0]),
    }
}

fn summarize_gpkg(path: &Path) -> Result<Vec<LayerSummary>, ToolError> {
    let conn = open_gpkg(path)?;
    let layers = load_gpkg_layers(&conn)?;
    let mut out = Vec::with_capacity(layers.len());

    for l in layers {
        let qtable = quote_ident(&l.table_name);
        let count: i64 = conn
            .query_row(&format!("SELECT COUNT(*) FROM {qtable}"), [], |r| r.get(0))
            .unwrap_or(0);

        let mut bbox = l.declared_bbox;
        if bbox.is_none() && count > 0 {
            let qgeom = quote_ident(&l.geom_col);
            if let Ok(mut stmt) = conn.prepare(&format!("SELECT {qgeom} FROM {qtable}")) {
                let mut acc = BBox2Acc::default();
                if let Ok(mut rows) = stmt.query([]) {
                    while let Ok(Some(row)) = rows.next() {
                        if let Ok(ValueRef::Blob(b)) = row.get_ref(0)
                            && let Ok(dec) = decode_gpkg_geometry(b)
                            && let Some(gb) = dec.bbox
                        {
                            acc.push_bbox(gb);
                        }
                    }
                }
                bbox = acc.finish();
            }
        }

        let warnings = check_crs_sanity(l.epsg, bbox);
        out.push(LayerSummary {
            layer: l.table_name,
            geometry_type: l.geom_type,
            feature_count: count.max(0) as usize,
            epsg: l.epsg.or(l.srs_id),
            srs_name: l.srs_name,
            has_z: l.has_z,
            bbox,
            warnings,
        });
    }
    Ok(out)
}

fn schema_gpkg(path: &Path, layer: Option<&str>) -> Result<Vec<LayerSchema>, ToolError> {
    let conn = open_gpkg(path)?;
    let layers = load_gpkg_layers(&conn)?;
    let targets: Vec<&GpkgLayerMeta> = match layer {
        Some(name) => vec![resolve_gpkg_layer(&layers, Some(name))?],
        None => layers.iter().collect(),
    };

    let mut out = Vec::with_capacity(targets.len());
    for l in targets {
        let qtable = quote_ident(&l.table_name);
        let feature_count: usize = conn
            .query_row(&format!("SELECT COUNT(*) FROM {qtable}"), [], |r| {
                r.get::<_, i64>(0)
            })
            .unwrap_or(0)
            .max(0) as usize;

        let mut col_stmt = conn
            .prepare(&format!("PRAGMA table_info({qtable})"))
            .map_err(|e| ToolError::new("cannot_open_gis", format!("PRAGMA table_info failed: {e}")))?;

        let cols: Vec<(String, String)> = col_stmt
            .query_map([], |r| Ok((r.get::<_, String>(1)?, r.get::<_, String>(2)?)))
            .map_err(|e| ToolError::new("cannot_open_gis", format!("table_info query failed: {e}")))?
            .filter_map(Result::ok)
            .collect();

        let mut fields = Vec::new();
        for (name, dtype) in cols {
            if name == l.geom_col {
                continue;
            }
            let qcol = quote_ident(&name);
            let nulls: usize = conn
                .query_row(
                    &format!("SELECT COUNT(*) FROM {qtable} WHERE {qcol} IS NULL"),
                    [],
                    |r| r.get::<_, i64>(0),
                )
                .unwrap_or(0)
                .max(0) as usize;
            let null_rate = if feature_count > 0 {
                round3(nulls as f64 / feature_count as f64)
            } else {
                0.0
            };
            fields.push(FieldSchema {
                name,
                data_type: if dtype.is_empty() {
                    "ANY".to_string()
                } else {
                    dtype
                },
                null_count: nulls,
                null_rate,
            });
        }

        out.push(LayerSchema {
            layer: l.table_name.clone(),
            feature_count,
            geometry_column: Some(l.geom_col.clone()),
            fields,
        });
    }
    Ok(out)
}

fn sqlite_val_to_json(v: ValueRef<'_>) -> Value {
    match v {
        ValueRef::Null => Value::Null,
        ValueRef::Integer(i) => json!(i),
        ValueRef::Real(f) => json!(f),
        ValueRef::Text(t) => json!(String::from_utf8_lossy(t)),
        ValueRef::Blob(b) => json!(format!("<blob {} bytes>", b.len())),
    }
}

// spatial-cad-review[SC-04]: Keep deterministic pages and surface row or geometry errors.
fn query_gpkg_features(
    path: &Path,
    layer: Option<&str>,
    bbox: Option<[f64; 4]>,
    attr_name: Option<&str>,
    attr_contains: Option<&str>,
) -> Result<(String, Vec<FeatureSummary>), ToolError> {
    let conn = open_gpkg(path)?;
    let layers = load_gpkg_layers(&conn)?;
    let l = resolve_gpkg_layer(&layers, layer)?;
    let qtable = quote_ident(&l.table_name);

    let mut col_stmt = conn
        .prepare(&format!("PRAGMA table_info({qtable})"))
        .map_err(|e| ToolError::new("cannot_open_gis", format!("table_info failed: {e}")))?;
    let cols: Vec<String> = col_stmt
        .query_map([], |r| r.get::<_, String>(1))
        .map_err(|e| ToolError::new("cannot_open_gis", format!("table_info read failed: {e}")))?
        .filter_map(Result::ok)
        .collect();

    let prop_cols: Vec<String> = cols
        .into_iter()
        .filter(|c| c != &l.geom_col)
        .collect();

    let select_list = std::iter::once("rowid".to_string())
        .chain(std::iter::once(quote_ident(&l.geom_col)))
        .chain(prop_cols.iter().map(|c| quote_ident(c)))
        .collect::<Vec<_>>()
        .join(", ");

    let mut stmt = conn
        .prepare(&format!("SELECT {select_list} FROM {qtable} ORDER BY rowid ASC"))
        .map_err(|e| ToolError::new("cannot_open_gis", format!("SELECT failed: {e}")))?;

    let needle_lc = attr_contains.map(str::to_ascii_lowercase);
    let mut rows = stmt
        .query([])
        .map_err(|e| ToolError::new("cannot_open_gis", format!("Query execution failed: {e}")))?;
    let mut out = Vec::new();

    while let Ok(Some(row)) = rows.next() {
        let fid: i64 = row.get(0).unwrap_or(0);
        let decoded = match row.get_ref(1) {
            Ok(ValueRef::Blob(b)) => decode_gpkg_geometry(b).ok(),
            _ => None,
        };

        let gbbox = decoded.as_ref().and_then(|d| d.bbox);
        if let Some(b) = bbox {
            let Some(gb) = gbbox else {
                continue;
            };
            if !bbox_intersects(gb, b) {
                continue;
            }
        }

        let mut props = Map::new();
        for (idx, col) in prop_cols.iter().enumerate() {
            if let Ok(vr) = row.get_ref(idx + 2) {
                props.insert(col.clone(), sqlite_val_to_json(vr));
            }
        }

        if !matches_attribute_filter(&props, attr_name, needle_lc.as_deref()) {
            continue;
        }

        out.push(FeatureSummary {
            fid,
            layer: l.table_name.clone(),
            geometry_type: decoded
                .as_ref()
                .map(|d| d.geom_type.clone())
                .unwrap_or_else(|| "Empty".to_string()),
            vertex_count: decoded.as_ref().map(|d| d.vertex_count).unwrap_or(0),
            bbox: gbbox,
            properties: props,
        });
    }

    Ok((l.table_name.clone(), out))
}

fn collect_gpkg_geojson_features(
    path: &Path,
    layer: Option<&str>,
    bbox: [f64; 4],
) -> Result<(String, usize, Vec<Value>), ToolError> {
    let conn = open_gpkg(path)?;
    let layers = load_gpkg_layers(&conn)?;
    let l = resolve_gpkg_layer(&layers, layer)?;
    let qtable = quote_ident(&l.table_name);

    let mut col_stmt = conn
        .prepare(&format!("PRAGMA table_info({qtable})"))
        .map_err(|e| ToolError::new("cannot_open_gis", format!("table_info failed: {e}")))?;
    let prop_cols: Vec<String> = col_stmt
        .query_map([], |r| r.get::<_, String>(1))
        .map_err(|e| ToolError::new("cannot_open_gis", format!("table_info read failed: {e}")))?
        .filter_map(Result::ok)
        .filter(|c| c != &l.geom_col)
        .collect();

    let select_list = std::iter::once("rowid".to_string())
        .chain(std::iter::once(quote_ident(&l.geom_col)))
        .chain(prop_cols.iter().map(|c| quote_ident(c)))
        .collect::<Vec<_>>()
        .join(", ");

    let mut stmt = conn
        .prepare(&format!("SELECT {select_list} FROM {qtable} ORDER BY rowid ASC"))
        .map_err(|e| ToolError::new("cannot_open_gis", format!("SELECT failed: {e}")))?;

    let mut rows = stmt
        .query([])
        .map_err(|e| ToolError::new("cannot_open_gis", format!("Query execution failed: {e}")))?;
    let mut total = 0usize;
    let mut kept = Vec::new();

    while let Ok(Some(row)) = rows.next() {
        total += 1;
        let fid: i64 = row.get(0).unwrap_or(0);
        let Ok(ValueRef::Blob(b)) = row.get_ref(1) else {
            continue;
        };
        let Ok(dec) = decode_gpkg_geometry(b) else {
            continue;
        };
        let Some(gb) = dec.bbox else {
            continue;
        };
        if !bbox_intersects(gb, bbox) {
            continue;
        }
        let mut props = Map::new();
        for (idx, col) in prop_cols.iter().enumerate() {
            if let Ok(vr) = row.get_ref(idx + 2) {
                props.insert(col.clone(), sqlite_val_to_json(vr));
            }
        }
        kept.push(json!({
            "type": "Feature",
            "id": fid,
            "geometry": dec.geojson,
            "properties": props,
        }));
    }

    Ok((l.table_name.clone(), total, kept))
}

// ---------- GeoJSON ----------

fn load_geojson_features(path: &Path) -> Result<(String, Vec<Value>), ToolError> {
    let raw = std::fs::read_to_string(path).map_err(|e| {
        ToolError::new("cannot_open_gis", format!("Failed to read GeoJSON file: {e}"))
    })?;
    let root: Value = serde_json::from_str(&raw).map_err(|e| {
        ToolError::new("cannot_open_gis", format!("Invalid GeoJSON JSON: {e}"))
    })?;
    let default_name = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("default")
        .to_string();

    let kind = root.get("type").and_then(|v| v.as_str()).unwrap_or("");
    match kind {
        "FeatureCollection" => {
            let name = root
                .get("name")
                .and_then(|v| v.as_str())
                .map(String::from)
                .unwrap_or(default_name);
            let feats = root
                .get("features")
                .and_then(|v| v.as_array())
                .cloned()
                .ok_or_else(|| {
                    ToolError::new(
                        "cannot_open_gis",
                        "GeoJSON FeatureCollection is missing a 'features' array.",
                    )
                })?;
            Ok((name, feats))
        }
        "Feature" => Ok((default_name, vec![root])),
        "Point"
        | "LineString"
        | "Polygon"
        | "MultiPoint"
        | "MultiLineString"
        | "MultiPolygon"
        | "GeometryCollection" => Ok((
            default_name,
            vec![json!({"type": "Feature", "geometry": root, "properties": {}})],
        )),
        _ => Err(ToolError::new(
            "cannot_open_gis",
            "JSON root does not have a valid GeoJSON 'type' field.",
        )),
    }
}

fn inspect_geojson_geom(geom: &Value) -> (String, usize, bool, Option<BBox2D>) {
    let gtype = geom
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("Null")
        .to_string();
    let mut acc = BBox2Acc::default();
    let mut vcount = 0usize;
    let mut has_z = false;

    fn walk_coords(v: &Value, acc: &mut BBox2Acc, count: &mut usize, has_z: &mut bool) {
        if let Some(arr) = v.as_array() {
            if arr.len() >= 2 && arr[0].is_number() && arr[1].is_number() {
                if let (Some(x), Some(y)) = (arr[0].as_f64(), arr[1].as_f64()) {
                    acc.push(x, y);
                    *count += 1;
                    if arr.len() >= 3 && arr[2].is_number() {
                        *has_z = true;
                    }
                }
            } else {
                for child in arr {
                    walk_coords(child, acc, count, has_z);
                }
            }
        }
    }

    if gtype == "GeometryCollection" {
        if let Some(geoms) = geom.get("geometries").and_then(|v| v.as_array()) {
            for g in geoms {
                if let Some(c) = g.get("coordinates") {
                    walk_coords(c, &mut acc, &mut vcount, &mut has_z);
                }
            }
        }
    } else if let Some(c) = geom.get("coordinates") {
        walk_coords(c, &mut acc, &mut vcount, &mut has_z);
    }

    (gtype, vcount, has_z, acc.finish())
}

fn summarize_geojson(path: &Path) -> Result<LayerSummary, ToolError> {
    let (name, feats) = load_geojson_features(path)?;
    let mut types = BTreeSet::new();
    let mut acc = BBox2Acc::default();
    let mut any_z = false;

    for f in &feats {
        if let Some(g) = f.get("geometry")
            && !g.is_null()
        {
            let (gt, _, hz, gb) = inspect_geojson_geom(g);
            types.insert(gt);
            if hz {
                any_z = true;
            }
            if let Some(b) = gb {
                acc.push_bbox(b);
            }
        }
    }

    let geometry_type = if types.is_empty() {
        "None".to_string()
    } else if types.len() == 1 {
        types.into_iter().next().unwrap_or_default()
    } else {
        types.into_iter().collect::<Vec<_>>().join("|")
    };
    let bbox = acc.finish();
    let warnings = check_crs_sanity(Some(4326), bbox);

    Ok(LayerSummary {
        layer: name,
        geometry_type,
        feature_count: feats.len(),
        epsg: Some(4326),
        srs_name: Some("WGS 84 (GeoJSON RFC 7946 default)".to_string()),
        has_z: any_z,
        bbox,
        warnings,
    })
}

fn schema_geojson(path: &Path) -> Result<LayerSchema, ToolError> {
    let (name, feats) = load_geojson_features(path)?;
    let mut field_types: BTreeMap<String, BTreeSet<&'static str>> = BTreeMap::new();
    let mut non_nulls: BTreeMap<String, usize> = BTreeMap::new();

    for f in &feats {
        if let Some(props) = f.get("properties").and_then(|v| v.as_object()) {
            for (k, v) in props {
                let entry = field_types.entry(k.clone()).or_default();
                if !v.is_null() {
                    *non_nulls.entry(k.clone()).or_insert(0) += 1;
                    entry.insert(match v {
                        Value::Bool(_) => "BOOLEAN",
                        Value::Number(n) if n.is_i64() || n.is_u64() => "INTEGER",
                        Value::Number(_) => "REAL",
                        Value::String(_) => "TEXT",
                        Value::Array(_) => "ARRAY",
                        Value::Object(_) => "OBJECT",
                        Value::Null => "NULL",
                    });
                }
            }
        }
    }

    let total = feats.len();
    let fields = field_types
        .into_iter()
        .map(|(k, ts)| {
            let nn = non_nulls.get(&k).copied().unwrap_or(0);
            let nulls = total.saturating_sub(nn);
            let dtype = if ts.is_empty() {
                "NULL".to_string()
            } else {
                ts.into_iter().collect::<Vec<_>>().join("|")
            };
            FieldSchema {
                name: k,
                data_type: dtype,
                null_count: nulls,
                null_rate: if total > 0 {
                    round3(nulls as f64 / total as f64)
                } else {
                    0.0
                },
            }
        })
        .collect();

    Ok(LayerSchema {
        layer: name,
        feature_count: total,
        geometry_column: None,
        fields,
    })
}

fn query_geojson_features(
    path: &Path,
    bbox: Option<[f64; 4]>,
    attr_name: Option<&str>,
    attr_contains: Option<&str>,
) -> Result<(String, Vec<FeatureSummary>), ToolError> {
    let (name, feats) = load_geojson_features(path)?;
    let needle_lc = attr_contains.map(str::to_ascii_lowercase);
    let mut out = Vec::new();

    for (idx, f) in feats.into_iter().enumerate() {
        let fid = f
            .get("id")
            .and_then(|v| v.as_i64())
            .unwrap_or((idx + 1) as i64);
        let (gtype, vcount, _, gbbox) = match f.get("geometry") {
            Some(g) if !g.is_null() => inspect_geojson_geom(g),
            _ => ("Empty".to_string(), 0, false, None),
        };
        if let Some(b) = bbox {
            let Some(gb) = gbbox else {
                continue;
            };
            if !bbox_intersects(gb, b) {
                continue;
            }
        }
        let props = f
            .get("properties")
            .and_then(|v| v.as_object())
            .cloned()
            .unwrap_or_default();
        if !matches_attribute_filter(&props, attr_name, needle_lc.as_deref()) {
            continue;
        }
        out.push(FeatureSummary {
            fid,
            layer: name.clone(),
            geometry_type: gtype,
            vertex_count: vcount,
            bbox: gbbox,
            properties: props,
        });
    }

    Ok((name, out))
}

fn collect_geojson_filtered(
    path: &Path,
    bbox: [f64; 4],
) -> Result<(String, usize, Vec<Value>), ToolError> {
    let (name, feats) = load_geojson_features(path)?;
    let before = feats.len();
    let kept: Vec<Value> = feats
        .into_iter()
        .filter(|f| {
            let Some(g) = f.get("geometry").filter(|v| !v.is_null()) else {
                return false;
            };
            let (_, _, _, gbbox) = inspect_geojson_geom(g);
            gbbox.is_some_and(|gb| bbox_intersects(gb, bbox))
        })
        .collect();
    Ok((name, before, kept))
}

// ---------- GeoPackage Binary Header + WKB Decoder ----------

pub fn decode_gpkg_geometry(blob: &[u8]) -> Result<DecodedGeometry, ToolError> {
    let bad = || ToolError::new("cannot_open_gis", "Invalid GeoPackage geometry blob");
    if blob.len() < 8 || &blob[..3] != b"GP\0" {
        return Err(bad());
    }
    let flags = blob[3];
    let empty = (flags & 0x10) != 0;
    if empty {
        return Ok(DecodedGeometry {
            geom_type: "Empty".to_string(),
            vertex_count: 0,
            bbox: None,
            geojson: Value::Null,
        });
    }
    let env_len = match (flags >> 1) & 7 {
        0 => 0,
        1 => 32,
        2 | 3 => 48,
        4 => 64,
        _ => return Err(bad()),
    };
    let wkb = blob.get(8 + env_len..).ok_or_else(bad)?;
    let mut cursor = 0usize;
    let mut acc = BBox2Acc::default();
    let mut vcount = 0usize;
    let (gtype, geojson) = read_wkb(wkb, &mut cursor, &mut acc, &mut vcount).ok_or_else(bad)?;
    Ok(DecodedGeometry {
        geom_type: gtype,
        vertex_count: vcount,
        bbox: acc.finish(),
        geojson,
    })
}

fn read_u32(buf: &[u8], cur: &mut usize, little: bool) -> Option<u32> {
    let slice: [u8; 4] = buf.get(*cur..*cur + 4)?.try_into().ok()?;
    *cur += 4;
    Some(if little {
        u32::from_le_bytes(slice)
    } else {
        u32::from_be_bytes(slice)
    })
}

fn read_f64(buf: &[u8], cur: &mut usize, little: bool) -> Option<f64> {
    let slice: [u8; 8] = buf.get(*cur..*cur + 8)?.try_into().ok()?;
    *cur += 8;
    Some(if little {
        f64::from_le_bytes(slice)
    } else {
        f64::from_be_bytes(slice)
    })
}

fn read_point_coords(
    buf: &[u8],
    cur: &mut usize,
    little: bool,
    has_z: bool,
    has_m: bool,
    acc: &mut BBox2Acc,
    vcount: &mut usize,
) -> Option<Value> {
    let x = read_f64(buf, cur, little)?;
    let y = read_f64(buf, cur, little)?;
    let z = if has_z {
        Some(read_f64(buf, cur, little)?)
    } else {
        None
    };
    if has_m {
        let _ = read_f64(buf, cur, little)?;
    }
    acc.push(x, y);
    *vcount += 1;
    Some(match z {
        Some(zv) => json!([x, y, zv]),
        None => json!([x, y]),
    })
}

fn read_wkb(
    buf: &[u8],
    cur: &mut usize,
    acc: &mut BBox2Acc,
    vcount: &mut usize,
) -> Option<(String, Value)> {
    let byte_order = *buf.get(*cur)?;
    *cur += 1;
    let little = match byte_order {
        0 => false,
        1 => true,
        _ => return None,
    };
    let raw_type = read_u32(buf, cur, little)?;
    // Support both ISO WKB (+1000/+2000/+3000) and EWKB high-bit flags
    let ewkb_z = (raw_type & 0x8000_0000) != 0;
    let ewkb_m = (raw_type & 0x4000_0000) != 0;
    let ewkb_srid = (raw_type & 0x2000_0000) != 0;
    if ewkb_srid {
        let _ = read_u32(buf, cur, little)?;
    }
    let masked = raw_type & 0x0fff_ffff;
    let iso_dim = masked / 1000;
    let base_type = masked % 1000;
    let has_z = ewkb_z || iso_dim == 1 || iso_dim == 3;
    let has_m = ewkb_m || iso_dim == 2 || iso_dim == 3;

    match base_type {
        1 => {
            let pt = read_point_coords(buf, cur, little, has_z, has_m, acc, vcount)?;
            Some((
                "Point".to_string(),
                json!({"type": "Point", "coordinates": pt}),
            ))
        }
        2 => {
            let n = read_u32(buf, cur, little)? as usize;
            let mut coords = Vec::with_capacity(n.min(65536));
            for _ in 0..n {
                coords.push(read_point_coords(
                    buf, cur, little, has_z, has_m, acc, vcount,
                )?);
            }
            Some((
                "LineString".to_string(),
                json!({"type": "LineString", "coordinates": coords}),
            ))
        }
        3 => {
            let nrings = read_u32(buf, cur, little)? as usize;
            let mut rings = Vec::with_capacity(nrings.min(1024));
            for _ in 0..nrings {
                let n = read_u32(buf, cur, little)? as usize;
                let mut ring = Vec::with_capacity(n.min(65536));
                for _ in 0..n {
                    ring.push(read_point_coords(
                        buf, cur, little, has_z, has_m, acc, vcount,
                    )?);
                }
                rings.push(Value::Array(ring));
            }
            Some((
                "Polygon".to_string(),
                json!({"type": "Polygon", "coordinates": rings}),
            ))
        }
        4 => {
            let n = read_u32(buf, cur, little)? as usize;
            let mut pts = Vec::with_capacity(n.min(4096));
            for _ in 0..n {
                let (_, sub) = read_wkb(buf, cur, acc, vcount)?;
                pts.push(sub.get("coordinates").cloned().unwrap_or(Value::Null));
            }
            Some((
                "MultiPoint".to_string(),
                json!({"type": "MultiPoint", "coordinates": pts}),
            ))
        }
        5 => {
            let n = read_u32(buf, cur, little)? as usize;
            let mut lines = Vec::with_capacity(n.min(4096));
            for _ in 0..n {
                let (_, sub) = read_wkb(buf, cur, acc, vcount)?;
                lines.push(sub.get("coordinates").cloned().unwrap_or(Value::Null));
            }
            Some((
                "MultiLineString".to_string(),
                json!({"type": "MultiLineString", "coordinates": lines}),
            ))
        }
        6 => {
            let n = read_u32(buf, cur, little)? as usize;
            let mut polys = Vec::with_capacity(n.min(4096));
            for _ in 0..n {
                let (_, sub) = read_wkb(buf, cur, acc, vcount)?;
                polys.push(sub.get("coordinates").cloned().unwrap_or(Value::Null));
            }
            Some((
                "MultiPolygon".to_string(),
                json!({"type": "MultiPolygon", "coordinates": polys}),
            ))
        }
        7 => {
            let n = read_u32(buf, cur, little)? as usize;
            let mut geoms = Vec::with_capacity(n.min(4096));
            for _ in 0..n {
                let (_, sub) = read_wkb(buf, cur, acc, vcount)?;
                geoms.push(sub);
            }
            Some((
                "GeometryCollection".to_string(),
                json!({"type": "GeometryCollection", "geometries": geoms}),
            ))
        }
        _ => None,
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct PointMatchSummary {
    pub fid: i64,
    pub layer: String,
    pub geometry_type: String,
    pub contains: bool,
    pub distance: f64,
    pub bbox: Option<BBox2D>,
    pub properties: Map<String, Value>,
}

/// Spatial point query on a GeoPackage or GeoJSON layer: finds features
/// containing `xy` (exact point-in-polygon with hole rings) or within `radius`.
pub fn query_features_by_point(
    path: &Path,
    layer: Option<&str>,
    xy: [f64; 2],
    radius: f64,
) -> Result<(String, Vec<PointMatchSummary>), ToolError> {
    let search_bbox = [xy[0] - radius, xy[1] - radius, xy[0] + radius, xy[1] + radius];
    let (layer_name, mut matches) = match detect_format(path)? {
        GisFormat::Geopackage => {
            let (lname, _, feats) = collect_gpkg_geojson_features(path, layer, search_bbox)?;
            let mut out = Vec::new();
            for feat in feats {
                if let Some(m) = eval_geojson_point_match(&feat, &lname, xy, radius) {
                    out.push(m);
                }
            }
            (lname, out)
        }
        GisFormat::Geojson => {
            let (lname, feats) = load_geojson_features(path)?;
            let mut out = Vec::new();
            for (idx, mut feat) in feats.into_iter().enumerate() {
                if feat.get("id").is_none()
                    && let Some(obj) = feat.as_object_mut()
                {
                    obj.insert("id".to_string(), json!((idx + 1) as i64));
                }
                if let Some(m) = eval_geojson_point_match(&feat, &lname, xy, radius) {
                    out.push(m);
                }
            }
            (lname, out)
        }
    };

    matches.sort_by(|a, b| {
        b.contains
            .cmp(&a.contains)
            .then_with(|| a.distance.total_cmp(&b.distance))
            .then_with(|| a.fid.cmp(&b.fid))
    });
    Ok((layer_name, matches))
}

fn eval_geojson_point_match(
    feat: &Value,
    layer: &str,
    xy: [f64; 2],
    radius: f64,
) -> Option<PointMatchSummary> {
    let geom = feat.get("geometry")?;
    if geom.is_null() {
        return None;
    }
    let (gtype, _, _, gbbox) = inspect_geojson_geom(geom);
    let gb = gbbox?;
    let search_bbox = [xy[0] - radius, xy[1] - radius, xy[0] + radius, xy[1] + radius];
    if !bbox_intersects(gb, search_bbox) {
        return None;
    }
    let (contains, dist) = geom_point_distance(geom, xy)?;
    if !contains && dist > radius + 1e-9 {
        return None;
    }
    let fid = feat.get("id").and_then(|v| v.as_i64()).unwrap_or(0);
    let props = feat
        .get("properties")
        .and_then(|v| v.as_object())
        .cloned()
        .unwrap_or_default();
    Some(PointMatchSummary {
        fid,
        layer: layer.to_string(),
        geometry_type: gtype,
        contains,
        distance: round6(dist),
        bbox: Some(gb),
        properties: props,
    })
}

fn parse_xy(v: &Value) -> Option<[f64; 2]> {
    let arr = v.as_array()?;
    if arr.len() >= 2 {
        Some([arr[0].as_f64()?, arr[1].as_f64()?])
    } else {
        None
    }
}

fn parse_ring(v: &Value) -> Vec<[f64; 2]> {
    v.as_array()
        .map(|arr| arr.iter().filter_map(parse_xy).collect())
        .unwrap_or_default()
}

fn dist_pt_seg(p: [f64; 2], a: [f64; 2], b: [f64; 2]) -> f64 {
    let dx = b[0] - a[0];
    let dy = b[1] - a[1];
    let l2 = dx * dx + dy * dy;
    if l2 <= 1e-24 {
        return (p[0] - a[0]).hypot(p[1] - a[1]);
    }
    let t = (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / l2).clamp(0.0, 1.0);
    let px = a[0] + t * dx;
    let py = a[1] + t * dy;
    (p[0] - px).hypot(p[1] - py)
}

fn dist_pt_polyline(p: [f64; 2], pts: &[[f64; 2]], closed: bool) -> f64 {
    if pts.is_empty() {
        return f64::INFINITY;
    }
    if pts.len() == 1 {
        return (p[0] - pts[0][0]).hypot(p[1] - pts[0][1]);
    }
    let mut best = f64::INFINITY;
    for w in pts.windows(2) {
        best = best.min(dist_pt_seg(p, w[0], w[1]));
    }
    if closed && pts.len() >= 3 {
        best = best.min(dist_pt_seg(p, pts[pts.len() - 1], pts[0]));
    }
    best
}

fn point_in_ring(p: [f64; 2], ring: &[[f64; 2]]) -> bool {
    if ring.len() < 3 {
        return false;
    }
    if dist_pt_polyline(p, ring, true) <= 1e-9 {
        return true;
    }
    let mut inside = false;
    let mut j = ring.len() - 1;
    for i in 0..ring.len() {
        let (xi, yi) = (ring[i][0], ring[i][1]);
        let (xj, yj) = (ring[j][0], ring[j][1]);
        let intersects = ((yi > p[1]) != (yj > p[1]))
            && (p[0] < (xj - xi) * (p[1] - yi) / (yj - yi) + xi);
        if intersects {
            inside = !inside;
        }
        j = i;
    }
    inside
}

fn polygon_point_distance(rings_val: &Value, p: [f64; 2]) -> Option<(bool, f64)> {
    let rings_arr = rings_val.as_array()?;
    if rings_arr.is_empty() {
        return None;
    }
    let outer = parse_ring(&rings_arr[0]);
    if outer.len() < 3 {
        return None;
    }
    let mut inside = point_in_ring(p, &outer);
    let mut best_d = dist_pt_polyline(p, &outer, true);
    for hole_val in &rings_arr[1..] {
        let hole = parse_ring(hole_val);
        if hole.len() >= 3 {
            best_d = best_d.min(dist_pt_polyline(p, &hole, true));
            if inside && point_in_ring(p, &hole) && dist_pt_polyline(p, &hole, true) > 1e-9 {
                inside = false;
            }
        }
    }
    if inside {
        Some((true, 0.0))
    } else {
        Some((false, best_d))
    }
}

fn geom_point_distance(geom: &Value, p: [f64; 2]) -> Option<(bool, f64)> {
    let gtype = geom.get("type")?.as_str()?;
    let coords = geom.get("coordinates");
    match gtype {
        "Point" => {
            let q = parse_xy(coords?)?;
            let d = (p[0] - q[0]).hypot(p[1] - q[1]);
            Some((d <= 1e-9, d))
        }
        "MultiPoint" => {
            let pts = parse_ring(coords?);
            let best = pts
                .iter()
                .map(|q| (p[0] - q[0]).hypot(p[1] - q[1]))
                .fold(f64::INFINITY, f64::min);
            best.is_finite().then_some((best <= 1e-9, best))
        }
        "LineString" => {
            let pts = parse_ring(coords?);
            let d = dist_pt_polyline(p, &pts, false);
            d.is_finite().then_some((d <= 1e-9, d))
        }
        "MultiLineString" => {
            let arr = coords?.as_array()?;
            let best = arr
                .iter()
                .map(|line| dist_pt_polyline(p, &parse_ring(line), false))
                .fold(f64::INFINITY, f64::min);
            best.is_finite().then_some((best <= 1e-9, best))
        }
        "Polygon" => polygon_point_distance(coords?, p),
        "MultiPolygon" => {
            let arr = coords?.as_array()?;
            let mut best = f64::INFINITY;
            for poly in arr {
                if let Some((inside, d)) = polygon_point_distance(poly, p) {
                    if inside {
                        return Some((true, 0.0));
                    }
                    best = best.min(d);
                }
            }
            best.is_finite().then_some((false, best))
        }
        "GeometryCollection" => {
            let arr = geom.get("geometries")?.as_array()?;
            let mut best = f64::INFINITY;
            for sub in arr {
                if let Some((inside, d)) = geom_point_distance(sub, p) {
                    if inside {
                        return Some((true, 0.0));
                    }
                    best = best.min(d);
                }
            }
            best.is_finite().then_some((false, best))
        }
        _ => None,
    }
}

// ---------- Helpers ----------

#[derive(Default)]
struct BBox2Acc {
    any: bool,
    min: [f64; 2],
    max: [f64; 2],
}

impl BBox2Acc {
    fn push(&mut self, x: f64, y: f64) {
        if !x.is_finite() || !y.is_finite() {
            return;
        }
        if !self.any {
            self.any = true;
            self.min = [x, y];
            self.max = [x, y];
        } else {
            self.min[0] = self.min[0].min(x);
            self.min[1] = self.min[1].min(y);
            self.max[0] = self.max[0].max(x);
            self.max[1] = self.max[1].max(y);
        }
    }

    fn push_bbox(&mut self, b: BBox2D) {
        self.push(b.min[0], b.min[1]);
        self.push(b.max[0], b.max[1]);
    }

    fn finish(&self) -> Option<BBox2D> {
        self.any.then_some(BBox2D {
            min: [round6(self.min[0]), round6(self.min[1])],
            max: [round6(self.max[0]), round6(self.max[1])],
        })
    }
}

fn bbox_intersects(a: BBox2D, b: [f64; 4]) -> bool {
    a.max[0] >= b[0] && a.min[0] <= b[2] && a.max[1] >= b[1] && a.min[1] <= b[3]
}

fn matches_attribute_filter(
    props: &Map<String, Value>,
    attr_name: Option<&str>,
    needle_lc: Option<&str>,
) -> bool {
    match (attr_name, needle_lc) {
        (Some(name), Some(needle)) => {
            let Some(val) = props.get(name) else {
                return false;
            };
            val_to_search_str(val).to_ascii_lowercase().contains(needle)
        }
        (Some(name), None) => props.get(name).is_some_and(|v| !v.is_null()),
        (None, Some(needle)) => props
            .values()
            .any(|v| val_to_search_str(v).to_ascii_lowercase().contains(needle)),
        (None, None) => true,
    }
}

fn val_to_search_str(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gpkg_point_blob(x: f64, y: f64, srs: i32) -> Vec<u8> {
        let mut b = b"GP\0\x01".to_vec();
        b.extend_from_slice(&srs.to_le_bytes());
        b.push(1); // little-endian WKB
        b.extend_from_slice(&1u32.to_le_bytes()); // Point
        b.extend_from_slice(&x.to_le_bytes());
        b.extend_from_slice(&y.to_le_bytes());
        b
    }

    #[test]
    fn decodes_gpkg_point_and_detects_crs_mismatch() {
        let blob = gpkg_point_blob(500_123.0, 5_650_000.0, 4326);
        let dec = decode_gpkg_geometry(&blob).unwrap();
        assert_eq!(dec.geom_type, "Point");
        assert_eq!(dec.vertex_count, 1);
        let warns = check_crs_sanity(Some(4326), dec.bbox);
        assert!(!warns.is_empty());
    }

    #[test]
    fn reads_and_exports_synthetic_geopackage() {
        let dir = std::env::temp_dir().join(format!("mcp-gis-gpkg-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let db = dir.join("demo.gpkg");
        let conn = Connection::open(&db).unwrap();
        conn.execute_batch(
            "CREATE TABLE gpkg_spatial_ref_sys (srs_name TEXT, srs_id INTEGER PRIMARY KEY, organization TEXT, organization_coordsys_id INTEGER, definition TEXT);
             INSERT INTO gpkg_spatial_ref_sys VALUES ('WGS 84', 4326, 'EPSG', 4326, 'GEOGCS[...]');
             CREATE TABLE gpkg_contents (table_name TEXT PRIMARY KEY, data_type TEXT, identifier TEXT, srs_id INTEGER, min_x REAL, min_y REAL, max_x REAL, max_y REAL);
             INSERT INTO gpkg_contents VALUES ('stations', 'features', 'stations', 4326, 10.0, 50.0, 12.0, 52.0);
             CREATE TABLE gpkg_geometry_columns (table_name TEXT, column_name TEXT, geometry_type_name TEXT, srs_id INTEGER, z INTEGER, m INTEGER);
             INSERT INTO gpkg_geometry_columns VALUES ('stations', 'geom', 'POINT', 4326, 0, 0);
             CREATE TABLE stations (fid INTEGER PRIMARY KEY, name TEXT, geom BLOB);",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO stations (fid, name, geom) VALUES (1, 'Alpha', ?1), (2, 'Beta', ?2)",
            rusqlite::params![
                gpkg_point_blob(10.5, 50.5, 4326),
                gpkg_point_blob(11.8, 51.9, 4326),
            ],
        )
        .unwrap();
        drop(conn);

        let (fmt, layers) = summarize_dataset(&db).unwrap();
        assert_eq!(fmt, GisFormat::Geopackage);
        assert_eq!(layers.len(), 1);
        assert_eq!(layers[0].feature_count, 2);
        assert_eq!(layers[0].epsg, Some(4326));
        assert!(layers[0].warnings.is_empty());

        let schemas = extract_schema(&db, Some("stations")).unwrap();
        assert_eq!(schemas[0].fields.len(), 2);

        let out_file = dir.join("clip.geojson");
        let stats = export_bbox_geojson(&db, Some("stations"), [10.0, 50.0, 11.0, 51.0], &out_file)
            .unwrap();
        assert_eq!(stats.before_features, 2);
        assert_eq!(stats.after_features, 1);
        assert!(out_file.is_file());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
