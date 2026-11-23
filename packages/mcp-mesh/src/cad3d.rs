//! General-purpose 3D CAD surface geometry operations on `.2dm`/`.obj`/`.stl` meshes:
//! - `mesh_cut_plane`: exact mesh/plane intersection -> chained 3D DXF polylines (contours / arbitrary planes)
//! - `mesh_volumes_intersect`: exact cut/fill volume & zero-line overlay between two surfaces (no rasterization)
//! - `mesh_to_dxf`: export mesh faces (`3DFACE` per part/material) + 3D boundary polylines to DXF

use crate::mesh::Mesh;
use cad_envelope::ToolError;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;

fn round3(v: f64) -> f64 {
    (v * 1000.0).round() / 1000.0
}

fn round4(v: f64) -> f64 {
    (v * 10_000.0).round() / 10_000.0
}

/// Write 3D polylines as a clean R12 (`AC1009`) ASCII DXF file.
fn write_dxf_polylines_3d(
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
        ToolError::new("internal_error", format!("Failed writing DXF output: {e}"))
    })
}

/// Chain undirected segments `(a, b)` into deterministic open paths and closed loops.
fn chain_segments<K: Copy + Ord + std::hash::Hash>(segments: &[(K, K)]) -> Vec<Vec<K>> {
    let mut adj: BTreeMap<K, Vec<usize>> = BTreeMap::new();
    let mut seen: BTreeSet<(K, K)> = BTreeSet::new();
    let mut edges: Vec<(K, K)> = Vec::new();
    for &(a, b) in segments {
        if a == b {
            continue;
        }
        let key = if a <= b { (a, b) } else { (b, a) };
        if !seen.insert(key) {
            continue;
        }
        let idx = edges.len();
        edges.push((a, b));
        adj.entry(a).or_default().push(idx);
        adj.entry(b).or_default().push(idx);
    }

    let mut used = vec![false; edges.len()];
    let mut chains = Vec::new();

    let walk = |start: K, first_e: usize, used: &mut [bool]| -> Vec<K> {
        let mut chain = vec![start];
        let mut cur = start;
        let mut eidx = Some(first_e);
        while let Some(ei) = eidx {
            used[ei] = true;
            let (a, b) = edges[ei];
            let nxt = if a == cur { b } else { a };
            chain.push(nxt);
            cur = nxt;
            let deg = adj.get(&cur).map_or(0, Vec::len);
            if deg != 2 {
                break;
            }
            eidx = adj[&cur].iter().copied().find(|&e| !used[e]);
        }
        chain
    };

    // Open chains start at degree != 2
    let keys: Vec<K> = adj.keys().copied().collect();
    for v in keys {
        if adj[&v].len() != 2 {
            let es = adj[&v].clone();
            for e in es {
                if !used[e] {
                    chains.push(walk(v, e, &mut used));
                }
            }
        }
    }
    // Remaining are closed rings
    for (e, &(a, _)) in edges.iter().enumerate() {
        if !used[e] {
            chains.push(walk(a, e, &mut used));
        }
    }
    chains
}

/// Triangulate all faces of a mesh (quads split along diagonal 0-2).
fn mesh_triangles(mesh: &Mesh) -> Vec<[usize; 3]> {
    let mut tris = Vec::with_capacity(mesh.faces.len());
    for f in &mesh.faces {
        if f.verts.len() == 3 {
            tris.push([f.verts[0], f.verts[1], f.verts[2]]);
        } else if f.verts.len() >= 4 {
            for k in 1..f.verts.len() - 1 {
                tris.push([f.verts[0], f.verts[k], f.verts[k + 1]]);
            }
        }
    }
    tris
}

// ---------- mesh_cut_plane ----------

#[derive(Debug, Clone, Copy)]
pub struct Plane3D {
    pub origin: [f64; 3],
    pub normal: [f64; 3],
}

impl Plane3D {
    pub fn horizontal(z: f64) -> Self {
        Self {
            origin: [0.0, 0.0, z],
            normal: [0.0, 0.0, 1.0],
        }
    }

    pub fn from_point_normal(origin: [f64; 3], normal: [f64; 3]) -> Result<Self, ToolError> {
        let len = (normal[0] * normal[0] + normal[1] * normal[1] + normal[2] * normal[2]).sqrt();
        if !len.is_finite() || len <= 1e-12 {
            return Err(ToolError::new(
                "invalid_input",
                "Plane normal vector must be finite and non-zero.",
            ));
        }
        Ok(Self {
            origin,
            normal: [normal[0] / len, normal[1] / len, normal[2] / len],
        })
    }

    pub fn from_three_points(p1: [f64; 3], p2: [f64; 3], p3: [f64; 3]) -> Result<Self, ToolError> {
        let u = [p2[0] - p1[0], p2[1] - p1[1], p2[2] - p1[2]];
        let v = [p3[0] - p1[0], p3[1] - p1[1], p3[2] - p1[2]];
        let n = [
            u[1] * v[2] - u[2] * v[1],
            u[2] * v[0] - u[0] * v[2],
            u[0] * v[1] - u[1] * v[0],
        ];
        let scale = (u[0] * u[0] + u[1] * u[1] + u[2] * u[2])
            .max(v[0] * v[0] + v[1] * v[1] + v[2] * v[2]);
        let nlen = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
        if scale <= 1e-18 || nlen <= 1e-9 * scale {
            return Err(ToolError::new(
                "invalid_input",
                "The three plane points are collinear (or nearly collinear).",
            ));
        }
        Self::from_point_normal(p1, n)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct CutPlaneReport {
    pub polyline_count: usize,
    pub closed_count: usize,
    pub total_length_3d: f64,
    pub triangles_cut: usize,
    pub coplanar_triangles: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum CutKey {
    Vert(usize),
    Edge(usize, usize),
}

/// Cut a surface mesh with an arbitrary 3D plane and write chained 3D polylines
/// to `output_dxf`.
pub fn cut_mesh_with_plane(
    mesh: &Mesh,
    plane: Plane3D,
    layer_name: &str,
    output_dxf: &Path,
) -> Result<CutPlaneReport, ToolError> {
    if mesh.vertices.is_empty() {
        return Err(ToolError::new("invalid_input", "Mesh has no vertices."));
    }
    let origin = mesh.vertices[0];
    let local_plane_origin = [
        plane.origin[0] - origin[0],
        plane.origin[1] - origin[1],
        plane.origin[2] - origin[2],
    ];
    let tol = 1e-7;
    let dists: Vec<f64> = mesh
        .vertices
        .iter()
        .map(|v| {
            let d = (v[0] - origin[0] - local_plane_origin[0]) * plane.normal[0]
                + (v[1] - origin[1] - local_plane_origin[1]) * plane.normal[1]
                + (v[2] - origin[2] - local_plane_origin[2]) * plane.normal[2];
            if d.abs() <= tol { 0.0 } else { d }
        })
        .collect();
    let signs: Vec<i8> = dists
        .iter()
        .map(|&d| if d > 0.0 { 1 } else if d < 0.0 { -1 } else { 0 })
        .collect();

    let tris = mesh_triangles(mesh);
    let mut points: HashMap<CutKey, [f64; 3]> = HashMap::new();
    let mut segments: Vec<(CutKey, CutKey)> = Vec::new();
    let mut coplanar = 0usize;

    for t in tris {
        let s = [signs[t[0]], signs[t[1]], signs[t[2]]];
        if (s[0] > 0 && s[1] > 0 && s[2] > 0) || (s[0] < 0 && s[1] < 0 && s[2] < 0) {
            continue;
        }
        if s[0] == 0 && s[1] == 0 && s[2] == 0 {
            coplanar += 1;
            continue;
        }
        let mut keys: Vec<CutKey> = Vec::with_capacity(3);
        for i in 0..3 {
            let a = t[i];
            let b = t[(i + 1) % 3];
            if s[i] == 0 {
                let k = CutKey::Vert(a);
                points.entry(k).or_insert_with(|| mesh.vertices[a]);
                if !keys.contains(&k) {
                    keys.push(k);
                }
            }
            if s[i] * s[(i + 1) % 3] < 0 {
                let k = if a < b {
                    CutKey::Edge(a, b)
                } else {
                    CutKey::Edge(b, a)
                };
                points.entry(k).or_insert_with(|| {
                    let frac = dists[a] / (dists[a] - dists[b]);
                    let va = mesh.vertices[a];
                    let vb = mesh.vertices[b];
                    [
                        va[0] + frac * (vb[0] - va[0]),
                        va[1] + frac * (vb[1] - va[1]),
                        va[2] + frac * (vb[2] - va[2]),
                    ]
                });
                if !keys.contains(&k) {
                    keys.push(k);
                }
            }
        }
        if keys.len() == 2 {
            segments.push((keys[0], keys[1]));
        }
    }

    let chains = chain_segments(&segments);
    let mut polylines: Vec<(Vec<[f64; 3]>, bool)> = Vec::with_capacity(chains.len());
    let mut total_len = 0.0;
    let mut closed_count = 0usize;

    for ch in chains {
        let closed = ch.len() > 2 && ch.first() == ch.last();
        let mut pts: Vec<[f64; 3]> = ch.iter().map(|k| points[k]).collect();
        for w in pts.windows(2) {
            let dx = w[1][0] - w[0][0];
            let dy = w[1][1] - w[0][1];
            let dz = w[1][2] - w[0][2];
            total_len += (dx * dx + dy * dy + dz * dz).sqrt();
        }
        if closed {
            pts.pop(); // DXF closed flag closes last -> first automatically
            closed_count += 1;
        }
        polylines.push((pts, closed));
    }

    let refs: Vec<(&str, &[[f64; 3]], bool)> = polylines
        .iter()
        .map(|(pts, closed)| (layer_name, pts.as_slice(), *closed))
        .collect();
    write_dxf_polylines_3d(&refs, output_dxf)?;

    Ok(CutPlaneReport {
        polyline_count: polylines.len(),
        closed_count,
        total_length_3d: round3(total_len),
        triangles_cut: segments.len(),
        coplanar_triangles: coplanar,
    })
}

// ---------- mesh_volumes_intersect ----------

#[derive(Debug, Clone, Serialize)]
pub struct VolumesIntersectReport {
    pub common_area: f64,
    pub fill_area: f64,
    pub cut_area: f64,
    pub coplanar_area: f64,
    pub fill_volume: f64,
    pub cut_volume: f64,
    pub net_volume: f64,
    pub dz_min: Option<f64>,
    pub dz_max: Option<f64>,
    pub zero_line_count: usize,
    pub zero_line_length_2d: f64,
    pub overlap_pieces: usize,
}

struct Tri25D {
    xy: [[f64; 2]; 3], // CCW in local XY
    aabb: [f64; 4],
    plane: [f64; 3], // z = plane[0]*x + plane[1]*y + plane[2]
}

fn build_tri25d(mesh: &Mesh, origin: [f64; 3]) -> Vec<Tri25D> {
    let tris = mesh_triangles(mesh);
    let mut out = Vec::with_capacity(tris.len());
    for t in tris {
        let mut p = [
            [
                mesh.vertices[t[0]][0] - origin[0],
                mesh.vertices[t[0]][1] - origin[1],
                mesh.vertices[t[0]][2] - origin[2],
            ],
            [
                mesh.vertices[t[1]][0] - origin[0],
                mesh.vertices[t[1]][1] - origin[1],
                mesh.vertices[t[1]][2] - origin[2],
            ],
            [
                mesh.vertices[t[2]][0] - origin[0],
                mesh.vertices[t[2]][1] - origin[1],
                mesh.vertices[t[2]][2] - origin[2],
            ],
        ];
        let cross = (p[1][0] - p[0][0]) * (p[2][1] - p[0][1])
            - (p[1][1] - p[0][1]) * (p[2][0] - p[0][0]);
        if cross.abs() < 1e-12 {
            continue;
        }
        if cross < 0.0 {
            p.swap(1, 2);
        }
        let ux = p[1][0] - p[0][0];
        let uy = p[1][1] - p[0][1];
        let uz = p[1][2] - p[0][2];
        let vx = p[2][0] - p[0][0];
        let vy = p[2][1] - p[0][1];
        let vz = p[2][2] - p[0][2];
        let det = ux * vy - uy * vx;
        let a = (uz * vy - uy * vz) / det;
        let b = (ux * vz - uz * vx) / det;
        let c = p[0][2] - a * p[0][0] - b * p[0][1];
        let min_x = p[0][0].min(p[1][0]).min(p[2][0]);
        let min_y = p[0][1].min(p[1][1]).min(p[2][1]);
        let max_x = p[0][0].max(p[1][0]).max(p[2][0]);
        let max_y = p[0][1].max(p[1][1]).max(p[2][1]);
        out.push(Tri25D {
            xy: [[p[0][0], p[0][1]], [p[1][0], p[1][1]], [p[2][0], p[2][1]]],
            aabb: [min_x, min_y, max_x, max_y],
            plane: [a, b, c],
        });
    }
    out
}

/// Sutherland-Hodgman clip of a convex 2D polygon against a CCW half-plane (edge a -> b).
fn clip_poly_edge(poly: &[[f64; 2]], a: [f64; 2], b: [f64; 2]) -> Vec<[f64; 2]> {
    let mut out = Vec::new();
    let n = poly.len();
    if n == 0 {
        return out;
    }
    let ex = b[0] - a[0];
    let ey = b[1] - a[1];
    let side = |p: [f64; 2]| -> f64 { ex * (p[1] - a[1]) - ey * (p[0] - a[0]) };

    for i in 0..n {
        let cur = poly[i];
        let nxt = poly[(i + 1) % n];
        let sc = side(cur);
        let sn = side(nxt);
        let in_c = sc >= -1e-12;
        let in_n = sn >= -1e-12;
        if in_c {
            out.push(cur);
        }
        if in_c != in_n {
            let denom = sc - sn;
            if denom.abs() > 1e-18 {
                let t = (sc / denom).clamp(0.0, 1.0);
                out.push([cur[0] + t * (nxt[0] - cur[0]), cur[1] + t * (nxt[1] - cur[1])]);
            }
        }
    }
    out
}

fn clip_convex_by_val(poly: &[[f64; 2]], vals: &[f64]) -> Vec<[f64; 2]> {
    let mut out = Vec::new();
    let n = poly.len();
    for i in 0..n {
        let j = (i + 1) % n;
        if vals[i] >= 0.0 {
            out.push(poly[i]);
        }
        if (vals[i] >= 0.0) != (vals[j] >= 0.0) {
            let t = vals[i] / (vals[i] - vals[j]);
            out.push([
                poly[i][0] + t * (poly[j][0] - poly[i][0]),
                poly[i][1] + t * (poly[j][1] - poly[i][1]),
            ]);
        }
    }
    out
}

fn poly_area_and_centroid(poly: &[[f64; 2]]) -> (f64, [f64; 2]) {
    let n = poly.len();
    if n < 3 {
        return (0.0, [0.0, 0.0]);
    }
    let mut a2 = 0.0;
    let mut cx = 0.0;
    let mut cy = 0.0;
    for i in 0..n {
        let p = poly[i];
        let q = poly[(i + 1) % n];
        let cross = p[0] * q[1] - q[0] * p[1];
        a2 += cross;
        cx += (p[0] + q[0]) * cross;
        cy += (p[1] + q[1]) * cross;
    }
    let area = 0.5 * a2.abs();
    if area <= 1e-14 {
        return (0.0, [0.0, 0.0]);
    }
    (area, [cx / (3.0 * a2), cy / (3.0 * a2)])
}

/// Exact non-rasterized overlay of two 2.5D surfaces (A = existing, B = design):
/// computes exact fill/cut volumes, areas, dz range, and zero-lines (zB = zA).
pub fn overlay_volumes_and_intersect(
    surf_a: &Mesh,
    surf_b: &Mesh,
    height_tol: f64,
    output_dxf: Option<&Path>,
) -> Result<VolumesIntersectReport, ToolError> {
    if surf_a.vertices.is_empty() || surf_b.vertices.is_empty() {
        return Err(ToolError::new(
            "invalid_input",
            "Both surfaces must contain vertices.",
        ));
    }
    let origin = surf_a.vertices[0];
    let tris_a = build_tri25d(surf_a, origin);
    let tris_b = build_tri25d(surf_b, origin);

    let mut common_area = 0.0;
    let mut fill_area = 0.0;
    let mut cut_area = 0.0;
    let mut coplanar_area = 0.0;
    let mut fill_vol = 0.0;
    let mut cut_vol = 0.0;
    let mut dz_min = f64::INFINITY;
    let mut dz_max = f64::NEG_INFINITY;
    let mut overlap_pieces = 0usize;

    // Weld zero-line endpoints at 1e-5 m in local XY
    let mut zpt_map: HashMap<[i64; 2], usize> = HashMap::new();
    let mut zpt_sum: Vec<[f64; 3]> = Vec::new();
    let mut zpt_cnt: Vec<usize> = Vec::new();
    let mut zero_segs: Vec<(usize, usize)> = Vec::new();

    let mut intern_zpt = |pt: [f64; 3]| -> usize {
        let key = [(pt[0] * 1e5).round() as i64, (pt[1] * 1e5).round() as i64];
        let idx = *zpt_map.entry(key).or_insert_with(|| {
            let id = zpt_sum.len();
            zpt_sum.push([0.0; 3]);
            zpt_cnt.push(0);
            id
        });
        zpt_sum[idx][0] += pt[0];
        zpt_sum[idx][1] += pt[1];
        zpt_sum[idx][2] += pt[2];
        zpt_cnt[idx] += 1;
        idx
    };

    for ta in &tris_a {
        for tb in &tris_b {
            if ta.aabb[2] < tb.aabb[0]
                || ta.aabb[0] > tb.aabb[2]
                || ta.aabb[3] < tb.aabb[1]
                || ta.aabb[1] > tb.aabb[3]
            {
                continue;
            }
            // Exact triangle-triangle intersection in 2D via Sutherland-Hodgman
            let mut poly = ta.xy.to_vec();
            for e in 0..3 {
                poly = clip_poly_edge(&poly, tb.xy[e], tb.xy[(e + 1) % 3]);
                if poly.len() < 3 {
                    break;
                }
            }
            if poly.len() < 3 {
                continue;
            }
            let (area, centroid) = poly_area_and_centroid(&poly);
            if area <= 1e-10 {
                continue;
            }
            overlap_pieces += 1;
            common_area += area;

            let coef = [
                tb.plane[0] - ta.plane[0],
                tb.plane[1] - ta.plane[1],
                tb.plane[2] - ta.plane[2],
            ];
            let vals: Vec<f64> = poly
                .iter()
                .map(|p| {
                    let v = coef[0] * p[0] + coef[1] * p[1] + coef[2];
                    if v.abs() <= 1e-6 { 0.0 } else { v }
                })
                .collect();
            let vmin = vals.iter().copied().fold(f64::INFINITY, f64::min);
            let vmax = vals.iter().copied().fold(f64::NEG_INFINITY, f64::max);
            dz_min = dz_min.min(vmin);
            dz_max = dz_max.max(vmax);

            let eff_tol = height_tol.max(1e-6);
            if (-vmin).max(vmax) <= eff_tol {
                coplanar_area += area;
                continue;
            }

            if vmin >= 0.0 {
                let dc = coef[0] * centroid[0] + coef[1] * centroid[1] + coef[2];
                fill_area += area;
                fill_vol += area * dc;
            } else if vmax <= 0.0 {
                let dc = coef[0] * centroid[0] + coef[1] * centroid[1] + coef[2];
                cut_area += area;
                cut_vol -= area * dc;
            } else {
                // Mixed sign: clip along d = 0 for exact fill & cut sub-polygons
                let pos_poly = clip_convex_by_val(&poly, &vals);
                let neg_vals: Vec<f64> = vals.iter().map(|v| -v).collect();
                let neg_poly = clip_convex_by_val(&poly, &neg_vals);
                let (ap, cp) = poly_area_and_centroid(&pos_poly);
                if ap > 1e-12 {
                    fill_area += ap;
                    fill_vol += ap * (coef[0] * cp[0] + coef[1] * cp[1] + coef[2]);
                }
                let (an, cn) = poly_area_and_centroid(&neg_poly);
                if an > 1e-12 {
                    cut_area += an;
                    cut_vol -= an * (coef[0] * cn[0] + coef[1] * cn[1] + coef[2]);
                }
            }

            // Extract zero-line segment if d = 0 crosses or touches >= 2 vertices
            let n_zero = vals.iter().filter(|&&v| v == 0.0).count();
            let mixed = vmin < 0.0 && vmax > 0.0;
            if mixed || n_zero >= 2 {
                let mut zpts: Vec<[f64; 2]> = Vec::new();
                for i in 0..poly.len() {
                    let j = (i + 1) % poly.len();
                    if vals[i] == 0.0 {
                        zpts.push(poly[i]);
                    } else if vals[j] != 0.0 && (vals[i] > 0.0) != (vals[j] > 0.0) {
                        let t = vals[i] / (vals[i] - vals[j]);
                        zpts.push([
                            poly[i][0] + t * (poly[j][0] - poly[i][0]),
                            poly[i][1] + t * (poly[j][1] - poly[i][1]),
                        ]);
                    }
                }
                if zpts.len() >= 2 {
                    let p0 = zpts[0];
                    let mut far_idx = 1;
                    let mut far_d = 0.0;
                    for (idx, q) in zpts.iter().enumerate().skip(1) {
                        let d = (q[0] - p0[0]).hypot(q[1] - p0[1]);
                        if d > far_d {
                            far_d = d;
                            far_idx = idx;
                        }
                    }
                    if far_d > 1e-6 {
                        let q0 = p0;
                        let q1 = zpts[far_idx];
                        let za0 = ta.plane[0] * q0[0] + ta.plane[1] * q0[1] + ta.plane[2];
                        let za1 = ta.plane[0] * q1[0] + ta.plane[1] * q1[1] + ta.plane[2];
                        let i0 = intern_zpt([q0[0], q0[1], za0]);
                        let i1 = intern_zpt([q1[0], q1[1], za1]);
                        if i0 != i1 {
                            zero_segs.push((i0, i1));
                        }
                    }
                }
            }
            // Keep vals owned to avoid unused warning if simplified
            let _ = vals.len();
        }
    }

    let zpts_world: Vec<[f64; 3]> = zpt_sum
        .iter()
        .zip(&zpt_cnt)
        .map(|(s, &c)| {
            [
                s[0] / c as f64 + origin[0],
                s[1] / c as f64 + origin[1],
                s[2] / c as f64 + origin[2],
            ]
        })
        .collect();
    let chains = chain_segments(&zero_segs);
    let mut zero_lines: Vec<(Vec<[f64; 3]>, bool)> = Vec::with_capacity(chains.len());
    let mut zero_len_2d = 0.0;
    for ch in chains {
        let closed = ch.len() > 2 && ch.first() == ch.last();
        let mut pts: Vec<[f64; 3]> = ch.iter().map(|&i| zpts_world[i]).collect();
        for w in pts.windows(2) {
            zero_len_2d += (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1]);
        }
        if closed {
            pts.pop();
        }
        zero_lines.push((pts, closed));
    }

    if let Some(out_path) = output_dxf {
        let refs: Vec<(&str, &[[f64; 3]], bool)> = zero_lines
            .iter()
            .map(|(pts, closed)| ("ZERO_LINE", pts.as_slice(), *closed))
            .collect();
        write_dxf_polylines_3d(&refs, out_path)?;
    }

    Ok(VolumesIntersectReport {
        common_area: round3(common_area),
        fill_area: round3(fill_area),
        cut_area: round3(cut_area),
        coplanar_area: round3(coplanar_area),
        fill_volume: round3(fill_vol),
        cut_volume: round3(cut_vol),
        net_volume: round3(fill_vol - cut_vol),
        dz_min: dz_min.is_finite().then_some(round4(dz_min)),
        dz_max: dz_max.is_finite().then_some(round4(dz_max)),
        zero_line_count: zero_lines.len(),
        zero_line_length_2d: round3(zero_len_2d),
        overlap_pieces,
    })
}

// ---------- mesh_to_dxf ----------

#[derive(Debug, Clone, Serialize)]
pub struct MeshToDxfReport {
    pub face_count: usize,
    pub layer_count: usize,
    pub boundary_loops: usize,
    pub faces_per_layer: BTreeMap<String, usize>,
}

/// Export a mesh (`.2dm`/`.obj`/`.stl`) to an ASCII DXF file with `3DFACE`
/// entities (one layer per part/material) and optional 3D boundary polylines.
pub fn export_mesh_to_dxf(
    mesh: &Mesh,
    include_boundary: bool,
    output_dxf: &Path,
) -> Result<MeshToDxfReport, ToolError> {
    let mut layers: BTreeSet<String> = BTreeSet::new();
    let mut faces_per_layer: BTreeMap<String, usize> = BTreeMap::new();
    let mut min_pt = [f64::INFINITY; 3];
    let mut max_pt = [f64::NEG_INFINITY; 3];

    for v in &mesh.vertices {
        for k in 0..3 {
            min_pt[k] = min_pt[k].min(v[k]);
            max_pt[k] = max_pt[k].max(v[k]);
        }
    }

    let sanitize_layer = |raw: &str| -> String {
        let s: String = raw
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        if s.is_empty() { "0".to_string() } else { s }
    };

    for f in &mesh.faces {
        let lname = sanitize_layer(&f.part);
        layers.insert(lname.clone());
        *faces_per_layer.entry(lname).or_insert(0) += 1;
    }

    let boundary_chains = if include_boundary {
        let mut edge_counts: HashMap<(usize, usize), usize> = HashMap::new();
        for f in &mesh.faces {
            let nv = f.verts.len();
            for k in 0..nv {
                let a = f.verts[k];
                let b = f.verts[(k + 1) % nv];
                let key = if a <= b { (a, b) } else { (b, a) };
                *edge_counts.entry(key).or_insert(0) += 1;
            }
        }
        let b_segs: Vec<(usize, usize)> = edge_counts
            .into_iter()
            .filter_map(|(e, c)| (c == 1).then_some(e))
            .collect();
        let chains = chain_segments(&b_segs);
        if !chains.is_empty() {
            layers.insert("BOUNDARY".to_string());
        }
        chains
    } else {
        Vec::new()
    };

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

    let mut written_faces = 0usize;
    for f in &mesh.faces {
        if f.verts.len() < 3 {
            continue;
        }
        let lname = sanitize_layer(&f.part);
        let p0 = mesh.vertices[f.verts[0]];
        let p1 = mesh.vertices[f.verts[1]];
        let p2 = mesh.vertices[f.verts[2]];
        let p3 = if f.verts.len() >= 4 {
            mesh.vertices[f.verts[3]]
        } else {
            p2
        };
        out.push_str(&format!(
            "0\n3DFACE\n8\n{lname}\n\
             10\n{}\n20\n{}\n30\n{}\n\
             11\n{}\n21\n{}\n31\n{}\n\
             12\n{}\n22\n{}\n32\n{}\n\
             13\n{}\n23\n{}\n33\n{}\n",
            p0[0], p0[1], p0[2], p1[0], p1[1], p1[2], p2[0], p2[1], p2[2], p3[0], p3[1], p3[2]
        ));
        written_faces += 1;
    }

    for ch in &boundary_chains {
        let closed = ch.len() > 2 && ch.first() == ch.last();
        let slice = if closed { &ch[..ch.len() - 1] } else { ch.as_slice() };
        if slice.len() < 2 {
            continue;
        }
        let flag = 8 | if closed { 1 } else { 0 };
        out.push_str(&format!(
            "0\nPOLYLINE\n8\nBOUNDARY\n66\n1\n10\n0.0\n20\n0.0\n30\n0.0\n70\n{flag}\n"
        ));
        for &vi in slice {
            let p = mesh.vertices[vi];
            out.push_str(&format!(
                "0\nVERTEX\n8\nBOUNDARY\n10\n{}\n20\n{}\n30\n{}\n70\n32\n",
                p[0], p[1], p[2]
            ));
        }
        out.push_str("0\nSEQEND\n8\nBOUNDARY\n");
    }

    out.push_str("0\nENDSEC\n0\nEOF\n");
    std::fs::write(output_dxf, out).map_err(|e| {
        ToolError::new("internal_error", format!("Failed writing DXF output: {e}"))
    })?;

    Ok(MeshToDxfReport {
        face_count: written_faces,
        layer_count: layers.len(),
        boundary_loops: boundary_chains.len(),
        faces_per_layer,
    })
}
