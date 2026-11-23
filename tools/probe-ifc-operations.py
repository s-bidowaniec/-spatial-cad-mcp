#!/usr/bin/env python3
"""Exercise IFC geometry APIs against disposable synthetic IFC4 models."""

import json
from pathlib import Path
from tempfile import TemporaryDirectory

import ifcopenshell
import ifcopenshell.geom
import numpy as np
from ifcopenshell.api import context, feature, geometry, root, unit
from ifcopenshell.util.placement import get_local_placement
from ifcopenshell.util.unit import calculate_unit_scale


def make_model():
    model = ifcopenshell.file(schema="IFC4")
    root.create_entity(model, ifc_class="IfcProject", name="Synthetic IFC operations")
    unit.assign_unit(model)
    parent = context.add_context(model, context_type="Model")
    body = context.add_context(
        model,
        context_type="Model",
        context_identifier="Body",
        target_view="MODEL_VIEW",
        parent=parent,
    )
    return model, body


def add_box(model, body, ifc_class, name, location, size):
    product = root.create_entity(model, ifc_class=ifc_class, name=name)
    length, width, height = size
    rep = geometry.add_wall_representation(
        model, context=body, length=length, thickness=width, height=height
    )
    geometry.assign_representation(model, product=product, representation=rep)
    matrix = np.eye(4)
    matrix[:3, 3] = location
    geometry.edit_object_placement(model, product=product, matrix=matrix, is_si=True)
    return product


def clash_tree(model, objects, *, cut_openings=True):
    settings = ifcopenshell.geom.settings()
    settings.set("disable-opening-subtractions", not cut_openings)
    iterator = ifcopenshell.geom.iterator(settings, model, 1, include=objects)
    assert iterator.initialize()
    tree = ifcopenshell.geom.tree()
    indexed_ids = set()
    while True:
        shape = iterator.get()
        tree.add_element(shape)
        indexed_ids.add(shape.id)
        if not iterator.next():
            break
    assert indexed_ids == {obj.id() for obj in objects}
    return tree


def probe_openings_and_clashes():
    model, body = make_model()
    host = add_box(model, body, "IfcWall", "Host", (0, 0, 0), (4, 0.3, 3))
    opening = add_box(
        model, body, "IfcOpeningElement", "Passage", (1.6, -0.1, -0.05), (0.8, 0.5, 2.25)
    )
    feature.add_feature(model, feature=opening, element=host)

    def probe(name, point, size):
        return add_box(model, body, "IfcBuildingElementProxy", name, point, size)

    solid = probe("Crosses wall", (0.6, -0.4, 0.8), (0.2, 1.1, 0.2))
    through = probe("Crosses opening", (1.8, -0.4, 0.8), (0.2, 1.1, 0.2))
    touching = probe("Touches wall", (3.0, 0.30, 0.8), (0.2, 0.2, 0.2))
    gap = probe("40 mm gap", (3.0, 0.34, 0.8), (0.2, 0.2, 0.2))

    objects = [host, solid, through, touching, gap]
    cut = clash_tree(model, objects)
    gross = clash_tree(model, objects, cut_openings=False)
    assert cut.clash_intersection_many([host], [solid], tolerance=0.002)
    assert not cut.clash_collision_many([host], [through])
    assert gross.clash_collision_many([host], [through])
    assert not cut.clash_collision_many([host], [touching], allow_touching=False)
    assert cut.clash_collision_many([host], [touching], allow_touching=True)
    assert not cut.clash_collision_many([host], [gap], allow_touching=True)
    (clearance,) = cut.clash_clearance_many([host], [gap], clearance=0.06, check_all=True)
    assert np.isclose(clearance.distance, 0.04, atol=1e-6, rtol=0)

    feature.remove_feature(model, feature=opening)
    repaired = clash_tree(model, objects)
    assert repaired.clash_collision_many([host], [through])
    return {
        "positive_overlap": True,
        "touch_detected_separately": True,
        "clearance_m": round(clearance.distance, 6),
        "passage_disappears_when_opening_removed": True,
    }


def world_vertices(product):
    settings = ifcopenshell.geom.settings()
    settings.set("use-world-coords", True)
    shape = ifcopenshell.geom.create_shape(settings, product)
    return np.asarray(shape.geometry.verts).reshape(-1, 3)


def probe_rotation_and_extension(directory):
    model, body = make_model()
    edited = add_box(model, body, "IfcWall", "Edited wall", (2, 3, 0), (4, 0.2, 3))
    neighbour = add_box(model, body, "IfcWall", "Neighbour", (0, 10, 0), (4, 0.2, 3))
    original_neighbour_rep = neighbour.Representation.Representations[0]
    original_edited_rep = edited.Representation.Representations[0]
    geometry.unassign_representation(
        model, product=neighbour, representation=original_neighbour_rep
    )
    geometry.assign_representation(model, product=neighbour, representation=original_edited_rep)
    assert neighbour.Representation.Representations[0] == original_edited_rep
    neighbour_vertices = world_vertices(neighbour)
    guids = edited.GlobalId, neighbour.GlobalId

    replacement = geometry.add_wall_representation(
        model, context=body, length=5, thickness=0.2, height=3
    )
    geometry.unassign_representation(model, product=edited, representation=original_edited_rep)
    geometry.assign_representation(model, product=edited, representation=replacement)

    # Rotate 90 degrees around the wall's start point in model coordinates.
    pivot = np.array([2, 3, 0])
    matrix = np.eye(4)
    matrix[:3, :3] = [[0, -1, 0], [1, 0, 0], [0, 0, 1]]
    matrix[:3, 3] = pivot
    geometry.edit_object_placement(model, product=edited, matrix=matrix, is_si=True)

    candidate = directory / "synthetic-wall-edit.ifc"
    model.write(candidate)
    reopened = ifcopenshell.open(candidate)
    assert calculate_unit_scale(reopened) == 0.001
    edited, neighbour = map(reopened.by_guid, guids)
    np.testing.assert_allclose(world_vertices(neighbour), neighbour_vertices, atol=1e-9, rtol=0)
    reloaded_matrix = get_local_placement(edited.ObjectPlacement)
    reloaded_matrix[:3, 3] *= calculate_unit_scale(reopened)
    np.testing.assert_allclose(reloaded_matrix, matrix, atol=1e-9, rtol=0)
    points = world_vertices(edited)
    np.testing.assert_allclose(points.min(axis=0), [1.8, 3, 0], atol=1e-9, rtol=0)
    np.testing.assert_allclose(points.max(axis=0), [2, 8, 3], atol=1e-9, rtol=0)
    return {
        "wall_length_m": 5,
        "wall_width_m": 0.2,
        "wall_height_m": 3,
        "rotation_about_model_z_deg": 90,
        "pivot_m": pivot.tolist(),
        "shared_representation_neighbour_unchanged": True,
    }


def main():
    with TemporaryDirectory(prefix="spatial-ifc-probes-") as scratch:
        result = {
            "synthetic": True,
            "ifcopenshell": ifcopenshell.version,
            "clashes_and_openings": probe_openings_and_clashes(),
            "wall_edit": probe_rotation_and_extension(Path(scratch)),
        }
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
