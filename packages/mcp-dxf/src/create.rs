//! `dxf_create` (new drawing from scratch) and `dxf_add_entities` (copy an
//! existing ASCII DXF, optionally apply task-01 select+ops first, then append
//! new entities). Both write a NEW file; inputs are never modified.

use crate::edit::{DxfEditOp, DxfEditStats, DxfSelect, render_edited_dxf};
use crate::parser::collect_pairs;
use crate::writer::{DxfNewEntity, MAX_CREATE_ENTITIES, write_dxf_file};
use cad_envelope::ToolError;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

#[derive(Debug, Clone, Serialize)]
pub struct DxfCreateStats {
    pub created: usize,
    pub layers: Vec<String>,
    pub bbox: [f64; 6],
}

#[derive(Debug, Clone, Serialize)]
pub struct DxfAddEntitiesStats {
    pub added: usize,
    pub added_by_type: BTreeMap<String, usize>,
    pub edited_existing: usize,
    pub deleted_existing: usize,
    pub entity_type_counts_before: BTreeMap<String, usize>,
    pub entity_type_counts_after: BTreeMap<String, usize>,
}

pub fn create_dxf_entities(
    output_path: &Path,
    entities: &[DxfNewEntity],
    acad_version: Option<&str>,
    layers: &[String],
) -> Result<(DxfCreateStats, Option<usize>), ToolError> {
    if entities.is_empty() {
        return Err(ToolError::new(
            "invalid_input",
            "entities must contain at least one entity.",
        ));
    }
    if output_path.exists() {
        return Err(ToolError::new(
            "output_exists",
            format!("Output file already exists: {}", output_path.display()),
        ));
    }
    let acad_ver = acad_version
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("AC1015");
    if !acad_ver.starts_with("AC10") {
        return Err(ToolError::new(
            "invalid_input",
            format!("unsupported acad_version '{acad_ver}' (expected e.g. 'AC1015')"),
        ));
    }
    let (clamped, limit_clamped_to) = if entities.len() > MAX_CREATE_ENTITIES {
        (&entities[..MAX_CREATE_ENTITIES], Some(MAX_CREATE_ENTITIES))
    } else {
        (entities, None)
    };
    for ent in clamped {
        ent.validate()
            .map_err(|m| ToolError::new("invalid_input", m))?;
    }

    let mut layer_set: BTreeSet<String> = BTreeSet::new();
    layer_set.insert("0".to_string());
    for l in layers {
        if !l.trim().is_empty() {
            layer_set.insert(l.trim().to_string());
        }
    }
    for ent in clamped {
        layer_set.insert(ent.layer().trim().to_string());
    }
    let declared: Vec<String> = layer_set.into_iter().collect();

    let mut bbox = [
        f64::INFINITY,
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    ];
    let mut handle = 0x100u64;
    let mut serialized: Vec<Vec<(i32, String)>> = Vec::with_capacity(clamped.len());
    for ent in clamped {
        let eb = ent.bbox_3d();
        for k in 0..3 {
            bbox[k] = bbox[k].min(eb[k]);
            bbox[k + 3] = bbox[k + 3].max(eb[k + 3]);
        }
        serialized.push(ent.to_group_pairs(&mut handle));
    }

    let mut header_vars = BTreeMap::new();
    header_vars.insert("$ACADVER".to_string(), vec![(1, acad_ver.to_string())]);
    write_dxf_file(output_path, &header_vars, &declared, &[], &serialized).map_err(|e| {
        ToolError::new("internal_error", format!("Failed to write DXF: {e}"))
    })?;

    Ok((
        DxfCreateStats {
            created: clamped.len(),
            layers: declared,
            bbox,
        },
        limit_clamped_to,
    ))
}

pub fn add_dxf_entities(
    input_path: &Path,
    output_path: &Path,
    new_entities: &[DxfNewEntity],
    select: Option<&DxfSelect>,
    ops: &[DxfEditOp],
    limit: Option<usize>,
) -> Result<(DxfAddEntitiesStats, Option<usize>), ToolError> {
    if new_entities.is_empty() {
        return Err(ToolError::new(
            "invalid_input",
            "entities must contain at least one entity to add.",
        ));
    }
    if output_path.exists() {
        return Err(ToolError::new(
            "output_exists",
            format!("Output file already exists: {}", output_path.display()),
        ));
    }
    let (clamped_new, mut limit_clamped_to) = if new_entities.len() > MAX_CREATE_ENTITIES {
        (&new_entities[..MAX_CREATE_ENTITIES], Some(MAX_CREATE_ENTITIES))
    } else {
        (new_entities, None)
    };
    for ent in clamped_new {
        ent.validate()
            .map_err(|m| ToolError::new("invalid_input", m))?;
    }

    let raw = std::fs::read(input_path).map_err(|e| {
        ToolError::new("cannot_open_dxf", format!("Failed to read DXF file: {e}"))
    })?;
    if raw.starts_with(b"AutoCAD Binary DXF") {
        return Err(ToolError::new(
            "unsupported_format",
            "Binary DXF is not supported yet. Convert the drawing to ASCII DXF and retry.",
        ));
    }
    let content = String::from_utf8_lossy(&raw);
    let pairs = collect_pairs(&content)?;

    let default_select = DxfSelect::default();
    let select = select.unwrap_or(&default_select);
    let (edited_text, edit_stats) = if ops.is_empty() {
        (render_pairs(&pairs), count_stats(&pairs))
    } else {
        let (text, stats, cl) = render_edited_dxf(&pairs, select, ops, limit)?;
        if limit_clamped_to.is_none() {
            limit_clamped_to = cl;
        }
        (text, stats)
    };

    let edited_pairs = collect_pairs(&edited_text)?;
    let next_handle = max_handle(&edited_pairs).map(|m| m + 1).unwrap_or(0x100);
    let mut handle = next_handle;
    let mut added_by_type: BTreeMap<String, usize> = BTreeMap::new();
    let mut serialized: Vec<Vec<(i32, String)>> = Vec::with_capacity(clamped_new.len());
    for ent in clamped_new {
        *added_by_type
            .entry(ent.entity_type_name().to_string())
            .or_insert(0) += 1;
        serialized.push(ent.to_group_pairs(&mut handle));
    }

    let final_text = append_entities(&edited_pairs, &serialized)?;
    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            ToolError::new("internal_error", format!("Failed to create outputs directory: {e}"))
        })?;
    }
    std::fs::write(output_path, final_text).map_err(|e| {
        ToolError::new("internal_error", format!("Failed writing combined DXF: {e}"))
    })?;

    let counts_after = {
        let written = std::fs::read_to_string(output_path)
            .map_err(|e| ToolError::new("internal_error", format!("{e}")))?;
        let after_pairs = collect_pairs(&written)?;
        count_types(&after_pairs)
    };

    let stats = DxfAddEntitiesStats {
        added: clamped_new.len(),
        added_by_type,
        edited_existing: edit_stats.edited,
        deleted_existing: edit_stats.deleted,
        entity_type_counts_before: edit_stats.entity_type_counts_before,
        entity_type_counts_after: counts_after,
    };
    Ok((stats, limit_clamped_to))
}

fn render_pairs(pairs: &[(i32, &str)]) -> String {
    let mut out = String::new();
    for (c, v) in pairs {
        out.push_str(&format!("{c}\n{v}\n"));
    }
    out
}

fn count_stats(pairs: &[(i32, &str)]) -> DxfEditStats {
    let counts = count_types(pairs);
    DxfEditStats {
        selected: 0,
        edited: 0,
        deleted: 0,
        per_op: BTreeMap::new(),
        entity_type_counts_after: counts.clone(),
        entity_type_counts_before: counts,
    }
}

fn count_types(pairs: &[(i32, &str)]) -> BTreeMap<String, usize> {
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut in_entities = false;
    let mut i = 0usize;
    while i < pairs.len() {
        let (c, v) = pairs[i];
        if c == 0 && v == "SECTION" {
            in_entities = pairs
                .get(i + 1)
                .is_some_and(|(c2, v2)| *c2 == 2 && v2.eq_ignore_ascii_case("ENTITIES"));
            i += 2;
            continue;
        }
        if c == 0 && v == "ENDSEC" {
            in_entities = false;
            i += 1;
            continue;
        }
        if in_entities && c == 0 && v != "EOF" {
            *counts.entry(v.to_uppercase()).or_insert(0) += 1;
        }
        i += 1;
    }
    counts
}

fn max_handle(pairs: &[(i32, &str)]) -> Option<u64> {
    let mut in_entities = false;
    let mut max = None::<u64>;
    let mut i = 0usize;
    while i < pairs.len() {
        let (c, v) = pairs[i];
        if c == 0 && v == "SECTION" {
            in_entities = pairs
                .get(i + 1)
                .is_some_and(|(c2, v2)| *c2 == 2 && v2.eq_ignore_ascii_case("ENTITIES"));
            i += 2;
            continue;
        }
        if c == 0 && v == "ENDSEC" {
            in_entities = false;
            i += 1;
            continue;
        }
        if in_entities && c == 5
            && let Ok(h) = u64::from_str_radix(v.trim(), 16)
        {
            max = Some(max.map_or(h, |m: u64| m.max(h)));
        }
        i += 1;
    }
    max
}

/// Inserts serialized entities at the end of the ENTITIES section.
fn append_entities(
    pairs: &[(i32, &str)],
    new_entities: &[Vec<(i32, String)>],
) -> Result<String, ToolError> {
    let mut out = String::new();
    let mut in_entities = false;
    let mut inserted = false;
    let mut i = 0usize;
    while i < pairs.len() {
        let (c, v) = pairs[i];
        if c == 0 && v == "SECTION" {
            in_entities = pairs
                .get(i + 1)
                .is_some_and(|(c2, v2)| *c2 == 2 && v2.eq_ignore_ascii_case("ENTITIES"));
            out.push_str(&format!("{c}\n{v}\n"));
            if let Some((c2, v2)) = pairs.get(i + 1) {
                out.push_str(&format!("{c2}\n{v2}\n"));
                i += 1;
            }
            i += 1;
            continue;
        }
        if in_entities && c == 0 && v == "ENDSEC" {
            if !inserted {
                for ent in new_entities {
                    for (c2, v2) in ent {
                        out.push_str(&format!("{c2}\n{v2}\n"));
                    }
                }
                inserted = true;
            }
            in_entities = false;
            out.push_str("0\nENDSEC\n");
            i += 1;
            continue;
        }
        out.push_str(&format!("{c}\n{v}\n"));
        i += 1;
    }
    if !inserted {
        return Err(ToolError::new(
            "cannot_open_dxf",
            "input DXF has no ENTITIES section to append to.",
        ));
    }
    Ok(out)
}
