//! Shared workspace path sandboxing for the Rust side of spatial-cad-mcp.
//!
//! This crate mirrors the rules of the Python `mcp_cad_core.sandbox`
//! module so every binary in the monorepo rejects paths identically:
//!
//! 1. Reject any relative path containing a `..` component (`path_traversal`).
//! 2. Reject absolute paths outside the workspace root (`path_outside_workspace`).
//! 3. Reject paths that traverse a symlink escaping the root
//!    (`path_outside_workspace`): existing path components are
//!    canonicalized before the containment check.
//! 4. Mutating commands write to *new* files under `<root>/outputs` only,
//!    and refuses to overwrite (`output_exists`).
//!
//! The workspace root is provided per process via `MCP_CAD_WORKSPACE_ROOT`.

use std::collections::VecDeque;
use std::fmt;
use std::path::{Component, Path, PathBuf};

/// Environment variable naming the sandbox root directory.
pub const WORKSPACE_ROOT_ENV: &str = "MCP_CAD_WORKSPACE_ROOT";

/// A sandbox violation, carrying a machine-readable (snake_case) code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxError {
    /// Stable code matching the shared error envelope vocabulary
    /// (`path_traversal`, `path_outside_workspace`, `output_exists`, ...).
    pub code: &'static str,
    /// Human-readable explanation, safe to surface to a calling agent.
    pub message: String,
}

impl SandboxError {
    fn new(code: &'static str, message: String) -> Self {
        Self { code, message }
    }

    fn traversal() -> Self {
        Self::new(
            "path_traversal",
            "Path must stay inside the workspace: '..' components are not allowed.".to_string(),
        )
    }
}

impl fmt::Display for SandboxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for SandboxError {}

/// A fixed workspace root that user-supplied paths are resolved against.
#[derive(Debug, Clone)]
pub struct WorkspaceSandbox {
    root: PathBuf,
}

impl WorkspaceSandbox {
    /// Build a sandbox from an explicit root directory (must exist).
    pub fn from_root(root: impl AsRef<Path>) -> Result<Self, SandboxError> {
        let root = root.as_ref();
        let resolved = std::fs::canonicalize(root).map_err(|_| {
            SandboxError::new(
                "workspace_root_missing",
                format!(
                    "MCP_CAD_WORKSPACE_ROOT points to a directory that does not exist: {}",
                    root.display()
                ),
            )
        })?;
        if !resolved.is_dir() {
            return Err(SandboxError::new(
                "workspace_root_not_dir",
                format!(
                    "MCP_CAD_WORKSPACE_ROOT must be a directory, got a file: {}",
                    resolved.display()
                ),
            ));
        }
        Ok(Self { root: resolved })
    }

    /// Build a sandbox from `MCP_CAD_WORKSPACE_ROOT`.
    pub fn from_env() -> Result<Self, SandboxError> {
        match std::env::var(WORKSPACE_ROOT_ENV) {
            Ok(value) if !value.trim().is_empty() => Self::from_root(value.trim()),
            _ => Err(SandboxError::new(
                "workspace_not_configured",
                format!(
                    "The workspace sandbox is not configured. Set the {WORKSPACE_ROOT_ENV} environment variable to the directory that servers are allowed to read and write."
                ),
            )),
        }
    }

    /// The resolved, absolute workspace root.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Resolve a user-supplied path to an absolute path inside the root.
    ///
    /// Rejections carry the codes `path_traversal` and
    /// `path_outside_workspace`; see the module docs for the rules.
    pub fn resolve(&self, relative: impl AsRef<Path>) -> Result<PathBuf, SandboxError> {
        let raw = relative.as_ref();
        // Normalize Windows-style separators so '..\..\x' cannot smuggle traversal in.
        let normalized = raw.to_string_lossy().replace('\\', "/");
        let normalized = Path::new(&normalized);

        if normalized.is_absolute() {
            // Absolute paths are tolerated only when already inside the root.
            return self.checked(normalized.to_path_buf());
        }
        for component in normalized.components() {
            if matches!(component, Component::ParentDir) {
                return Err(SandboxError::traversal());
            }
        }
        self.checked(self.root.join(normalized))
    }

    /// Resolve a *new* output path under `<root>/outputs` for mutating tools.
    pub fn resolve_output(&self, file_name: &str) -> Result<PathBuf, SandboxError> {
        if file_name.is_empty()
            || Path::new(file_name.replace('\\', "/").as_str())
                .file_name()
                .and_then(|f| f.to_str())
                != Some(file_name)
        {
            return Err(SandboxError::new(
                "path_traversal",
                "Output file name must be a plain file name without directories or '..'."
                    .to_string(),
            ));
        }
        let dir = self.root.join("outputs");
        std::fs::create_dir_all(&dir).map_err(|e| {
            SandboxError::new("internal", format!("Cannot create outputs directory: {e}"))
        })?;
        let target = dir.join(file_name);
        if target.exists() {
            return Err(SandboxError::new(
                "output_exists",
                format!(
                    "Output file already exists, refusing to overwrite it: {file_name}. Choose a new, unique output name."
                ),
            ));
        }
        Ok(target)
    }

    /// Canonicalize existing ancestors and enforce containment of the real path.
    fn checked(&self, candidate: PathBuf) -> Result<PathBuf, SandboxError> {
        let resolved = canonicalize_prefix(&candidate);
        if !resolved.starts_with(&self.root) {
            return Err(SandboxError::new(
                "path_outside_workspace",
                format!(
                    "Path resolves outside the workspace root ({}): {}",
                    self.root.display(),
                    candidate.display()
                ),
            ));
        }
        Ok(resolved)
    }
}

/// Resolve symlinks of the longest existing prefix of `path` and append the
/// non-existing remainder verbatim. This is what lets symlink escapes be
/// caught while still resolving paths that do not exist yet.
fn canonicalize_prefix(path: &Path) -> PathBuf {
    if path.exists() {
        return std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    }
    let mut tail: VecDeque<_> = VecDeque::new();
    let mut current = path.to_path_buf();
    while !current.exists() {
        match current.file_name() {
            Some(name) => {
                tail.push_front(name.to_os_string());
                current = match current.parent() {
                    Some(parent) => parent.to_path_buf(),
                    None => return path.to_path_buf(),
                };
            }
            None => return path.to_path_buf(),
        }
    }
    let mut resolved = std::fs::canonicalize(&current).unwrap_or(current);
    for name in tail {
        resolved.push(name);
    }
    resolved
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    #[test]
    fn resolves_paths_inside_root() {
        let tmp = std::env::temp_dir().join(format!("cad-sandbox-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(tmp.join("sub")).unwrap();
        fs::write(tmp.join("sub").join("f.ifc"), "x").unwrap();

        let sandbox = WorkspaceSandbox::from_root(&tmp).unwrap();
        let resolved = sandbox.resolve("sub/f.ifc").unwrap();
        assert!(resolved.is_file());
        assert_eq!(resolved, sandbox.root().join("sub").join("f.ifc"));

        let absolute: PathBuf = fs::canonicalize(sandbox.root().join("sub/f.ifc")).unwrap();
        assert_eq!(sandbox.resolve(&absolute).unwrap(), absolute);
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn rejects_traversal() {
        let tmp = std::env::temp_dir().join(format!("cad-sandbox-tr-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        let sandbox = WorkspaceSandbox::from_root(&tmp).unwrap();
        for bad in [
            "../secret.txt",
            "sub/../../secret.txt",
            "..\\..\\secret.txt",
        ] {
            assert_eq!(sandbox.resolve(bad).unwrap_err().code, "path_traversal");
        }
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn rejects_paths_outside_root_and_symlink_escapes() {
        let tmp = std::env::temp_dir().join(format!("cad-sandbox-out-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(tmp.join("ws")).unwrap();
        fs::create_dir_all(tmp.join("outside")).unwrap();
        fs::write(tmp.join("outside").join("s.txt"), "s").unwrap();

        let sandbox = WorkspaceSandbox::from_root(tmp.join("ws")).unwrap();
        let outside_abs = fs::canonicalize(tmp.join("outside/s.txt")).unwrap();
        assert_eq!(
            sandbox.resolve(&outside_abs).unwrap_err().code,
            "path_outside_workspace"
        );

        #[cfg(unix)]
        std::os::unix::fs::symlink(tmp.join("outside"), tmp.join("ws").join("link")).unwrap();
        #[cfg(unix)]
        assert_eq!(
            sandbox.resolve("link/s.txt").unwrap_err().code,
            "path_outside_workspace"
        );
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn output_paths_are_new_and_sandboxed() {
        let tmp = std::env::temp_dir().join(format!("cad-sandbox-out2-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        let sandbox = WorkspaceSandbox::from_root(&tmp).unwrap();

        let target = sandbox.resolve_output("modified.ifc").unwrap();
        assert_eq!(target.parent().unwrap(), sandbox.root().join("outputs"));
        assert!(!target.exists());

        fs::write(&target, "x").unwrap();
        assert_eq!(
            sandbox.resolve_output("modified.ifc").unwrap_err().code,
            "output_exists"
        );
        for bad in ["../evil.ifc", "sub/model.ifc", "", ".."] {
            assert_eq!(
                sandbox.resolve_output(bad).unwrap_err().code,
                "path_traversal"
            );
        }
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn missing_root_is_reported() {
        let err = WorkspaceSandbox::from_root("/definitely/not/here").unwrap_err();
        assert_eq!(err.code, "workspace_root_missing");
    }
}
