"""Tests for `ifc_set_property` (task 08): property editing mutator."""

from __future__ import annotations

import hashlib
from pathlib import Path

import ifcopenshell
import ifcopenshell.guid
import ifcopenshell.util.element

from mcp_ifc import tools as t
from mcp_ifc.model_cache import _cache


def _sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def _build_walls_with_and_without_pset(ifc_path: Path) -> tuple[str, str]:
    """Two IfcWalls; the first has Pset_WallCommon.FireRating=REI30, the second no pset."""
    model = ifcopenshell.file(schema="IFC4")
    model.create_entity("IfcProject", GlobalId=ifcopenshell.guid.new(), Name="PropProj")

    w_with = model.create_entity("IfcWall", GlobalId=ifcopenshell.guid.new(), Name="WallWithPset")
    w_without = model.create_entity(
        "IfcWall", GlobalId=ifcopenshell.guid.new(), Name="WallWithoutPset"
    )

    prop_fire = model.create_entity(
        "IfcPropertySingleValue",
        Name="FireRating",
        NominalValue=model.create_entity("IfcText", "REI30"),
    )
    pset = model.create_entity(
        "IfcPropertySet",
        GlobalId=ifcopenshell.guid.new(),
        Name="Pset_WallCommon",
        HasProperties=[prop_fire],
    )
    model.create_entity(
        "IfcRelDefinesByProperties",
        GlobalId=ifcopenshell.guid.new(),
        RelatedObjects=[w_with],
        RelatingPropertyDefinition=pset,
    )

    ifc_path.parent.mkdir(parents=True, exist_ok=True)
    model.write(str(ifc_path))
    return w_with.GlobalId, w_without.GlobalId


def test_set_property_existing_new_and_numeric_types(workspace: Path) -> None:
    ifc_file = workspace / "walls.ifc"
    w_with_gid, w_without_gid = _build_walls_with_and_without_pset(ifc_file)
    _cache.clear()
    orig_hash = _sha256(ifc_file)

    res = t.ifc_set_property(
        "walls.ifc",
        "tagged.ifc",
        pset="Pset_WallCommon",
        prop="FireRating",
        value="REI60",
        class_filter="IfcWall",
    )
    assert res["ok"] is True, res
    data = res["data"]
    assert data["output"] == "outputs/tagged.ifc"
    assert data["elements_matched"] == 2
    assert data["properties_set"] == 2
    assert _sha256(ifc_file) == orig_hash

    c_with = next(c for c in data["changes"] if c["global_id"] == w_with_gid)
    assert c_with["old"] == "REI30"
    assert c_with["new"] == "REI60"
    c_without = next(c for c in data["changes"] if c["global_id"] == w_without_gid)
    assert c_without["old"] == ""
    assert c_without["new"] == "REI60"

    reopened = ifcopenshell.open(str(workspace / "outputs/tagged.ifc"))
    for gid in (w_with_gid, w_without_gid):
        psets = ifcopenshell.util.element.get_psets(reopened.by_guid(gid))
        assert psets["Pset_WallCommon"]["FireRating"] == "REI60"

    res_num = t.ifc_set_property(
        "walls.ifc",
        "numeric.ifc",
        pset="Pset_Custom",
        prop="LoadFactor",
        value="2.5",
        global_ids=[w_with_gid],
    )
    assert res_num["ok"] is True, res_num
    assert res_num["data"]["elements_matched"] == 1
    reopened_num = ifcopenshell.open(str(workspace / "outputs/numeric.ifc"))
    wall = reopened_num.by_guid(w_with_gid)
    pdef = None
    for rel in getattr(wall, "IsDefinedBy", []) or []:
        if (
            rel.is_a("IfcRelDefinesByProperties")
            and getattr(rel.RelatingPropertyDefinition, "Name", None) == "Pset_Custom"
        ):
            pdef = rel.RelatingPropertyDefinition
    assert pdef is not None
    load_factor = next(p for p in pdef.HasProperties if p.Name == "LoadFactor")
    assert load_factor.NominalValue.is_a("IfcReal")

    res_int = t.ifc_set_property(
        "walls.ifc",
        "int.ifc",
        pset="Pset_Custom",
        prop="Floors",
        value="42",
        global_ids=[w_with_gid],
    )
    assert res_int["ok"] is True, res_int
    reopened_int = ifcopenshell.open(str(workspace / "outputs/int.ifc"))
    psets = ifcopenshell.util.element.get_psets(reopened_int.by_guid(w_with_gid))
    assert psets["Pset_Custom"]["Floors"] == 42


def test_set_property_rejections(workspace: Path) -> None:
    ifc_file = workspace / "walls.ifc"
    _build_walls_with_and_without_pset(ifc_file)
    _cache.clear()

    res = t.ifc_set_property(
        "walls.ifc", "out.ifc", pset="Pset_WallCommon", prop="FireRating", value="REI60"
    )
    assert res["ok"] is False
    assert res["error"]["code"] == "invalid_input"

    res = t.ifc_set_property(
        "walls.ifc",
        "out.ifc",
        pset="Pset_WallCommon",
        prop="FireRating",
        value="REI60",
        class_filter="IfcColumn",
    )
    assert res["ok"] is False
    assert res["error"]["code"] == "element_not_found"

    res = t.ifc_set_property(
        "walls.ifc",
        "sub/out.ifc",
        pset="Pset_WallCommon",
        prop="FireRating",
        value="REI60",
        class_filter="IfcWall",
    )
    assert res["ok"] is False
    assert res["error"]["code"] == "invalid_input"

    res = t.ifc_set_property(
        "walls.ifc",
        "out.txt",
        pset="Pset_WallCommon",
        prop="FireRating",
        value="REI60",
        class_filter="IfcWall",
    )
    assert res["ok"] is False
    assert res["error"]["code"] == "invalid_input"

    res = t.ifc_set_property(
        "walls.ifc",
        "exists.ifc",
        pset="Pset_WallCommon",
        prop="FireRating",
        value="REI60",
        class_filter="IfcWall",
    )
    assert res["ok"] is True, res
    res = t.ifc_set_property(
        "walls.ifc",
        "exists.ifc",
        pset="Pset_WallCommon",
        prop="FireRating",
        value="REI60",
        class_filter="IfcWall",
    )
    assert res["ok"] is False
    assert res["error"]["code"] == "output_exists"

    res = t.ifc_set_property(
        "../evil.ifc",
        "out2.ifc",
        pset="Pset_WallCommon",
        prop="FireRating",
        value="REI60",
        class_filter="IfcWall",
    )
    assert res["ok"] is False
    assert res["error"]["code"] == "path_traversal"

    res = t.ifc_set_property(
        "walls.ifc",
        "out3.ifc",
        pset="  ",
        prop="FireRating",
        value="REI60",
        class_filter="IfcWall",
    )
    assert res["ok"] is False
    assert res["error"]["code"] == "invalid_input"
