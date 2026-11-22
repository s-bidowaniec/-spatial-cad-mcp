"""Path sandboxing tests: traversal, absolute escapes, symlinks, outputs."""

from __future__ import annotations

from pathlib import Path

import pytest

from mcp_cad_core import WORKSPACE_ROOT_ENV
from mcp_cad_core.sandbox import SandboxError, WorkspaceSandbox, workspace_sandbox


@pytest.fixture
def sandbox(workspace: Path) -> WorkspaceSandbox:
    (workspace / "sub").mkdir()
    (workspace / "sub" / "file.ifc").write_text("stub")
    return WorkspaceSandbox(workspace)


class TestResolveAllowed:
    def test_relative_path_inside_root(self, sandbox: WorkspaceSandbox) -> None:
        resolved = sandbox.resolve("sub/file.ifc")
        assert resolved == sandbox.root / "sub" / "file.ifc"

    def test_absolute_path_inside_root_tolerated(self, sandbox: WorkspaceSandbox) -> None:
        absolute = sandbox.root / "sub" / "file.ifc"
        assert sandbox.resolve(absolute) == absolute

    def test_must_exist_and_expect_file(self, sandbox: WorkspaceSandbox) -> None:
        resolved = sandbox.resolve("sub/file.ifc", must_exist=True, expect=("file",))
        assert resolved.is_file()


class TestResolveRejected:
    @pytest.mark.parametrize("bad", ["../secret.txt", "sub/../../secret.txt", "sub/.."])
    def test_traversal_rejected(self, sandbox: WorkspaceSandbox, bad: str) -> None:
        with pytest.raises(SandboxError) as excinfo:
            sandbox.resolve(bad)
        assert excinfo.value.code == "path_traversal"

    def test_windows_style_traversal_rejected(self, sandbox: WorkspaceSandbox) -> None:
        with pytest.raises(SandboxError) as excinfo:
            sandbox.resolve("..\\..\\secret.txt")
        assert excinfo.value.code == "path_traversal"

    def test_absolute_path_outside_root_rejected(
        self, sandbox: WorkspaceSandbox, tmp_path: Path
    ) -> None:
        outside = tmp_path / "outside.txt"
        outside.write_text("secret")
        with pytest.raises(SandboxError) as excinfo:
            sandbox.resolve(outside)
        assert excinfo.value.code == "path_outside_workspace"

    def test_symlink_escape_rejected(self, sandbox: WorkspaceSandbox, tmp_path: Path) -> None:
        outside_dir = tmp_path / "outside"
        outside_dir.mkdir()
        (outside_dir / "secret.txt").write_text("secret")
        (sandbox.root / "link").symlink_to(outside_dir)
        with pytest.raises(SandboxError) as excinfo:
            sandbox.resolve("link/secret.txt", must_exist=True)
        assert excinfo.value.code == "path_outside_workspace"

    def test_symlink_staying_inside_allowed(self, sandbox: WorkspaceSandbox) -> None:
        (sandbox.root / "alias").symlink_to(sandbox.root / "sub", target_is_directory=True)
        resolved = sandbox.resolve("alias/file.ifc")
        assert resolved.is_file()

    def test_missing_file_rejected_with_must_exist(self, sandbox: WorkspaceSandbox) -> None:
        with pytest.raises(SandboxError) as excinfo:
            sandbox.resolve("does-not-exist.ifc", must_exist=True)
        assert excinfo.value.code == "file_not_found"

    def test_expect_file_rejects_directory(self, sandbox: WorkspaceSandbox) -> None:
        with pytest.raises(SandboxError) as excinfo:
            sandbox.resolve("sub", must_exist=True, expect=("file",))
        assert excinfo.value.code == "not_a_file"

    def test_expect_dir_rejects_file(self, sandbox: WorkspaceSandbox) -> None:
        with pytest.raises(SandboxError) as excinfo:
            sandbox.resolve("sub/file.ifc", must_exist=True, expect=("dir",))
        assert excinfo.value.code == "not_a_dir"


class TestOutputs:
    def test_output_path_is_new(self, sandbox: WorkspaceSandbox) -> None:
        target = sandbox.resolve_output("modified-model.ifc")
        assert target.parent == sandbox.root / "outputs"
        assert not target.exists()

    def test_output_must_be_plain_filename(self, sandbox: WorkspaceSandbox) -> None:
        for bad in ("../evil.ifc", "sub/model.ifc", "", ".."):
            with pytest.raises(SandboxError) as excinfo:
                sandbox.resolve_output(bad)
            assert excinfo.value.code == "path_traversal"

    def test_existing_output_rejected(self, sandbox: WorkspaceSandbox) -> None:
        sandbox.resolve_output("already-there.ifc").write_text("content")
        with pytest.raises(SandboxError) as excinfo:
            sandbox.resolve_output("already-there.ifc")
        assert excinfo.value.code == "output_exists"


class TestEnvironment:
    def test_unconfigured_workspace(self, monkeypatch: pytest.MonkeyPatch) -> None:
        monkeypatch.delenv(WORKSPACE_ROOT_ENV, raising=False)
        with pytest.raises(SandboxError) as excinfo:
            workspace_sandbox()
        assert excinfo.value.code == "workspace_not_configured"

    def test_empty_env_value(self, monkeypatch: pytest.MonkeyPatch) -> None:
        monkeypatch.setenv(WORKSPACE_ROOT_ENV, "")
        with pytest.raises(SandboxError) as excinfo:
            workspace_sandbox()
        assert excinfo.value.code == "workspace_not_configured"

    def test_nonexistent_root(self, tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
        monkeypatch.setenv(WORKSPACE_ROOT_ENV, str(tmp_path / "nope"))
        with pytest.raises(SandboxError) as excinfo:
            workspace_sandbox()
        assert excinfo.value.code == "workspace_root_missing"

    def test_root_must_be_directory(self, tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
        not_a_dir = tmp_path / "file.txt"
        not_a_dir.write_text("x")
        monkeypatch.setenv(WORKSPACE_ROOT_ENV, str(not_a_dir))
        with pytest.raises(SandboxError) as excinfo:
            workspace_sandbox()
        assert excinfo.value.code == "workspace_root_not_dir"
