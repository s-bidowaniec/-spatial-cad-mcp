//! General ASCII DXF (R2000) writer and the `DxfNewEntity` tagged union used
//! by `dxf_create` / `dxf_add_entities`. Emits HEADER, TABLES (LAYER records),
//! optional BLOCKS, ENTITIES and EOF.

use rmcp::schemars;
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::Path;

pub const MAX_CREATE_ENTITIES: usize = 200;
pub const MAX_TEXT_CHARS: usize = 256;

fn default_layer() -> String {
    "0".to_string()
}

/// Tagged union of new entities supported by `dxf_create` and `dxf_add_entities`.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DxfNewEntity {
    #[serde(alias = "line")]
    Line {
        #[serde(default = "default_layer")]
        layer: String,
        start: Vec<f64>,
        end: Vec<f64>,
        color: Option<i32>,
    },
    #[serde(alias = "circle")]
    Circle {
        #[serde(default = "default_layer")]
        layer: String,
        center: Vec<f64>,
        radius: f64,
        color: Option<i32>,
    },
    #[serde(alias = "arc")]
    Arc {
        #[serde(default = "default_layer")]
        layer: String,
        center: Vec<f64>,
        radius: f64,
        start_angle_deg: f64,
        end_angle_deg: f64,
        color: Option<i32>,
    },
    #[serde(alias = "lwpolyline")]
    Lwpolyline {
        #[serde(default = "default_layer")]
        layer: String,
        points: Vec<[f64; 2]>,
        #[serde(default)]
        closed: bool,
        color: Option<i32>,
    },
    #[serde(alias = "polyline3d", alias = "POLYLINE3D")]
    Polyline3d {
        #[serde(default = "default_layer")]
        layer: String,
        points: Vec<[f64; 3]>,
        color: Option<i32>,
    },
    #[serde(alias = "text")]
    Text {
        #[serde(default = "default_layer")]
        layer: String,
        insert: Vec<f64>,
        height: f64,
        text: String,
        rotation_deg: Option<f64>,
        color: Option<i32>,
    },
    #[serde(alias = "point")]
    Point {
        #[serde(default = "default_layer")]
        layer: String,
        at: Vec<f64>,
        color: Option<i32>,
    },
}

impl DxfNewEntity {
    pub fn entity_type_name(&self) -> &'static str {
        match self {
            DxfNewEntity::Line { .. } => "LINE",
            DxfNewEntity::Circle { .. } => "CIRCLE",
            DxfNewEntity::Arc { .. } => "ARC",
            DxfNewEntity::Lwpolyline { .. } => "LWPOLYLINE",
            DxfNewEntity::Polyline3d { .. } => "POLYLINE",
            DxfNewEntity::Text { .. } => "TEXT",
            DxfNewEntity::Point { .. } => "POINT",
        }
    }

    pub fn layer(&self) -> &str {
        match self {
            DxfNewEntity::Line { layer, .. }
            | DxfNewEntity::Circle { layer, .. }
            | DxfNewEntity::Arc { layer, .. }
            | DxfNewEntity::Lwpolyline { layer, .. }
            | DxfNewEntity::Polyline3d { layer, .. }
            | DxfNewEntity::Text { layer, .. }
            | DxfNewEntity::Point { layer, .. } => layer.as_str(),
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.layer().trim().is_empty() {
            return Err("entity layer cannot be empty".into());
        }
        match self {
            DxfNewEntity::Line {
                start, end, color, ..
            } => {
                validate_pt23(start, "LINE.start")?;
                validate_pt23(end, "LINE.end")?;
                validate_opt_color(*color)?;
            }
            DxfNewEntity::Circle {
                center,
                radius,
                color,
                ..
            } => {
                validate_pt23(center, "CIRCLE.center")?;
                if !radius.is_finite() || *radius <= 0.0 {
                    return Err("CIRCLE.radius must be finite and > 0".into());
                }
                validate_opt_color(*color)?;
            }
            DxfNewEntity::Arc {
                center,
                radius,
                start_angle_deg,
                end_angle_deg,
                color,
                ..
            } => {
                validate_pt23(center, "ARC.center")?;
                if !radius.is_finite() || *radius <= 0.0 {
                    return Err("ARC.radius must be finite and > 0".into());
                }
                if !start_angle_deg.is_finite() || !end_angle_deg.is_finite() {
                    return Err("ARC angles must be finite".into());
                }
                validate_opt_color(*color)?;
            }
            DxfNewEntity::Lwpolyline {
                points, color, ..
            } => {
                if points.len() < 2 {
                    return Err("LWPOLYLINE requires at least 2 points".into());
                }
                for p in points {
                    if !p[0].is_finite() || !p[1].is_finite() {
                        return Err("LWPOLYLINE points must be finite".into());
                    }
                }
                validate_opt_color(*color)?;
            }
            DxfNewEntity::Polyline3d { points, color, .. } => {
                if points.len() < 2 {
                    return Err("POLYLINE3D requires at least 2 points".into());
                }
                for p in points {
                    if !p[0].is_finite() || !p[1].is_finite() || !p[2].is_finite() {
                        return Err("POLYLINE3D points must be finite".into());
                    }
                }
                validate_opt_color(*color)?;
            }
            DxfNewEntity::Text {
                insert,
                height,
                rotation_deg,
                color,
                ..
            } => {
                validate_pt23(insert, "TEXT.insert")?;
                if !height.is_finite() || *height <= 0.0 {
                    return Err("TEXT.height must be finite and > 0".into());
                }
                if let Some(rot) = rotation_deg
                    && !rot.is_finite()
                {
                    return Err("TEXT.rotation_deg must be finite".into());
                }
                validate_opt_color(*color)?;
            }
            DxfNewEntity::Point { at, color, .. } => {
                validate_pt23(at, "POINT.at")?;
                validate_opt_color(*color)?;
            }
        }
        Ok(())
    }

    pub fn bbox_3d(&self) -> [f64; 6] {
        match self {
            DxfNewEntity::Line { start, end, .. } => {
                let (x1, y1, z1) = xyz(start);
                let (x2, y2, z2) = xyz(end);
                [
                    x1.min(x2),
                    y1.min(y2),
                    z1.min(z2),
                    x1.max(x2),
                    y1.max(y2),
                    z1.max(z2),
                ]
            }
            DxfNewEntity::Circle {
                center, radius, ..
            }
            | DxfNewEntity::Arc {
                center, radius, ..
            } => {
                let (cx, cy, cz) = xyz(center);
                let r = radius.abs();
                [cx - r, cy - r, cz, cx + r, cy + r, cz]
            }
            DxfNewEntity::Lwpolyline { points, .. } => {
                let mut min_x = f64::INFINITY;
                let mut min_y = f64::INFINITY;
                let mut max_x = f64::NEG_INFINITY;
                let mut max_y = f64::NEG_INFINITY;
                for p in points {
                    min_x = min_x.min(p[0]);
                    min_y = min_y.min(p[1]);
                    max_x = max_x.max(p[0]);
                    max_y = max_y.max(p[1]);
                }
                [min_x, min_y, 0.0, max_x, max_y, 0.0]
            }
            DxfNewEntity::Polyline3d { points, .. } => {
                let mut acc = [f64::INFINITY; 3];
                let mut max = [f64::NEG_INFINITY; 3];
                for p in points {
                    for k in 0..3 {
                        acc[k] = acc[k].min(p[k]);
                        max[k] = max[k].max(p[k]);
                    }
                }
                [acc[0], acc[1], acc[2], max[0], max[1], max[2]]
            }
            DxfNewEntity::Text { insert, .. } => {
                let (x, y, z) = xyz(insert);
                [x, y, z, x, y, z]
            }
            DxfNewEntity::Point { at, .. } => {
                let (x, y, z) = xyz(at);
                [x, y, z, x, y, z]
            }
        }
    }

    /// Serializes this entity into DXF group pairs, allocating handles from `next_handle`.
    pub fn to_group_pairs(&self, next_handle: &mut u64) -> Vec<(i32, String)> {
        let mut g = Vec::new();
        let alloc = |h: &mut u64| {
            let cur = *h;
            *h += 1;
            format!("{cur:X}")
        };
        match self {
            DxfNewEntity::Line {
                layer,
                start,
                end,
                color,
            } => {
                let (x1, y1, z1) = xyz(start);
                let (x2, y2, z2) = xyz(end);
                g.push((0, "LINE".to_string()));
                g.push((5, alloc(next_handle)));
                g.push((8, layer.clone()));
                if let Some(c) = color {
                    g.push((62, c.to_string()));
                }
                g.push((10, fmt_f64(x1)));
                g.push((20, fmt_f64(y1)));
                g.push((30, fmt_f64(z1)));
                g.push((11, fmt_f64(x2)));
                g.push((21, fmt_f64(y2)));
                g.push((31, fmt_f64(z2)));
            }
            DxfNewEntity::Circle {
                layer,
                center,
                radius,
                color,
            } => {
                let (cx, cy, cz) = xyz(center);
                g.push((0, "CIRCLE".to_string()));
                g.push((5, alloc(next_handle)));
                g.push((8, layer.clone()));
                if let Some(c) = color {
                    g.push((62, c.to_string()));
                }
                g.push((10, fmt_f64(cx)));
                g.push((20, fmt_f64(cy)));
                g.push((30, fmt_f64(cz)));
                g.push((40, fmt_f64(*radius)));
            }
            DxfNewEntity::Arc {
                layer,
                center,
                radius,
                start_angle_deg,
                end_angle_deg,
                color,
            } => {
                let (cx, cy, cz) = xyz(center);
                g.push((0, "ARC".to_string()));
                g.push((5, alloc(next_handle)));
                g.push((8, layer.clone()));
                if let Some(c) = color {
                    g.push((62, c.to_string()));
                }
                g.push((10, fmt_f64(cx)));
                g.push((20, fmt_f64(cy)));
                g.push((30, fmt_f64(cz)));
                g.push((40, fmt_f64(*radius)));
                g.push((50, fmt_f64(*start_angle_deg)));
                g.push((51, fmt_f64(*end_angle_deg)));
            }
            DxfNewEntity::Lwpolyline {
                layer,
                points,
                closed,
                color,
            } => {
                g.push((0, "LWPOLYLINE".to_string()));
                g.push((5, alloc(next_handle)));
                g.push((8, layer.clone()));
                if let Some(c) = color {
                    g.push((62, c.to_string()));
                }
                g.push((90, points.len().to_string()));
                g.push((70, if *closed { "1" } else { "0" }.to_string()));
                for p in points {
                    g.push((10, fmt_f64(p[0])));
                    g.push((20, fmt_f64(p[1])));
                }
            }
            DxfNewEntity::Polyline3d {
                layer,
                points,
                color,
            } => {
                g.push((0, "POLYLINE".to_string()));
                g.push((5, alloc(next_handle)));
                g.push((8, layer.clone()));
                if let Some(c) = color {
                    g.push((62, c.to_string()));
                }
                g.push((66, "1".to_string()));
                g.push((10, "0.000000".to_string()));
                g.push((20, "0.000000".to_string()));
                g.push((30, "0.000000".to_string()));
                g.push((70, "8".to_string()));
                for p in points {
                    g.push((0, "VERTEX".to_string()));
                    g.push((5, alloc(next_handle)));
                    g.push((8, layer.clone()));
                    g.push((10, fmt_f64(p[0])));
                    g.push((20, fmt_f64(p[1])));
                    g.push((30, fmt_f64(p[2])));
                    g.push((70, "32".to_string()));
                }
                g.push((0, "SEQEND".to_string()));
                g.push((5, alloc(next_handle)));
                g.push((8, layer.clone()));
            }
            DxfNewEntity::Text {
                layer,
                insert,
                height,
                text,
                rotation_deg,
                color,
            } => {
                let (x, y, z) = xyz(insert);
                let normalized = normalize_dxf_text(text);
                g.push((0, "TEXT".to_string()));
                g.push((5, alloc(next_handle)));
                g.push((8, layer.clone()));
                if let Some(c) = color {
                    g.push((62, c.to_string()));
                }
                g.push((10, fmt_f64(x)));
                g.push((20, fmt_f64(y)));
                g.push((30, fmt_f64(z)));
                g.push((40, fmt_f64(*height)));
                g.push((1, normalized));
                if let Some(rot) = rotation_deg {
                    g.push((50, fmt_f64(*rot)));
                }
            }
            DxfNewEntity::Point { layer, at, color } => {
                let (x, y, z) = xyz(at);
                g.push((0, "POINT".to_string()));
                g.push((5, alloc(next_handle)));
                g.push((8, layer.clone()));
                if let Some(c) = color {
                    g.push((62, c.to_string()));
                }
                g.push((10, fmt_f64(x)));
                g.push((20, fmt_f64(y)));
                g.push((30, fmt_f64(z)));
            }
        }
        g
    }
}

/// Normalizes newlines and truncates text at 256 chars with an explicit marker.
pub fn normalize_dxf_text(raw: &str) -> String {
    let cleaned = raw.replace(['\r', '\n'], " ");
    let chars: Vec<char> = cleaned.chars().collect();
    if chars.len() <= MAX_TEXT_CHARS {
        cleaned
    } else {
        let marker = "...[truncated]";
        let keep = MAX_TEXT_CHARS.saturating_sub(marker.len());
        let prefix: String = chars[..keep].iter().collect();
        format!("{prefix}{marker}")
    }
}

/// Writes a minimal valid R2000 ASCII DXF: HEADER (`$ACADVER` + extra vars),
/// TABLES with LAYER records, optional BLOCKS, ENTITIES, EOF.
pub fn write_dxf_file(
    path: &Path,
    header_vars: &BTreeMap<String, Vec<(i32, String)>>,
    layers: &[String],
    blocks: &[(i32, String)],
    entities: &[Vec<(i32, String)>],
) -> std::io::Result<()> {
    let mut header_pairs: Vec<(i32, String)> = Vec::new();
    if !header_vars.contains_key("$ACADVER") {
        header_pairs.push((9, "$ACADVER".to_string()));
        header_pairs.push((1, "AC1015".to_string()));
    }
    for (var, pairs) in header_vars {
        header_pairs.push((9, var.clone()));
        header_pairs.extend(pairs.iter().cloned());
    }

    let mut ordered: BTreeSet<String> = BTreeSet::new();
    ordered.insert("0".to_string());
    for l in layers {
        if !l.trim().is_empty() {
            ordered.insert(l.trim().to_string());
        }
    }
    let mut tables: Vec<(i32, String)> = vec![
        (0, "TABLE".to_string()),
        (2, "LAYER".to_string()),
        (70, ordered.len().to_string()),
    ];
    for name in &ordered {
        tables.push((0, "LAYER".to_string()));
        tables.push((2, name.clone()));
        tables.push((70, "0".to_string()));
        tables.push((62, "7".to_string()));
        tables.push((6, "CONTINUOUS".to_string()));
    }
    tables.push((0, "ENDTAB".to_string()));

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let file = File::create(path)?;
    let mut w = BufWriter::new(file);
    write_section(&mut w, "HEADER", &header_pairs)?;
    write_section(&mut w, "TABLES", &tables)?;
    if !blocks.is_empty() {
        write_section(&mut w, "BLOCKS", blocks)?;
    }
    writeln!(w, "  0\nSECTION\n  2\nENTITIES")?;
    for ent in entities {
        for (c, v) in ent {
            writeln!(w, "{c:>3}\n{v}")?;
        }
    }
    writeln!(w, "  0\nENDSEC")?;
    writeln!(w, "  0\nEOF")?;
    w.flush()
}

fn write_section(
    w: &mut BufWriter<File>,
    name: &str,
    pairs: &[(i32, String)],
) -> std::io::Result<()> {
    writeln!(w, "  0\nSECTION\n  2\n{name}")?;
    for (c, v) in pairs {
        writeln!(w, "{c:>3}\n{v}")?;
    }
    writeln!(w, "  0\nENDSEC")
}

fn validate_pt23(pt: &[f64], field: &str) -> Result<(), String> {
    if pt.len() != 2 && pt.len() != 3 {
        return Err(format!("{field} must have 2 or 3 coordinates [x, y, z?]"));
    }
    if !pt.iter().all(|v| v.is_finite()) {
        return Err(format!("{field} coordinates must be finite numbers"));
    }
    Ok(())
}

fn validate_opt_color(color: Option<i32>) -> Result<(), String> {
    if let Some(c) = color
        && !(1..=255).contains(&c)
    {
        return Err(format!("ACI color must be in 1..=255, got {c}"));
    }
    Ok(())
}

fn xyz(pt: &[f64]) -> (f64, f64, f64) {
    (pt[0], pt[1], pt.get(2).copied().unwrap_or(0.0))
}

fn fmt_f64(v: f64) -> String {
    let clean = if v.abs() < 1e-12 { 0.0 } else { v };
    format!("{clean:.6}")
}
