"""Regression tests for the data-policy guard (tools/check_no_data.py)."""

import shutil
import subprocess
import sys
from pathlib import Path

import pytest

SCRIPT = Path(__file__).resolve().parents[1] / "tools" / "check_no_data.py"


@pytest.mark.parametrize(
    ("name", "index_state"),
    [
        ("model.ifc", None),
        ("site.gpkg-wal", None),
        ("terrain.tif", "removed"),
        ("oversized.txt", "replaced"),
    ],
)
def test_guard_rejects_model_data_in_worktree_or_index(
    tmp_path: Path, name: str, index_state: str | None
) -> None:
    subprocess.run(["git", "init", "-q"], cwd=tmp_path, check=True)
    script = tmp_path / "tools" / SCRIPT.name
    script.parent.mkdir()
    shutil.copyfile(SCRIPT, script)
    candidate = tmp_path / name
    candidate.parent.mkdir(parents=True, exist_ok=True)
    candidate.write_bytes(b"x" * (1024**2 + 1) if index_state == "replaced" else b"")

    if index_state is not None:
        subprocess.run(["git", "add", "--", name], cwd=tmp_path, check=True)
    if index_state == "removed":
        candidate.unlink()
    elif index_state == "replaced":
        candidate.write_text("small worktree version", encoding="utf-8")

    run = subprocess.run(
        [sys.executable, str(script)], cwd=tmp_path, capture_output=True, text=True
    )
    assert run.returncode == 1, run.stdout
    assert str(Path(name)) in run.stderr
