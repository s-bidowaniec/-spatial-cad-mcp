#!/usr/bin/env python3
"""Build reproducible Gemini task ZIPs from one committed spatial-cad-mcp tree."""

import argparse
import hashlib
import json
import subprocess
from pathlib import Path, PurePosixPath
from zipfile import ZIP_DEFLATED, ZipFile, ZipInfo

REPO = Path(__file__).resolve().parents[1]
DOCS = "docs/gemini-review"
REVIEWED_COMMIT = "116e562d6bdbfeac46d5c58f84bb942085c8b4fd"
DEPENDENCIES = {
    "SC-01": (),
    "SC-02": ("SC-01",),
    "SC-03": ("SC-01", "SC-02"),
    "SC-04": ("SC-01",),
    "SC-05": ("SC-01",),
    "SC-06": (),
    "SC-07": ("SC-01", "SC-02", "SC-03", "SC-04", "SC-05", "SC-06"),
}


def git(*args: str) -> bytes:
    return subprocess.check_output(["git", *args], cwd=REPO)


def tree_files(commit: str) -> dict[str, str]:
    files = {}
    for entry in git("ls-tree", "-rz", "--full-tree", commit).split(b"\0"):
        if not entry:
            continue
        meta, path = entry.split(b"\t", 1)
        mode, kind, object_id = meta.decode().split()
        if mode in ("100644", "100755") and kind == "blob":
            files[path.decode()] = object_id
    return files


def read_blob(tree: dict[str, str], name: str) -> bytes:
    path = PurePosixPath(name)
    if not path.parts or path.is_absolute() or ".." in path.parts or name not in tree:
        raise ValueError(f"Expected a regular tracked file: {name!r}")
    return git("cat-file", "blob", tree[name])


def gemini_instructions(phase: str, commit: str) -> bytes:
    requirements = ", ".join(DEPENDENCIES[phase]) or "brak"
    return f"""# Gemini · {phase}

Implementuj zadanie w `s-bidowaniec/spatial-cad-mcp` dla bazy `{commit}`.
Zacznij od `TASK.md`, sekcji wspólnych kryteriów w `README.md` i
`input-code/AGENTS.md`. Użyj komentarzy `spatial-cad-review[{phase}]`.
Zależności to {requirements}. Dla fazy zależnej użyj paczki z commitu
zawierającego zaakceptowane wcześniejsze zmiany.

Zwróć diff względem bazy, wyniki poszczególnych kryteriów odbioru,
polecenia testowe wraz z wynikami i opis ograniczeń formatu. Oznacz
oddzielnie przypadki sprawdzone przez MCP i przypadki otwarte w niezależnym
czytniku. Użyj syntetycznych danych, a wygenerowane pliki trzymaj w katalogu
tymczasowym. Ścieżki w diffie mają być względne wobec repozytorium.
""".encode()


def write_zip(path: Path, entries: dict[str, bytes]) -> None:
    with ZipFile(path, "w", compression=ZIP_DEFLATED) as archive:
        for name, data in sorted(entries.items()):
            archive.writestr(ZipInfo(name), data, compress_type=ZIP_DEFLATED, compresslevel=9)
    with ZipFile(path) as archive:
        if set(archive.namelist()) != entries.keys() or archive.testzip() is not None:
            raise ValueError(f"Archive verification failed: {path}")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--ref", default="HEAD")
    parser.add_argument("--phase", action="append", choices=DEPENDENCIES)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()

    revision = git("rev-parse", "--verify", "--end-of-options", f"{args.ref}^{{commit}}")
    commit = revision.decode().strip()
    tree = tree_files(commit)
    manifest = read_blob(tree, f"{DOCS}/inputs.txt")
    input_paths = [line for line in manifest.decode().splitlines() if line]
    source = {f"input-code/{path}": read_blob(tree, path) for path in input_paths}

    task_paths = [f"tasks/{phase}.md" for phase in DEPENDENCIES]
    docs = {
        name: read_blob(tree, f"{DOCS}/{name}") for name in ("README.md", "REVIEW.md", *task_paths)
    }
    common = source | docs | {"inputs.txt": manifest}

    args.output.mkdir(exist_ok=True, parents=True)
    for phase in DEPENDENCIES:
        if args.phase and phase not in args.phase:
            continue
        metadata = {
            "repository": "s-bidowaniec/spatial-cad-mcp",
            "commit": commit,
            "reviewed_commit": REVIEWED_COMMIT,
            "phase": phase,
            "requires": DEPENDENCIES[phase],
        }
        files = common | {
            "TASK.md": docs[f"tasks/{phase}.md"],
            "GEMINI.md": gemini_instructions(phase, commit),
            "source.json": (json.dumps(metadata, indent=2) + "\n").encode(),
        }
        path = args.output / f"spatial-cad-{phase}-{commit[:7]}.zip"
        write_zip(path, files)
        print(f"{hashlib.sha256(path.read_bytes()).hexdigest()}  {path}")


if __name__ == "__main__":
    main()
