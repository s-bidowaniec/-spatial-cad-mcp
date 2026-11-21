"""Logging for MCP servers: **stderr only**.

The default transport is stdio: MCP messages travel over stdout. A stray
``print()`` on stdout corrupts the protocol stream for the client, so every
log record in this monorepo goes to stderr. Treat any stdout write outside
the SDK as a bug.

Usage::

    from mcp_cad_core.log import configure_logging, get_logger

    configure_logging("DEBUG")  # once, at server startup
    logger = get_logger(__name__)
    logger.info("loaded %s elements", count)
"""

from __future__ import annotations

import logging
import os
import sys

_LOG_FORMAT = "%(asctime)s %(levelname)s %(name)s %(message)s"
_configured = False


def configure_logging(level: str | int | None = None) -> None:
    """Install a stderr logging handler once per process.

    Reads the level from ``MCP_CAD_LOG_LEVEL`` unless given explicitly.
    Calling it twice is a no-op unless the level changes.
    """
    global _configured
    if level is None:
        level = os.environ.get("MCP_CAD_LOG_LEVEL", "INFO").upper()
    root = logging.getLogger("mcp_cad")
    if _configured and root.level == logging.getLevelName(level):
        return

    handler = logging.StreamHandler(sys.stderr)
    handler.setFormatter(logging.Formatter(_LOG_FORMAT))
    root.handlers.clear()
    root.addHandler(handler)
    root.setLevel(level)
    # Never let the root logger write to stdout handlers inherited from
    # environments that configure logging before us.
    root.propagate = False
    _configured = True


def get_logger(name: str) -> logging.Logger:
    """Return a child logger under the shared ``mcp_cad`` stderr namespace."""
    if not name.startswith("mcp_cad"):
        name = f"mcp_cad.{name}"
    return logging.getLogger(name)
