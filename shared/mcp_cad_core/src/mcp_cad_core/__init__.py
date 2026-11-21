"""Shared core for spatial-cad-mcp MCP servers.

Import this package from every server package in the monorepo so that
sandboxing, response envelopes, pagination and logging behave identically
across CAD/BIM/DXF/mesh tooling.

Design rules for AI-to-AI workflows (see docs/tool-design.md):

- Never write to stdout (stdout is the MCP stdio transport).
- Always resolve user-supplied paths through :func:`workspace_sandbox`.
- Always return the standard response envelope from ``mcp_cad_core.responses``.
- Keep responses compact: summaries, counts, paginated lists.
"""

from mcp_cad_core.log import configure_logging, get_logger
from mcp_cad_core.pagination import Page, paginate
from mcp_cad_core.responses import ToolError, error, ok, safe_tool
from mcp_cad_core.sandbox import (
    WORKSPACE_ROOT_ENV,
    SandboxError,
    WorkspaceSandbox,
    workspace_sandbox,
)

__all__ = [
    "Page",
    "SandboxError",
    "ToolError",
    "WORKSPACE_ROOT_ENV",
    "WorkspaceSandbox",
    "configure_logging",
    "error",
    "get_logger",
    "ok",
    "paginate",
    "safe_tool",
    "workspace_sandbox",
]
