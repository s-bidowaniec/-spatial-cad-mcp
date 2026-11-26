#![forbid(unsafe_code)]
//! Shared AI-to-AI response envelopes, deterministic pagination, and value
//! truncation for the Rust servers in `spatial-cad-mcp`.
//!
//! Mirrors `shared/mcp_cad_core/{responses,pagination}.py` so Python and Rust
//! MCP servers speak the exact same contract.

use cad_sandbox::{SandboxError, WorkspaceSandbox};
use serde::Serialize;
use serde_json::{Map, Value, json};
use std::path::PathBuf;
use std::time::Instant;

/// Default page size when the caller omits `limit`.
pub const DEFAULT_LIMIT: usize = 50;

/// Hard page cap across all tools (never silently return more).
pub const MAX_LIMIT: usize = 200;

/// Default string truncation budget for noisy values.
pub const MAX_VALUE_CHARS: usize = 300;

/// Machine-readable tool failure mapped onto the standard error envelope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolError {
    pub code: &'static str,
    pub message: String,
    pub retryable: bool,
    pub hints: Vec<String>,
}

impl ToolError {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            retryable: false,
            hints: Vec::new(),
        }
    }

    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hints.push(hint.into());
        self
    }

    pub fn from_sandbox(err: SandboxError) -> Self {
        Self::new(err.code, err.message)
    }
}

/// Resolve an existing file inside the workspace sandbox, returning
/// `file_not_found` or `invalid_input` when appropriate.
pub fn resolve_input_file(
    sandbox: &WorkspaceSandbox,
    raw_path: &str,
) -> Result<PathBuf, ToolError> {
    if raw_path.trim().is_empty() {
        return Err(ToolError::new(
            "invalid_input",
            "File path must be a non-empty path relative to MCP_CAD_WORKSPACE_ROOT.",
        ));
    }
    let resolved = sandbox.resolve(raw_path).map_err(ToolError::from_sandbox)?;
    if !resolved.exists() {
        return Err(ToolError::new(
            "file_not_found",
            format!("File does not exist inside the workspace: {raw_path}"),
        ));
    }
    if !resolved.is_file() {
        return Err(ToolError::new(
            "invalid_input",
            format!("Expected a regular file, got a directory or special path: {raw_path}"),
        ));
    }
    Ok(resolved)
}

/// One deterministic page of results plus metadata.
#[derive(Debug, Clone)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub total: usize,
    pub limit: usize,
    pub offset: usize,
    pub limit_clamped_to: Option<usize>,
}

impl<T> Page<T> {
    pub fn has_more(&self) -> bool {
        self.offset + self.items.len() < self.total
    }

    pub fn next_offset(&self) -> Option<usize> {
        if self.has_more() {
            Some(self.offset + self.limit)
        } else {
            None
        }
    }

    pub fn meta_value(&self) -> Value {
        let mut p = Map::new();
        p.insert("total".to_string(), json!(self.total));
        p.insert("limit".to_string(), json!(self.limit));
        p.insert("offset".to_string(), json!(self.offset));
        p.insert("returned".to_string(), json!(self.items.len()));
        p.insert("has_more".to_string(), json!(self.has_more()));
        p.insert("next_offset".to_string(), json!(self.next_offset()));
        if let Some(clamped) = self.limit_clamped_to {
            p.insert("limit_clamped_to".to_string(), json!(clamped));
        }
        Value::Object(p)
    }
}

/// Deterministically slice `items` with `limit`/`offset` validation and hard
/// cap enforcement (`MAX_LIMIT = 200`).
pub fn paginate<T: Clone>(
    items: &[T],
    limit: Option<i64>,
    offset: Option<i64>,
) -> Result<Page<T>, ToolError> {
    let raw_limit = limit.unwrap_or(DEFAULT_LIMIT as i64);
    let raw_offset = offset.unwrap_or(0);
    if raw_limit < 1 {
        return Err(ToolError::new("invalid_input", "limit must be >= 1"));
    }
    if raw_offset < 0 {
        return Err(ToolError::new("invalid_input", "offset must be >= 0"));
    }
    let req_limit = raw_limit as usize;
    let effective_limit = req_limit.min(MAX_LIMIT);
    let clamped_to = (effective_limit < req_limit).then_some(MAX_LIMIT);
    let off = raw_offset as usize;
    let total = items.len();
    let window = if off >= total {
        Vec::new()
    } else {
        let end = (off + effective_limit).min(total);
        items[off..end].to_vec()
    };
    Ok(Page {
        items: window,
        total,
        limit: effective_limit,
        offset: off,
        limit_clamped_to: clamped_to,
    })
}

/// Truncate a string to at most `max_chars` Unicode scalar values, appending
/// an explicit marker when clipped.
pub fn truncate_value(text: &str, max_chars: usize) -> (String, bool) {
    let count = text.chars().count();
    if count <= max_chars {
        return (text.to_string(), false);
    }
    let prefix: String = text.chars().take(max_chars).collect();
    (format!("{prefix}... (+{} chars)", count - max_chars), true)
}

/// Build a standard success JSON envelope.
pub fn ok_json<T: Serialize>(
    tool: &str,
    data: &T,
    extra_meta: Option<Map<String, Value>>,
    started: Instant,
) -> String {
    let mut meta = extra_meta.unwrap_or_default();
    meta.insert("tool".to_string(), json!(tool));
    meta.insert(
        "duration_ms".to_string(),
        json!((started.elapsed().as_secs_f64() * 1000.0 * 100.0).round() / 100.0),
    );
    json!({
        "ok": true,
        "data": data,
        "meta": meta,
    })
    .to_string()
}

/// Build a standard failure JSON envelope.
pub fn err_json(tool: &str, err: &ToolError, started: Instant) -> String {
    let mut err_obj = Map::new();
    err_obj.insert("code".to_string(), json!(err.code));
    err_obj.insert("message".to_string(), json!(err.message));
    err_obj.insert("retryable".to_string(), json!(err.retryable));
    if !err.hints.is_empty() {
        err_obj.insert("hints".to_string(), json!(err.hints));
    }
    let duration_ms = (started.elapsed().as_secs_f64() * 1000.0 * 100.0).round() / 100.0;
    json!({
        "ok": false,
        "error": err_obj,
        "meta": {
            "tool": tool,
            "duration_ms": duration_ms,
        }
    })
    .to_string()
}

/// Run a tool body and always serialize its outcome into the standard envelope.
pub fn run_envelope<T: Serialize, F>(tool: &str, f: F) -> String
where
    F: FnOnce() -> Result<(T, Option<Map<String, Value>>), ToolError>,
{
    let started = Instant::now();
    match f() {
        Ok((data, meta)) => ok_json(tool, &data, meta, started),
        Err(err) => err_json(tool, &err, started),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paginate_clamps_above_max_limit() {
        let items: Vec<usize> = (0..250).collect();
        let page = paginate(&items, Some(500), Some(0)).unwrap();
        assert_eq!(page.items.len(), MAX_LIMIT);
        assert_eq!(page.limit_clamped_to, Some(MAX_LIMIT));
        assert!(page.has_more());
        assert_eq!(page.next_offset(), Some(MAX_LIMIT));
    }

    #[test]
    fn paginate_rejects_negative_offset_and_zero_limit() {
        assert_eq!(
            paginate::<i32>(&[], Some(0), Some(0)).unwrap_err().code,
            "invalid_input"
        );
        assert_eq!(
            paginate::<i32>(&[], Some(10), Some(-1)).unwrap_err().code,
            "invalid_input"
        );
    }

    #[test]
    fn truncate_value_is_utf8_safe() {
        let (out, truncated) = truncate_value("zażółć gęślą jaźń", 6);
        assert!(truncated);
        assert!(out.starts_with("zażółć... (+11 chars)"));
    }
}
