"""Response envelope, safe_tool error handling and pagination tests."""

from __future__ import annotations

import pytest

from mcp_cad_core.pagination import MAX_LIMIT, Page, PaginationError, paginate
from mcp_cad_core.responses import ToolError, error, ok, safe_tool
from mcp_cad_core.sandbox import SandboxError


class TestEnvelopes:
    def test_ok_envelope_shape(self) -> None:
        env = ok({"value": 42}, meta={"tool": "demo"})
        assert env == {"ok": True, "data": {"value": 42}, "meta": {"tool": "demo"}}

    def test_ok_meta_defaults_to_empty(self) -> None:
        assert ok({})["meta"] == {}

    def test_error_envelope_shape(self) -> None:
        env = error("path_outside_workspace", "nope", details={"p": 1}, retryable=False)
        assert env["ok"] is False
        assert env["error"]["code"] == "path_outside_workspace"
        assert env["error"]["retryable"] is False
        assert env["error"]["details"] == {"p": 1}
        assert "hints" not in env["error"]

    def test_error_default_fields(self) -> None:
        env = error("x", "msg")
        assert env["error"]["retryable"] is False
        assert "details" not in env["error"]
        assert "hints" not in env["error"]

    def test_error_hints_listed(self) -> None:
        env = error("x", "msg", hints=["try ifc_query_elements"])
        assert env["error"]["hints"] == ["try ifc_query_elements"]


class TestSafeTool:
    def test_decorator_preserves_success_and_adds_duration(self) -> None:
        @safe_tool
        def adds(a: int, b: int) -> dict:
            return ok({"sum": a + b})

        env = adds(2, 3)
        assert env["ok"] is True
        assert env["data"] == {"sum": 5}
        assert "duration_ms" in env["meta"]

    def test_tool_error_becomes_envelope(self) -> None:
        @safe_tool
        def boom(value: str) -> dict:
            raise ToolError("element_not_found", f"no {value}", details={"value": value})

        env = boom("x")
        assert env["ok"] is False
        assert env["error"]["code"] == "element_not_found"
        assert env["error"]["details"] == {"value": "x"}
        assert "duration_ms" in env["meta"]

    def test_sandbox_error_becomes_envelope(self) -> None:
        @safe_tool
        def boom() -> dict:
            raise SandboxError("path_traversal", "stop")

        env = boom()
        assert env["error"]["code"] == "path_traversal"

    def test_unexpected_exception_becomes_internal_error(
        self, caplog: pytest.LogCaptureFixture
    ) -> None:
        @safe_tool
        def kaboom() -> dict:
            msg = "kaboom"
            raise ValueError(msg)

        env = kaboom()
        assert env["ok"] is False
        assert env["error"]["code"] == "internal_error"
        assert env["error"]["retryable"] is True


class TestPagination:
    def test_first_page(self) -> None:
        page = paginate(list(range(10)), limit=4, offset=0)
        assert isinstance(page, Page)
        assert page.items == [0, 1, 2, 3]
        assert page.total == 10
        assert page.has_more is True
        assert page.next_offset == 4

    def test_last_page(self) -> None:
        page = paginate(list(range(10)), limit=4, offset=8)
        assert page.items == [8, 9]
        assert page.has_more is False
        assert page.next_offset is None

    def test_limit_clamped_to_max(self) -> None:
        page = paginate(list(range(5)), limit=MAX_LIMIT * 100, offset=0)
        assert page.limit == MAX_LIMIT
        assert page.meta()["pagination"]["limit_clamped_to"] == MAX_LIMIT

    @pytest.mark.parametrize("limit,offset", [(0, 0), (-1, 0), (50, -1)])
    def test_invalid_arguments_raise(self, limit: int, offset: int) -> None:
        with pytest.raises(PaginationError):
            paginate(list(range(10)), limit=limit, offset=offset)
