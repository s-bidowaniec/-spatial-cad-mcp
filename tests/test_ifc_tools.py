"""IFC tool tests: happy paths, envelopes, pagination and error codes."""

from __future__ import annotations

import asyncio
from pathlib import Path
from typing import Any

import pytest

from mcp_ifc import tools
from mcp_ifc.model_cache import _cache


@pytest.fixture(autouse=True)
def _clean_model_cache() -> None:
    _cache.clear()


def data_of(env: dict[str, Any]) -> dict[str, Any]:
    assert env["ok"] is True, env
    return env["data"]


def err_code(env: dict[str, Any]) -> str:
    assert env["ok"] is False, env
    return env["error"]["code"]


class TestIfcSummary:
    def test_summary_happy_path(self, ifc_path: Path) -> None:
        env = tools.ifc_summary("model.ifc")
        data = data_of(env)
        assert data["schema"] == "IFC4"
        assert data["length_unit"] is None  # no IfcUnitAssignment in the minimal model
        assert data["total_products"] == 4  # IfcSite + 2 walls + 1 door (all IfcProduct)
        counts = {c["ifc_class"]: c["count"] for c in data["class_counts"]}
        assert counts["IfcWall"] == 2
        assert counts["IfcDoor"] == 1
        assert counts["IfcSite"] == 1
        assert env["meta"]["duration_ms"] >= 0

    def test_missing_file(self, workspace: Path) -> None:
        env = tools.ifc_summary("nope.ifc")
        assert err_code(env) == "file_not_found"

    def test_not_an_ifc_file(self, workspace: Path) -> None:
        (workspace / "garbage.ifc").write_text("this is not a STEP file\n")
        env = tools.ifc_summary("garbage.ifc")
        assert err_code(env) == "cannot_open_ifc"

    def test_escaping_path(self, workspace: Path, tmp_path: Path) -> None:
        outside = tmp_path / "outside.ifc"
        outside.write_text("x")
        env = tools.ifc_summary(str(outside))
        assert err_code(env) == "path_outside_workspace"

    def test_unconfigured_workspace(self, workspace: Path, monkeypatch: pytest.MonkeyPatch) -> None:
        monkeypatch.delenv("MCP_CAD_WORKSPACE_ROOT", raising=False)
        env = tools.ifc_summary("model.ifc")
        assert err_code(env) == "workspace_not_configured"


class TestIfcQueryElements:
    def test_lists_all_products(self, ifc_path: Path) -> None:
        env = tools.ifc_query_elements("model.ifc")
        data = data_of(env)
        names = {e["name"] for e in data["elements"]}
        assert names == {"Wall-A", "Wall-B", "Door-1", "Test Site"}
        assert all(e["global_id"] for e in data["elements"])

    def test_filter_by_class(self, ifc_path: Path) -> None:
        data = data_of(tools.ifc_query_elements("model.ifc", ifc_class="IfcWall"))
        assert [e["name"] for e in data["elements"]] == ["Wall-A", "Wall-B"]

    def test_filter_by_name_regex_case_insensitive(self, ifc_path: Path) -> None:
        data = data_of(tools.ifc_query_elements("model.ifc", name_pattern=r"^wall-[ab]$"))
        assert len(data["elements"]) == 2

    def test_pagination_metadata(self, ifc_path: Path) -> None:
        env = tools.ifc_query_elements("model.ifc", limit=1, offset=0)
        data = data_of(env)
        assert len(data["elements"]) == 1
        pagination = env["meta"]["pagination"]
        assert pagination["total"] == 4
        assert pagination["has_more"] is True
        assert pagination["next_offset"] == 1

    def test_unknown_class(self, ifc_path: Path) -> None:
        env = tools.ifc_query_elements("model.ifc", ifc_class="Ifcunicorn")
        assert err_code(env) == "invalid_input"

    def test_invalid_regex(self, ifc_path: Path) -> None:
        env = tools.ifc_query_elements("model.ifc", name_pattern="([unclosed")
        assert err_code(env) == "invalid_input"

    @pytest.mark.parametrize("limit,offset", [(0, 0), (-5, 0), (10, -1)])
    def test_invalid_pagination_args(self, ifc_path: Path, limit: int, offset: int) -> None:
        env = tools.ifc_query_elements("model.ifc", limit=limit, offset=offset)
        assert err_code(env) == "invalid_input"


class TestIfcGetPsets:
    def test_psets_happy_path(self, ifc_path: Path) -> None:
        elements = data_of(tools.ifc_query_elements("model.ifc"))["elements"]
        wall_a = next(e for e in elements if e["name"] == "Wall-A")

        env = tools.ifc_get_psets("model.ifc", wall_a["global_id"])
        data = data_of(env)
        assert data["ifc_class"] == "IfcWall"
        assert data["psets"]["Pset_WallCommon"] == {"LoadBearing": True, "IsExternal": True}
        assert data["pset_count"] == 1

    def test_unknown_global_id(self, ifc_path: Path) -> None:
        env = tools.ifc_get_psets("model.ifc", "0123456789abcdefghijk")
        assert err_code(env) == "element_not_found"

    def test_empty_global_id(self, ifc_path: Path) -> None:
        env = tools.ifc_get_psets("model.ifc", "   ")
        assert err_code(env) == "invalid_input"


class TestIfcValidateRequiredProps:
    def test_reports_missing_properties(self, ifc_path: Path) -> None:
        env = tools.ifc_validate_required_props(
            "model.ifc", ifc_class="IfcWall", properties=["LoadBearing", "IsExternal"]
        )
        data = data_of(env)
        summary = data["summary"]
        assert summary["checked"] == 2
        assert summary["elements_with_issues"] == 1
        assert summary["elements_passing"] == 1
        issue = data["issues"][0]
        assert issue["name"] == "Wall-B"
        assert set(issue["missing"]) == {"LoadBearing", "IsExternal"}

    def test_scoped_pset_audit(self, ifc_path: Path) -> None:
        # Wall-B has no Pset_WallCommon, so requiring LoadBearing there flags it.
        env = tools.ifc_validate_required_props(
            "model.ifc",
            ifc_class="IfcWall",
            pset_name="pset_wallcommon",
            properties=["LoadBearing"],
        )
        data = data_of(env)
        assert data["summary"]["elements_with_issues"] == 1
        assert data["summary"]["checked"] == 2

    def test_scoped_audit_other_pset(self, ifc_path: Path) -> None:
        # Requiring AcousticRating inside Pset_WallAcoustic flags Wall-A instead.
        env = tools.ifc_validate_required_props(
            "model.ifc",
            ifc_class="IfcWall",
            pset_name="Pset_WallAcoustic",
            properties=["AcousticRating"],
        )
        data = data_of(env)
        assert data["summary"]["elements_with_issues"] == 1
        assert data["issues"][0]["name"] == "Wall-A"

    def test_requires_properties(self, ifc_path: Path) -> None:
        env = tools.ifc_validate_required_props("model.ifc", ifc_class="IfcWall", properties=[])
        assert err_code(env) == "invalid_input"


class TestServerRegistration:
    def test_all_tools_registered_with_agent_friendly_names(self) -> None:
        from mcp_ifc import server

        assert server.mcp.name == "mcp-ifc"
        registered = asyncio.run(server.mcp.list_tools())
        names = {tool.name for tool in registered}
        assert names == {
            "ifc_summary",
            "ifc_query_spatial",
            "ifc_query_elements",
            "ifc_get_psets",
            "ifc_validate_required_props",
        }
