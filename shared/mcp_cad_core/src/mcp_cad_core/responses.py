"""Standardized JSON response and error envelopes for AI-to-AI tool calls.

Every tool in this monorepo returns one of two shapes (and nothing else):

Success::

    {"ok": true, "data": {...}, "meta": {...}}

Failure::

    {"ok": false,
     "error": {"code": "machine_readable_code", "message": "human explanation",
               "retryable": false, "hints": ["..."], "details": {...}},
     "meta": {...}}

Rationale (docs/tool-design.md): LLM agents must be able to branch on
``ok`` and ``error.code`` without parsing prose, and one failure mode must
never corrupt the ability to reason about the next call.

Use :func:`ok` / :func:`error` to build envelopes and the :func:`safe_tool`
decorator to guarantee that *any* exception exits as an envelope instead of
an opaque transport error.
"""

from __future__ import annotations

import functools
import time
from collections.abc import Callable
from typing import Any, TypeVar, cast

from mcp_cad_core.log import get_logger
from mcp_cad_core.sandbox import SandboxError

F = TypeVar("F", bound=Callable[..., dict[str, Any]])

_logger = get_logger(__name__)


class ToolError(Exception):
    """Deterministic, tool-level failure mapped onto the error envelope.

    Raise inside tools for expected failure modes (bad input, unknown
    GlobalId, unsupported schema, ...). Never raise it for bugs: those are
    caught by :func:`safe_tool` and reported as ``internal_error``.
    """

    def __init__(
        self,
        code: str,
        message: str,
        *,
        details: dict[str, Any] | None = None,
        retryable: bool = False,
        hints: list[str] | None = None,
    ) -> None:
        self.code = code
        self.message = message
        self.details = details
        self.retryable = retryable
        self.hints = hints
        super().__init__(message)


def ok(data: dict[str, Any], meta: dict[str, Any] | None = None) -> dict[str, Any]:
    """Build a success envelope.

    Args:
        data: Tool payload. Keep it compact and agent-friendly.
        meta: Optional execution metadata (pagination, timings, ...).
    """
    return {"ok": True, "data": data, "meta": meta or {}}


def error(
    code: str,
    message: str,
    meta: dict[str, Any] | None = None,
    **fields: Any,
) -> dict[str, Any]:
    """Build a failure envelope.

    Args:
        code: Short machine-readable code, e.g. ``path_outside_workspace``.
        message: One-sentence human explanation (also read by agents).
        meta: Optional execution metadata.
        **fields: Optional extra fields merged into the error object
            (``details``, ``retryable: bool``, ``hints: list[str]``).
    """
    err: dict[str, Any] = {"code": code, "message": message}
    if "details" in fields:
        err["details"] = fields["details"]
    err["retryable"] = bool(fields.get("retryable", False))
    if fields.get("hints"):
        err["hints"] = list(fields["hints"])
    return {"ok": False, "error": err, "meta": meta or {}}


def _error_from_exception(exc: BaseException) -> dict[str, Any]:
    if isinstance(exc, SandboxError):
        return error(exc.code, exc.message)
    if isinstance(exc, ToolError):
        return error(
            exc.code, exc.message, details=exc.details, retryable=exc.retryable, hints=exc.hints
        )
    return error(
        "internal_error",
        "Unexpected error while executing the tool. See server stderr logs for details and retry.",
        retryable=True,
    )


def safe_tool(func: F) -> F:
    """Decorator that guarantees a tool always returns a response envelope.

    Catches :class:`ToolError` and :class:`SandboxError` as deterministic
    failures; any other exception is logged (stderr) and returned as
    ``internal_error``. Adds ``duration_ms`` to ``meta`` so agents can reason
    about cost.

    FastMCP sees the original signature (via ``functools.wraps``), so the
    generated JSON Schema keeps full parameter descriptions.
    """

    @functools.wraps(func)
    def wrapper(*args: Any, **kwargs: Any) -> dict[str, Any]:
        start = time.perf_counter()
        try:
            result = func(*args, **kwargs)
        except (ToolError, SandboxError) as exc:
            env = _error_from_exception(exc)
        except Exception:
            _logger.exception("Unhandled error in tool %s", func.__name__)
            env = _error_from_exception(RuntimeError())
        else:
            duration_ms = round((time.perf_counter() - start) * 1000, 2)
            if isinstance(result, dict):
                result.setdefault("meta", {})
                if isinstance(result.get("meta"), dict):
                    result["meta"].setdefault("duration_ms", duration_ms)
            return result
        duration_ms = round((time.perf_counter() - start) * 1000, 2)
        env["meta"]["duration_ms"] = duration_ms
        return env

    return cast(F, wrapper)
