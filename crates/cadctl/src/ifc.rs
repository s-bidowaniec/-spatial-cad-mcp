//! Streaming IFC (ISO 10303-21 STEP) inspection without any CAD library.
//!
//! IFC SPF files are line-oriented, so a single pass over the file yields the
//! header schema and per-class entity counts without the cost of a full
//! `ifcopenshell` parse. Deliberately bounded output: an agent gets compact
//! JSON, never the raw entity tree.

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::Path;

use anyhow::{bail, Context, Result};
use serde::Serialize;

/// Compact summary of one IFC file, shaped for agent consumption.
#[derive(Debug, Serialize)]
pub struct IfcSummary {
    pub file_path: String,
    pub file_bytes: u64,
    /// Schema declared in FILE_SCHEMA, e.g. `IFC4` or `IFC2X3`.
    pub schema: Option<String>,
    /// FILE_NAME timestamp field (usually ISO 8601), when present.
    pub timestamp: Option<String>,
    pub total_entities: usize,
    /// Most frequent entity classes (descending), truncated to the top N.
    pub top_classes: Vec<ClassCount>,
}

#[derive(Debug, Serialize)]
pub struct ClassCount {
    pub ifc_class: String,
    pub count: usize,
}

/// Inspect an IFC file: header schema + entity class counts in one pass.
pub fn inspect(path: &Path, top: usize) -> Result<IfcSummary> {
    let file_bytes = fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    let reader = File::open(path).with_context(|| format!("cannot open {}", path.display()))?;
    let mut reader = BufReader::new(reader);

    let mut saw_magic = false;
    let mut schema: Option<String> = None;
    let mut timestamp: Option<String> = None;
    let mut total_entities = 0usize;
    let mut counts: HashMap<String, usize> = HashMap::new();

    let mut line = String::new();
    loop {
        line.clear();
        let bytes = reader.read_line(&mut line).with_context(|| "read error")?;
        if bytes == 0 {
            break;
        }
        let trimmed = line.trim();
        if trimmed.starts_with("ISO-10303-21") {
            saw_magic = true;
        }
        if schema.is_none() && trimmed.starts_with("FILE_SCHEMA") {
            schema = first_quoted_token(trimmed);
        }
        if timestamp.is_none() && trimmed.starts_with("FILE_NAME") {
            // FILE_NAME('name','timestamp',...); timestamp is the 2nd quoted field.
            timestamp = quoted_tokens(trimmed).nth(1);
        }
        if trimmed.starts_with('#') && trimmed.contains('=') {
            if let Some(class) = entity_class(trimmed) {
                *counts.entry(class).or_insert(0) += 1;
                total_entities += 1;
            }
        }
    }

    if !saw_magic {
        bail!(
            "{} is not an IFC/STEP file (missing ISO-10303-21 magic)",
            path.display()
        );
    }

    let mut top_classes: Vec<(String, usize)> = counts.into_iter().collect();
    top_classes.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    let top_classes = top_classes
        .into_iter()
        .take(top)
        .map(|(ifc_class, count)| ClassCount { ifc_class, count })
        .collect();

    Ok(IfcSummary {
        file_path: path.display().to_string(),
        file_bytes,
        schema,
        timestamp,
        total_entities,
        top_classes,
    })
}

/// Class name of an SPF instance line like `#42=IFCWALL(...);`.
fn entity_class(line: &str) -> Option<String> {
    let start = line.find('=')? + 1;
    let rest = &line[start..];
    let rest = rest.trim_start();
    let end = rest.find('(')?;
    let class = rest[..end].trim();
    let mut bytes = class.bytes();
    match bytes.next() {
        Some(b) if b.is_ascii_uppercase() || b.is_ascii_digit() => Some(class.to_string()),
        _ => None,
    }
}

/// First single-quoted token on the line (used for FILE_SCHEMA(('IFC4'))).
fn first_quoted_token(line: &str) -> Option<String> {
    quoted_tokens(line).next()
}

/// Iterator over single-quoted fields on a header line.
fn quoted_tokens(line: &str) -> impl Iterator<Item = String> + '_ {
    let mut rest = line;
    std::iter::from_fn(move || {
        let start = rest.find('\'')?;
        let after = &rest[start + 1..];
        let end = after.find('\'')?;
        let token = after[..end].to_string();
        rest = &after[end + 1..];
        Some(token)
    })
}
