//! General-purpose 3D CAD geometry operations on ASCII DXF files:
//! - `dxf_to_mesh`: weld 3DFACE surfaces into `.2dm` or `.obj` meshes with vertical-step guard
//! - `dxf_centerline`: 3D centreline between two DXF polylines (arc-length or perpendicular pairing)
//! - `dxf_labels_to_3d`: assign elevation labels (`TEXT`/`MTEXT`/`POINT`) onto 2D DXF lines -> 3D DXF

use crate::parser::{repair_mesh_counts, strip_autocad_formatting};
use cad_envelope::ToolError;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;

#[derive(Debug, Clone)]
pub struct DxfLine {
    pub handle: String,
    pub layer: String,
    pub pts: Vec<[f64; 3]>,
    pub closed: bool,
}

#[derive(Debug, Clone)]
pub struct DxfFace {
    pub layer: String,
    pub verts: Vec<[f64; 3]>,
}

#[derive(Debug, Clone)]
pub struct DxfPoint {
    pub layer: String,
    pub xyz: [f64; 3],
}

#[derive(Debug, Clone)]
pub struct DxfText {
    pub handle: String,
    pub layer: String,
    pub text: String,
    pub xyz: [f64; 3],
}

#[derive(Debug, Default)]
pub struct DxfGeometry {
    pub lines: Vec<DxfLine>,
    pub faces: Vec<DxfFace>,
    pub points: Vec<DxfPoint>,
    pub texts: Vec<DxfText>,
}

fn round3(v: f64) -> f64 {
    (v * 1000.0).round() / 1000.0
}

fn round4(v: f64) -> f64 {
    (v * 10_000.0).round() / 10_000.0
}

/// Extract concrete 3D geometry (lines, 3DFACEs, points, texts) from an ASCII DXF file.
pub fn extract_dxf_geometry(path: &Path) -> Result<DxfGeometry, ToolError> {
    let raw = std::fs::read(path)
        .map_err(|e| ToolError::new("cannot_open_dxf", format!("Failed to read DXF file: {e}")))?;
    if raw.starts_with(b"AutoCAD Binary DXF") {
        return Err(ToolError::new(
            "unsupported_format",
            "Binary DXF is not supported yet. Convert the drawing to ASCII DXF and retry.",
        ));
    }
    let (repaired, _) = repair_mesh_counts(&raw);
    let content = String::from_utf8_lossy(&repaired);
    let lines_vec: Vec<&str> = content.lines().collect();
    let mut pairs: Vec<(i32, &str)> = Vec::with_capacity(lines_vec.len() / 2);
    let mut k = 0;
    while k + 1 < lines_vec.len() {
        if let Ok(code) = lines_vec[k].trim().parse::<i32>() {
            pairs.push((code, lines_vec[k + 1].trim()));
        }
        k += 2;
    }

    let mut geom = DxfGeometry::default();
    let mut section = String::new();
    let mut i = 0usize;

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
        if section != "ENTITIES" || code != 0 || val == "EOF" {
            i += 1;
            continue;
        }

        let etype = val;
        let start = i + 1;
        let mut end = start;
        while end < pairs.len() && pairs[end].0 != 0 {
            end += 1;
        }
        let slice = &pairs[start..end];

        if etype == "POLYLINE" {
            let (maybe_line, next_i) = extract_polyline(slice, &pairs, end);
            if let Some(ln) = maybe_line {
                geom.lines.push(ln);
            }
            i = next_i;
            continue;
        }
        if etype == "VERTEX" || etype == "SEQEND" {
            i = end;
            continue;
        }

        match etype {
            "LINE" => {
                let mut handle = String::new();
                let mut layer = "0".to_string();
                let (mut x0, mut y0, mut z0) = (None, None, 0.0);
                let (mut x1, mut y1, mut z1) = (None, None, 0.0);
                for &(c, v) in slice {
                    match c {
                        5 => handle = v.to_string(),
                        8 => layer = v.to_string(),
                        10 => x0 = v.parse().ok(),
                        20 => y0 = v.parse().ok(),
                        30 => z0 = v.parse().unwrap_or(0.0),
                        11 => x1 = v.parse().ok(),
                        21 => y1 = v.parse().ok(),
                        31 => z1 = v.parse().unwrap_or(0.0),
                        _ => {}
                    }
                }
                if let (Some(ax), Some(ay), Some(bx), Some(by)) = (x0, y0, x1, y1) {
                    geom.lines.push(DxfLine {
                        handle,
                        layer,
                        pts: vec![[ax, ay, z0], [bx, by, z1]],
                        closed: false,
                    });
                }
            }
            "LWPOLYLINE" => {
                let mut handle = String::new();
                let mut layer = "0".to_string();
                let mut elev = 0.0f64;
                let mut closed = false;
                let mut pts: Vec<[f64; 3]> = Vec::new();
                let mut cx: Option<f64> = None;
                let mut cy: Option<f64> = None;
                for &(c, v) in slice {
                    match c {
                        5 => handle = v.to_string(),
                        8 => layer = v.to_string(),
                        38 => elev = v.parse().unwrap_or(0.0),
                        70 => closed = (v.parse::<i32>().unwrap_or(0) & 1) != 0,
                        10 => {
                            if let (Some(x), Some(y)) = (cx, cy) {
                                pts.push([x, y, elev]);
                            }
                            cx = v.parse().ok();
                            cy = None;
                        }
                        20 => cy = v.parse().ok(),
                        _ => {}
                    }
                }
                if let (Some(x), Some(y)) = (cx, cy) {
                    pts.push([x, y, elev]);
                }
                for p in &mut pts {
                    p[2] = elev;
                }
                if pts.len() >= 2 {
                    geom.lines.push(DxfLine {
                        handle,
                        layer,
                        pts,
                        closed,
                    });
                }
            }
            "3DFACE" => {
                let mut layer = "0".to_string();
                let mut corners = [[None::<f64>; 3]; 4];
                for &(c, v) in slice {
                    match c {
                        8 => layer = v.to_string(),
                        10..=13 => corners[(c - 10) as usize][0] = v.parse().ok(),
                        20..=23 => corners[(c - 20) as usize][1] = v.parse().ok(),
                        30..=33 => corners[(c - 30) as usize][2] = v.parse().ok(),
                        _ => {}
                    }
                }
                let mut verts: Vec<[f64; 3]> = Vec::new();
                for corner in corners {
                    if let (Some(x), Some(y)) = (corner[0], corner[1]) {
                        let pt = [x, y, corner[2].unwrap_or(0.0)];
                        let dup = verts.last().is_some_and(|prev| {
                            (prev[0] - pt[0]).abs() < 1e-12
                                && (prev[1] - pt[1]).abs() < 1e-12
                                && (prev[2] - pt[2]).abs() < 1e-12
                        });
                        if !dup {
                            verts.push(pt);
                        }
                    }
                }
                if verts.len() >= 3 {
                    geom.faces.push(DxfFace { layer, verts });
                }
            }
            "POINT" => {
                let mut layer = "0".to_string();
                let (mut x, mut y, mut z) = (None, None, 0.0);
                for &(c, v) in slice {
                    match c {
                        8 => layer = v.to_string(),
                        10 => x = v.parse().ok(),
                        20 => y = v.parse().ok(),
                        30 => z = v.parse().unwrap_or(0.0),
                        _ => {}
                    }
                }
                if let (Some(px), Some(py)) = (x, y) {
                    geom.points.push(DxfPoint {
                        layer,
                        xyz: [px, py, z],
                    });
                }
            }
            "TEXT" | "MTEXT" | "ATTRIB" => {
                let mut handle = String::new();
                let mut layer = "0".to_string();
                let (mut x, mut y, mut z) = (None, None, 0.0);
                let mut chunks: Vec<String> = Vec::new();
                for &(c, v) in slice {
                    match c {
                        5 => handle = v.to_string(),
                        8 => layer = v.to_string(),
                        10 => x = v.parse().ok(),
                        20 => y = v.parse().ok(),
                        30 => z = v.parse().unwrap_or(0.0),
                        1 | 3 => chunks.push(v.to_string()),
                        _ => {}
                    }
                }
                if let (Some(tx), Some(ty)) = (x, y)
                    && !chunks.is_empty()
                {
                    geom.texts.push(DxfText {
                        handle,
                        layer,
                        text: strip_autocad_formatting(&chunks.concat()),
                        xyz: [tx, ty, z],
                    });
                }
            }
            _ => {}
        }

        i = end;
    }

    Ok(geom)
}

fn extract_polyline(
    header: &[(i32, &str)],
    all_pairs: &[(i32, &str)],
    mut cursor: usize,
) -> (Option<DxfLine>, usize) {
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
    let mut pts = Vec::new();

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
                let (mut vx, mut vy, mut vz, mut vf) = (None, None, default_z, 0i32);
                while cursor < all_pairs.len() && all_pairs[cursor].0 != 0 {
                    match all_pairs[cursor].0 {
                        10 => vx = all_pairs[cursor].1.parse().ok(),
                        20 => vy = all_pairs[cursor].1.parse().ok(),
                        30 => vz = all_pairs[cursor].1.parse().unwrap_or(default_z),
                        70 => vf = all_pairs[cursor].1.parse().unwrap_or(0),
                        _ => {}
                    }
                    cursor += 1;
                }
                let is_face = (vf & 128) != 0 && (vf & 64) == 0;
                if !is_face
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

    let line = (pts.len() >= 2).then_some(DxfLine {
        handle,
        layer,
        pts,
        closed,
    });
    (line, cursor)
}

/// Write 3D polylines as a clean R12 (`AC1009`) ASCII DXF file.
pub fn write_dxf_polylines_3d(
    lines: &[(&str, &[[f64; 3]], bool)],
    output_path: &Path,
) -> Result<(), ToolError> {
    let mut layers: BTreeSet<&str> = BTreeSet::new();
    let mut min_pt = [f64::INFINITY; 3];
    let mut max_pt = [f64::NEG_INFINITY; 3];
    for &(layer, pts, _) in lines {
        layers.insert(layer);
        for p in pts {
            for k in 0..3 {
                min_pt[k] = min_pt[k].min(p[k]);
                max_pt[k] = max_pt[k].max(p[k]);
            }
        }
    }

    let mut out = String::new();
    out.push_str("0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\nAC1009\n9\n$INSUNITS\n70\n6\n");
    if min_pt[0].is_finite() {
        out.push_str(&format!(
            "9\n$EXTMIN\n10\n{}\n20\n{}\n30\n{}\n9\n$EXTMAX\n10\n{}\n20\n{}\n30\n{}\n",
            min_pt[0], min_pt[1], min_pt[2], max_pt[0], max_pt[1], max_pt[2]
        ));
    }
    out.push_str("0\nENDSEC\n0\nSECTION\n2\nTABLES\n0\nTABLE\n2\nLAYER\n");
    for layer in &layers {
        out.push_str(&format!("0\nLAYER\n2\n{layer}\n70\n0\n62\n7\n6\nCONTINUOUS\n"));
    }
    out.push_str("0\nENDTAB\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n");

    for &(layer, pts, closed) in lines {
        if pts.len() < 2 {
            continue;
        }
        let flag = 8 | if closed { 1 } else { 0 };
        out.push_str(&format!(
            "0\nPOLYLINE\n8\n{layer}\n66\n1\n10\n0.0\n20\n0.0\n30\n0.0\n70\n{flag}\n"
        ));
        for p in pts {
            out.push_str(&format!(
                "0\nVERTEX\n8\n{layer}\n10\n{}\n20\n{}\n30\n{}\n70\n32\n",
                p[0], p[1], p[2]
            ));
        }
        out.push_str(&format!("0\nSEQEND\n8\n{layer}\n"));
    }
    out.push_str("0\nENDSEC\n0\nEOF\n");

    std::fs::write(output_path, out).map_err(|e| {
        ToolError::new("internal_error", format!("Failed writing 3D DXF output: {e}"))
    })
}

// ---------- dxf_to_mesh ----------

#[derive(Debug, Clone, Serialize)]
pub struct DxfToMeshReport {
    pub node_count: usize,
    pub face_count: usize,
    pub triangle_count: usize,
    pub quad_count: usize,
    pub dropped_degenerate: usize,
    pub dropped_duplicates: usize,
    pub materials: BTreeMap<String, i64>,
    pub warnings: Vec<String>,
}

fn signed_poly_area_xy(pts: &[[f64; 3]]) -> f64 {
    if pts.len() < 3 {
        return 0.0;
    }
    let o = pts[0];
    let mut sum = 0.0;
    for k in 0..pts.len() {
        let a = pts[k];
        let b = pts[(k + 1) % pts.len()];
        let (ax, ay) = (a[0] - o[0], a[1] - o[1]);
        let (bx, by) = (b[0] - o[0], b[1] - o[1]);
        sum += ax * by - bx * ay;
    }
    0.5 * sum
}

/// Convert DXF `3DFACE` entities into a welded `.2dm` or `.obj` surface mesh.
pub fn dxf_to_mesh(
    input_path: &Path,
    layer_filter: Option<&str>,
    weld_tol: f64,
    max_z_step: f64,
    output_path: &Path,
) -> Result<DxfToMeshReport, ToolError> {
    let geom = extract_dxf_geometry(input_path)?;
    let layer_lc = layer_filter.map(str::to_ascii_lowercase);
    let faces: Vec<&DxfFace> = geom
        .faces
        .iter()
        .filter(|f| {
            layer_lc
                .as_deref()
                .is_none_or(|w| f.layer.to_ascii_lowercase() == w)
        })
        .collect();
    if faces.is_empty() {
        return Err(ToolError::new(
            "invalid_input",
            "No 3DFACE entities found in the DXF matching the requested layer filter.",
        ));
    }

    // Assign deterministic material IDs per sorted layer name (recognising MAT_<id>)
    let layer_names: BTreeSet<String> = faces.iter().map(|f| f.layer.clone()).collect();
    let mut mat_map: BTreeMap<String, i64> = BTreeMap::new();
    let mut used_ids: BTreeSet<i64> = BTreeSet::new();
    for lname in &layer_names {
        if let Some(rest) = lname.strip_prefix("MAT_").or_else(|| lname.strip_prefix("mat_"))
            && let Some(num_tok) = rest.split('_').next()
            && let Ok(id) = num_tok.parse::<i64>()
            && id > 0
        {
            mat_map.insert(lname.clone(), id);
            used_ids.insert(id);
        }
    }
    let mut next_id = 1i64;
    for lname in &layer_names {
        if !mat_map.contains_key(lname) {
            while used_ids.contains(&next_id) {
                next_id += 1;
            }
            mat_map.insert(lname.clone(), next_id);
            used_ids.insert(next_id);
        }
    }

    // Quantized XY welding with vertical-step guard
    let inv = 1.0 / weld_tol.max(1e-9);
    let mut cell_to_idx: HashMap<[i64; 2], usize> = HashMap::new();
    let mut sum_xyz: Vec<[f64; 3]> = Vec::new();
    let mut z_minmax: Vec<(f64, f64)> = Vec::new();
    let mut counts: Vec<usize> = Vec::new();
    let mut face_node_ids: Vec<(Vec<usize>, i64)> = Vec::with_capacity(faces.len());

    for f in &faces {
        let mut ids = Vec::with_capacity(f.verts.len());
        for &v in &f.verts {
            let key = [(v[0] * inv).round() as i64, (v[1] * inv).round() as i64];
            let idx = *cell_to_idx.entry(key).or_insert_with(|| {
                let id = sum_xyz.len();
                sum_xyz.push([0.0; 3]);
                z_minmax.push((f64::INFINITY, f64::NEG_INFINITY));
                counts.push(0);
                id
            });
            sum_xyz[idx][0] += v[0];
            sum_xyz[idx][1] += v[1];
            sum_xyz[idx][2] += v[2];
            z_minmax[idx].0 = z_minmax[idx].0.min(v[2]);
            z_minmax[idx].1 = z_minmax[idx].1.max(v[2]);
            counts[idx] += 1;
            ids.push(idx);
        }
        face_node_ids.push((ids, mat_map[&f.layer]));
    }

    let mut worst_z_step = 0.0f64;
    for &(zmin, zmax) in &z_minmax {
        worst_z_step = worst_z_step.max(zmax - zmin);
    }
    if worst_z_step > max_z_step {
        return Err(ToolError::new(
            "invalid_input",
            format!(
                "Vertical Z step of {:.3} m at a welded XY node exceeds max_z_step ({:.3} m). Filter layers or model vertical walls as sloped faces.",
                worst_z_step, max_z_step
            ),
        ));
    }

    let mut warnings = Vec::new();
    if worst_z_step > weld_tol {
        warnings.push(format!(
            "Welded vertices had up to {:.4} m Z spread (averaged within max_z_step).",
            worst_z_step
        ));
    }

    let nodes: Vec<[f64; 3]> = sum_xyz
        .iter()
        .zip(&counts)
        .map(|(s, &c)| [s[0] / c as f64, s[1] / c as f64, s[2] / c as f64])
        .collect();

    let mut seen_faces: BTreeSet<Vec<usize>> = BTreeSet::new();
    let mut kept_faces: Vec<(Vec<usize>, i64)> = Vec::new();
    let mut dropped_degenerate = 0usize;
    let mut dropped_duplicates = 0usize;

    for (mut ids, mat) in face_node_ids {
        ids.dedup();
        if ids.len() > 1 && ids.first() == ids.last() {
            ids.pop();
        }
        let uniq: BTreeSet<usize> = ids.iter().copied().collect();
        if ids.len() < 3 || uniq.len() != ids.len() {
            dropped_degenerate += 1;
            continue;
        }
        let pts: Vec<[f64; 3]> = ids.iter().map(|&i| nodes[i]).collect();
        let area = signed_poly_area_xy(&pts);
        if area.abs() < 1e-10 {
            dropped_degenerate += 1;
            continue;
        }
        if area < 0.0 {
            // Preserve first vertex & n0-n2 diagonal for quads
            ids[1..].reverse();
        }
        let mut key = ids.clone();
        key.sort_unstable();
        if !seen_faces.insert(key) {
            dropped_duplicates += 1;
            continue;
        }
        kept_faces.push((ids, mat));
    }

    if kept_faces.is_empty() {
        return Err(ToolError::new(
            "invalid_input",
            "No non-degenerate 3DFACE elements remained after welding.",
        ));
    }

    // Compact referenced nodes
    let mut remap: HashMap<usize, usize> = HashMap::new();
    let mut out_nodes: Vec<[f64; 3]> = Vec::new();
    let mut tri_count = 0usize;
    let mut quad_count = 0usize;
    let mut out_faces: Vec<(Vec<usize>, i64)> = Vec::with_capacity(kept_faces.len());
    for (ids, mat) in kept_faces {
        let mapped: Vec<usize> = ids
            .into_iter()
            .map(|old| {
                *remap.entry(old).or_insert_with(|| {
                    let n = out_nodes.len();
                    out_nodes.push(nodes[old]);
                    n
                })
            })
            .collect();
        if mapped.len() == 3 {
            tri_count += 1;
        } else {
            quad_count += 1;
        }
        out_faces.push((mapped, mat));
    }

    let ext = output_path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();

    let mut out = String::new();
    if ext == "obj" {
        out.push_str("# Exported by spatial-cad-mcp (mcp-dxf dxf_to_mesh)\n");
        for v in &out_nodes {
            out.push_str(&format!("v {} {} {}\n", v[0], v[1], v[2]));
        }
        let mut last_mat = -1i64;
        for (verts, mat) in &out_faces {
            if *mat != last_mat {
                out.push_str(&format!("g material_{mat}\n"));
                last_mat = *mat;
            }
            out.push('f');
            for &vi in verts {
                out.push_str(&format!(" {}", vi + 1));
            }
            out.push('\n');
        }
    } else {
        out.push_str("MESH2D\n");
        for (i, v) in out_nodes.iter().enumerate() {
            out.push_str(&format!("ND {} {} {} {}\n", i + 1, v[0], v[1], v[2]));
        }
        for (idx, (verts, mat)) in out_faces.iter().enumerate() {
            if verts.len() == 3 {
                out.push_str(&format!(
                    "E3T {} {} {} {} {}\n",
                    idx + 1,
                    verts[0] + 1,
                    verts[1] + 1,
                    verts[2] + 1,
                    mat
                ));
            } else if verts.len() == 4 {
                out.push_str(&format!(
                    "E4Q {} {} {} {} {} {}\n",
                    idx + 1,
                    verts[0] + 1,
                    verts[1] + 1,
                    verts[2] + 1,
                    verts[3] + 1,
                    mat
                ));
            }
        }
    }

    std::fs::write(output_path, out).map_err(|e| {
        ToolError::new("internal_error", format!("Failed writing mesh output: {e}"))
    })?;

    Ok(DxfToMeshReport {
        node_count: out_nodes.len(),
        face_count: out_faces.len(),
        triangle_count: tri_count,
        quad_count,
        dropped_degenerate,
        dropped_duplicates,
        materials: mat_map,
        warnings,
    })
}

// ---------- dxf_centerline ----------

#[derive(Debug, Clone, Serialize)]
pub struct CenterlineReport {
    pub method: String,
    pub ratio: f64,
    pub vertex_count: usize,
    pub length_2d: f64,
    pub length_3d: f64,
    pub line_b_reversed: bool,
    pub min_width: f64,
    pub max_width: f64,
    pub mean_width: f64,
    pub pairing_crossings: usize,
}

fn clean_polyline(raw: &[[f64; 3]]) -> Vec<[f64; 3]> {
    let mut out: Vec<[f64; 3]> = Vec::new();
    for &p in raw {
        if out.last().is_none_or(|prev| (p[0] - prev[0]).hypot(p[1] - prev[1]) > 1e-9) {
            out.push(p);
        }
    }
    out
}

fn arclengths_2d(pts: &[[f64; 3]]) -> Vec<f64> {
    let mut s = Vec::with_capacity(pts.len());
    s.push(0.0);
    for w in pts.windows(2) {
        let prev = *s.last().unwrap_or(&0.0);
        s.push(prev + (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1]));
    }
    s
}

fn interp_at_norm_t(pts: &[[f64; 3]], ts: &[f64], t: f64) -> [f64; 3] {
    let tc = t.clamp(0.0, 1.0);
    let idx = ts.partition_point(|&v| v < tc);
    if idx == 0 {
        return pts[0];
    }
    if idx >= pts.len() {
        return pts[pts.len() - 1];
    }
    let t0 = ts[idx - 1];
    let t1 = ts[idx];
    let u = if (t1 - t0).abs() > 1e-12 {
        (tc - t0) / (t1 - t0)
    } else {
        0.0
    };
    let a = pts[idx - 1];
    let b = pts[idx];
    [
        a[0] + u * (b[0] - a[0]),
        a[1] + u * (b[1] - a[1]),
        a[2] + u * (b[2] - a[2]),
    ]
}

fn densify_params(base: &[f64], total_len: f64, step: Option<f64>) -> Vec<f64> {
    let Some(st) = step else {
        return base.to_vec();
    };
    let mut out = Vec::new();
    for w in base.windows(2) {
        out.push(w[0]);
        let seg_len = (w[1] - w[0]) * total_len;
        if seg_len > st {
            let n = (seg_len / st).ceil() as usize;
            for k in 1..n {
                out.push(w[0] + (w[1] - w[0]) * (k as f64 / n as f64));
            }
        }
    }
    if let Some(&last) = base.last() {
        out.push(last);
    }
    out.sort_by(|a, b| a.total_cmp(b));
    out.dedup_by(|a, b| (*a - *b).abs() < 1e-11);
    out
}

fn project_norm_onto_line(p: [f64; 2], pts: &[[f64; 3]], s: &[f64]) -> f64 {
    let total = *s.last().unwrap_or(&1.0);
    if total <= 1e-12 {
        return 0.0;
    }
    let mut best_d2 = f64::INFINITY;
    let mut best_s = 0.0;
    for i in 0..pts.len() - 1 {
        let a = pts[i];
        let b = pts[i + 1];
        let dx = b[0] - a[0];
        let dy = b[1] - a[1];
        let l2 = dx * dx + dy * dy;
        let u = if l2 > 1e-24 {
            (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / l2).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let qx = a[0] + u * dx;
        let qy = a[1] + u * dy;
        let d2 = (p[0] - qx) * (p[0] - qx) + (p[1] - qy) * (p[1] - qy);
        if d2 < best_d2 {
            best_d2 = d2;
            best_s = s[i] + u * (s[i + 1] - s[i]);
        }
    }
    (best_s / total).clamp(0.0, 1.0)
}

fn segments_strictly_cross(a0: [f64; 2], a1: [f64; 2], b0: [f64; 2], b1: [f64; 2]) -> bool {
    let orient = |p: [f64; 2], q: [f64; 2], r: [f64; 2]| -> f64 {
        (q[0] - p[0]) * (r[1] - p[1]) - (q[1] - p[1]) * (r[0] - p[0])
    };
    let o1 = orient(a0, a1, b0);
    let o2 = orient(a0, a1, b1);
    let o3 = orient(b0, b1, a0);
    let o4 = orient(b0, b1, a1);
    (o1 * o2 < -1e-12) && (o3 * o4 < -1e-12)
}

/// Compute a 3D centreline between two DXF polylines (selected by `layer_a` and `layer_b`)
/// and write the result as a 3D `POLYLINE` DXF under `output_path`.
pub fn compute_dxf_centerline(
    input_path: &Path,
    layer_a: &str,
    layer_b: &str,
    method: &str,
    ratio: f64,
    step: Option<f64>,
    output_path: &Path,
) -> Result<CenterlineReport, ToolError> {
    let geom = extract_dxf_geometry(input_path)?;
    let pick_longest = |want: &str| -> Option<Vec<[f64; 3]>> {
        let want_lc = want.to_ascii_lowercase();
        geom.lines
            .iter()
            .filter(|l| l.layer.to_ascii_lowercase() == want_lc)
            .max_by(|a, b| {
                let la = *arclengths_2d(&a.pts).last().unwrap_or(&0.0);
                let lb = *arclengths_2d(&b.pts).last().unwrap_or(&0.0);
                la.total_cmp(&lb)
            })
            .map(|l| clean_polyline(&l.pts))
    };

    let a = pick_longest(layer_a).ok_or_else(|| {
        ToolError::new(
            "invalid_input",
            format!("No LINE/LWPOLYLINE/POLYLINE found on layer '{layer_a}'."),
        )
    })?;
    let mut b = pick_longest(layer_b).ok_or_else(|| {
        ToolError::new(
            "invalid_input",
            format!("No LINE/LWPOLYLINE/POLYLINE found on layer '{layer_b}'."),
        )
    })?;
    if a.len() < 2 || b.len() < 2 {
        return Err(ToolError::new(
            "invalid_input",
            "Both lines must have at least 2 distinct XY vertices.",
        ));
    }

    let d_same = (a[0][0] - b[0][0]).hypot(a[0][1] - b[0][1])
        + (a[a.len() - 1][0] - b[b.len() - 1][0]).hypot(a[a.len() - 1][1] - b[b.len() - 1][1]);
    let d_flip = (a[0][0] - b[b.len() - 1][0]).hypot(a[0][1] - b[b.len() - 1][1])
        + (a[a.len() - 1][0] - b[0][0]).hypot(a[a.len() - 1][1] - b[0][1]);
    let line_b_reversed = d_flip < d_same;
    if line_b_reversed {
        b.reverse();
    }

    let sa = arclengths_2d(&a);
    let sb = arclengths_2d(&b);
    let la = *sa.last().unwrap_or(&0.0);
    let lb = *sb.last().unwrap_or(&0.0);
    if la <= 1e-9 || lb <= 1e-9 {
        return Err(ToolError::new(
            "invalid_input",
            "Centerline inputs must have non-zero 2D length.",
        ));
    }
    let ta: Vec<f64> = sa.iter().map(|v| v / la).collect();
    let tb: Vec<f64> = sb.iter().map(|v| v / lb).collect();

    let (pa_vec, pb_vec): (Vec<[f64; 3]>, Vec<[f64; 3]>) = match method {
        "arc_length" => {
            let mut merged = ta.clone();
            merged.extend_from_slice(&tb);
            merged.sort_by(|x, y| x.total_cmp(y));
            merged.dedup_by(|x, y| (*x - *y).abs() < 1e-11);
            let ts = densify_params(&merged, la.max(lb), step);
            let pa = ts.iter().map(|&t| interp_at_norm_t(&a, &ta, t)).collect();
            let pb = ts.iter().map(|&t| interp_at_norm_t(&b, &tb, t)).collect();
            (pa, pb)
        }
        "perpendicular" => {
            let ts = densify_params(&ta, la, step);
            let pa: Vec<[f64; 3]> = ts.iter().map(|&t| interp_at_norm_t(&a, &ta, t)).collect();
            let mut max_sb = 0.0f64;
            let mut pb = Vec::with_capacity(pa.len());
            for p in &pa {
                let t_proj = project_norm_onto_line([p[0], p[1]], &b, &sb);
                max_sb = max_sb.max(t_proj);
                pb.push(interp_at_norm_t(&b, &tb, max_sb));
            }
            (pa, pb)
        }
        other => {
            return Err(ToolError::new(
                "invalid_input",
                format!("Unknown method '{other}'. Supported: 'arc_length', 'perpendicular'."),
            ));
        }
    };

    let mut center_pts: Vec<[f64; 3]> = Vec::with_capacity(pa_vec.len());
    let mut rungs: Vec<([f64; 2], [f64; 2])> = Vec::with_capacity(pa_vec.len());
    let mut min_w = f64::INFINITY;
    let mut max_w = 0.0f64;
    let mut sum_w = 0.0f64;

    for (pa, pb) in pa_vec.iter().zip(&pb_vec) {
        let pt = [
            (1.0 - ratio) * pa[0] + ratio * pb[0],
            (1.0 - ratio) * pa[1] + ratio * pb[1],
            (1.0 - ratio) * pa[2] + ratio * pb[2],
        ];
        if center_pts
            .last()
            .is_none_or(|prev| (pt[0] - prev[0]).hypot(pt[1] - prev[1]) > 1e-9)
        {
            let w = (pb[0] - pa[0]).hypot(pb[1] - pa[1]);
            min_w = min_w.min(w);
            max_w = max_w.max(w);
            sum_w += w;
            center_pts.push(pt);
            rungs.push(([pa[0], pa[1]], [pb[0], pb[1]]));
        }
    }

    let mut crossings = 0usize;
    for i in 0..rungs.len() {
        for j in (i + 1)..rungs.len() {
            if segments_strictly_cross(rungs[i].0, rungs[i].1, rungs[j].0, rungs[j].1) {
                crossings += 1;
            }
        }
    }

    let mut len_2d = 0.0;
    let mut len_3d = 0.0;
    for w in center_pts.windows(2) {
        let dx = w[1][0] - w[0][0];
        let dy = w[1][1] - w[0][1];
        let dz = w[1][2] - w[0][2];
        len_2d += dx.hypot(dy);
        len_3d += (dx * dx + dy * dy + dz * dz).sqrt();
    }

    write_dxf_polylines_3d(&[("CENTERLINE", &center_pts, false)], output_path)?;

    Ok(CenterlineReport {
        method: method.to_string(),
        ratio,
        vertex_count: center_pts.len(),
        length_2d: round3(len_2d),
        length_3d: round3(len_3d),
        line_b_reversed,
        min_width: round3(min_w),
        max_width: round3(max_w),
        mean_width: round3(sum_w / center_pts.len().max(1) as f64),
        pairing_crossings: crossings,
    })
}

// ---------- dxf_labels_to_3d ----------

#[derive(Debug, Clone, Serialize)]
pub struct LabelsTo3dReport {
    pub height_labels_found: usize,
    pub labels_assigned: usize,
    pub labels_unassigned: usize,
    pub labels_rejected_range: usize,
    pub input_lines: usize,
    pub output_lines_3d: usize,
    pub lines_without_height: usize,
    pub vertices_measured: usize,
    pub vertices_interpolated: usize,
    pub vertices_held: usize,
    pub z_min_out: Option<f64>,
    pub z_max_out: Option<f64>,
    pub warnings: Vec<String>,
}

fn parse_leading_number(text: &str) -> Option<f64> {
    let trimmed = text.trim();
    // Skip any non-numeric prefix (e.g. "EL 102.50 m" or "+102,50")
    let bytes = trimmed.as_bytes();
    let mut start = None;
    for i in 0..bytes.len() {
        let b = bytes[i];
        if b.is_ascii_digit()
            || ((b == b'+' || b == b'-') && i + 1 < bytes.len() && bytes[i + 1].is_ascii_digit())
        {
            start = Some(i);
            break;
        }
    }
    let s = start?;
    let mut end = s + 1;
    let mut seen_dot = false;
    while end < bytes.len() {
        let b = bytes[end];
        if b.is_ascii_digit() {
            end += 1;
        } else if (b == b'.' || b == b',') && !seen_dot {
            seen_dot = true;
            end += 1;
        } else {
            break;
        }
    }
    let token = trimmed[s..end].replace(',', ".");
    token.parse::<f64>().ok().filter(|v| v.is_finite())
}

fn dist_pt_seg_2d(p: [f64; 2], a: [f64; 2], b: [f64; 2]) -> (f64, [f64; 2], f64) {
    let dx = b[0] - a[0];
    let dy = b[1] - a[1];
    let l2 = dx * dx + dy * dy;
    let u = if l2 > 1e-24 {
        (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / l2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let q = [a[0] + u * dx, a[1] + u * dy];
    ((p[0] - q[0]).hypot(p[1] - q[1]), q, u)
}

/// Assign elevation annotations (`TEXT`/`MTEXT`/`POINT` on `label_layer`) onto
/// 2D DXF lines (`line_layer`), interpolate Z linearly along arc length, and
/// write 3D polylines to `output_path`.
#[allow(clippy::too_many_arguments)]
pub fn dxf_labels_to_3d(
    input_path: &Path,
    label_layer: &str,
    line_layer: &str,
    max_distance: f64,
    z_min: Option<f64>,
    z_max: Option<f64>,
    use_existing_z: bool,
    output_path: &Path,
) -> Result<LabelsTo3dReport, ToolError> {
    let geom = extract_dxf_geometry(input_path)?;
    let label_lc = label_layer.to_ascii_lowercase();
    let line_lc = line_layer.to_ascii_lowercase();

    let mut warnings = Vec::new();
    let mut raw_pts: Vec<([f64; 2], f64)> = Vec::new();
    for t in &geom.texts {
        if t.layer.to_ascii_lowercase() != label_lc {
            continue;
        }
        let parsed = parse_leading_number(&t.text);
        let z = if t.xyz[2].abs() > 1e-6 {
            if let Some(pv) = parsed
                && (pv - t.xyz[2]).abs() > 0.05
            {
                warnings.push(format!(
                    "Label {} ({}) Z {:.3} differs from text {:.3}; using entity Z.",
                    t.handle, t.text, t.xyz[2], pv
                ));
            }
            t.xyz[2]
        } else if let Some(pv) = parsed {
            pv
        } else {
            continue;
        };
        raw_pts.push(([t.xyz[0], t.xyz[1]], z));
    }
    for p in &geom.points {
        if p.layer.to_ascii_lowercase() == label_lc && p.xyz[2].abs() > 1e-6 {
            raw_pts.push(([p.xyz[0], p.xyz[1]], p.xyz[2]));
        }
    }

    let lines: Vec<&DxfLine> = geom
        .lines
        .iter()
        .filter(|l| l.layer.to_ascii_lowercase() == line_lc)
        .collect();
    if lines.is_empty() {
        return Err(ToolError::new(
            "invalid_input",
            format!("No lines found on layer '{line_layer}'."),
        ));
    }
    if raw_pts.is_empty() {
        return Err(ToolError::new(
            "invalid_input",
            format!("No elevation labels or 3D points found on layer '{label_layer}'."),
        ));
    }

    let mut rejected_range = 0usize;
    let mut pts: Vec<([f64; 2], f64)> = Vec::new();
    for (xy, z) in raw_pts {
        if z_min.is_some_and(|lo| z < lo) || z_max.is_some_and(|hi| z > hi) {
            rejected_range += 1;
        } else {
            pts.push((xy, z));
        }
    }
    if pts.is_empty() {
        return Err(ToolError::new(
            "invalid_input",
            "All height labels fell outside the [z_min, z_max] range.",
        ));
    }

    // For each label, find the nearest line & vertex within max_distance
    let mut anchors: Vec<BTreeMap<usize, Vec<(f64, f64)>>> = vec![BTreeMap::new(); lines.len()];
    let mut assigned = 0usize;
    let mut unassigned = 0usize;

    for &(q, z) in &pts {
        let mut best: Option<(f64, usize, usize)> = None;
        for (li, ln) in lines.iter().enumerate() {
            let seg_count = if ln.closed && ln.pts.len() >= 3 {
                ln.pts.len()
            } else {
                ln.pts.len().saturating_sub(1)
            };
            let mut min_line_d = f64::INFINITY;
            for si in 0..seg_count {
                let a = ln.pts[si];
                let b = ln.pts[(si + 1) % ln.pts.len()];
                let (d, _, _) = dist_pt_seg_2d(q, [a[0], a[1]], [b[0], b[1]]);
                min_line_d = min_line_d.min(d);
            }
            if min_line_d <= max_distance {
                // Snap to nearest vertex on this line
                let mut best_vi = 0usize;
                let mut best_vd = f64::INFINITY;
                for (vi, v) in ln.pts.iter().enumerate() {
                    let vd = (q[0] - v[0]).hypot(q[1] - v[1]);
                    if vd < best_vd {
                        best_vd = vd;
                        best_vi = vi;
                    }
                }
                if best.is_none_or(|(bd, _, _)| min_line_d < bd) {
                    best = Some((min_line_d, li, best_vi));
                }
            }
        }
        if let Some((d, li, vi)) = best {
            anchors[li].entry(vi).or_default().push((d, z));
            assigned += 1;
        } else {
            unassigned += 1;
        }
    }

    if use_existing_z {
        for (li, ln) in lines.iter().enumerate() {
            for (vi, v) in ln.pts.iter().enumerate() {
                if v[2].abs() > 1e-6 {
                    anchors[li]
                        .entry(vi)
                        .or_insert_with(|| vec![(f64::INFINITY, v[2])]);
                }
            }
        }
    }

    let mut out_lines: Vec<(String, Vec<[f64; 3]>, bool)> = Vec::new();
    let mut lines_without_height = 0usize;
    let mut v_meas = 0usize;
    let mut v_interp = 0usize;
    let mut v_held = 0usize;
    let mut out_z_min = f64::INFINITY;
    let mut out_z_max = f64::NEG_INFINITY;

    for (li, ln) in lines.iter().enumerate() {
        if anchors[li].is_empty() {
            lines_without_height += 1;
            continue;
        }
        let mut anc_z: BTreeMap<usize, f64> = BTreeMap::new();
        for (&vi, list) in &anchors[li] {
            let mut sorted = list.clone();
            sorted.sort_by(|a, b| a.0.total_cmp(&b.0));
            let zmin_v = sorted.iter().map(|x| x.1).fold(f64::INFINITY, f64::min);
            let zmax_v = sorted.iter().map(|x| x.1).fold(f64::NEG_INFINITY, f64::max);
            if zmax_v - zmin_v > 0.05 {
                warnings.push(format!(
                    "Line {} ({}) vertex {vi} received conflicting heights ({:.3}..{:.3}); using nearest.",
                    ln.handle, ln.layer, zmin_v, zmax_v
                ));
            }
            anc_z.insert(vi, sorted[0].1);
        }

        let s = arclengths_2d(&ln.pts);
        let idxs: Vec<usize> = anc_z.keys().copied().collect();
        let mut out_pts = Vec::with_capacity(ln.pts.len());
        for (vi, p) in ln.pts.iter().enumerate() {
            let z = if let Some(&mz) = anc_z.get(&vi) {
                v_meas += 1;
                mz
            } else if vi < idxs[0] {
                v_held += 1;
                anc_z[&idxs[0]]
            } else if vi > *idxs.last().unwrap_or(&0) {
                v_held += 1;
                anc_z[idxs.last().unwrap_or(&0)]
            } else {
                // Linear interpolation by arc length between surrounding anchors
                let pos = idxs.partition_point(|&a| a < vi);
                let a = idxs[pos - 1];
                let b = idxs[pos];
                let span = s[b] - s[a];
                let t = if span > 1e-12 { (s[vi] - s[a]) / span } else { 0.0 };
                v_interp += 1;
                anc_z[&a] + t * (anc_z[&b] - anc_z[&a])
            };
            out_z_min = out_z_min.min(z);
            out_z_max = out_z_max.max(z);
            out_pts.push([p[0], p[1], z]);
        }
        out_lines.push((ln.layer.clone(), out_pts, ln.closed));
    }

    let refs: Vec<(&str, &[[f64; 3]], bool)> = out_lines
        .iter()
        .map(|(l, pts, c)| (l.as_str(), pts.as_slice(), *c))
        .collect();
    write_dxf_polylines_3d(&refs, output_path)?;

    Ok(LabelsTo3dReport {
        height_labels_found: pts.len() + rejected_range,
        labels_assigned: assigned,
        labels_unassigned: unassigned,
        labels_rejected_range: rejected_range,
        input_lines: lines.len(),
        output_lines_3d: out_lines.len(),
        lines_without_height,
        vertices_measured: v_meas,
        vertices_interpolated: v_interp,
        vertices_held: v_held,
        z_min_out: out_z_min.is_finite().then_some(round4(out_z_min)),
        z_max_out: out_z_max.is_finite().then_some(round4(out_z_max)),
        warnings,
    })
}
