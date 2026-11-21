"""Strict workspace path sandboxing.

Every server in this monorepo resolves user-supplied paths through
:class:`WorkspaceSandbox`. The sandbox root is configured once per server
process through the ``MCP_CAD_WORKSPACE_ROOT`` environment variable and is
the *only* directory tree tools are allowed to touch.

Guarantees:

1. Rejected: any relative path containing a ``..`` component (``path_traversal``).
2. Rejected: absolute paths that are outside the root (``path_outside_workspace``).
3. Rejected: paths that traverse a symlink whose target escapes the root
   (``path_outside_workspace``). Resolution happens *after* the anti-traversal
   check, so symlink tricks cannot smuggle a ``..`` in either.
4. Mutating tools never overwrite inputs: use :meth:`WorkspaceSandbox.resolve_output`,
   which requires a *new* file name under ``<root>/outputs``.

Path arguments must be given relative to the workspace root. Absolute paths
inside the root are tolerated (common mistake by agents) but never required.
"""

from __future__ import annotations

import os
from collections.abc import Mapping
from pathlib import Path, PurePath

WORKSPACE_ROOT_ENV = "MCP_CAD_WORKSPACE_ROOT"

_OUTPUT_DIR_NAME = "outputs"


class SandboxError(Exception):
    """A path violates the workspace sandbox rules.

    Attributes ``code`` and ``message`` map 1:1 onto the standardized error
    envelope produced by :func:`mcp_cad_core.responses.error`.
    """

    def __init__(self, code: str, message: str) -> None:
        self.code = code
        self.message = message
        super().__init__hmessage,
        


class WorkspaceSandbox:
    """Resolve and validate user-supplied paths against a fixed workspace root."""

    def __init__(self, root: str | Path) -> None:
        root_path = Path(root).expanduser()
        if not root_path.is_absolute():
            root_path = Path.cwd() / root_path
        root_resolved = root_path.resolve(strict=False)
        if not root_resolved.exists():
            raise SandboxError(
                "workspace_root_missing",
                f"MCP_CAD_WORKSPACE_ROOT points to a directory that does not exist: {root_resolved}",
            )
        if not root_resolved.is_dir():
            raise SandboxError(
                "workspace_root_not_dir",
                f"MCP_CAD_WORKSPACE_ROOT must be a directory, got a file: {root_resolved}",
            )
        self._root = root_resolved

    @property
    def root(self) -> Path:
        """The resolved, absolute workspace root."""
        return self._root

    def contains(self, path: Path) -> bool:
        """Return True if ``path`` is inside the workspace root."""
        try:
            path.relative_to(self._root)
        except ValueError:
            return False
        return True

    def resolve(
        self,
        relative: str | Path,
        *,
        must_exist: bool = False,
        expect: tuple[str, ...] = (),
    ) -> Path:
        """Resolve a user-supplied path to an absolute path inside the root.

        Args:
            relative: Path relative to the workspace root (an absolute path is
                accepted only if it is already inside the root).
            must_exist: When True, raise ``file_not_found`` if the path does not
                exist on disk.
            expect: Optionally require the resolved path to be a ``"file"`` or
                ``"dir"`` (both allowed if omitted).

        Returns:
            The resolved absolute path, guaranteed to be inside the root.

        Raises:
            SandboxError: With codes ``path_traversal``, ``path_outside_workspace``,
                ``file_not_found``, ``not_a_file`` or ``not_a_dir``.
        """
        if not isinstance(expect, tuple):  # pragma: no cover - defensive
            expect = tuple(expect)
        invalid_expect = set(expect) - {"file", "dir"}
        if invalid_expect:  # pragma: no cover - defensive, programmer error
            raise SandboxError("internal", f"Invalid expect kinds: {sorted(invalid_expect)}")

        # Rule 1: reject '..' before any resolution. Normalize backslashes so
        # Windows-style '..\..\' separators cannot smuggle traversal in.
        parts = PurePath(str(relative).replace("\\", "/")).parts
        if ".." in parts:
            raise SandboxError(
                "path_traversal",
                "Path must stay inside the workspace: '..' components are not allowed.",
            )

        raw = Path(str(relative))
        candidate = raw if raw.is_absolute() else self._root / raw

        # Rules 2 & 3: resolve symlinks, then require containment of the *real* path.
        resolved = candidate.resolve(strict=False)
        if not self.contains(resolved):
            raise SandboxError(
                "path_outside_workspace",
                f"Path resolves outside the workspace root ({self._root}): {relative}",
            )

        if must_exist and not resolved.exists():
            raise SandboxError(
                "file_not_found",
                f"File not found inside the workspace: {relative}",
            )
        if "file" in expect and not resolved.is_file():
            raise SandboxError("not_a_file", f"Expected a file, got something else: {relative}")
        if "dir" in expect and not resolved.is_dir():
            raise SandboxError("not_a_dir", f"Expected a directory, got something else: {relative}")
        return resolved

    def resolve_output(self, filename: str) -> Path:
        """Resolve a *new* output path under ``<root>/outputs`` for mutating tools.

        Mutating tools must never modify input files in place. They write a new
        file under the sandboxed ``outputs`` directory and return a structured
        diff/summary. Existing output names are refused so that destructive
        overwrites are impossible.

        Returns:
            Absolute path to the (not yet existing) output file.

        Raises:
            SandboxError: With codes ``path_traversal``, ``path_outside_workspace``,
                ``output_exists``.
        """
        if not filename or PurePath(filename).name != filename:
            raise SandboxError(
                "path_traversal",
                "Output file name must be a plain file name without directories or '..'.",
            )

        outputs_dir = self._root / _OUTPUT_DIR_NAME
        outputs_dir.mkdir(parents=True, exist_ok=True)
        target = outputs_dir / filename
        if target.exists():
            raise SandboxError(
                "output_exists",
                f"Output file already exists, refusing to overwrite it: {filename}. "
                "Choose a new, unique output name.",
            )
        return target


def workspace_sandbox(env: Mapping[str, str] | None = None) -> WorkspaceSandbox:
    """Build a :class:`WorkspaceSandbox` from the process environment.

    Reads ``MCP_CAD_WORKSPACE_ROOT`` (or the provided ``env`` mapping, which
    also makes deterministic testing easy).

    Raises:
        SandboxError: ``workspace_not_configured`` when the variable is unset
            or empty; the sandbox constructor's codes for invalid roots.
    """
    source = os.environ if env is None else env
    root = source.get(WORKSPACE_ROOT_ENV, "")
    if not root:
        raise SandboxError(
            "workspace_not_configured",
            "The workspace sandbox is not configured. "
            f"Set the {WORKSPACE_ROOT_ENV} environment variable to the "
            "directory that servers are allowed to read and write.",
        )
    return WorkspaceSandbox(root)
