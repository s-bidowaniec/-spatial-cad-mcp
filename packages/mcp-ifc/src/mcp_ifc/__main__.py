"""Allow `python -m mcp_ifc` alongside the `mcp-ifc` console script."""

from mcp_ifc.server import main

if __name__ == "__main__":
    main()
