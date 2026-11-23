"""Tests for `ifc_query_spatial` and `container_global_id` filtering (task 05)."""

from __future__ import annotations

from pathlib import Path

import ifcopenshell
import ifcopenshell.guid

from mcp_ifc import tools as t
from mcp_ifc.spatial import filter_elements_by_container, query_spatial_hierarchy


def _build_model(path: Path) -> tuple[ifcopenshell.file, str, str]:
    """IfcProject > IfcSite > IfcBuilding > 2 storeys, 3 walls contained in storey 1."""
    model = ifcopenshell.file(schema="IFC4")
    project = model.create_entity(
        "IfcProject", GlobalId=ifcopenshell.guid.new(), Name="TestProject"
    )
    site = model.create_entity("IfcSite", GlobalId=ifcopenshell.guid.new(), Name="TestSite")
    building = model.create_entity("IfcBuilding", GlobalId=ifcopenshell.guid.new(), Name="Main")
    storey1 = model.create_entity(
        "IfcBuildingStorey", GlobalId=ifcopenshell.guid.new(), Name="Storey_01"
    )
    storey2 = model.create_entity(
        "IfcBuildingStorey", GlobalId=ifcopenshell.guid.new(), Name="Storey_02"
    )
    model.create_entity(
        "IfcRelAggregates",
        GlobalId=ifcopenshell.guid.new(),
        RelatingObject=project,
        RelatedObjects=[site],
    )
    model.create_entity(
        "IfcRelAggregates",
        GlobalId=ifcopenshell.guid.new(),
        RelatingObject=site,
        RelatedObjects=[building],
    )
    model.create_entity(
        "IfcRelAggregates",
        GlobalId=ifcopenshell.guid.new(),
        RelatingObject=building,
        RelatedObjects=[storey1, storey2],
    )
    walls = [
        model.create_entity("IfcWall", GlobalId=ifcopenshell.guid.new(), Name=f"Wall_{i}")
        for i in range(3)
    ]
    model.create_entity(
        "IfcRelContainedInSpatialStructure",
        GlobalId=ifcopenshell.guid.new(),
        RelatingStructure=storey1,
        RelatedElements=walls,
    )
    model.write(str(path))
    return model, storey1.GlobalId, storey2.GlobalId


def test_spatial_hierarchy_rows_and_counts(workspace: Path) -> None:
    model, s1_gid, s2_gid = _build_model(workspace / "building.ifc")

    data, page = query_spatial_hierarchy(model, depth=3, limit=200, offset=0)
    assert page.limit_clamped_to is None
    assert page.total == 4  # site, building, 2 storeys
    rows = {r["global_id"]: r for r in data["rows"]}
    assert rows[s1_gid]["level"] == 2
    assert rows[s1_gid]["contained_elements"] == 3
    assert rows[s2_gid]["contained_elements"] == 0
    assert rows[s1_gid]["class"] == "IfcBuildingStorey"

    walls = list(model.by_type("IfcWall"))
    filtered = filter_elements_by_container(model, walls, s1_gid)
    assert len(filtered) == 3
    assert filter_elements_by_container(model, walls, s2_gid) == []


def test_ifc_query_spatial_tool_envelopes(workspace: Path) -> None:
    _build_model(workspace / "building.ifc")

    res = t.ifc_query_spatial("building.ifc", limit=500)
    assert res["ok"] is True
    assert res["meta"]["pagination"]["limit_clamped_to"] == 200
    assert res["meta"]["pagination"]["total"] == 4

    for bad_depth in (0, 11):
        res = t.ifc_query_spatial("building.ifc", depth=bad_depth)
        assert res["ok"] is False
        assert res["error"]["code"] == "invalid_input"

    res = t.ifc_query_spatial("building.ifc", root_global_id="NON_EXISTENT_GUID_999")
    assert res["ok"] is False
    assert res["error"]["code"] == "element_not_found"


def test_ifc_query_elements_container_filter(workspace: Path) -> None:
    _build_model(workspace / "building.ifc")
    model = ifcopenshell.open(str(workspace / "building.ifc"))
    storey1 = next(s for s in model.by_type("IfcBuildingStorey") if s.Name == "Storey_01")

    res = t.ifc_query_elements(
        "building.ifc", ifc_class="IfcWall", container_global_id=storey1.GlobalId
    )
    assert res["ok"] is True
    assert res["meta"]["pagination"]["total"] == 3
    assert len(res["data"]["elements"]) == 3

    res = t.ifc_query_elements("building.ifc", ifc_class="IfcWall", container_global_id="NOPE")
    assert res["ok"] is False
    assert res["error"]["code"] == "element_not_found"
