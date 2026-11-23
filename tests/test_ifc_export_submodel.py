"""Tests for `ifc_export_submodel` (task 07): the first IFC mutating tool."""

from __future__ import annotations

import hashlib
from pathlib import Path

import ifcopenshell
import ifcopenshell.guid

from mcp_ifc import tools as t
from mcp_ifc.model_cache import _cache


def _sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def _build_two_storey_model(ifc_path: Path) -> tuple[str, str, str]:
    """Synthetic IFC4 model: two storeys; storey1 has 1 wall + 1 slab, storey2 the same."""
    model = ifcopenshell.file(schema="IFC4")
    project = model.create_entity("IfcProject", GlobalId=ifcopenshell.guid.new(), Name="Proj")
    site = model.create_entity("IfcSite", GlobalId=ifcopenshell.guid.new(), Name="Site")
    building = model.create_entity("IfcBuilding", GlobalId=ifcopenshell.guid.new(), Name="Bldg")
    s1 = model.create_entity("IfcBuildingStorey", GlobalId=ifcopenshell.guid.new(), Name="Storey1")
    s2 = model.create_entity("IfcBuildingStorey", GlobalId=ifcopenshell.guid.new(), Name="Storey2")

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
        RelatedObjects=[s1, s2],
    )

    w1 = model.create_entity("IfcWall", GlobalId=ifcopenshell.guid.new(), Name="W1")
    w2 = model.create_entity("IfcWall", GlobalId=ifcopenshell.guid.new(), Name="W2")
    slab1 = model.create_entity("IfcSlab", GlobalId=ifcopenshell.guid.new(), Name="Slab1")
    slab2 = model.create_entity("IfcSlab", GlobalId=ifcopenshell.guid.new(), Name="Slab2")

    model.create_entity(
        "IfcRelContainedInSpatialStructure",
        GlobalId=ifcopenshell.guid.new(),
        RelatingStructure=s1,
        RelatedElements=[w1, slab1],
    )
    model.create_entity(
        "IfcRelContainedInSpatialStructure",
        GlobalId=ifcopenshell.guid.new(),
        RelatingStructure=s2,
        RelatedElements=[w2, slab2],
    )

    ifc_path.parent.mkdir(parents=True, exist_ok=True)
    model.write(str(ifc_path))
    return s1.GlobalId, w1.GlobalId, slab2.GlobalId


def test_export_by_class_global_ids_and_container(workspace: Path) -> None:
    ifc_file = workspace / "model.ifc"
    s1_gid, w1_gid, slab2_gid = _build_two_storey_model(ifc_file)
    _cache.clear()
    orig_hash = _sha256(ifc_file)

    r_cls = t.ifc_export_submodel("model.ifc", "walls_only.ifc", class_filter=["IfcWall"])
    assert r_cls["ok"] is True, r_cls
    assert r_cls["data"]["output"] == "outputs/walls_only.ifc"
    assert r_cls["data"]["elements_after"] == 2
    assert r_cls["data"]["by_class"] == {"IfcWall": 2}
    assert r_cls["data"]["file_size_bytes"] > 0
    assert _sha256(ifc_file) == orig_hash

    reopened = ifcopenshell.open(str(workspace / "outputs/walls_only.ifc"))
    assert len(reopened.by_type("IfcWall")) == 2
    assert len(reopened.by_type("IfcSlab")) == 0
    # spatial hierarchy survives the export
    assert len(reopened.by_type("IfcBuildingStorey")) == 2

    r_gids = t.ifc_export_submodel(
        "model.ifc", "by_ids.ifc", global_ids=[w1_gid, slab2_gid, "UNKNOWN_GUID_404"]
    )
    assert r_gids["ok"] is True, r_gids
    assert r_gids["data"]["elements_after"] == 2
    assert r_gids["data"]["missing_global_ids"] == ["UNKNOWN_GUID_404"]

    r_cont = t.ifc_export_submodel("model.ifc", "storey1.ifc", container_global_id=s1_gid)
    assert r_cont["ok"] is True, r_cont
    assert r_cont["data"]["elements_after"] == 2
    assert r_cont["data"]["by_class"] == {"IfcSlab": 1, "IfcWall": 1}

    assert _sha256(ifc_file) == orig_hash


def test_export_global_ids_clamped_at_200(workspace: Path) -> None:
    ifc_file = workspace / "model.ifc"
    _, w1_gid, _ = _build_two_storey_model(ifc_file)
    _cache.clear()

    gids = [w1_gid] + [f"GID_{i:018d}" for i in range(250)]
    res = t.ifc_export_submodel("model.ifc", "clamped.ifc", global_ids=gids)
    assert res["ok"] is True, res
    assert res["meta"]["limit_clamped_to"] == 200
    assert res["data"]["elements_after"] == 1
    assert len(res["data"]["missing_global_ids"]) == 199


def test_export_rejections(workspace: Path) -> None:
    ifc_file = workspace / "model.ifc"
    _build_two_storey_model(ifc_file)
    _cache.clear()

    res = t.ifc_export_submodel("model.ifc", "out.ifc")
    assert res["ok"] is False
    assert res["error"]["code"] == "invalid_input"

    res = t.ifc_export_submodel("model.ifc", "out.txt", class_filter=["IfcWall"])
    assert res["ok"] is False
    assert res["error"]["code"] == "invalid_input"

    res = t.ifc_export_submodel("model.ifc", "../evil.ifc", class_filter=["IfcWall"])
    assert res["ok"] is False
    assert res["error"]["code"] == "invalid_input"

    res = t.ifc_export_submodel("model.ifc", "dup.ifc", class_filter=["IfcWall"])
    assert res["ok"] is True, res
    res = t.ifc_export_submodel("model.ifc", "dup.ifc", class_filter=["IfcWall"])
    assert res["ok"] is False
    assert res["error"]["code"] == "output_exists"

    res = t.ifc_export_submodel("../evil.ifc", "out2.ifc", class_filter=["IfcWall"])
    assert res["ok"] is False
    assert res["error"]["code"] == "path_traversal"

    res = t.ifc_export_submodel("nope.ifc", "out3.ifc", class_filter=["IfcWall"])
    assert res["ok"] is False
    assert res["error"]["code"] == "file_not_found"

    res = t.ifc_export_submodel("model.ifc", "bad_class.ifc", class_filter=["Ifcunicorn"])
    assert res["ok"] is False
    assert res["error"]["code"] == "invalid_input"
