"""Deterministic limit/offset pagination for large result sets.

CAD/BIM files routinely contain tens of thousands of elements. Tools must
never dump a full element tree into an agent's context; they slice results
into pages with predictable metadata so an agent can decide whether to
fetch more.
"""

from __future__ import annotations

from collections.abc import Sequence
from dataclasses import dataclass
from typing import Any

DEFAULT_LIMIT = 50
MAX_LIMIT = 200

#: Maximum length of a stringified value before it is truncated in responses.
MAX_VALUE_CHARS = 300


class PaginationError(ValueError):
    """Raised on structurally invalid pagination arguments."""


@dataclass(frozen=True)
class Page:
    """One page of a result set plus paging metadata."""

    items: list[Any]
    total: int
    limit: int
    offset: int
    limit_clamped_to: int | None = None

    @property
    def has_more(self) -> bool:
        return self.offset + len(self.items) < self.total

    @property
    def next_offset(self) -> int | None:
        return self.offset + self.limit if self.has_more else None

    def meta(self) -> dict[str, Any]:
        """Envelope ``meta`` block describing this page."""
        pagination: dict[str, Any] = {
            "total": self.total,
            "limit": self.limit,
            "offset": self.offset,
            "returned": len(self.items),
            "has_more": self.has_more,
            "next_offset": self.next_offset,
        }
        if self.limit_clamped_to is not None:
            pagination["limit_clamped_to"] = self.limit_clamped_to
        return {"pagination": pagination}


def _normalize(limit: int, offset: int) -> tuple[int, int]:
    if limit is None or offset is None:  # pragma: no cover - pydantic supplies defaults
        raise PaginationError("limit and offset are required")
    if not isinstance(limit, int) or not isinstance(offset, int):  # pragma: no cover - typed layer
        raise PaginationError("limit and offset must be integers")
    if offset < 0:
        raise PaginationError("offset must be >= 0")
    if limit < 1:
        raise PaginationError("limit must be >= 1")
    return (min(limit, MAX_LIMIT), offset)


def paginate(items: Sequence[Any], *, limit: int = DEFAULT_LIMIT, offset: int = 0) -> Page:
    """Slice ``items`` into one deterministic :class:`Page`.

    Limits larger than :data:`MAX_LIMIT` are clamped (agents guessing
    ``limit=10000`` still get a bounded answer, and the effective limit is
    always visible in ``meta``).
    """
    limit_n, offset_n = _normalize(limit, offset)
    total = len(items)
    window = list(items[offset_n : offset_n + limit_n])
    clamped_to = MAX_LIMIT if limit_n < limit else None
    return Page(items=window, total=total, limit=limit_n, offset=offset_n, limit_clamped_to=clamped_to)


def truncate_value(value: Any, max_chars: int = MAX_VALUE_CHARS) -> Any:
    """Stringify and truncate a long value so responses stay compact.

    Non-string values are stringified only when they exceed ``max_chars``;
    primitives (bool/int/float/None) are returned unchanged for clean JSON.
    """
    if value is None or isinstance(value, bool | int | float):
        return value
    text = value if isinstance(value, str) else repr(value)
    if len(text) > max_chars:
        return text[:max_chars] + f"... (+{len(text) - max_chars} chars)"
    return text
