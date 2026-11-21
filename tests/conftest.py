"""Shared fixtures: a sandboxed workspace and a tiny deterministic IFC model."""

from __future__ import annotations

from collections.abc import Callable
from pathlib import Path

import ifcopenshell
import pytest
from ifcopenshell.api import run

from mcp_cad_core import WORKSPACE_ROOT_ENV


@pytest.fixture
def workspace(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> Path:
    """A workspace directory wired into MCP_CAD_WORKSPACE_ROOT."""
    ws = tmp_path / "workspace"
    ws.mkdir()
    monkeypatch.setenv(WORKSPACE_ROOT_ENV, str(ws))
    return ws


def make_ifc_model() -> ifcopenshell.file:
    """Build a tiny IFC4 model: 2 walls + 1 door, psets on the first wall."""
    f = ifcopenshell.file(schema="IFC4")
    run("root.create_entity", f, ifc_class="IfcProject", name="Test Project")
    run("root.create_entity", f, ifc_class="IfcSite", name="Test Site")

    wall_1 = run("root.create_entity", f, ifc_class="IfcWall", name="Wall-A")
    wall_2 = run("root.create_entity", f, ifc_class="IfcWall", name="Wall-B")
    run("root.create_entity", f, ifc_class="IfcDoor", name="Door-1")

    pset = run("pset.add_pset", f, product=wall_1, name="Pset_WallCommon")
    run("pset.edit_pset", f, pset=pset, properties={"LoadBearing": True, "IsExternal": True})
    acoustic = run("pset.add_pset", f, product=wall_2, name="Pset_WallAcoustic")
    run("pset.edit_pset", f, pset=acoustic, properties={"AcousticRating": "55 dB"})
    return f


@pytest.fixture
def ifc_path(workspace: Path) -> Path:
    """Write the test IFC model into the workspace and return its path."""
    model = make_ifc_model()
    path = workspace / "model.ifc"
    model.write(str(path))
    return path