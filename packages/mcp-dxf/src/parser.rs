//! Pure-Rust streaming ASCII DXF parser, lossless LibreDWG mesh-crease repair,
//! Z=0 elevation audit, AutoCAD text formatting stripper, and generic
//! coordinate-convention heuristics.

use cad_envelope::ToolError;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// Losslessly repair a known LibreDWG ASCII DXF omission where `AcDbSubDMesh`
/// omits group `95` (edge crease count). All original bytes are preserved
/// unchanged; returns `(repaired_bytes, repaired_mesh_count)`.
pub fn repair_mesh_counts(data: &[u8]) -> (Vec<u8>, usize) {
    if data.starts_with(b"AutoCAD Binary DXF") {
        return (data.to_vec(), 0);
    }
    let lines: Vec<&[u8]> = data.split_inclusive(|&b| b == b'\n').collect();
    if !lines.len().is_multiple_of(2) {
        return (data.to_vec(), 0);
    }

    let mut out = Vec::with_capacity(data.len() + 64);
    let mut entity: &[u8] = b"";
    let mut repaired = 0usize;
    let mut i = 0usize;

    while i + 1 < lines.len() {
        let code = trim_ascii_bytes(lines[i]);
        let value = trim_ascii_bytes(lines[i + 1]);
        if code == b"0" {
            entity = value;
        }
        if entity == b"MESH" && code == b"100" && value == b"AcDbSubDMesh" {
            let mut end = i + 2;
            while end + 1 < lines.len() {
                let c = trim_ascii_bytes(lines[end]);
                if c == b"0" || c == b"100" || c == b"1001" {
                    break;
                }
                end += 2;
            }
            let chunk = &lines[i..end];
            let mut has_95 = false;
            let mut count_140 = 0usize;
            let mut first_140_pos: Option<usize> = None;
            let mut k = 0usize;
            while k < chunk.len() {
                let c = trim_ascii_bytes(chunk[k]);
                if c == b"95" {
                    has_95 = true;
                } else if c == b"140" {
                    if first_140_pos.is_none() {
                        first_140_pos = Some(k);
                    }
                    count_140 += 1;
                }
                k += 2;
            }

            if !has_95 {
                let insert_at = first_140_pos.unwrap_or(chunk.len());
                let nl: &[u8] = if lines[i].ends_with(b"\r\n") {
                    b"\r\n"
                } else {
                    b"\n"
                };
                for line in &chunk[..insert_at] {
                    out.extend_from_slice(line);
                }
                out.extend_from_slice(b" 95");
                out.extend_from_slice(nl);
                out.extend_from_slice(count_140.to_string().as_bytes());
                out.extend_from_slice(nl);
                for line in &chunk[insert_at..] {
                    out.extend_from_slice(line);
                }
                repaired += 1;
            } else {
                for line in chunk {
                    out.extend_from_slice(line);
                }
            }
            i = end;
        } else {
            out.extend_from_slice(lines[i]);
            out.extend_from_slice(lines[i + 1]);
            i += 2;
        }
    }

    (out, repaired)
}

fn trim_ascii_bytes(b: &[u8]) -> &[u8] {
    let start = b
        .iter()
        .position(|c| !c.is_ascii_whitespace())
        .unwrap_or(b.len());
    let end = b
        .iter()
        .rposition(|c| !c.is_ascii_whitespace())
        .map(|p| p + 1)
        .unwrap_or(start);
    &b[start..end]
}

/// Strip common AutoCAD TEXT/MTEXT control sequences (`%%d`, `%%p`, `%%c`,
/// `%%u`, `\P`, `{\f...;...}`) into readable plain text.
pub fn strip_autocad_formatting(raw: &str) -> String {
    let mut s = raw
        .replace("\\P", "\n")
        .replace("\\p", "\n")
        .replace("\\~", " ")
        .replace("%%d", "°")
        .replace("%%D", "°")
        .replace("%%p", "±")
        .replace("%%P", "±")
        .replace("%%c", "Ø")
        .replace("%%C", "Ø")
        .replace("%%u", "")
        .replace("%%U", "")
        .replace("%%o", "")
        .replace("%%O", "");

    // Strip MTEXT inline formatting escapes like `\fArial|b0;`, `\H2.5;`, `\C1;`
    let mut out = String::with_capacity(s.len());
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let ch = chars[i];
        if ch == '{' || ch == '}' {
            i += 1;
            continue;
        }
        if ch == '\\' && i + 1 < chars.len() {
            let next = chars[i + 1];
            if matches!(
                next,
                'f' | 'F' | 'H' | 'C' | 'W' | 'T' | 'Q' | 'A' | 'p' | 'S' | 'c'
            ) {
                i += 2;
                while i < chars.len() && chars[i] != ';' {
                    i += 1;
                }
                if i < chars.len() && chars[i] == ';' {
                    i += 1;
                }
                continue;
            }
            if matches!(next, 'L' | 'l' | 'O' | 'o' | 'K' | 'k') {
                i += 2;
                continue;
            }
            if next == '\\' || next == '{' || next == '}' {
                out.push(next);
                i += 2;
                continue;
            }
        }
        out.push(ch);
        i += 1;
    }
    s = out.trim().to_string();
    s
}

#[derive(Debug, Clone, Copy, Serialize, Default)]
pub struct BBox3D {
    pub min: [f64; 3],
    pub max: [f64; 3],
}

#[derive(Debug, Clone, Default)]
struct BBoxAcc {
    any: bool,
    min: [f64; 3],
    max: [f64; 3],
}

impl BBoxAcc {
    fn push(&mut self, x: f64, y: f64, z: f64) {
        if !x.is_finite() || !y.is_finite() || !z.is_finite() {
            return;
        }
        if !self.any {
            self.any = true;
            self.min = [x, y, z];
            self.max = [x, y, z];
        } else {
            self.min[0] = self.min[0].min(x);
            self.min[1] = self.min[1].min(y);
            self.min[2] = self.min[2].min(z);
            self.max[0] = self.max[0].max(x);
            self.max[1] = self.max[1].max(y);
            self.max[2] = self.max[2].max(z);
        }
    }

    fn finish(&self) -> Option<BBox3D> {
        self.any.then_some(BBox3D {
            min: [
                round3(self.min[0]),
                round3(self.min[1]),
                round3(self.min[2]),
            ],
            max: [
                round3(self.max[0]),
                round3(self.max[1]),
                round3(self.max[2]),
            ],
        })
    }
}

fn round3(v: f64) -> f64 {
    (v * 1000.0).round() / 1000.0
}

#[derive(Debug, Clone, Serialize)]
pub struct CoordConvention {
    pub convention: String,
    pub likely_epsg: Option<u32>,
    pub note: String,
}

/// Generic heuristic classifying an easting/northing pair without tying to any
/// single country or client project.
pub fn detect_coord_convention(median_x: f64, median_y: f64) -> CoordConvention {
    let ax = median_x.abs();
    let ay = median_y.abs();

    if ax <= 180.0 && ay <= 90.0 && (ax > 0.0 || ay > 0.0) {
        return CoordConvention {
            convention: "geographic_or_local_small".to_string(),
            likely_epsg: None,
            note: "Small coordinate magnitude (<= 180): either WGS84 lon/lat (EPSG:4326) or local/sheet coordinates — confirm before georeferencing.".to_string(),
        };
    }
    if ax < 100_000.0 && ay < 100_000.0 {
        return CoordConvention {
            convention: "local_or_sheet".to_string(),
            likely_epsg: None,
            note: "Small planar coordinates (< 100,000): local engineering or sheet coordinate system.".to_string(),
        };
    }

    let x_int = ax as u64;
    let digits = x_int.to_string().len();

    if digits == 8 {
        let prefix = (x_int / 1_000_000) as u32;
        if (1..=60).contains(&prefix) {
            let epsg = (28..=38)
                .contains(&prefix)
                .then_some(25800 + prefix);
            return CoordConvention {
                convention: format!("utm_zone_prefixed_{prefix}"),
                likely_epsg: epsg,
                note: format!(
                    "8-digit easting with leading UTM zone prefix {prefix}: strip the 2-digit prefix before standard GIS ingestion."
                ),
            };
        }
    }

    if digits == 7 {
        let zone = (x_int / 1_000_000) as u32;
        if (2..=5).contains(&zone) {
            return CoordConvention {
                convention: format!("gauss_krueger_zone_{zone}"),
                likely_epsg: Some(31464 + zone),
                note: format!(
                    "7-digit easting starting with {zone}: looks like Gauss-Kruger zone {zone} (reproject to target projected CRS if needed)."
                ),
            };
        }
    }

    if digits == 6 && (100_000.0..=999_999.0).contains(&ax) && ay >= 1_000_000.0 {
        return CoordConvention {
            convention: "utm_unprefixed".to_string(),
            likely_epsg: None,
            note: "6-digit easting with 7-digit northing: unprefixed UTM (zone cannot be inferred from easting alone — confirm EPSG explicitly).".to_string(),
        };
    }

    CoordConvention {
        convention: "unknown".to_string(),
        likely_epsg: None,
        note: "Coordinates do not match a standard projected or local shorthand convention.".to_string(),
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct EntityRecord {
    pub handle: String,
    pub entity_type: String,
    pub layer: String,
    pub vertex_count: usize,
    pub has_z: bool,
    pub closed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub length_2d: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub block_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bbox: Option<BBox3D>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TextRecord {
    pub handle: String,
    pub entity_type: String,
    pub layer: String,
    pub text: String,
    pub position: [f64; 3],
    pub height: f64,
    pub rotation: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ZAudit {
    pub entities_without_z: usize,
    pub vertices_at_z_zero: usize,
    pub total_vertices: usize,
}

#[derive(Debug, Clone)]
pub struct DxfDocument {
    pub acad_version: Option<String>,
    pub acad_release: Option<String>,
    pub insunits_code: Option<i32>,
    pub insunits_name: String,
    pub declared_layers: BTreeSet<String>,
    pub entities: Vec<EntityRecord>,
    pub texts: Vec<TextRecord>,
    pub extents: Option<BBox3D>,
    pub header_extents: Option<BBox3D>,
    pub z_audit: ZAudit,
    pub coord_convention: Option<CoordConvention>,
    pub mesh_counts_repaired: usize,
    pub insert_count: usize,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LayerDetail {
    pub layer: String,
    pub entity_count: usize,
    pub entities_without_z: usize,
    pub entity_types: BTreeMap<String, usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bbox: Option<BBox3D>,
}

#[derive(Debug, Clone)]
pub struct DxfSliceStats {
    pub before_entities: usize,
    pub after_entities: usize,
    pub kept_layers: usize,
    pub extents: Option<BBox3D>,
}

impl DxfDocument {
    pub fn layer_details(&self) -> Vec<LayerDetail> {
        struct Acc {
            count: usize,
            no_z: usize,
            types: BTreeMap<String, usize>,
            bbox: BBoxAcc,
        }
        let mut map: BTreeMap<String, Acc> = BTreeMap::new();
        for l in &self.declared_layers {
            map.entry(l.clone()).or_insert_with(|| Acc {
                count: 0,
                no_z: 0,
                types: BTreeMap::new(),
                bbox: BBoxAcc::default(),
            });
        }
        for e in &self.entities {
            let acc = map.entry(e.layer.clone()).or_insert_with(|| Acc {
                count: 0,
                no_z: 0,
                types: BTreeMap::new(),
                bbox: BBoxAcc::default(),
            });
            acc.count += 1;
            if !e.has_z {
                acc.no_z += 1;
            }
            *acc.types.entry(e.entity_type.clone()).or_insert(0) += 1;
            if let Some(b) = e.bbox {
                acc.bbox.push(b.min[0], b.min[1], b.min[2]);
                acc.bbox.push(b.max[0], b.max[1], b.max[2]);
            }
        }
        let mut out: Vec<LayerDetail> = map
            .into_iter()
            .map(|(layer, acc)| LayerDetail {
                layer,
                entity_count: acc.count,
                entities_without_z: acc.no_z,
                entity_types: acc.types,
                bbox: acc.bbox.finish(),
            })
            .collect();
        out.sort_by(|a, b| {
            b.entity_count
                .cmp(&a.entity_count)
                .then_with(|| a.layer.cmp(&b.layer))
        });
        out
    }

    pub fn entity_type_counts(&self) -> Vec<(String, usize)> {
        let mut counts: BTreeMap<String, usize> = BTreeMap::new();
        for e in &self.entities {
            *counts.entry(e.entity_type.clone()).or_insert(0) += 1;
        }
        let mut out: Vec<(String, usize)> = counts.into_iter().collect();
        out.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        out
    }

    pub fn layer_counts(&self) -> Vec<(String, usize)> {
        let mut counts: BTreeMap<String, usize> = BTreeMap::new();
        for l in &self.declared_layers {
            counts.entry(l.clone()).or_insert(0);
        }
        for e in &self.entities {
            *counts.entry(e.layer.clone()).or_insert(0) += 1;
        }
        let mut out: Vec<(String, usize)> = counts.into_iter().collect();
        out.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        out
    }
}

/// Losslessly slice an ASCII DXF to entities intersecting `bbox` (plus optional
/// `layer` / `entity_type` filters), preserving original DXF entity group-code
/// records and writing a clean valid ASCII DXF (R2010 header + layer table +
/// recomputed `$EXTMIN`/`$EXTMAX`) to `output_path`.
pub fn export_dxf_slice(
    input_path: &Path,
    bbox: [f64; 4],
    layer_filter: Option<&str>,
    entity_type_filter: Option<&str>,
    output_path: &Path,
) -> Result<DxfSliceStats, ToolError> {
    let raw = std::fs::read(input_path).map_err(|e| {
        ToolError::new("cannot_open_dxf", format!("Failed to read DXF file: {e}"))
    })?;
    if raw.starts_with(b"AutoCAD Binary DXF") {
        return Err(ToolError::new(
            "unsupported_format",
            "Binary DXF is not supported yet. Convert the drawing to ASCII DXF and retry.",
        ));
    }

    let (repaired_bytes, _) = repair_mesh_counts(&raw);
    let content = String::from_utf8_lossy(&repaired_bytes);
    let pairs = collect_pairs(&content)?;

    let mut acad_ver = "AC1024".to_string();
    let mut insunits = 6i32;
    let layer_lc = layer_filter.map(str::to_ascii_lowercase);
    let type_uc = entity_type_filter.map(str::to_ascii_uppercase);

    let mut before_entities = 0usize;
    let mut after_entities = 0usize;
    let mut kept_layers: BTreeSet<String> = BTreeSet::new();
    let mut kept_extents = BBoxAcc::default();
    let mut kept_blocks: Vec<Vec<(i32, &str)>> = Vec::new();

    let mut i = 0usize;
    let mut section = String::new();
    while i < pairs.len() {
        let (code, val) = pairs[i];
        if code == 0 && val == "SECTION" && i + 1 < pairs.len() && pairs[i + 1].0 == 2 {
            section = pairs[i + 1].1.to_string();
            i += 2;
            continue;
        }
        if code == 0 && val == "ENDSEC" {
            section.clear();
            i += 1;
            continue;
        }
        if section == "HEADER" && code == 9 {
            let var = val;
            i += 1;
            while i < pairs.len() && pairs[i].0 != 9 && pairs[i].0 != 0 {
                if var == "$ACADVER" && pairs[i].0 == 1 {
                    acad_ver = pairs[i].1.to_string();
                } else if var == "$INSUNITS"
                    && pairs[i].0 == 70
                    && let Ok(u) = pairs[i].1.parse::<i32>()
                {
                    insunits = u;
                }
                i += 1;
            }
            continue;
        }
        if section == "ENTITIES" && code == 0 && val != "ENDSEC" && val != "EOF" {
            let entity_type = val;
            let block_start = i;
            let start = i + 1;
            let mut end = start;
            while end < pairs.len() && pairs[end].0 != 0 {
                end += 1;
            }
            let (rec, block_end) = if entity_type == "POLYLINE" {
                let (rec, _, next_i) = parse_polyline(&pairs[start..end], &pairs, end);
                (rec, next_i)
            } else if entity_type == "SEQEND" || entity_type == "VERTEX" {
                i = end;
                continue;
            } else {
                let (rec, _, _) = parse_single_entity(entity_type, &pairs[start..end]);
                (rec, end)
            };

            before_entities += 1;
            let layer_ok = layer_lc
                .as_deref()
                .is_none_or(|w| rec.layer.to_ascii_lowercase() == w);
            let type_ok = type_uc.as_deref().is_none_or(|w| rec.entity_type == w);
            let bbox_ok = rec.bbox.is_some_and(|eb| {
                eb.max[0] >= bbox[0]
                    && eb.min[0] <= bbox[2]
                    && eb.max[1] >= bbox[1]
                    && eb.min[1] <= bbox[3]
            });

            if layer_ok && type_ok && bbox_ok {
                after_entities += 1;
                kept_layers.insert(rec.layer.clone());
                if let Some(eb) = rec.bbox {
                    kept_extents.push(eb.min[0], eb.min[1], eb.min[2]);
                    kept_extents.push(eb.max[0], eb.max[1], eb.max[2]);
                }
                kept_blocks.push(pairs[block_start..block_end].to_vec());
            }
            i = block_end;
            continue;
        }
        i += 1;
    }

    let extents = kept_extents.finish();
    let mut out = String::new();
    out.push_str("0\nSECTION\n2\nHEADER\n");
    out.push_str(&format!("9\n$ACADVER\n1\n{acad_ver}\n"));
    out.push_str(&format!("9\n$INSUNITS\n70\n{insunits}\n"));
    if let Some(ext) = extents {
        out.push_str(&format!(
            "9\n$EXTMIN\n10\n{}\n20\n{}\n30\n{}\n",
            ext.min[0], ext.min[1], ext.min[2]
        ));
        out.push_str(&format!(
            "9\n$EXTMAX\n10\n{}\n20\n{}\n30\n{}\n",
            ext.max[0], ext.max[1], ext.max[2]
        ));
    }
    out.push_str("0\nENDSEC\n");

    out.push_str("0\nSECTION\n2\nTABLES\n0\nTABLE\n2\nLAYER\n");
    for layer in &kept_layers {
        out.push_str(&format!("0\nLAYER\n2\n{layer}\n70\n0\n62\n7\n6\nCONTINUOUS\n"));
    }
    out.push_str("0\nENDTAB\n0\nENDSEC\n");

    out.push_str("0\nSECTION\n2\nENTITIES\n");
    for block in &kept_blocks {
        for (c, v) in block {
            out.push_str(&format!("{c}\n{v}\n"));
        }
    }
    out.push_str("0\nENDSEC\n0\nEOF\n");

    std::fs::write(output_path, out).map_err(|e| {
        ToolError::new("internal_error", format!("Failed writing sliced DXF: {e}"))
    })?;

    Ok(DxfSliceStats {
        before_entities,
        after_entities,
        kept_layers: kept_layers.len(),
        extents,
    })
}

pub fn parse_dxf_file(path: &Path) -> Result<DxfDocument, ToolError> {
    let raw = std::fs::read(path).map_err(|e| {
        ToolError::new("cannot_open_dxf", format!("Failed to read DXF file: {e}"))
    })?;
    parse_dxf_bytes(&raw)
}

pub fn parse_dxf_bytes(raw: &[u8]) -> Result<DxfDocument, ToolError> {
    if raw.starts_with(b"AutoCAD Binary DXF") {
        return Err(ToolError::new(
            "unsupported_format",
            "Binary DXF is not supported yet. Convert the drawing to ASCII DXF and retry.",
        ));
    }

    let (repaired_bytes, mesh_counts_repaired) = repair_mesh_counts(raw);
    let content = String::from_utf8_lossy(&repaired_bytes);
    let pairs = collect_pairs(&content)?;

    let mut acad_version: Option<String> = None;
    let mut insunits_code: Option<i32> = None;
    let mut header_extmin: Option<[f64; 3]> = None;
    let mut header_extmax: Option<[f64; 3]> = None;
    let mut declared_layers: BTreeSet<String> = BTreeSet::new();
    let mut entities: Vec<EntityRecord> = Vec::new();
    let mut texts: Vec<TextRecord> = Vec::new();
    let mut global_bbox = BBoxAcc::default();
    let mut sample_xs: Vec<f64> = Vec::new();
    let mut sample_ys: Vec<f64> = Vec::new();
    let mut z_audit = ZAudit {
        entities_without_z: 0,
        vertices_at_z_zero: 0,
        total_vertices: 0,
    };
    let mut insert_count = 0usize;

    let mut i = 0usize;
    let mut section = String::new();

    while i < pairs.len() {
        let (code, val) = pairs[i];
        if code == 0 && val == "SECTION" {
            if i + 1 < pairs.len() && pairs[i + 1].0 == 2 {
                section = pairs[i + 1].1.to_string();
                i += 2;
                continue;
            }
        } else if code == 0 && val == "ENDSEC" {
            section.clear();
            i += 1;
            continue;
        }

        match section.as_str() {
            "HEADER" => {
                if code == 9 {
                    let var = val;
                    i += 1;
                    let mut x = 0.0;
                    let mut y = 0.0;
                    let mut z = 0.0;
                    let mut got_pt = false;
                    while i < pairs.len() && pairs[i].0 != 9 && pairs[i].0 != 0 {
                        let (vc, vv) = pairs[i];
                        match (var, vc) {
                            ("$ACADVER", 1) => acad_version = Some(vv.to_string()),
                            ("$INSUNITS", 70) => insunits_code = vv.parse::<i32>().ok(),
                            ("$EXTMIN" | "$EXTMAX", 10) => {
                                if let Ok(v) = vv.parse::<f64>() {
                                    x = v;
                                    got_pt = true;
                                }
                            }
                            ("$EXTMIN" | "$EXTMAX", 20) => {
                                if let Ok(v) = vv.parse::<f64>() {
                                    y = v;
                                }
                            }
                            ("$EXTMIN" | "$EXTMAX", 30) => {
                                if let Ok(v) = vv.parse::<f64>() {
                                    z = v;
                                }
                            }
                            _ => {}
                        }
                        i += 1;
                    }
                    if got_pt {
                        if var == "$EXTMIN" {
                            header_extmin = Some([round3(x), round3(y), round3(z)]);
                        } else if var == "$EXTMAX" {
                            header_extmax = Some([round3(x), round3(y), round3(z)]);
                        }
                    }
                    continue;
                }
            }
            "TABLES" => {
                if code == 0 && val == "LAYER" {
                    i += 1;
                    while i < pairs.len() && pairs[i].0 != 0 {
                        if pairs[i].0 == 2 && !pairs[i].1.is_empty() {
                            declared_layers.insert(pairs[i].1.to_string());
                        }
                        i += 1;
                    }
                    continue;
                }
            }
            "ENTITIES" if code == 0 && val != "ENDSEC" && val != "EOF" => {
                let entity_type = val;
                let start = i + 1;
                let mut end = start;
                while end < pairs.len() && pairs[end].0 != 0 {
                    end += 1;
                }
                let slice = &pairs[start..end];

                if entity_type == "POLYLINE" {
                    let (rec, pts, next_i) = parse_polyline(slice, &pairs, end);
                    record_points(
                        &pts,
                        &mut global_bbox,
                        &mut sample_xs,
                        &mut sample_ys,
                        &mut z_audit,
                    );
                    if !rec.has_z {
                        z_audit.entities_without_z += 1;
                    }
                    entities.push(rec);
                    i = next_i;
                    continue;
                }

                if entity_type == "SEQEND" || entity_type == "VERTEX" {
                    i = end;
                    continue;
                }

                let (rec, pts, maybe_text) = parse_single_entity(entity_type, slice);
                if entity_type == "INSERT" {
                    insert_count += 1;
                }
                record_points(
                    &pts,
                    &mut global_bbox,
                    &mut sample_xs,
                    &mut sample_ys,
                    &mut z_audit,
                );
                if !rec.has_z {
                    z_audit.entities_without_z += 1;
                }
                entities.push(rec);
                if let Some(txt) = maybe_text {
                    texts.push(txt);
                }
                i = end;
                continue;
            }
            _ => {}
        }
        i += 1;
    }

    if acad_version.is_none() && declared_layers.is_empty() && entities.is_empty() {
        return Err(ToolError::new(
            "cannot_open_dxf",
            "Input does not appear to be a valid ASCII DXF file (no HEADER, TABLES, or ENTITIES found).",
        ));
    }

    // Deterministic ordering across runs
    entities.sort_by(|a, b| {
        a.layer
            .cmp(&b.layer)
            .then_with(|| a.entity_type.cmp(&b.entity_type))
            .then_with(|| a.handle.cmp(&b.handle))
    });
    texts.sort_by(|a, b| {
        a.layer
            .cmp(&b.layer)
            .then_with(|| a.handle.cmp(&b.handle))
            .then_with(|| a.text.cmp(&b.text))
    });

    let header_extents = match (header_extmin, header_extmax) {
        (Some(min), Some(max))
            if min[0] <= max[0] && min[1] <= max[1] && min[0].abs() < 1e18 && max[0].abs() < 1e18 =>
        {
            Some(BBox3D { min, max })
        }
        _ => None,
    };

    let coord_convention = if !sample_xs.is_empty() && !sample_ys.is_empty() {
        sample_xs.sort_by(|a, b| a.total_cmp(b));
        sample_ys.sort_by(|a, b| a.total_cmp(b));
        let mx = sample_xs[sample_xs.len() / 2];
        let my = sample_ys[sample_ys.len() / 2];
        Some(detect_coord_convention(mx, my))
    } else {
        None
    };

    let mut warnings = Vec::new();
    if mesh_counts_repaired > 0 {
        warnings.push(format!(
            "Repaired {mesh_counts_repaired} LibreDWG AcDbSubDMesh entity(ies) missing crease count (group 95)."
        ));
    }
    if insert_count > 0 {
        warnings.push(format!(
            "Drawing contains {insert_count} block INSERT reference(s); nested block geometry is not exploded."
        ));
    }
    if z_audit.entities_without_z > 0 {
        warnings.push(format!(
            "{} entity(ies) have no 3D elevation (2D / Z=0).",
            z_audit.entities_without_z
        ));
    }

    let acad_release = acad_version.as_deref().map(acad_version_label);
    let insunits_name = insunits_label(insunits_code).to_string();

    Ok(DxfDocument {
        acad_version,
        acad_release,
        insunits_code,
        insunits_name,
        declared_layers,
        entities,
        texts,
        extents: global_bbox.finish(),
        header_extents,
        z_audit,
        coord_convention,
        mesh_counts_repaired,
        insert_count,
        warnings,
    })
}

fn collect_pairs(content: &str) -> Result<Vec<(i32, &str)>, ToolError> {
    let lines: Vec<&str> = content.lines().collect();
    if lines.len() < 2 {
        return Err(ToolError::new(
            "cannot_open_dxf",
            "DXF file is empty or truncated.",
        ));
    }
    let mut pairs = Vec::with_capacity(lines.len() / 2);
    let mut i = 0;
    while i + 1 < lines.len() {
        let code_str = lines[i].trim();
        let val = lines[i + 1].trim();
        if let Ok(code) = code_str.parse::<i32>() {
            pairs.push((code, val));
        }
        i += 2;
    }
    Ok(pairs)
}

fn record_points(
    pts: &[[f64; 3]],
    global_bbox: &mut BBoxAcc,
    sample_xs: &mut Vec<f64>,
    sample_ys: &mut Vec<f64>,
    z_audit: &mut ZAudit,
) {
    for &[x, y, z] in pts {
        if !x.is_finite() || !y.is_finite() || !z.is_finite() {
            continue;
        }
        global_bbox.push(x, y, z);
        if sample_xs.len() < 10_000 {
            sample_xs.push(x);
            sample_ys.push(y);
        }
        z_audit.total_vertices += 1;
        if z.abs() < 1e-9 {
            z_audit.vertices_at_z_zero += 1;
        }
    }
}

fn polyline_length_2d(pts: &[[f64; 3]], closed: bool) -> Option<f64> {
    if pts.len() < 2 {
        return None;
    }
    let mut len = 0.0;
    for w in pts.windows(2) {
        len += (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1]);
    }
    if closed && pts.len() >= 3 {
        let last = pts[pts.len() - 1];
        let first = pts[0];
        len += (first[0] - last[0]).hypot(first[1] - last[1]);
    }
    Some(round3(len))
}

fn parse_polyline(
    header: &[(i32, &str)],
    all_pairs: &[(i32, &str)],
    mut cursor: usize,
) -> (EntityRecord, Vec<[f64; 3]>, usize) {
    let mut handle = String::new();
    let mut layer = "0".to_string();
    let mut flags = 0i32;
    let mut default_z = 0.0f64;
    for &(c, v) in header {
        match c {
            5 => handle = v.to_string(),
            8 => layer = v.to_string(),
            70 => flags = v.parse().unwrap_or(0),
            30 => default_z = v.parse().unwrap_or(0.0),
            _ => {}
        }
    }
    let closed = (flags & 1) != 0;
    let mut pts: Vec<[f64; 3]> = Vec::new();
    let mut has_z = default_z.abs() > 1e-9;

    while cursor < all_pairs.len() {
        if all_pairs[cursor].0 == 0 {
            let kind = all_pairs[cursor].1;
            if kind == "SEQEND" {
                cursor += 1;
                while cursor < all_pairs.len() && all_pairs[cursor].0 != 0 {
                    cursor += 1;
                }
                break;
            }
            if kind == "VERTEX" {
                cursor += 1;
                let mut vx = None;
                let mut vy = None;
                let mut vz = default_z;
                let mut vflags = 0i32;
                while cursor < all_pairs.len() && all_pairs[cursor].0 != 0 {
                    match all_pairs[cursor].0 {
                        10 => vx = all_pairs[cursor].1.parse().ok(),
                        20 => vy = all_pairs[cursor].1.parse().ok(),
                        30 => {
                            if let Ok(z) = all_pairs[cursor].1.parse::<f64>() {
                                vz = z;
                                if z.abs() > 1e-9 {
                                    has_z = true;
                                }
                            }
                        }
                        70 => vflags = all_pairs[cursor].1.parse().unwrap_or(0),
                        _ => {}
                    }
                    cursor += 1;
                }
                // Skip polyface face-index records (flag 128 without 64)
                let is_face_record = (vflags & 128) != 0 && (vflags & 64) == 0;
                if !is_face_record
                    && let (Some(x), Some(y)) = (vx, vy)
                {
                    pts.push([x, y, vz]);
                }
                continue;
            }
            break;
        }
        cursor += 1;
    }

    let mut bbox = BBoxAcc::default();
    for &[x, y, z] in &pts {
        bbox.push(x, y, z);
    }
    let length_2d = polyline_length_2d(&pts, closed);
    let rec = EntityRecord {
        handle,
        entity_type: "POLYLINE".to_string(),
        layer,
        vertex_count: pts.len(),
        has_z,
        closed,
        length_2d,
        block_name: None,
        bbox: bbox.finish(),
    };
    (rec, pts, cursor)
}

fn parse_single_entity(
    entity_type: &str,
    pairs: &[(i32, &str)],
) -> (EntityRecord, Vec<[f64; 3]>, Option<TextRecord>) {
    let mut handle = String::new();
    let mut layer = "0".to_string();
    let mut closed = false;
    let mut block_name: Option<String> = None;
    let mut elevation = 0.0f64;
    let mut has_elev = false;
    let mut pts: Vec<[f64; 3]> = Vec::new();
    let mut has_z = false;

    // Text fields
    let mut text_chunks: Vec<String> = Vec::new();
    let mut text_tag: Option<String> = None;
    let mut text_height = 0.0f64;
    let mut text_rot = 0.0f64;

    // Temporary coordinate slots for primary/secondary points
    let mut cur_x: Option<f64> = None;
    let mut cur_y: Option<f64> = None;
    let mut cur_z: Option<f64> = None;

    let mut p11 = [None::<f64>; 3];
    let mut p12 = [None::<f64>; 3];
    let mut p13 = [None::<f64>; 3];
    let mut radius: Option<f64> = None;

    let flush_cur = |pts: &mut Vec<[f64; 3]>,
                     cx: &mut Option<f64>,
                     cy: &mut Option<f64>,
                     cz: &mut Option<f64>,
                     elev: f64,
                     has_z: &mut bool| {
        if let (Some(x), Some(y)) = (*cx, *cy) {
            let z = cz.unwrap_or(elev);
            if z.abs() > 1e-9 {
                *has_z = true;
            }
            pts.push([x, y, z]);
        }
        *cx = None;
        *cy = None;
        *cz = None;
    };

    for &(code, val) in pairs {
        match code {
            5 => handle = val.to_string(),
            8 => layer = val.to_string(),
            2 => {
                if matches!(entity_type, "INSERT" | "ATTRIB" | "ATTDEF") {
                    if entity_type == "INSERT" {
                        block_name = Some(val.to_string());
                    } else {
                        text_tag = Some(val.to_string());
                    }
                }
            }
            38 => {
                if let Ok(e) = val.parse::<f64>() {
                    elevation = e;
                    has_elev = e.abs() > 1e-9;
                }
            }
            70 => {
                if entity_type == "LWPOLYLINE" {
                    let f: i32 = val.parse().unwrap_or(0);
                    closed = (f & 1) != 0;
                }
            }
            10 => {
                flush_cur(
                    &mut pts,
                    &mut cur_x,
                    &mut cur_y,
                    &mut cur_z,
                    elevation,
                    &mut has_z,
                );
                cur_x = val.parse().ok();
            }
            20 => cur_y = val.parse().ok(),
            30 => {
                cur_z = val.parse().ok();
                if let Some(z) = cur_z
                    && z.abs() > 1e-9
                {
                    has_z = true;
                }
            }
            11 => p11[0] = val.parse().ok(),
            21 => p11[1] = val.parse().ok(),
            31 => p11[2] = val.parse().ok(),
            12 => p12[0] = val.parse().ok(),
            22 => p12[1] = val.parse().ok(),
            32 => p12[2] = val.parse().ok(),
            13 => p13[0] = val.parse().ok(),
            23 => p13[1] = val.parse().ok(),
            33 => p13[2] = val.parse().ok(),
            40 => {
                if matches!(entity_type, "CIRCLE" | "ARC") {
                    radius = val.parse().ok();
                } else if matches!(entity_type, "TEXT" | "MTEXT" | "ATTRIB") {
                    text_height = val.parse().unwrap_or(0.0);
                }
            }
            50 => {
                if matches!(entity_type, "TEXT" | "MTEXT" | "ATTRIB") {
                    text_rot = val.parse().unwrap_or(0.0);
                }
            }
            1 | 3 => {
                if matches!(entity_type, "TEXT" | "MTEXT" | "ATTRIB") {
                    text_chunks.push(val.to_string());
                }
            }
            _ => {}
        }
    }

    flush_cur(
        &mut pts,
        &mut cur_x,
        &mut cur_y,
        &mut cur_z,
        elevation,
        &mut has_z,
    );

    // Apply elevation to LWPOLYLINE vertices if elevation appeared after vertices
    if entity_type == "LWPOLYLINE" && has_elev {
        has_z = true;
        for p in &mut pts {
            if p[2].abs() < 1e-9 {
                p[2] = elevation;
            }
        }
    }

    // Secondary points for LINE / 3DFACE / SOLID
    for slot in [p11, p12, p13] {
        if let (Some(x), Some(y)) = (slot[0], slot[1]) {
            let z = slot[2].unwrap_or(elevation);
            if z.abs() > 1e-9 {
                has_z = true;
            }
            if !matches!(entity_type, "TEXT" | "MTEXT" | "ATTRIB") {
                pts.push([x, y, z]);
            }
        }
    }

    let mut bbox = BBoxAcc::default();
    if matches!(entity_type, "CIRCLE" | "ARC")
        && let (Some(&[cx, cy, cz]), Some(r)) = (pts.first(), radius)
    {
        bbox.push(cx - r, cy - r, cz);
        bbox.push(cx + r, cy + r, cz);
    } else {
        for &[x, y, z] in &pts {
            bbox.push(x, y, z);
        }
    }

    let length_2d = match entity_type {
        "LINE" | "LWPOLYLINE" | "SPLINE" => polyline_length_2d(&pts, closed),
        "CIRCLE" => radius.map(|r| round3(2.0 * std::f64::consts::PI * r)),
        _ => None,
    };

    let text_rec = if matches!(entity_type, "TEXT" | "MTEXT" | "ATTRIB") && !text_chunks.is_empty()
    {
        let raw_text = text_chunks.concat();
        let cleaned = strip_autocad_formatting(&raw_text);
        let pos = pts.first().copied().unwrap_or([0.0, 0.0, elevation]);
        Some(TextRecord {
            handle: handle.clone(),
            entity_type: entity_type.to_string(),
            layer: layer.clone(),
            text: cleaned,
            position: [round3(pos[0]), round3(pos[1]), round3(pos[2])],
            height: round3(text_height),
            rotation: round3(text_rot),
            tag: text_tag,
        })
    } else {
        None
    };

    let rec = EntityRecord {
        handle,
        entity_type: entity_type.to_string(),
        layer,
        vertex_count: pts.len(),
        has_z,
        closed,
        length_2d,
        block_name,
        bbox: bbox.finish(),
    };
    (rec, pts, text_rec)
}

fn acad_version_label(code: &str) -> String {
    match code {
        "AC1009" => "R12".to_string(),
        "AC1012" => "R13".to_string(),
        "AC1014" => "R14".to_string(),
        "AC1015" => "R2000".to_string(),
        "AC1018" => "R2004".to_string(),
        "AC1021" => "R2007".to_string(),
        "AC1024" => "R2010".to_string(),
        "AC1027" => "R2013".to_string(),
        "AC1032" => "R2018".to_string(),
        other => other.to_string(),
    }
}

fn insunits_label(code: Option<i32>) -> &'static str {
    match code {
        Some(0) | None => "unitless",
        Some(1) => "inches",
        Some(2) => "feet",
        Some(3) => "miles",
        Some(4) => "millimeters",
        Some(5) => "centimeters",
        Some(6) => "meters",
        Some(7) => "kilometers",
        Some(14) => "decimeters",
        _ => "other",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_DXF: &str = "\
0
SECTION
2
HEADER
9
$ACADVER
1
AC1024
9
$INSUNITS
70
6
0
ENDSEC
0
SECTION
2
TABLES
0
TABLE
2
LAYER
0
LAYER
2
TOPO
0
LAYER
2
LABELS
0
ENDTAB
0
ENDSEC
0
SECTION
2
ENTITIES
0
LINE
5
A1
8
TOPO
10
500100.0
20
5650000.0
30
120.5
11
500110.0
21
5650000.0
31
121.0
0
LWPOLYLINE
5
A2
8
TOPO
70
1
38
0.0
10
500100.0
20
5650000.0
10
500105.0
20
5650000.0
10
500105.0
20
5650005.0
0
MTEXT
5
A3
8
LABELS
10
500102.0
20
5650002.0
30
120.8
40
1.5
1
{\\fArial|b0;Elevation 120.80%%p0.02\\PCheck %%d slope}
0
ENDSEC
0
EOF
";

    #[test]
    fn parses_summary_extents_z_audit_and_mtext() {
        let doc = parse_dxf_bytes(SAMPLE_DXF.as_bytes()).unwrap();
        assert_eq!(doc.acad_version.as_deref(), Some("AC1024"));
        assert_eq!(doc.acad_release.as_deref(), Some("R2010"));
        assert_eq!(doc.insunits_name, "meters");
        assert_eq!(doc.entities.len(), 3);
        assert_eq!(doc.texts.len(), 1);
        assert_eq!(doc.texts[0].text, "Elevation 120.80±0.02\nCheck ° slope");
        assert_eq!(doc.z_audit.entities_without_z, 1); // LWPOLYLINE at Z=0
        assert_eq!(
            doc.coord_convention.as_ref().unwrap().convention,
            "utm_unprefixed"
        );
    }

    #[test]
    fn repairs_missing_libredwg_mesh_group_95() {
        let dxf = b"0\nMESH\n100\nAcDbSubDMesh\n92\n3\n140\n0.5\n140\n1.0\n0\nENDSEC\n";
        let (repaired, count) = repair_mesh_counts(dxf);
        assert_eq!(count, 1);
        let text = String::from_utf8(repaired).unwrap();
        assert!(text.contains(" 95\n2\n140\n0.5"));
    }

    #[test]
    fn rejects_binary_dxf() {
        let err = parse_dxf_bytes(b"AutoCAD Binary DXF\r\n\x1a\0").unwrap_err();
        assert_eq!(err.code, "unsupported_format");
    }
}
