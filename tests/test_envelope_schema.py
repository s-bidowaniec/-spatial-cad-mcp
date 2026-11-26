"""Contract tests: every tool envelope validates against schemas/envelope.v1.schema.json.

The schema is the cross-language contract between the Python servers
(mcp_cad_core.responses) and the Rust servers (cad-envelope): the golden Rust
envelopes below mirror the exact JSON produced by cad_envelope::ok_json /
err_json, so both ecosystems are pinned to the same shape.
"""

from __future__ import annotations

import json
from pathlib import Path

import jsonschema
import pytest

from mcp_cad_core.pagination import Page
from mcp_cad_core.responses import ToolError, error, ok, safe_tool

ROOT = Path(__file__).resolve().parents[1]
SCHEMA = json.loads((ROOT / "schemas" / "envelope.v1.schema.json").read_text(encoding="utf-8"))
VALIDATOR = jsonschema.Draft202012Validator(SCHEMA)


def assert_valid(envelope: dict) -> None:
    errors = list(VALIDATOR.iter_errors(envelope))
    assert not errors, [e.message for e in errors]


def assert_invalid(envelope: dict) -> None:
    assert list(VALIDATOR.iter_errors(envelope)), envelope


def test_success_envelope_validates() -> None:
    assert_valid(ok({"schema": "IFC4", "total_elements": 3}, {"tool": "ifc_summary"}))


def test_error_envelope_with_hints_and_details_validates() -> None:
    assert_valid(
        error(
            "path_outside_workspace",
            "Path resolves outside the workspace root; use a path relative to it.",
            retryable=False,
            hints=["call ifc_summary first to see available files"],
            details={"path": "../secret.ifc"},
        )
    )


def test_safe_tool_converts_exceptions_and_stays_valid() -> None:
    @safe_tool
    def boom() -> dict:
        raise ToolError("invalid_input", "limit must be >= 1", retryable=False)

    envelope = boom()
    assert_valid(envelope)
    assert envelope["ok"] is False
    assert envelope["error"]["code"] == "invalid_input"
    assert "duration_ms" in envelope["meta"]

    @safe_tool
    def crash() -> dict:
        raise RuntimeError("bug")

    envelope = crash()
    assert_valid(envelope)
    assert envelope["error"]["code"] == "internal_error"
    assert envelope["error"]["retryable"] is True


def test_pagination_meta_validates() -> None:
    page = Page(items=list(range(50)), total=312, limit=50, offset=50, limit_clamped_to=None)
    envelope = ok(
        {"items": page.items},
        {"pagination": page.meta()["pagination"], "tool": "ifc_query_elements"},
    )
    assert_valid(envelope)
    assert envelope["meta"]["pagination"]["next_offset"] == 100

    clamped = Page(items=list(range(200)), total=250, limit=500, offset=0, limit_clamped_to=200)
    assert_valid(ok({}, clamped.meta()))


def test_rust_golden_success_envelope_validates() -> None:
    # Mirrors cad_envelope::ok_json("dxf_summary", &data, meta, started).
    rust_ok = {
        "ok": True,
        "data": {"dxf_path": "drawing.dxf", "total_entities": 42},
        "meta": {"tool": "dxf_summary", "duration_ms": 3.21},
    }
    assert_valid(rust_ok)


def test_rust_golden_error_envelope_validates() -> None:
    # Mirrors cad_envelope::err_json("dxf_summary", &ToolError, started).
    rust_err = {
        "ok": False,
        "error": {
            "code": "file_not_found",
            "message": "No such file inside the workspace root.",
            "retryable": False,
            "hints": ["call dxf_summary on an existing path"],
        },
        "meta": {"tool": "dxf_summary", "duration_ms": 0.4},
    }
    assert_valid(rust_err)


def test_rust_pagination_meta_golden_validates() -> None:
    rust_page = {
        "pagination": {
            "total": 250,
            "limit": 200,
            "offset": 0,
            "returned": 200,
            "has_more": True,
            "next_offset": 200,
            "limit_clamped_to": 200,
        }
    }
    assert_valid(ok({"items": []}, rust_page))


@pytest.mark.parametrize(
    "broken",
    [
        {"ok": True, "data": {}},  # missing meta
        {
            "ok": True,
            "error": {"code": "x", "message": "m", "retryable": False},
            "meta": {},
        },  # ok:true with error
        {"ok": False, "meta": {}},  # missing error
        {
            "ok": False,
            "error": {"code": "Path-Outside", "message": "m", "retryable": False},
            "meta": {},
        },  # bad code pattern
        {"ok": False, "error": {"code": "x", "message": "m"}, "meta": {}},  # missing retryable
        ok(
            {},
            {"pagination": {"total": 1, "limit": 0, "offset": 0, "returned": 1, "has_more": False}},
        ),  # limit below 1
        ok({}, {"pagination": {"total": 1, "limit": 50}}),  # pagination missing required keys
    ],
)
def test_invalid_envelopes_are_rejected(broken: dict) -> None:
    assert_invalid(broken)
