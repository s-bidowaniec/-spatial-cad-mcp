"""Check repository Markdown links, anchors, cited paths and AGENTS.md.

Git mode reads tracked Markdown from the working tree and requires link and
citation targets to be tracked. Use --no-git to check a standalone tree.
Print problems to stdout and return 1 on failure, or 0 when clean.
"""

from __future__ import annotations

import argparse
import os
import re
import string
import subprocess
import sys
from collections.abc import Iterator, Sequence
from html import unescape
from pathlib import Path
from urllib.parse import unquote

DEFAULT_AGENTS_LIMIT = 120
INDEX_REL = Path("docs/index.md")
AGENTS_REL = Path("AGENTS.md")

_SKIP_DIRS = frozenset({".git", ".venv", ".pytest_cache", "__pycache__", "node_modules"})
_FILE_SUFFIXES = frozenset(
    {
        ".cfg",
        ".css",
        ".gif",
        ".html",
        ".ico",
        ".ini",
        ".jpeg",
        ".jpg",
        ".js",
        ".json",
        ".md",
        ".pdf",
        ".png",
        ".py",
        ".sh",
        ".svg",
        ".toml",
        ".txt",
        ".webp",
        ".yaml",
        ".yml",
    }
)
_ROOT_FILES = frozenset({"Dockerfile", "LICENSE", "Makefile"})
_SCHEME = re.compile(r"^[a-zA-Z][a-zA-Z0-9+.-]*:")
_IDENTIFIER_PATH = re.compile(r"[A-Z]+\d+(?:/[A-Z]+\d+)+")
_FENCE = re.compile(r"^ {0,3}(`{3,}|~{3,})(.*)$")
_ATX = re.compile(r"^ {0,3}#{1,6}(?:[ \t]+(.*)|$)")
_SETEXT = re.compile(r"^ {0,3}(?:=+|-+)[ \t]*$")
_PARAGRAPH_BREAK = re.compile(
    r"^ {0,3}(?:>|[-+*][ \t]|\d{1,9}[.)][ \t]|\||(?:\*[ \t]*){3,}$"
    r"|(?:_[ \t]*){3,}$|(?:-[ \t]*){3,}$)"
)
_CODE = re.compile(r"(?<!`)(`+)(?!`)((?:(?!\n[ \t]*\n).)+?)(?<!`)\1(?!`)", re.S)
_ESCAPE = re.compile(r"\\([" + re.escape(string.punctuation) + r"])")
_ANGLE_DESTINATION = re.compile(r"<((?:\\[^\n]|[^<>\n])*)>")
_DESTINATION_START = re.compile(r"[^\s()<>\\]+|\\[" + re.escape(string.punctuation) + r"]|[()\\]")
_TITLE = re.compile(r"""(?:"(?:\\.|[^"\\])*"|'(?:\\.|[^'\\])*'|\((?:\\.|[^()\\])*\))""")
_LABEL_TEXT = r"(?:\\[^\n]|[^\[\]\\]|\[(?:\\[^\n]|[^\[\]\\])*\])*"
_LABEL = re.compile(r"\[(" + _LABEL_TEXT + r")\]")
_REFERENCE = re.compile(r"^ {0,3}\[([^\[\]\n]+)\]:[ \t]*(?:\n[ \t]*)?", re.M)
_INLINE_HTML = re.compile(r"</?[A-Za-z][A-Za-z0-9-]*(?:\s+(?:\"[^\"]*\"|'[^']*'|[^'\"<>])*)?\s*/?>")
_EMPHASIS = re.compile(
    r"(?<!\w)(__|_)(?!\s)(.+?)(?<!\s)\1(?!\w)|"
    r"(?<!\*)(\*{1,3})(?![\s*])(.+?)(?<!\s)\3(?!\*)"
)


def _mask(text: str) -> str:
    return re.sub(r"[^\n]", " ", text)


def _visible_markdown(text: str) -> str:
    """Blank fences and comments without moving source positions."""
    lines: list[str] = []
    fence = ""
    in_comment = False
    for line in text.splitlines(keepends=True):
        match = _FENCE.fullmatch(line.rstrip("\n"))
        if fence:
            if match and match[1].startswith(fence) and not match[2].strip():
                fence = ""
            lines.append(_mask(line))
            continue
        if not in_comment and match and (match[1][0] == "~" or "`" not in match[2]):
            fence = match[1]
            lines.append(_mask(line))
            continue
        parts: list[str] = []
        while line:
            marker = "-->" if in_comment else "<!--"
            before, found, after = line.partition(marker)
            parts.append(_mask(before) if in_comment else before)
            if found:
                parts.append(_mask(found))
                in_comment = not in_comment
            line = after
        lines.append("".join(parts))
    return "".join(lines)


def _skip_space(text: str, offset: int) -> int:
    while offset < len(text) and text[offset] in " \t\n":
        offset += 1
    return offset


def _destination(text: str, start: int) -> tuple[str, int] | None:
    if text.startswith("<", start):
        match = _ANGLE_DESTINATION.match(text, start)
        return (match[1], match.end()) if match else None
    end = start
    depth = 0
    while (match := _DESTINATION_START.match(text, end)) is not None:
        token = match[0]
        if token == ")":
            if depth == 0:
                break
            depth -= 1
        elif token == "(":
            depth += 1
        end = match.end()
    return (text[start:end], end) if depth == 0 else None


def _inline_destination(text: str, delimiter: int) -> tuple[str, int] | None:
    if not text.startswith("(", delimiter):
        return None
    start = _skip_space(text, delimiter + 1)
    dest = _destination(text, start)
    if dest is None:
        return None
    target, end = dest
    close = _skip_space(text, end)
    if close > end and (title := _TITLE.match(text, close)):
        close = _skip_space(text, title.end())
    return (target, close + 1) if text.startswith(")", close) else None


def _label_key(label: str) -> str:
    return " ".join(label.split()).casefold()


def _reference_definitions(text: str) -> Iterator[tuple[re.Match[str], str, int]]:
    for match in _REFERENCE.finditer(text):
        dest = _destination(text, match.end())
        if dest is None:
            continue
        target, end = dest
        if not target and end == match.end():
            continue
        title_start = _skip_space(text, end)
        if title_start > end and (title := _TITLE.match(text, title_start)):
            end = title.end()
        line_end = text.find("\n", end)
        line_end = len(text) if line_end < 0 else line_end
        if not text[end:line_end].strip():
            yield match, target, line_end


def _render_heading(heading: str, references: set[str] | None) -> str:
    literals: list[str] = []

    def protect(value: str) -> str:
        literals.append(value)
        return f"\0{len(literals) - 1}\0"

    def code_text(match: re.Match[str]) -> str:
        value = match[2].replace("\n", " ")
        if value.startswith(" ") and value.endswith(" ") and value.strip(" "):
            value = value[1:-1]
        return protect(value)

    heading = _CODE.sub(code_text, heading)
    heading = _ESCAPE.sub(lambda match: protect(match[1]), heading)
    parts: list[str] = []
    offset = 0
    while (label := _LABEL.search(heading, offset)) is not None:
        end = label.end()
        dest = _inline_destination(heading, end)
        reference = _LABEL.match(heading, end)
        if dest is not None:
            end = dest[1]
        elif reference is not None and (
            references is None or _label_key(reference[1] or label[1]) in references
        ):
            end = reference.end()
        parts.extend((heading[offset : label.start()], label[1]))
        offset = end
    parts.append(heading[offset:])
    heading = _INLINE_HTML.sub("", "".join(parts))
    while (plain := _EMPHASIS.sub(lambda match: match[2] or match[4], heading)) != heading:
        heading = plain
    return re.sub(r"\0(\d+)\0", lambda match: literals[int(match[1])], unescape(heading))


def github_slug(heading: str, *, references: set[str] | None = None) -> str:
    """Return the GitHub anchor slug of the rendered heading."""
    rendered = _render_heading(heading, references).strip().lower()
    return re.sub(r"[^\w\- ]", "", rendered).replace(" ", "-")


def heading_anchors(text: str) -> set[str]:
    """Return generated anchors, reserving every assigned slug."""
    body = _visible_markdown(text)
    references = {_label_key(match[1]) for match, _, _ in _reference_definitions(body)}
    anchors: set[str] = set()
    next_suffix: dict[str, int] = {}
    paragraph: list[str] = []
    for line in body.splitlines():
        if match := _ATX.fullmatch(line):
            heading = re.sub(r"(?:^|[ \t])#+[ \t]*$", "", match[1] or "")
        elif paragraph and _SETEXT.fullmatch(line):
            heading = " ".join(paragraph)
        else:
            if not line.strip() or line.startswith(("    ", "\t")) or _PARAGRAPH_BREAK.match(line):
                paragraph.clear()
            else:
                paragraph.append(line.strip())
            continue
        paragraph.clear()
        base = github_slug(heading, references=references)
        slug = base
        suffix = next_suffix.get(base, 1)
        while slug in anchors:
            slug = f"{base}-{suffix}"
            suffix += 1
        anchors.add(slug)
        next_suffix[base] = suffix
    return anchors


def _link_parts(text: str, start: int, target: str) -> tuple[int, str, str]:
    target = unescape(_ESCAPE.sub(r"\1", target))
    path, _, anchor = target.partition("#")
    return (
        text.count("\n", 0, start) + 1,
        unquote(path.split("?", 1)[0]),
        unquote(anchor),
    )


def iter_links(text: str) -> Iterator[tuple[int, str, str]]:
    """Yield source line, path and anchor for inline links and reference definitions."""
    body = _visible_markdown(text)
    body = _CODE.sub(lambda match: _mask(match[0]), body)
    definitions = list(_reference_definitions(body))
    sections: list[str] = []
    start = 0
    for match, _, end in definitions:
        sections.extend((body[start : match.start()], _mask(body[match.start() : end])))
        start = end
    sections.append(body[start:])
    inline_body = "".join(sections)
    links = [(match.start(), target) for match, target, _ in definitions]
    offset = 0
    while (label := _LABEL.search(inline_body, offset)) is not None:
        offset = label.end()
        if dest := _inline_destination(inline_body, offset):
            target, offset = dest
            links.append((label.start(), target))
    for start, target in sorted(links):
        yield _link_parts(body, start, target)


def cited_paths(text: str) -> list[str]:
    """Return code spans of the index that look like repo file paths."""
    paths: list[str] = []
    for match in _CODE.finditer(_visible_markdown(text)):
        candidate = match[2].strip()
        if not candidate or any(character.isspace() for character in candidate):
            continue
        if _SCHEME.match(candidate) or candidate.startswith("//"):
            continue
        path = candidate.split("#", 1)[0].split("::", 1)[0].rstrip("/")
        if not path or _IDENTIFIER_PATH.fullmatch(path):
            continue
        if (
            Path(path).suffix.lower() in _FILE_SUFFIXES
            or path in _ROOT_FILES
            or ("/" in path and re.search(r"[^\W\d_]", path.rsplit("/", 1)[-1]))
        ):
            paths.append(path)
    return paths


def _tracked_paths(root: Path) -> set[Path]:
    run = subprocess.run(
        ["git", "-C", str(root), "ls-files", "-z", "--cached"],
        capture_output=True,
        check=True,
    )
    return {root / os.fsdecode(name) for name in run.stdout.split(b"\0") if name}


def check_tree(
    root: Path,
    *,
    agents_limit: int = DEFAULT_AGENTS_LIMIT,
    use_git: bool = True,
) -> list[str]:
    """Return one plain-text problem line per violated invariant."""
    root = root.resolve()
    problems: list[str] = []
    tracked: set[Path] | None = None
    if use_git:
        try:
            tracked = _tracked_paths(root)
        except OSError as error:
            return [f"Git discovery failed (cannot run git): {error}"]
        except subprocess.CalledProcessError as error:
            return [f"Git discovery failed: {os.fsdecode(error.stderr).strip()}"]
        markdown = sorted(path for path in tracked if path.suffix.lower() == ".md")
        if not markdown:
            return [
                "Git discovery found no tracked Markdown files (use --no-git for standalone trees)"
            ]
    else:
        markdown = sorted(
            path
            for path in root.rglob("*")
            if path.suffix.lower() == ".md"
            and path.is_file()
            and not _SKIP_DIRS.intersection(path.relative_to(root).parts)
        )
    known_paths = (
        {
            parent
            for path in tracked
            for parent in (path, *path.parents)
            if parent.is_relative_to(root)
        }
        if tracked is not None
        else None
    )

    def validate_target(target: Path) -> str | None:
        if not target.is_relative_to(root) or not target.resolve().is_relative_to(root):
            return "outside repository"
        if not target.exists():
            return "missing"
        if known_paths is not None and target not in known_paths:
            return "untracked"
        return None

    texts: dict[Path, str] = {}
    for path in markdown:
        try:
            if not path.resolve().is_relative_to(root):
                problems.append(
                    f"{path.relative_to(root).as_posix()}: Markdown is outside repository"
                )
                continue
            texts[path] = path.read_text(encoding="utf-8")
        except (OSError, UnicodeDecodeError) as error:
            problems.append(f"{path.relative_to(root).as_posix()}: cannot read Markdown ({error})")
    anchors = {path: heading_anchors(body) for path, body in texts.items()}

    for path, body in texts.items():
        relative = path.relative_to(root).as_posix()
        for line, link_path, anchor in iter_links(body):
            if _SCHEME.match(link_path) or link_path.startswith("//"):
                continue
            if link_path:
                base = root if link_path.startswith("/") else path.parent
                target = Path(os.path.normpath(base / link_path.lstrip("/")))
            else:
                target = path
            if failure := validate_target(target):
                problems.append(f"{relative}:{line}: broken link -> {link_path!r} ({failure})")
            elif anchor and target in anchors and anchor not in anchors[target]:
                problems.append(
                    f"{relative}:{line}: broken anchor -> {link_path}#{anchor} (missing)"
                )

    index_text = texts.get(root / INDEX_REL)
    if index_text is None:
        problems.append(f"{INDEX_REL.as_posix()}: missing or unreadable")
    else:
        for cited in cited_paths(index_text):
            if failure := validate_target(Path(os.path.normpath(root / cited))):
                problems.append(f"{INDEX_REL.as_posix()}: missing cited path `{cited}` ({failure})")

    agents_text = texts.get(root / AGENTS_REL)
    if agents_text is None:
        problems.append(f"{AGENTS_REL.as_posix()}: missing or unreadable")
    elif (line_count := len(agents_text.splitlines())) > agents_limit:
        problems.append(
            f"{AGENTS_REL.as_posix()}: {line_count} lines exceeds the limit of {agents_limit}"
        )
    return problems


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--root",
        type=Path,
        default=Path(__file__).resolve().parents[1],
        help="repository root to check (default: the repo containing this tool)",
    )
    parser.add_argument(
        "--agents-limit",
        type=int,
        default=DEFAULT_AGENTS_LIMIT,
        help=f"maximum line count of {AGENTS_REL} (default: {DEFAULT_AGENTS_LIMIT})",
    )
    parser.add_argument(
        "--no-git",
        action="store_true",
        help="walk the file tree instead of asking Git for tracked files",
    )
    args = parser.parse_args(argv)
    problems = check_tree(args.root, agents_limit=args.agents_limit, use_git=not args.no_git)
    for problem in problems:
        print(problem)
    if problems:
        print(f"FAILED: {len(problems)} problem(s) under {args.root}")
        return 1
    print(
        f"OK: documentation links, anchors, cited paths and {AGENTS_REL} limit"
        f" hold under {args.root}"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
