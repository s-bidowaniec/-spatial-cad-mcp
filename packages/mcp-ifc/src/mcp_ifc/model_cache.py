"""Small LRU cache for parsed IFC models.

Agents typically fire a sequence of queries at the same file
(summary -> query -> psets). Re-opening a multi-hundred-MB IFC per tool call
is wasteful and non-deterministic in timing; this cache reuses a parsed model
while the file on disk is unchanged (mtime + size) and evicts the
least-recently used entry otherwise.
"""

from __future__ import annotations

import threading
from collections import OrderedDict
from dataclasses import dataclass
from pathlib import Path

import ifcopenshell

from mcp_cad_core.log import get_logger
from mcp_cad_core.responses import ToolError

_logger = get_logger(__name__)


@dataclass(frozen=True)
class _Entry:
    model: ifcopenshell.file
    st_mtime_ns: int
    st_size: int


class IfcModelCache:
    """Thread-safe LRU cache keyed by absolute path and file stat."""

    def __init__(self, capacity: int = 3) -> None:
        if capacity < 1:
            raise ValueError("capacity must be >= 1")
        self._capacity = capacity
        self._entries: OrderedDict[str, _Entry] = OrderedDict()
        self._lock = threading.Lock()

    def load(self, path: Path) -> ifcopenshell.file:
        """Return the parsed model for ``path``, opening/refreshing if stale."""
        stat = path.stat()  # sandbox guarantees existence before we get here
        key = str(path)
        with self._lock:
            entry = self._entries.get(key)
        if entry and entry.st_mtime_ns == stat.st_mtime_ns and entry.st_size == stat.st_size:
            with self._lock:
                self._entries.move_to_end(key)
            return entry.model

        _logger.info("opening IFC model %s", path)
        model = self._open(path)
        with self._lock:
            self._entries[key] = _Entry(model, stat.st_mtime_ns, stat.st_size)
            self._entries.move_to_end(key)
            while len(self._entries) > self._capacity:
                evicted_key, _ = self._entries.popitem(last=False)
                _logger.info("evicted cached model %s", evicted_key)
        return model

    def clear(self) -> None:
        with self._lock:
            self._entries.clear()

    @staticmethod
    def _open(path: Path) -> ifcopenshell.file:
        try:
            return ifcopenshell.open(str(path))
        except Exception as exc:
            raise ToolError(
                "cannot_open_ifc",
                "The file could not be parsed as an IFC/STEP model. "
                "Confirm it is a valid .ifc (ISO 10303-21) file.",
                details={"reason": str(exc)[:300]},
            ) from exc


_cache = IfcModelCache()


def load_ifc(path: Path) -> ifcopenshell.file:
    """Load an IFC model through the process-wide cache."""
    return _cache.load(path)
