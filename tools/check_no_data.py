"""Reject real model data files in Git candidates (data-policy guard)."""

import os
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
MAX_BYTES = 1024 * 1024
DATA_SUFFIXES = {
    ".2dm",
    ".asc",
    ".dbf",
    ".dxf",
    ".dwg",
    ".e57",
    ".fgb",
    ".gdb",
    ".geojson",
    ".gpkg",
    ".h5",
    ".hdf5",
    ".ifc",
    ".ifcxml",
    ".inp",
    ".kml",
    ".kmz",
    ".las",
    ".laz",
    ".nc",
    ".obj",
    ".prj",
    ".res",
    ".shp",
    ".shx",
    ".slf",
    ".sqlite",
    ".sqlite3",
    ".stl",
    ".tif",
    ".tiff",
    ".vtk",
    ".vtu",
    ".xyz",
    ".zip",
}
DATABASE_SIDECARS = tuple(
    f"{suffix}-{kind}"
    for suffix in (".gpkg", ".sqlite", ".sqlite3")
    for kind in ("wal", "shm", "journal")
)


def _git_output(*args: str, input_bytes: bytes | None = None) -> bytes:
    return subprocess.run(
        ["git", *args],
        cwd=ROOT,
        check=True,
        capture_output=True,
        input=input_bytes,
    ).stdout


def _oversized_index_files() -> set[bytes]:
    blobs = []
    for entry in _git_output("ls-files", "--stage", "-z").split(b"\0"):
        if entry:
            metadata, name = entry.split(b"\t", 1)
            mode, oid, _ = metadata.split()
            if mode.startswith(b"100"):
                blobs.append((name, oid))
    if not blobs:
        return set()

    oids = b"\n".join(oid for _, oid in blobs) + b"\n"
    sizes = _git_output("cat-file", "--batch-check=%(objectsize)", input_bytes=oids)
    return {
        name
        for (name, _), size in zip(blobs, sizes.splitlines(), strict=True)
        if int(size) > MAX_BYTES
    }


def main() -> int:
    candidates = _git_output("ls-files", "-z", "--cached", "--others", "--exclude-standard")
    oversized_index = _oversized_index_files()
    violations = []
    for name in sorted(set(candidates.split(b"\0")) - {b""}):
        relative_path = Path(os.fsdecode(name))
        path = ROOT / relative_path
        forbidden_path = (
            relative_path.suffix.casefold() in DATA_SUFFIXES
            or relative_path.name.casefold().endswith(DATABASE_SIDECARS)
            or any(parent.suffix.casefold() == ".gdb" for parent in relative_path.parents)
        )
        if (
            forbidden_path
            or name in oversized_index
            or (path.is_file() and path.stat().st_size > MAX_BYTES)
        ):
            violations.append(str(relative_path))

    if violations:
        print(
            "Project data or oversized file candidates:",
            *violations,
            sep="\n",
            file=sys.stderr,
        )
        return 1
    print("OK: no real model data file candidates.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
