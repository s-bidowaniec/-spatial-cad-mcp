//! Pure-Rust loaders (SMS `.2dm`, Wavefront `.obj`, ASCII & binary `.stl`)
//! and deterministic topology + element quality QA engine.

use cad_envelope::ToolError;
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MeshFormat {
    Sms2dm,
    Obj,
    StlAscii,
    StlBinary,
}

#[derive(Debug, Clone)]
pub struct Face {
    pub id: i64,
    /// 3 vertex indices for a triangle, or 4 for a quad.
    pub verts: Vec<usize>,
    pub part: String,
}

#[derive(Debug, Clone)]
pub struct Mesh {
    pub format: MeshFormat,
    pub vertices: Vec<[f64; 3]>,
    pub faces: Vec<Face>,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct BBox3D {
    pub min: [f64; 3],
    pub max: [f64; 3],
}

#[derive(Debug, Clone, Serialize)]
pub struct TopologySummary {
    pub unique_edges: usize,
    pub boundary_edges: usize,
    pub interior_edges: usize,
    pub non_manifold_edges: usize,
    pub watertight: bool,
    pub euler_characteristic: i64,
    pub valence_min: usize,
    pub valence_max: usize,
    pub valence_mean: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct QualitySummary {
    pub min_angle_deg: Option<f64>,
    pub max_angle_deg: Option<f64>,
    pub min_edge: Option<f64>,
    pub max_edge: Option<f64>,
    pub total_area: f64,
    pub degenerate_count: usize,
    pub nonconvex_count: usize,
    pub clockwise_xy_count: usize,
    pub non_manifold_face_count: usize,
    pub worst_neighbor_area_ratio: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PartSummary {
    pub part: String,
    pub face_count: usize,
    pub triangle_count: usize,
    pub quad_count: usize,
    pub bbox: Option<BBox3D>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FaceQuality {
    pub face_id: i64,
    pub kind: &'static str,
    pub part: String,
    pub min_angle_deg: f64,
    pub max_angle_deg: f64,
    pub min_edge: f64,
    pub max_edge: f64,
    pub area: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quad_aspect: Option<f64>,
    pub worst_neighbor_area_ratio: Option<f64>,
    pub degenerate: bool,
    pub nonconvex: bool,
    pub clockwise_xy: bool,
    pub non_manifold: bool,
    pub bbox: BBox3D,
}

#[derive(Debug, Clone)]
pub struct MeshAnalysis {
    pub format: MeshFormat,
    pub vertex_count: usize,
    pub face_count: usize,
    pub triangle_count: usize,
    pub quad_count: usize,
    pub bbox: Option<BBox3D>,
    pub topology: TopologySummary,
    pub quality: QualitySummary,
    pub parts: Vec<PartSummary>,
    pub face_qualities: Vec<FaceQuality>,
}

fn round3(v: f64) -> f64 {
    (v * 1000.0).round() / 1000.0
}

fn round4(v: f64) -> f64 {
    (v * 10_000.0).round() / 10_000.0
}

pub fn load_mesh_file(path: &Path) -> Result<Mesh, ToolError> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();

    let raw = std::fs::read(path).map_err(|e| {
        ToolError::new("cannot_open_mesh", format!("Failed to read mesh file: {e}"))
    })?;

    match ext.as_str() {
        "2dm" => parse_2dm(&String::from_utf8_lossy(&raw)),
        "obj" => parse_obj(&String::from_utf8_lossy(&raw)),
        "stl" => parse_stl(&raw),
        other => Err(ToolError::new(
            "unsupported_format",
            format!(
                "Unsupported mesh extension '.{other}'. Supported formats: .2dm, .obj, .stl."
            ),
        )),
    }
}

/// Parse SMS `.2dm` unstructured 2D/3D mesh (`ND`, `E3T`, `E4Q`).
pub fn parse_2dm(text: &str) -> Result<Mesh, ToolError> {
    let mut node_rows: Vec<(i64, [f64; 3])> = Vec::new();
    let mut elem_rows: Vec<(i64, Vec<i64>, i64)> = Vec::new();
    let mut block_state = "";

    for raw in text.lines() {
        let s = raw.trim();
        if s.is_empty() {
            continue;
        }
        if !block_state.is_empty() {
            if (block_state == "def" && s.starts_with("ENDPARAMDEF"))
                || (block_state == "curve" && s.starts_with("ENDCURVE"))
            {
                block_state = "";
            }
            continue;
        }
        let mut p = s.split_whitespace();
        let card = p.next().unwrap_or("");
        match card {
            "BEGPARAMDEF" => block_state = "def",
            "BEGCURVE" => block_state = "curve",
            "ND" => {
                let Some(id) = p.next().and_then(|v| v.parse::<i64>().ok()) else {
                    continue;
                };
                let (Some(x), Some(y), Some(z)) = (
                    p.next().and_then(|v| v.parse::<f64>().ok()),
                    p.next().and_then(|v| v.parse::<f64>().ok()),
                    p.next().and_then(|v| v.parse::<f64>().ok()),
                ) else {
                    continue;
                };
                node_rows.push((id, [x, y, z]));
            }
            "E3T" => {
                let (Some(id), Some(n1), Some(n2), Some(n3)) = (
                    p.next().and_then(|v| v.parse::<i64>().ok()),
                    p.next().and_then(|v| v.parse::<i64>().ok()),
                    p.next().and_then(|v| v.parse::<i64>().ok()),
                    p.next().and_then(|v| v.parse::<i64>().ok()),
                ) else {
                    continue;
                };
                let mat = p.next().and_then(|v| v.parse::<i64>().ok()).unwrap_or(0);
                elem_rows.push((id, vec![n1, n2, n3], mat));
            }
            "E4Q" => {
                let (Some(id), Some(n1), Some(n2), Some(n3), Some(n4)) = (
                    p.next().and_then(|v| v.parse::<i64>().ok()),
                    p.next().and_then(|v| v.parse::<i64>().ok()),
                    p.next().and_then(|v| v.parse::<i64>().ok()),
                    p.next().and_then(|v| v.parse::<i64>().ok()),
                    p.next().and_then(|v| v.parse::<i64>().ok()),
                ) else {
                    continue;
                };
                let mat = p.next().and_then(|v| v.parse::<i64>().ok()).unwrap_or(0);
                elem_rows.push((id, vec![n1, n2, n3, n4], mat));
            }
            _ => {}
        }
    }

    if node_rows.is_empty() && elem_rows.is_empty() {
        return Err(ToolError::new(
            "cannot_open_mesh",
            "No ND nodes or E3T/E4Q elements found in .2dm file.",
        ));
    }

    node_rows.sort_by_key(|(id, _)| *id);
    elem_rows.sort_by_key(|(id, _, _)| *id);

    let mut id_to_idx: HashMap<i64, usize> = HashMap::with_capacity(node_rows.len());
    let mut vertices = Vec::with_capacity(node_rows.len());
    for (id, xyz) in node_rows {
        let idx = vertices.len();
        id_to_idx.insert(id, idx);
        vertices.push(xyz);
    }

    let mut faces = Vec::with_capacity(elem_rows.len());
    for (id, nids, mat) in elem_rows {
        let mut verts = Vec::with_capacity(nids.len());
        for nid in nids {
            let Some(&idx) = id_to_idx.get(&nid) else {
                return Err(ToolError::new(
                    "cannot_open_mesh",
                    format!("Element {id} references unknown node ID {nid}."),
                ));
            };
            verts.push(idx);
        }
        faces.push(Face {
            id,
            verts,
            part: format!("material_{mat}"),
        });
    }

    Ok(Mesh {
        format: MeshFormat::Sms2dm,
        vertices,
        faces,
    })
}

/// Parse Wavefront `.obj` (`v`, `f`, `o`, `g`), fan-triangulating n-gons > 4.
pub fn parse_obj(text: &str) -> Result<Mesh, ToolError> {
    let mut vertices: Vec<[f64; 3]> = Vec::new();
    let mut faces: Vec<Face> = Vec::new();
    let mut current_part = "default".to_string();
    let mut next_id = 1i64;

    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.split_whitespace();
        let kw = parts.next().unwrap_or("");
        match kw {
            "v" => {
                let (Some(x), Some(y), Some(z)) = (
                    parts.next().and_then(|v| v.parse::<f64>().ok()),
                    parts.next().and_then(|v| v.parse::<f64>().ok()),
                    parts.next().and_then(|v| v.parse::<f64>().ok()),
                ) else {
                    continue;
                };
                vertices.push([x, y, z]);
            }
            "o" | "g" => {
                let name: Vec<&str> = parts.collect();
                if !name.is_empty() {
                    current_part = name.join(" ");
                }
            }
            "f" => {
                let mut poly = Vec::new();
                for tok in parts {
                    let v_str = tok.split('/').next().unwrap_or("");
                    let Ok(raw_idx) = v_str.parse::<isize>() else {
                        continue;
                    };
                    let idx = if raw_idx > 0 {
                        (raw_idx - 1) as usize
                    } else if raw_idx < 0 {
                        let resolved = vertices.len() as isize + raw_idx;
                        if resolved < 0 {
                            return Err(ToolError::new(
                                "cannot_open_mesh",
                                format!("OBJ face has out-of-range negative index {raw_idx}."),
                            ));
                        }
                        resolved as usize
                    } else {
                        return Err(ToolError::new(
                            "cannot_open_mesh",
                            "OBJ face references invalid 0 vertex index.",
                        ));
                    };
                    if idx >= vertices.len() {
                        return Err(ToolError::new(
                            "cannot_open_mesh",
                            format!("OBJ face references vertex {idx} beyond vertex count {}.", vertices.len()),
                        ));
                    }
                    poly.push(idx);
                }
                if poly.len() == 3 || poly.len() == 4 {
                    faces.push(Face {
                        id: next_id,
                        verts: poly,
                        part: current_part.clone(),
                    });
                    next_id += 1;
                } else if poly.len() > 4 {
                    // Fan triangulate n-gons deterministically
                    for i in 1..(poly.len() - 1) {
                        faces.push(Face {
                            id: next_id,
                            verts: vec![poly[0], poly[i], poly[i + 1]],
                            part: current_part.clone(),
                        });
                        next_id += 1;
                    }
                }
            }
            _ => {}
        }
    }

    if vertices.is_empty() && faces.is_empty() {
        return Err(ToolError::new(
            "cannot_open_mesh",
            "No vertices or faces found in .obj file.",
        ));
    }

    Ok(Mesh {
        format: MeshFormat::Obj,
        vertices,
        faces,
    })
}

/// Parse ASCII or binary `.stl`, deduplicating shared vertices by quantized
/// 1e-9 coordinates so watertightness and edge topology work accurately.
pub fn parse_stl(raw: &[u8]) -> Result<Mesh, ToolError> {
    if is_binary_stl(raw) {
        parse_stl_binary(raw)
    } else {
        parse_stl_ascii(&String::from_utf8_lossy(raw))
    }
}

fn is_binary_stl(raw: &[u8]) -> bool {
    if raw.len() < 84 {
        return false;
    }
    let tri_count = u32::from_le_bytes([raw[80], raw[81], raw[82], raw[83]]) as usize;
    let expected = 84usize.saturating_add(tri_count.saturating_mul(50));
    if expected == raw.len() && tri_count > 0 {
        return true;
    }
    !raw.starts_with(b"solid")
}

fn quantize(v: [f64; 3]) -> [i64; 3] {
    [
        (v[0] * 1e6).round() as i64,
        (v[1] * 1e6).round() as i64,
        (v[2] * 1e6).round() as i64,
    ]
}

fn intern_vertex(
    xyz: [f64; 3],
    vertices: &mut Vec<[f64; 3]>,
    map: &mut HashMap<[i64; 3], usize>,
) -> usize {
    let key = quantize(xyz);
    if let Some(&idx) = map.get(&key) {
        return idx;
    }
    let idx = vertices.len();
    vertices.push(xyz);
    map.insert(key, idx);
    idx
}

fn parse_stl_binary(raw: &[u8]) -> Result<Mesh, ToolError> {
    if raw.len() < 84 {
        return Err(ToolError::new(
            "cannot_open_mesh",
            "Binary STL file is shorter than the 84-byte header.",
        ));
    }
    let tri_count = u32::from_le_bytes([raw[80], raw[81], raw[82], raw[83]]) as usize;
    let expected = 84usize.saturating_add(tri_count.saturating_mul(50));
    if raw.len() < expected {
        return Err(ToolError::new(
            "cannot_open_mesh",
            format!("Truncated binary STL: expected {expected} bytes for {tri_count} triangles, got {}.", raw.len()),
        ));
    }

    let mut vertices = Vec::new();
    let mut map = HashMap::new();
    let mut faces = Vec::with_capacity(tri_count);

    let read_f32 = |chunk: &[u8]| -> f64 {
        f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]) as f64
    };

    for t in 0..tri_count {
        let base = 84 + t * 50;
        let v1 = [
            read_f32(&raw[base + 12..base + 16]),
            read_f32(&raw[base + 16..base + 20]),
            read_f32(&raw[base + 20..base + 24]),
        ];
        let v2 = [
            read_f32(&raw[base + 24..base + 28]),
            read_f32(&raw[base + 28..base + 32]),
            read_f32(&raw[base + 32..base + 36]),
        ];
        let v3 = [
            read_f32(&raw[base + 36..base + 40]),
            read_f32(&raw[base + 40..base + 44]),
            read_f32(&raw[base + 44..base + 48]),
        ];
        let i1 = intern_vertex(v1, &mut vertices, &mut map);
        let i2 = intern_vertex(v2, &mut vertices, &mut map);
        let i3 = intern_vertex(v3, &mut vertices, &mut map);
        faces.push(Face {
            id: (t + 1) as i64,
            verts: vec![i1, i2, i3],
            part: "solid".to_string(),
        });
    }

    Ok(Mesh {
        format: MeshFormat::StlBinary,
        vertices,
        faces,
    })
}

fn parse_stl_ascii(text: &str) -> Result<Mesh, ToolError> {
    let mut vertices = Vec::new();
    let mut map = HashMap::new();
    let mut faces = Vec::new();
    let mut current_part = "solid".to_string();
    let mut tri_buf: Vec<usize> = Vec::with_capacity(3);
    let mut next_id = 1i64;

    for raw in text.lines() {
        let line = raw.trim();
        if let Some(rest) = line.strip_prefix("solid") {
            let name = rest.trim();
            if !name.is_empty() {
                current_part = name.to_string();
            }
        } else if let Some(rest) = line.strip_prefix("vertex") {
            let nums: Vec<f64> = rest
                .split_whitespace()
                .filter_map(|t| t.parse::<f64>().ok())
                .collect();
            if nums.len() >= 3 {
                let idx = intern_vertex([nums[0], nums[1], nums[2]], &mut vertices, &mut map);
                tri_buf.push(idx);
                if tri_buf.len() == 3 {
                    faces.push(Face {
                        id: next_id,
                        verts: std::mem::take(&mut tri_buf),
                        part: current_part.clone(),
                    });
                    next_id += 1;
                }
            }
        }
    }

    if faces.is_empty() {
        return Err(ToolError::new(
            "cannot_open_mesh",
            "No triangles found in ASCII STL file.",
        ));
    }

    Ok(Mesh {
        format: MeshFormat::StlAscii,
        vertices,
        faces,
    })
}

fn sub3(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot3(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross3(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn norm3(a: [f64; 3]) -> f64 {
    dot3(a, a).sqrt()
}

fn tri_area3(a: [f64; 3], b: [f64; 3], c: [f64; 3]) -> f64 {
    0.5 * norm3(cross3(sub3(b, a), sub3(c, a)))
}

fn signed_area_xy(pts: &[[f64; 3]]) -> f64 {
    let mut sum = 0.0;
    let n = pts.len();
    for i in 0..n {
        let a = pts[i];
        let b = pts[(i + 1) % n];
        sum += a[0] * b[1] - b[0] * a[1];
    }
    0.5 * sum
}

fn face_edges(verts: &[usize]) -> Vec<[usize; 2]> {
    let n = verts.len();
    (0..n)
        .map(|i| {
            let a = verts[i];
            let b = verts[(i + 1) % n];
            [a.min(b), a.max(b)]
        })
        .collect()
}

/// Run full deterministic topology, part breakdown, and element QA analysis.
pub fn analyze_mesh(mesh: &Mesh) -> MeshAnalysis {
    let mut edge_faces: HashMap<[usize; 2], Vec<usize>> = HashMap::new();
    let mut vertex_degree: Vec<usize> = vec![0; mesh.vertices.len()];

    let mut global_min = [f64::INFINITY; 3];
    let mut global_max = [f64::NEG_INFINITY; 3];
    for &v in &mesh.vertices {
        if v.iter().all(|x| x.is_finite()) {
            for k in 0..3 {
                global_min[k] = global_min[k].min(v[k]);
                global_max[k] = global_max[k].max(v[k]);
            }
        }
    }
    let bbox = (!mesh.vertices.is_empty() && global_min[0].is_finite()).then_some(BBox3D {
        min: [
            round3(global_min[0]),
            round3(global_min[1]),
            round3(global_min[2]),
        ],
        max: [
            round3(global_max[0]),
            round3(global_max[1]),
            round3(global_max[2]),
        ],
    });

    for (fi, face) in mesh.faces.iter().enumerate() {
        for e in face_edges(&face.verts) {
            edge_faces.entry(e).or_default().push(fi);
        }
    }
    for &[a, b] in edge_faces.keys() {
        vertex_degree[a] += 1;
        vertex_degree[b] += 1;
    }

    let mut boundary_edges = 0usize;
    let mut interior_edges = 0usize;
    let mut non_manifold_edges = 0usize;
    let mut face_non_manifold = vec![false; mesh.faces.len()];

    for owners in edge_faces.values() {
        match owners.len() {
            1 => boundary_edges += 1,
            2 => interior_edges += 1,
            _ => {
                non_manifold_edges += 1;
                for &fi in owners {
                    face_non_manifold[fi] = true;
                }
            }
        }
    }

    let unique_edges = edge_faces.len();
    let watertight = !mesh.faces.is_empty() && boundary_edges == 0 && non_manifold_edges == 0;
    let euler_characteristic =
        mesh.vertices.len() as i64 - unique_edges as i64 + mesh.faces.len() as i64;

    let (valence_min, valence_max, valence_mean) = if vertex_degree.is_empty() {
        (0, 0, 0.0)
    } else {
        let vmin = *vertex_degree.iter().min().unwrap_or(&0);
        let vmax = *vertex_degree.iter().max().unwrap_or(&0);
        let sum: usize = vertex_degree.iter().sum();
        (vmin, vmax, round3(sum as f64 / vertex_degree.len() as f64))
    };

    // First pass: per-face geometry metrics
    let mut face_areas = Vec::with_capacity(mesh.faces.len());
    let mut raw_metrics = Vec::with_capacity(mesh.faces.len());
    let mut triangle_count = 0usize;
    let mut quad_count = 0usize;

    struct PartAcc {
        faces: usize,
        tris: usize,
        quads: usize,
        min: [f64; 3],
        max: [f64; 3],
    }

    let mut part_acc: BTreeMap<String, PartAcc> = BTreeMap::new();

    for (fi, face) in mesh.faces.iter().enumerate() {
        let n = face.verts.len();
        if n == 3 {
            triangle_count += 1;
        } else {
            quad_count += 1;
        }
        let pts: Vec<[f64; 3]> = face.verts.iter().map(|&idx| mesh.vertices[idx]).collect();

        let mut fmin = [f64::INFINITY; 3];
        let mut fmax = [f64::NEG_INFINITY; 3];
        for p in &pts {
            for k in 0..3 {
                fmin[k] = fmin[k].min(p[k]);
                fmax[k] = fmax[k].max(p[k]);
            }
        }
        let fbbox = BBox3D {
            min: [round3(fmin[0]), round3(fmin[1]), round3(fmin[2])],
            max: [round3(fmax[0]), round3(fmax[1]), round3(fmax[2])],
        };

        let entry = part_acc.entry(face.part.clone()).or_insert(PartAcc {
            faces: 0,
            tris: 0,
            quads: 0,
            min: [f64::INFINITY; 3],
            max: [f64::NEG_INFINITY; 3],
        });
        entry.faces += 1;
        if n == 3 {
            entry.tris += 1;
        } else {
            entry.quads += 1;
        }
        for k in 0..3 {
            entry.min[k] = entry.min[k].min(fmin[k]);
            entry.max[k] = entry.max[k].max(fmax[k]);
        }

        let mut repeated = false;
        for a in 0..n {
            for b in (a + 1)..n {
                if face.verts[a] == face.verts[b] {
                    repeated = true;
                }
            }
        }

        let mut min_angle = f64::INFINITY;
        let mut max_angle = 0.0f64;
        let mut min_edge = f64::INFINITY;
        let mut max_edge = 0.0f64;
        let mut edge_lens = Vec::with_capacity(n);

        for k in 0..n {
            let prev = pts[(k + n - 1) % n];
            let cur = pts[k];
            let next = pts[(k + 1) % n];
            let u = sub3(prev, cur);
            let v = sub3(next, cur);
            let lu = norm3(u);
            let lv = norm3(v);
            edge_lens.push(lv);
            min_edge = min_edge.min(lv);
            max_edge = max_edge.max(lv);
            if lu > 1e-12 && lv > 1e-12 {
                let ang = (dot3(u, v) / (lu * lv))
                    .clamp(-1.0, 1.0)
                    .acos()
                    .to_degrees();
                min_angle = min_angle.min(ang);
                max_angle = max_angle.max(ang);
            } else {
                min_angle = 0.0;
                max_angle = 180.0;
            }
        }

        let area = if n == 3 {
            tri_area3(pts[0], pts[1], pts[2])
        } else {
            tri_area3(pts[0], pts[1], pts[2]) + tri_area3(pts[0], pts[2], pts[3])
        };

        let s_xy = signed_area_xy(&pts);
        let clockwise_xy = s_xy < -1e-12;

        // Convexity check for quads using 2D/3D corner cross products
        let nonconvex = if n == 4 {
            let mut signs = Vec::with_capacity(4);
            let n0 = cross3(sub3(pts[1], pts[0]), sub3(pts[3], pts[0]));
            for k in 0..4 {
                let u = sub3(pts[(k + 1) % 4], pts[k]);
                let v = sub3(pts[(k + 3) % 4], pts[k]);
                signs.push(dot3(cross3(u, v), n0));
            }
            signs.iter().any(|&s| s <= 1e-12)
        } else {
            false
        };

        let degenerate = repeated || area <= 1e-12 || min_edge <= 1e-12;
        let quad_aspect = if n == 4 {
            let a = (edge_lens[0] + edge_lens[2]) * 0.5;
            let b = (edge_lens[1] + edge_lens[3]) * 0.5;
            let denom = a.min(b);
            if denom > 1e-12 {
                Some(round3(a.max(b) / denom))
            } else {
                None
            }
        } else {
            None
        };

        face_areas.push(area);
        raw_metrics.push((
            fi,
            face.id,
            if n == 3 { "triangle" } else { "quad" },
            face.part.clone(),
            if min_angle.is_finite() { round3(min_angle) } else { 0.0 },
            round3(max_angle),
            if min_edge.is_finite() { round4(min_edge) } else { 0.0 },
            round4(max_edge),
            round4(area),
            quad_aspect,
            degenerate,
            nonconvex,
            clockwise_xy,
            face_non_manifold[fi],
            fbbox,
        ));
    }

    // Second pass: neighbor area ratios across shared edges
    let mut worst_ratio_per_face: Vec<Option<f64>> = vec![None; mesh.faces.len()];
    for owners in edge_faces.values() {
        if owners.len() == 2 {
            let a = owners[0];
            let b = owners[1];
            let area_a = face_areas[a];
            let area_b = face_areas[b];
            if area_a > 1e-12 && area_b > 1e-12 {
                let ratio = area_a.max(area_b) / area_a.min(area_b);
                worst_ratio_per_face[a] = Some(worst_ratio_per_face[a].map_or(ratio, |v| v.max(ratio)));
                worst_ratio_per_face[b] = Some(worst_ratio_per_face[b].map_or(ratio, |v| v.max(ratio)));
            }
        }
    }

    let mut q_min_angle = f64::INFINITY;
    let mut q_max_angle = 0.0f64;
    let mut q_min_edge = f64::INFINITY;
    let mut q_max_edge = 0.0f64;
    let mut total_area = 0.0f64;
    let mut degenerate_count = 0usize;
    let mut nonconvex_count = 0usize;
    let mut clockwise_xy_count = 0usize;
    let mut non_manifold_face_count = 0usize;
    let mut global_worst_ratio: Option<f64> = None;

    let mut face_qualities = Vec::with_capacity(raw_metrics.len());
    for (
        fi,
        face_id,
        kind,
        part,
        min_angle_deg,
        max_angle_deg,
        min_edge,
        max_edge,
        area,
        quad_aspect,
        degenerate,
        nonconvex,
        clockwise_xy,
        non_manifold,
        fbbox,
    ) in raw_metrics
    {
        q_min_angle = q_min_angle.min(min_angle_deg);
        q_max_angle = q_max_angle.max(max_angle_deg);
        q_min_edge = q_min_edge.min(min_edge);
        q_max_edge = q_max_edge.max(max_edge);
        total_area += face_areas[fi];
        if degenerate {
            degenerate_count += 1;
        }
        if nonconvex {
            nonconvex_count += 1;
        }
        if clockwise_xy {
            clockwise_xy_count += 1;
        }
        if non_manifold {
            non_manifold_face_count += 1;
        }
        let ratio = worst_ratio_per_face[fi].map(round3);
        if let Some(r) = ratio {
            global_worst_ratio = Some(global_worst_ratio.map_or(r, |v| v.max(r)));
        }
        face_qualities.push(FaceQuality {
            face_id,
            kind,
            part,
            min_angle_deg,
            max_angle_deg,
            min_edge,
            max_edge,
            area,
            quad_aspect,
            worst_neighbor_area_ratio: ratio,
            degenerate,
            nonconvex,
            clockwise_xy,
            non_manifold,
            bbox: fbbox,
        });
    }

    let mut parts: Vec<PartSummary> = part_acc
        .into_iter()
        .map(|(part, acc)| PartSummary {
            part,
            face_count: acc.faces,
            triangle_count: acc.tris,
            quad_count: acc.quads,
            bbox: acc.min[0].is_finite().then_some(BBox3D {
                min: [round3(acc.min[0]), round3(acc.min[1]), round3(acc.min[2])],
                max: [round3(acc.max[0]), round3(acc.max[1]), round3(acc.max[2])],
            }),
        })
        .collect();
    parts.sort_by(|a, b| b.face_count.cmp(&a.face_count).then_with(|| a.part.cmp(&b.part)));

    let quality = QualitySummary {
        min_angle_deg: (!mesh.faces.is_empty()).then_some(round3(q_min_angle)),
        max_angle_deg: (!mesh.faces.is_empty()).then_some(round3(q_max_angle)),
        min_edge: (!mesh.faces.is_empty()).then_some(round4(q_min_edge)),
        max_edge: (!mesh.faces.is_empty()).then_some(round4(q_max_edge)),
        total_area: round4(total_area),
        degenerate_count,
        nonconvex_count,
        clockwise_xy_count,
        non_manifold_face_count,
        worst_neighbor_area_ratio: global_worst_ratio,
    };

    MeshAnalysis {
        format: mesh.format,
        vertex_count: mesh.vertices.len(),
        face_count: mesh.faces.len(),
        triangle_count,
        quad_count,
        bbox,
        topology: TopologySummary {
            unique_edges,
            boundary_edges,
            interior_edges,
            non_manifold_edges,
            watertight,
            euler_characteristic,
            valence_min,
            valence_max,
            valence_mean,
        },
        quality,
        parts,
        face_qualities,
    }
}

/// Slice a mesh by 2D XY bbox `[min_x, min_y, max_x, max_y]`, compacting
/// referenced vertices and writing a new `.2dm` or `.obj` file.
pub fn write_bbox_slice(
    mesh: &Mesh,
    bbox: [f64; 4],
    output_path: &Path,
) -> Result<(usize, usize), ToolError> {
    let mut kept_faces = Vec::new();
    let mut old_to_new: HashMap<usize, usize> = HashMap::new();
    let mut new_verts: Vec<[f64; 3]> = Vec::new();

    for face in &mesh.faces {
        let mut fmin_x = f64::INFINITY;
        let mut fmin_y = f64::INFINITY;
        let mut fmax_x = f64::NEG_INFINITY;
        let mut fmax_y = f64::NEG_INFINITY;
        for &vi in &face.verts {
            let v = mesh.vertices[vi];
            fmin_x = fmin_x.min(v[0]);
            fmin_y = fmin_y.min(v[1]);
            fmax_x = fmax_x.max(v[0]);
            fmax_y = fmax_y.max(v[1]);
        }
        if fmax_x < bbox[0] || fmin_x > bbox[2] || fmax_y < bbox[1] || fmin_y > bbox[3] {
            continue;
        }
        let mut mapped = Vec::with_capacity(face.verts.len());
        for &vi in &face.verts {
            let new_idx = *old_to_new.entry(vi).or_insert_with(|| {
                let idx = new_verts.len();
                new_verts.push(mesh.vertices[vi]);
                idx
            });
            mapped.push(new_idx);
        }
        kept_faces.push((face.id, mapped, face.part.clone()));
    }

    let ext = output_path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();

    let mut out = String::new();
    if ext == "2dm" {
        out.push_str("MESH2D\n");
        for (i, v) in new_verts.iter().enumerate() {
            out.push_str(&format!("ND {} {} {} {}\n", i + 1, v[0], v[1], v[2]));
        }
        for (idx, (_, verts, part)) in kept_faces.iter().enumerate() {
            let mat: i64 = part
                .strip_prefix("material_")
                .and_then(|s| s.parse().ok())
                .unwrap_or(1);
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
    } else {
        out.push_str("# Sliced by spatial-cad-mcp (mcp-mesh)\n");
        for v in &new_verts {
            out.push_str(&format!("v {} {} {}\n", v[0], v[1], v[2]));
        }
        let mut last_part = String::new();
        for (_, verts, part) in &kept_faces {
            if *part != last_part {
                out.push_str(&format!("g {part}\n"));
                last_part = part.clone();
            }
            out.push('f');
            for &vi in verts {
                out.push_str(&format!(" {}", vi + 1));
            }
            out.push('\n');
        }
    }

    std::fs::write(output_path, out).map_err(|e| {
        ToolError::new("internal_error", format!("Failed writing sliced mesh: {e}"))
    })?;
    Ok((new_verts.len(), kept_faces.len()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn analyzes_2dm_with_triangles_and_quads() {
        let txt = "\
MESH2D
ND 1 0.0 0.0 10.0
ND 2 2.0 0.0 10.0
ND 3 2.0 2.0 10.0
ND 4 0.0 2.0 10.0
ND 5 4.0 1.0 10.0
E4Q 1 1 2 3 4 10
E3T 2 2 5 3 20
";
        let mesh = parse_2dm(txt).unwrap();
        let a = analyze_mesh(&mesh);
        assert_eq!(a.format, MeshFormat::Sms2dm);
        assert_eq!(a.vertex_count, 5);
        assert_eq!(a.triangle_count, 1);
        assert_eq!(a.quad_count, 1);
        assert_eq!(a.topology.interior_edges, 1);
        assert_eq!(a.topology.boundary_edges, 5);
        assert_eq!(a.topology.non_manifold_edges, 0);
        assert_eq!(a.quality.degenerate_count, 0);
        assert_eq!(a.quality.nonconvex_count, 0);
        assert_eq!(a.parts.len(), 2);
    }

    #[test]
    fn watertight_obj_cube_has_euler_2() {
        let obj = "\
o unit_cube
v 0 0 0
v 1 0 0
v 1 1 0
v 0 1 0
v 0 0 1
v 1 0 1
v 1 1 1
v 0 1 1
f 1 4 3 2
f 5 6 7 8
f 1 2 6 5
f 2 3 7 6
f 3 4 8 7
f 4 1 5 8
";
        let mesh = parse_obj(obj).unwrap();
        let a = analyze_mesh(&mesh);
        assert!(a.topology.watertight);
        assert_eq!(a.topology.boundary_edges, 0);
        assert_eq!(a.topology.non_manifold_edges, 0);
        assert_eq!(a.topology.euler_characteristic, 2);
        assert_eq!(a.quality.total_area, 6.0);
    }

    #[test]
    fn parses_ascii_and_binary_stl() {
        let ascii = "\
solid tri
  facet normal 0 0 1
    outer loop
      vertex 0 0 0
      vertex 1 0 0
      vertex 0 1 0
    endloop
  endfacet
endsolid tri
";
        let m1 = parse_stl(ascii.as_bytes()).unwrap();
        assert_eq!(m1.format, MeshFormat::StlAscii);
        assert_eq!(m1.faces.len(), 1);

        // Minimal 1-triangle binary STL (84 + 50 = 134 bytes)
        let mut bin = vec![0u8; 134];
        bin[80..84].copy_from_slice(&1u32.to_le_bytes());
        bin[84 + 24..84 + 28].copy_from_slice(&1.0f32.to_le_bytes());
        bin[84 + 40..84 + 44].copy_from_slice(&1.0f32.to_le_bytes());
        let m2 = parse_stl(&bin).unwrap();
        assert_eq!(m2.format, MeshFormat::StlBinary);
        assert_eq!(m2.faces.len(), 1);
    }
}
