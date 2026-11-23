//! Generic batch editor for ASCII DXF entities: select entities by layer,
//! type, hex handle range, or 2D bbox, apply an ordered list of ops, and write
//! a NEW DXF file. Inputs are never modified; untouched entities are copied
//! verbatim (lossless group-code stream).

use crate::parser::collect_pairs;
use cad_envelope::ToolError;
use rmcp::schemars;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::f64::consts::PI;
use std::path::Path;

/// Hard cap on how many entities one edit call may touch.
pub const MAX_EDIT_LIMIT: usize = 200;

#[derive(Debug, Clone, Default, Deserialize, schemars::JsonSchema)]
pub struct DxfSelect {
    /// Optional exact layer name filter (case-insensitive).
    #[serde(default)]
    pub layer: Option<String>,
    /// Optional DXF entity type filter (e.g. "LINE", "LWPOLYLINE", "3DFACE").
    #[serde(default)]
    pub entity_type: Option<String>,
    /// Optional inclusive lower hex handle bound (e.g. "1A").
    #[serde(default)]
    pub handle_from: Option<String>,
    /// Optional inclusive upper hex handle bound.
    #[serde(default)]
    pub handle_to: Option<String>,
    /// Optional 2D bbox [min_x, min_y, max_x, max_y]; entities whose bbox intersects it match.
    #[serde(default)]
    pub bbox: Option<[f64; 4]>,
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum DxfEditOp {
    SetLayer { layer: String },
    SetColor { color: i32 },
    SetLinetype { linetype: String },
    SetLineweight { lineweight: i32 },
    Move { dx: f64, dy: f64 },
    Rotate { cx: f64, cy: f64, angle_deg: f64 },
    Scale { cx: f64, cy: f64, factor: f64 },
    Delete,
}

impl DxfEditOp {
    pub fn name(&self) -> &'static str {
        match self {
            DxfEditOp::SetLayer { .. } => "set_layer",
            DxfEditOp::SetColor { .. } => "set_color",
            DxfEditOp::SetLinetype { .. } => "set_linetype",
            DxfEditOp::SetLineweight { .. } => "set_lineweight",
            DxfEditOp::Move { .. } => "move",
            DxfEditOp::Rotate { .. } => "rotate",
            DxfEditOp::Scale { .. } => "scale",
            DxfEditOp::Delete => "delete",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct DxfEditStats {
    pub selected: usize,
    pub edited: usize,
    pub deleted: usize,
    pub per_op: BTreeMap<String, usize>,
    pub entity_type_counts_before: BTreeMap<String, usize>,
    pub entity_type_counts_after: BTreeMap<String, usize>,
}

/// Apply `ops` (in order) to up to `limit` entities of `input_path` matching
/// `select`, writing the result to `output_path`. Returns stats plus the
/// clamped limit when `limit` exceeded [`MAX_EDIT_LIMIT`].
pub fn edit_dxf_entities(
    input_path: &Path,
    output_path: &Path,
    select: &DxfSelect,
    ops: &[DxfEditOp],
    limit: Option<usize>,
) -> Result<(DxfEditStats, Option<usize>), ToolError> {
    validate_ops(ops)?;
    let handle_range = parse_handle_range(&select.handle_from, &select.handle_to)?;
    if let Some([min_x, min_y, max_x, max_y]) = select.bbox
        && (![min_x, min_y, max_x, max_y].iter().all(|v| v.is_finite())
            || min_x > max_x
            || min_y > max_y)
    {
        return Err(ToolError::new(
            "invalid_input",
            "bbox must satisfy finite min_x <= max_x and min_y <= max_y.",
        ));
    }

    let raw = std::fs::read(input_path)
        .map_err(|e| ToolError::new("cannot_open_dxf", format!("Failed to read DXF file: {e}")))?;
    if raw.starts_with(b"AutoCAD Binary DXF") {
        return Err(ToolError::new(
            "unsupported_format",
            "Binary DXF is not supported yet. Convert the drawing to ASCII DXF and retry.",
        ));
    }
    let content = String::from_utf8_lossy(&raw);
    let pairs = collect_pairs(&content)?;

    let (effective_limit, limit_clamped_to) = match limit {
        Some(lim) if lim > MAX_EDIT_LIMIT => (MAX_EDIT_LIMIT, Some(MAX_EDIT_LIMIT)),
        Some(lim) => (lim, None),
        None => (MAX_EDIT_LIMIT, None),
    };

    let mut selected = 0usize;
    let mut edited = 0usize;
    let mut deleted = 0usize;
    let mut per_op: BTreeMap<String, usize> = BTreeMap::new();
    let mut counts_before: BTreeMap<String, usize> = BTreeMap::new();
    let mut counts_after: BTreeMap<String, usize> = BTreeMap::new();

    let mut out = String::new();
    let mut i = 0usize;
    let mut in_entities = false;
    while i < pairs.len() {
        let (code, val) = pairs[i];
        if code == 0 && val == "SECTION" {
            in_entities = pairs
                .get(i + 1)
                .is_some_and(|(c, v)| *c == 2 && v.eq_ignore_ascii_case("ENTITIES"));
            out.push_str(&format!("{code}\n{val}\n"));
            if let Some((c2, v2)) = pairs.get(i + 1) {
                out.push_str(&format!("{c2}\n{v2}\n"));
                i += 1;
            }
            i += 1;
            continue;
        }
        if code == 0 && val == "ENDSEC" {
            in_entities = false;
            out.push_str("0\nENDSEC\n");
            i += 1;
            continue;
        }
        if in_entities && code == 0 && val != "EOF" {
            let entity_type = val.to_uppercase();
            let end = entity_block_end(&pairs, i);
            let groups: Vec<(i32, String)> = pairs[i..end]
                .iter()
                .map(|(c, v)| (*c, (*v).to_string()))
                .collect();
            *counts_before.entry(entity_type.clone()).or_insert(0) += 1;
            if selected < effective_limit && matches_select(&groups, select, &handle_range) {
                selected += 1;
                let mut is_deleted = false;
                let mut was_modified = false;
                let mut new_groups = groups;
                for op in ops {
                    if is_deleted {
                        break;
                    }
                    *per_op.entry(op.name().to_string()).or_insert(0) += 1;
                    match op {
                        DxfEditOp::Delete => is_deleted = true,
                        other => {
                            apply_op(&mut new_groups, other);
                            was_modified = true;
                        }
                    }
                }
                if is_deleted {
                    deleted += 1;
                } else {
                    if was_modified {
                        edited += 1;
                    }
                    *counts_after.entry(entity_type).or_insert(0) += 1;
                    for (c, v) in &new_groups {
                        out.push_str(&format!("{c}\n{v}\n"));
                    }
                }
            } else {
                *counts_after.entry(entity_type).or_insert(0) += 1;
                for (c, v) in &pairs[i..end] {
                    out.push_str(&format!("{c}\n{v}\n"));
                }
            }
            i = end;
            continue;
        }
        out.push_str(&format!("{code}\n{val}\n"));
        i += 1;
    }

    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            ToolError::new(
                "internal_error",
                format!("Failed to create outputs directory: {e}"),
            )
        })?;
    }
    std::fs::write(output_path, out)
        .map_err(|e| ToolError::new("internal_error", format!("Failed writing edited DXF: {e}")))?;

    let stats = DxfEditStats {
        selected,
        edited,
        deleted,
        per_op,
        entity_type_counts_before: counts_before,
        entity_type_counts_after: counts_after,
    };
    Ok((stats, limit_clamped_to))
}

/// End index (exclusive) of the entity block starting at `start`: the next
/// `0` pair, extended through VERTEX/SEQEND sub-blocks of a POLYLINE.
fn entity_block_end(pairs: &[(i32, &str)], start: usize) -> usize {
    let entity_type = pairs[start].1;
    let mut end = start + 1;
    while end < pairs.len() && pairs[end].0 != 0 {
        end += 1;
    }
    if entity_type.eq_ignore_ascii_case("POLYLINE") {
        while end < pairs.len()
            && pairs[end].0 == 0
            && (pairs[end].1.eq_ignore_ascii_case("VERTEX")
                || pairs[end].1.eq_ignore_ascii_case("SEQEND"))
        {
            end += 1;
            while end < pairs.len() && pairs[end].0 != 0 {
                end += 1;
            }
        }
    }
    end
}

fn first_str(groups: &[(i32, String)], code: i32) -> Option<&str> {
    groups
        .iter()
        .find(|(c, _)| *c == code)
        .map(|(_, v)| v.as_str())
}

fn entity_bbox_2d(groups: &[(i32, String)]) -> Option<[f64; 4]> {
    let mut xs = Vec::new();
    let mut ys = Vec::new();
    for (c, v) in groups {
        match c {
            10..=18 => {
                if let Ok(x) = v.trim().parse::<f64>() {
                    xs.push(x);
                }
            }
            20..=28 => {
                if let Ok(y) = v.trim().parse::<f64>() {
                    ys.push(y);
                }
            }
            _ => {}
        }
    }
    if xs.is_empty() || ys.is_empty() {
        return None;
    }
    let mut min_x = xs.iter().copied().fold(f64::INFINITY, f64::min);
    let mut max_x = xs.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let mut min_y = ys.iter().copied().fold(f64::INFINITY, f64::min);
    let mut max_y = ys.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let entity_type = groups
        .first()
        .map(|(_, v)| v.to_uppercase())
        .unwrap_or_default();
    if matches!(entity_type.as_str(), "CIRCLE" | "ARC")
        && let Some(r) = first_str(groups, 40).and_then(|s| s.trim().parse::<f64>().ok())
    {
        let r = r.abs();
        min_x -= r;
        max_x += r;
        min_y -= r;
        max_y += r;
    }
    Some([min_x, min_y, max_x, max_y])
}

fn matches_select(
    groups: &[(i32, String)],
    select: &DxfSelect,
    handle_range: &(Option<u64>, Option<u64>),
) -> bool {
    if let Some(ref layer) = select.layer {
        let ent_layer = first_str(groups, 8).unwrap_or("0");
        if !ent_layer.eq_ignore_ascii_case(layer.trim()) {
            return false;
        }
    }
    if let Some(ref etype) = select.entity_type {
        let ent_type = groups.first().map(|(_, v)| v.as_str()).unwrap_or("");
        if !ent_type.eq_ignore_ascii_case(etype.trim()) {
            return false;
        }
    }
    if handle_range.0.is_some() || handle_range.1.is_some() {
        let h = match first_str(groups, 5).and_then(|h| u64::from_str_radix(h.trim(), 16).ok()) {
            Some(v) => v,
            None => return false,
        };
        if let Some(min_h) = handle_range.0
            && h < min_h
        {
            return false;
        }
        if let Some(max_h) = handle_range.1
            && h > max_h
        {
            return false;
        }
    }
    if let Some([q_min_x, q_min_y, q_max_x, q_max_y]) = select.bbox {
        let Some([e_min_x, e_min_y, e_max_x, e_max_y]) = entity_bbox_2d(groups) else {
            return false;
        };
        if !(e_min_x <= q_max_x && e_max_x >= q_min_x && e_min_y <= q_max_y && e_max_y >= q_min_y) {
            return false;
        }
    }
    true
}

fn apply_op(groups: &mut Vec<(i32, String)>, op: &DxfEditOp) {
    let entity_type = groups
        .first()
        .map(|(_, v)| v.to_uppercase())
        .unwrap_or_default();
    match op {
        DxfEditOp::SetLayer { layer } => set_or_insert_group(groups, 8, layer.clone()),
        DxfEditOp::SetColor { color } => set_or_insert_group(groups, 62, color.to_string()),
        DxfEditOp::SetLinetype { linetype } => set_or_insert_group(groups, 6, linetype.clone()),
        DxfEditOp::SetLineweight { lineweight } => {
            set_or_insert_group(groups, 370, lineweight.to_string());
        }
        DxfEditOp::Move { dx, dy } => {
            transform_xy_pairs(groups, |x, y| (x + dx, y + dy));
        }
        DxfEditOp::Rotate { cx, cy, angle_deg } => {
            let rad = angle_deg * PI / 180.0;
            let (sin_a, cos_a) = rad.sin_cos();
            transform_xy_pairs(groups, |x, y| {
                let rx = x - cx;
                let ry = y - cy;
                (cx + rx * cos_a - ry * sin_a, cy + rx * sin_a + ry * cos_a)
            });
            if matches!(entity_type.as_str(), "ARC" | "TEXT" | "MTEXT") {
                for pair in groups.iter_mut() {
                    if matches!(pair.0, 50 | 51)
                        && let Ok(a) = pair.1.trim().parse::<f64>()
                    {
                        pair.1 = format_coord(a + angle_deg);
                    }
                }
            }
        }
        DxfEditOp::Scale { cx, cy, factor } => {
            transform_xy_pairs(groups, |x, y| {
                (cx + (x - cx) * factor, cy + (y - cy) * factor)
            });
            if matches!(entity_type.as_str(), "CIRCLE" | "ARC" | "TEXT" | "MTEXT") {
                for pair in groups.iter_mut() {
                    if pair.0 == 40
                        && let Ok(r) = pair.1.trim().parse::<f64>()
                    {
                        pair.1 = format_coord(r * factor.abs());
                    }
                }
            }
        }
        DxfEditOp::Delete => {}
    }
}

fn set_or_insert_group(groups: &mut Vec<(i32, String)>, code: i32, value: String) {
    if let Some(pair) = groups.iter_mut().find(|(c, _)| *c == code) {
        pair.1 = value;
    } else {
        let idx = groups
            .iter()
            .position(|(c, _)| *c != 0 && *c != 5)
            .unwrap_or(groups.len());
        groups.insert(idx, (code, value));
    }
}

fn transform_xy_pairs<F>(groups: &mut [(i32, String)], mut f: F)
where
    F: FnMut(f64, f64) -> (f64, f64),
{
    let mut pending_x: [Option<(usize, f64)>; 9] = [None; 9];
    for idx in 0..groups.len() {
        let code = groups[idx].0;
        if (10..=18).contains(&code) {
            if let Ok(x) = groups[idx].1.trim().parse::<f64>() {
                pending_x[(code - 10) as usize] = Some((idx, x));
            }
        } else if (20..=28).contains(&code)
            && let Some((x_idx, x)) = pending_x[(code - 20) as usize].take()
            && let Ok(y) = groups[idx].1.trim().parse::<f64>()
        {
            let (nx, ny) = f(x, y);
            groups[x_idx].1 = format_coord(nx);
            groups[idx].1 = format_coord(ny);
        }
    }
}

fn format_coord(v: f64) -> String {
    let clean = if v.abs() < 1e-12 { 0.0 } else { v };
    format!("{clean:.6}")
}

fn validate_ops(ops: &[DxfEditOp]) -> Result<(), ToolError> {
    if ops.is_empty() {
        return Err(ToolError::new(
            "invalid_input",
            "ops must contain at least one edit operation.",
        ));
    }
    for op in ops {
        match op {
            DxfEditOp::SetLayer { layer } if layer.trim().is_empty() => {
                return Err(ToolError::new(
                    "invalid_input",
                    "set_layer requires a non-empty layer.",
                ));
            }
            DxfEditOp::SetColor { color } if !(1..=255).contains(color) => {
                return Err(ToolError::new(
                    "invalid_input",
                    format!("set_color requires ACI 1..=255, got {color}."),
                ));
            }
            DxfEditOp::SetLinetype { linetype } if linetype.trim().is_empty() => {
                return Err(ToolError::new(
                    "invalid_input",
                    "set_linetype requires a non-empty linetype.",
                ));
            }
            DxfEditOp::SetLineweight { lineweight } if !(0..=211).contains(lineweight) => {
                return Err(ToolError::new(
                    "invalid_input",
                    format!("set_lineweight requires 0..=211, got {lineweight}."),
                ));
            }
            DxfEditOp::Move { dx, dy } if !dx.is_finite() || !dy.is_finite() => {
                return Err(ToolError::new(
                    "invalid_input",
                    "move requires finite dx and dy.",
                ));
            }
            DxfEditOp::Rotate { cx, cy, angle_deg }
                if !cx.is_finite() || !cy.is_finite() || !angle_deg.is_finite() =>
            {
                return Err(ToolError::new(
                    "invalid_input",
                    "rotate requires finite cx, cy and angle_deg.",
                ));
            }
            DxfEditOp::Scale { cx, cy, factor }
                if !cx.is_finite() || !cy.is_finite() || !factor.is_finite() || *factor == 0.0 =>
            {
                return Err(ToolError::new(
                    "invalid_input",
                    "scale requires finite non-zero factor.",
                ));
            }
            _ => {}
        }
    }
    Ok(())
}

fn parse_handle_range(
    from: &Option<String>,
    to: &Option<String>,
) -> Result<(Option<u64>, Option<u64>), ToolError> {
    let parse = |s: &str| {
        u64::from_str_radix(s.trim(), 16)
            .map_err(|_| ToolError::new("invalid_input", format!("Invalid hex handle: '{s}'")))
    };
    let f = match from {
        Some(s) => Some(parse(s)?),
        None => None,
    };
    let t = match to {
        Some(s) => Some(parse(s)?),
        None => None,
    };
    if let (Some(fv), Some(tv)) = (f, t)
        && fv > tv
    {
        return Err(ToolError::new(
            "invalid_input",
            "handle_from must be <= handle_to.",
        ));
    }
    Ok((f, t))
}
