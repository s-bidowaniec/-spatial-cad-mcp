"""Tests for `ifc_element_bbox` and `ifc_geometry_summary` (task 06).

The OCCT kernel (`ifcopenshell.geom`) is stubbed with a 12-triangle box so the
tests are deterministic and fast; one test additionally exercises the real
kernel when it is installed (skipped otherwise). The missing-kernel path must
answer `not_implemented`, never crash, and never leak vertex arrays.
"""

from __future__ import annotations

import json
import sys
import types
from pathlib import Path
from typing import Any, ClassVar

import ifcopenshell
import ifcopenshell.guid
import pytest

from mcp_ifc import tools as t
from mcp_ifc.geometry_summary import GEOM_KERNEL_UNAVAILABLE, load_geom_kernel
from mcp_ifc.model_cache import ShapeSummaryTtlCache, _cache, _shape_summary_cache


@pytest.fixture(autouse=True)
def _clean_caches() -> None:
    _cache.clear()
    _shape_summary_cache.clear()


def _build_extruded_wall_ifc(ifc_path: Path) -> tuple[str, str]:
    """Synthetic IFC4 model: one IfcWall (extruded 4.0m x 0.2m x 3.0m) in a storey."""
    model = ifcopenshell.file(schema="IFC4")
    project = model.create_entity("IfcProject", GlobalId=ifcopenshell.guid.new(), Name="GeomProj")
    context = model.create_entity(
        "IfcGeometricRepresentationContext", ContextType="Model", CoordinateSpaceDimension=3
    )
    project.RepresentationContexts = [context]

    pt0 = model.create_entity("IfcCartesianPoint", Coordinates=(0.0, 0.0))
    pt1 = model.create_entity("IfcCartesianPoint", Coordinates=(4.0, 0.0))
    pt2 = model.create_entity("IfcCartesianPoint", Coordinates=(4.0, 0.2))
    pt3 = model.create_entity("IfcCartesianPoint", Coordinates=(0.0, 0.2))
    polyline = model.create_entity("IfcPolyline", Points=[pt0, pt1, pt2, pt3, pt0])
    profile = model.create_entity(
        "IfcArbitraryClosedProfileDef", ProfileType="AREA", OuterCurve=polyline
    )

    direction = model.create_entity("IfcDirection", DirectionRatios=(0.0, 0.0, 1.0))
    origin = model.create_entity("IfcCartesianPoint", Coordinates=(0.0, 0.0, 0.0))
    axis = model.create_entity("IfcAxis2Placement3D", Location=origin)
    extrusion = model.create_entity(
        "IfcExtrudedAreaSolid",
        SweptArea=profile,
        Position=axis,
        ExtrudedDirection=direction,
        Depth=3.0,
    )
    shape_rep = model.create_entity(
        "IfcShapeRepresentation",
        ContextOfItems=context,
        RepresentationIdentifier="Body",
        RepresentationType="SweptSolid",
        Items=[extrusion],
    )
    prod_def = model.create_entity("IfcProductDefinitionShape", Representations=[shape_rep])

    wall = model.create_entity(
        "IfcWall",
        GlobalId=ifcopenshell.guid.new(),
        Name="ExtWall_01",
        Representation=prod_def,
    )
    storey = model.create_entity(
        "IfcBuildingStorey", GlobalId=ifcopenshell.guid.new(), Name="Storey_01"
    )
    model.create_entity(
        "IfcRelContainedInSpatialStructure",
        GlobalId=ifcopenshell.guid.new(),
        RelatingStructure=storey,
        RelatedElements=[wall],
    )
    ifc_path.parent.mkdir(parents=True, exist_ok=True)
    model.write(str(ifc_path))
    return wall.GlobalId, storey.GlobalId


def _install_mock_geom(monkeypatch: pytest.MonkeyPatch) -> None:
    """Install a lightweight stub for `ifcopenshell.geom` returning a 12-triangle box."""
    fake_geom = types.ModuleType("ifcopenshell.geom")

    class _Settings:
        USE_WORLD_COORDS = 1
        TRIANGULATED = 2

        def __init__(self) -> None:
            self._opts: dict[int, Any] = {}

        def set(self, key: int, val: Any) -> None:
            self._opts[key] = val

    class _Geometry:
        verts: ClassVar[list[float]] = [
            0.0,
            0.0,
            0.0,
            4.0,
            0.0,
            0.0,
            4.0,
            0.2,
            0.0,
            0.0,
            0.2,
            0.0,
            0.0,
            0.0,
            3.0,
            4.0,
            0.0,
            3.0,
            4.0,
            0.2,
            3.0,
            0.0,
            0.2,
            3.0,
        ]
        # 12 triangles = 36 face indices
        faces: ClassVar[list[int]] = [
            0,
            1,
            2,
            0,
            2,
            3,
            4,
            5,
            6,
            4,
            6,
            7,
            0,
            1,
            5,
            0,
            5,
            4,
            2,
            3,
            7,
            2,
            7,
            6,
            0,
            3,
            7,
            0,
            7,
            4,
            1,
            2,
            6,
            1,
            6,
            5,
        ]

    class _Shape:
        geometry = _Geometry()

    fake_geom.settings = _Settings
    fake_geom.create_shape = lambda _settings, _product: _Shape()

    monkeypatch.setitem(sys.modules, "ifcopenshell.geom", fake_geom)
    monkeypatch.setattr(ifcopenshell, "geom", fake_geom, raising=False)


def _break_geom_kernel(monkeypatch: pytest.MonkeyPatch) -> None:
    """Simulate a missing OCCT kernel."""
    monkeypatch.setitem(sys.modules, "ifcopenshell.geom", None)
    monkeypatch.setattr(ifcopenshell, "geom", None, raising=False)


def test_element_bbox_and_geometry_summary_with_mock_kernel(
    workspace: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    wall_gid, _storey_gid = _build_extruded_wall_ifc(workspace / "wall.ifc")
    _install_mock_geom(monkeypatch)

    res = t.ifc_element_bbox("wall.ifc", wall_gid)
    assert res["ok"] is True, res
    data = res["data"]
    assert data["global_id"] == wall_gid
    assert data["class"] == "IfcWall"
    assert data["bbox"]["min"] == [0.0, 0.0, 0.0]
    assert data["bbox"]["max"] == [4.0, 0.2, 3.0]
    assert data["center"] == [2.0, 0.1, 1.5]
    assert data["geometry_tris"] == 12
    assert "verts" not in json.dumps(res)

    summary = t.ifc_geometry_summary("wall.ifc", class_filter="IfcWall")
    assert summary["ok"] is True, summary
    cls_list = summary["data"]["classes"]
    assert len(cls_list) == 1
    assert cls_list[0]["class"] == "IfcWall"
    assert cls_list[0]["count"] == 1
    assert cls_list[0]["bbox_union"]["min"] == [0.0, 0.0, 0.0]
    assert cls_list[0]["bbox_union"]["max"] == [4.0, 0.2, 3.0]
    assert cls_list[0]["bbox_min_area_element"]["global_id"] == wall_gid
    assert cls_list[0]["bbox_min_area_element"]["name"] == "ExtWall_01"
    assert len(json.dumps(summary)) < 2048


def test_element_bbox_include_children_unions_contained(
    workspace: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    _wall_gid, storey_gid = _build_extruded_wall_ifc(workspace / "wall.ifc")
    _install_mock_geom(monkeypatch)

    res = t.ifc_element_bbox("wall.ifc", storey_gid, include_children=True)
    assert res["ok"] is True, res
    data = res["data"]
    assert data["class"] == "IfcBuildingStorey"
    assert data["include_children"] is True
    assert data["children_count"] == 1
    assert data["bbox"]["max"] == [4.0, 0.2, 3.0]
    assert data["geometry_tris"] == 12


def test_element_bbox_unknown_global_id(workspace: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    _build_extruded_wall_ifc(workspace / "wall.ifc")
    _install_mock_geom(monkeypatch)

    res = t.ifc_element_bbox("wall.ifc", "0123456789abcdefghijk")
    assert res["ok"] is False
    assert res["error"]["code"] == "element_not_found"

    res = t.ifc_element_bbox("wall.ifc", "   ")
    assert res["ok"] is False
    assert res["error"]["code"] == "invalid_input"


def test_geometry_summary_pagination_and_clamp(
    workspace: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    _build_extruded_wall_ifc(workspace / "wall.ifc")
    _install_mock_geom(monkeypatch)

    res = t.ifc_geometry_summary("wall.ifc", limit=500)
    assert res["ok"] is True, res
    assert res["meta"]["pagination"]["limit_clamped_to"] == 200
    assert res["meta"]["pagination"]["total"] == 1

    res = t.ifc_geometry_summary("wall.ifc", class_filter="Ifcunicorn")
    assert res["ok"] is False
    assert res["error"]["code"] == "invalid_input"

    res = t.ifc_geometry_summary("wall.ifc", limit=0)
    assert res["ok"] is False
    assert res["error"]["code"] == "invalid_input"


def test_missing_geom_kernel_returns_not_implemented(
    workspace: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    wall_gid, _ = _build_extruded_wall_ifc(workspace / "wall.ifc")
    _break_geom_kernel(monkeypatch)

    res = t.ifc_element_bbox("wall.ifc", wall_gid)
    assert res["ok"] is False
    assert res["error"]["code"] == "not_implemented"
    assert res["error"]["message"] == GEOM_KERNEL_UNAVAILABLE

    res = t.ifc_geometry_summary("wall.ifc")
    assert res["ok"] is False
    assert res["error"]["code"] == "not_implemented"
    assert res["error"]["message"] == GEOM_KERNEL_UNAVAILABLE


def test_shape_summary_ttl_cache_expires(monkeypatch: pytest.MonkeyPatch) -> None:
    ticks = iter([100.0, 100.0, 100.0 + 301.0])
    monkeypatch.setattr("mcp_ifc.model_cache.time.monotonic", lambda: next(ticks))

    cache = ShapeSummaryTtlCache(ttl_seconds=300.0, max_entries=2)
    cache.set("file.ifc", "GID1", {"tris": 12})
    assert cache.get("file.ifc", "GID1") == {"tris": 12}
    assert cache.get("file.ifc", "GID1") is None  # expired after ttl
    assert cache.get("file.ifc", "MISSING") is None


def test_shape_summary_ttl_cache_evicts_oldest() -> None:
    cache = ShapeSummaryTtlCache(ttl_seconds=300.0, max_entries=2)
    cache.set("f", "A", 1)
    cache.set("f", "B", 2)
    cache.set("f", "C", 3)
    assert cache.get("f", "A") is None
    assert cache.get("f", "B") == 2
    assert cache.get("f", "C") == 3


@pytest.mark.skipif(load_geom_kernel() is None, reason="OCCT kernel not installed")
def test_real_kernel_bbox_of_extruded_wall(workspace: Path) -> None:
    wall_gid, _ = _build_extruded_wall_ifc(workspace / "wall.ifc")

    res = t.ifc_element_bbox("wall.ifc", wall_gid)
    assert res["ok"] is True, res
    data = res["data"]
    for got, want in zip(data["bbox"]["min"], [0.0, 0.0, 0.0], strict=True):
        assert got == pytest.approx(want, abs=1e-6)
    for got, want in zip(data["bbox"]["max"], [4.0, 0.2, 3.0], strict=True):
        assert got == pytest.approx(want, abs=1e-6)
    assert data["geometry_tris"] == 12
