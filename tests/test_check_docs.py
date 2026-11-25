"""Tests for tools/check_docs.py on synthetic documentation trees."""

import importlib.util
import subprocess
import sys
from pathlib import Path
from types import ModuleType

import pytest

SCRIPT = Path(__file__).resolve().parents[1] / "tools" / "check_docs.py"

_spec = importlib.util.spec_from_file_location("check_docs", SCRIPT)
assert _spec is not None and _spec.loader is not None
check_docs: ModuleType = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(check_docs)

INDEX = """# Index

Route via [AGENTS](../AGENTS.md) and [guide](guide.md).

Cite `src/core/responses.py` and [self](#index).
"""

GUIDE = """# Guide

## Śledzenie wymagań 2,5D

## Fale

## Fale
"""


def make_tree(
    tmp_path: Path, *, index: str = INDEX, agents: str = "# A\n\nx\n", guide: str = GUIDE
) -> Path:
    (tmp_path / "docs").mkdir(parents=True)
    (tmp_path / "docs" / "index.md").write_text(index, encoding="utf-8")
    (tmp_path / "docs" / "guide.md").write_text(guide, encoding="utf-8")
    (tmp_path / "AGENTS.md").write_text(agents, encoding="utf-8")
    (tmp_path / "src" / "core").mkdir(parents=True)
    (tmp_path / "src" / "core" / "responses.py").write_text("", encoding="utf-8")
    return tmp_path


def problems(root: Path) -> list[str]:
    return check_docs.check_tree(root, use_git=False)


def test_clean_tree_has_no_problems(tmp_path: Path) -> None:
    assert problems(make_tree(tmp_path)) == []


def test_broken_relative_link_is_reported(tmp_path: Path) -> None:
    root = make_tree(
        tmp_path,
        index=INDEX + "\n[bad](missing.md)\n",
    )
    found = problems(root)
    assert len(found) == 1
    assert "missing.md" in found[0]
    assert "docs/index.md" in found[0]


def test_broken_anchor_in_same_file_is_reported(tmp_path: Path) -> None:
    root = make_tree(tmp_path, index=INDEX + "\n[bad](#nie-istnieje)\n")
    found = problems(root)
    assert len(found) == 1
    assert "nie-istnieje" in found[0]


def test_broken_anchor_in_other_file_is_reported(tmp_path: Path) -> None:
    root = make_tree(
        tmp_path,
        index=INDEX + "\n[bad](guide.md#nie-istnieje)\n",
    )
    found = problems(root)
    assert len(found) == 1
    assert "nie-istnieje" in found[0]


def test_polish_heading_anchor_resolves_with_github_slugs(tmp_path: Path) -> None:
    index = INDEX + "\n[ok](guide.md#śledzenie-wymagań-25d)\n"
    assert problems(make_tree(tmp_path, index=index)) == []


def test_duplicate_headings_get_github_suffix_anchors(tmp_path: Path) -> None:
    index = INDEX + "\n[ok](guide.md#fale-1)\n[bad](guide.md#fale-2)\n"
    found = problems(make_tree(tmp_path, index=index))
    assert len(found) == 1
    assert "fale-2" in found[0]


def test_missing_cited_path_in_index_is_reported(tmp_path: Path) -> None:
    index = (
        INDEX
        + "\nSee `src/core/nope.py` and `tests/test_ghost.py::test_x`.\n"
    )
    found = problems(make_tree(tmp_path, index=index))
    assert len(found) == 2
    assert any("nope.py" in item for item in found)
    assert any("test_ghost.py" in item for item in found)


def test_non_path_backticks_are_ignored(tmp_path: Path) -> None:
    index = (
        INDEX
        + "\nCodes `path_outside_workspace`, `C01`, `limit/200` are not paths."
        + " Run `uv run pytest`.\n"
    )
    assert problems(make_tree(tmp_path, index=index)) == []


def test_overlong_agents_md_is_reported(tmp_path: Path) -> None:
    agents = "# A\n" + "x\n" * 120
    found = problems(make_tree(tmp_path, agents=agents))
    assert len(found) == 1
    assert "AGENTS.md" in found[0]


def test_agents_md_at_limit_passes(tmp_path: Path) -> None:
    agents = "# A\n" + "x\n" * 119
    assert len(agents.splitlines()) == 120
    assert problems(make_tree(tmp_path, agents=agents)) == []


def test_main_exit_codes_on_synthetic_tree(tmp_path: Path) -> None:
    clean = make_tree(tmp_path / "clean")
    broken = make_tree(tmp_path / "broken", index=INDEX + "\n[bad](nope.md)\n")
    for root, expected in ((clean, 0), (broken, 1)):
        run = subprocess.run(
            [sys.executable, str(SCRIPT), "--root", str(root), "--no-git"],
            capture_output=True,
            text=True,
        )
        assert run.returncode == expected, run.stdout + run.stderr
        if expected == 0:
            assert "OK" in run.stdout
        else:
            assert "nope.md" in run.stdout


@pytest.mark.parametrize(
    ("heading", "expected"),
    [
        ("C01. Profile, errors and CLI", "c01-profile-errors-and-cli"),
        ("Stage 0. Sanitization 2,5D", "stage-0-sanitization-25d"),
        ("#27 M0.1 — profile and CLI", "27-m01--profile-and-cli"),
        ("Envelope and diagnostics", "envelope-and-diagnostics"),
        ("[Patch *map*](elsewhere.md) __test__", "patch-map-test"),
        ("[Patch][ref] _version_ snake_case", "patch-version-snake_case"),
        ("<em>Patch</em> &amp; ``_code_``", "patch--_code_"),
        (
            "Import/Export API and atomic publication",
            "importexport-api-and-atomic-publication",
        ),
    ],
)
def test_github_slug(heading: str, expected: str) -> None:
    assert check_docs.github_slug(heading) == expected


def test_rendered_atx_and_setext_headings_reserve_assigned_slugs() -> None:
    document = "   ## Patch ###\n\n**Patch**\n===\n\nPatch-1\n---\n\n## Patch\n"
    assert check_docs.heading_anchors(document) == {
        "patch",
        "patch-1",
        "patch-1-1",
        "patch-2",
    }


@pytest.mark.parametrize("fence", ["`", "~"])
def test_fence_uses_opening_character_and_length(fence: str) -> None:
    other = "~" if fence == "`" else "`"
    document = (
        f"# Real\n\n   {fence * 4}md\n# Example\n{fence * 3}\n"
        f"{other * 5}\n[example](missing.md)\n{fence * 5}\n[real](real.md)\n"
    )
    assert check_docs.heading_anchors(document) == {"real"}
    assert list(check_docs.iter_links(document)) == [(9, "real.md", "")]


@pytest.mark.parametrize("fence", ["```", "~~~~~~"])
def test_unclosed_fence_extends_through_eof(fence: str) -> None:
    document = f"# Real\n\n{fence}\n# Example\n[bad](missing.md)"
    assert check_docs.heading_anchors(document) == {"real"}
    assert list(check_docs.iter_links(document)) == []


def test_link_after_fence_keeps_diagnostic_line_number(tmp_path: Path) -> None:
    index = INDEX + "\n```md\n# Example\n[example](skip.md)\n```\n[bad](missing.md)\n"
    found = problems(make_tree(tmp_path, index=index))
    assert found == [
        f"docs/index.md:{len(index.splitlines())}: broken link -> 'missing.md' (missing)"
    ]


def test_inline_code_hides_links_and_preserves_link_label() -> None:
    document = (
        "`[sample](skip.md)` and ``[another\nexample](skip-too.md) ` ``\n"
        "[`actual`](target.md)\n"
    )
    assert list(check_docs.iter_links(document)) == [(3, "target.md", "")]


@pytest.mark.parametrize(
    "link",
    [
        '[bad](missing.md "title")',
        "[bad](missing.md 'title')",
        "[bad](missing.md (title))",
        '[bad](<missing file.md> "title")',
        "[bad](missing(v2).md)",
        "[bad][ReF]\n\n[ref]: <missing file.md> 'title'",
        "[bad][]\n\n[bad]:\n  missing.md",
        "[bad]\n\n[bad]: missing.md",
        "[unused]: missing.md",
    ],
)
def test_markdown_link_forms_check_broken_destinations(
    tmp_path: Path, link: str
) -> None:
    found = problems(make_tree(tmp_path, index=INDEX + "\n" + link + "\n"))
    assert len(found) == 1
    assert (
        "broken link" in found[0]
        and (
            "missing file.md"
            if "<" in link
            else "missing(v2).md"
            if "(v2)" in link
            else "missing.md"
        )
        in found[0]
    )


def test_root_link_space_query_and_percent_encoded_anchor(tmp_path: Path) -> None:
    index = INDEX + "\n[ok](/AGENTS.md#a)\n[ok](</notes here.MD?plain=1#%6Eotes>)\n"
    root = make_tree(tmp_path, index=index)
    (root / "notes here.MD").write_text("# Notes\n", encoding="utf-8")
    assert problems(root) == []
    assert list(check_docs.iter_links(index))[-2:] == [
        (len(index.splitlines()) - 1, "/AGENTS.md", "a"),
        (len(index.splitlines()), "/notes here.MD", "notes"),
    ]


def test_uppercase_markdown_suffix_checks_anchors(tmp_path: Path) -> None:
    root = make_tree(tmp_path, index=INDEX + "\n[bad](notes.MD#absent)\n")
    (root / "docs" / "notes.MD").write_text("# Notes\n", encoding="utf-8")
    assert any("broken anchor" in problem for problem in problems(root))


def test_link_may_not_leave_root(tmp_path: Path) -> None:
    root = make_tree(tmp_path / "repo", index=INDEX + "\n[bad](../../outside.md)\n")
    (tmp_path / "outside.md").write_text("# Outside\n", encoding="utf-8")
    assert any("outside repository" in problem for problem in problems(root))


def test_cited_paths_distinguish_assets_and_identifiers(tmp_path: Path) -> None:
    index = (
        INDEX
        + "\n`missing.svg` `C01/C02` `https://example.org/file.md` `docs/index.md#index`\n"
    )
    assert problems(make_tree(tmp_path, index=index)) == [
        "docs/index.md: missing cited path `missing.svg` (missing)"
    ]


def test_missing_index_and_agents_are_reported(tmp_path: Path) -> None:
    root = make_tree(tmp_path)
    (root / "docs" / "index.md").unlink()
    (root / "AGENTS.md").unlink()
    found = problems(root)
    assert any("docs/index.md" in problem for problem in found)
    assert any("AGENTS.md" in problem for problem in found)


def test_no_git_walk_skips_generated_directories(tmp_path: Path) -> None:
    root = make_tree(tmp_path)
    (root / ".venv").mkdir()
    (root / ".venv/docs.md").write_text("[ignore](missing.md)\n", encoding="utf-8")
    assert problems(root) == []


def git(root: Path, *args: str) -> None:
    subprocess.run(["git", "-C", str(root), *args], capture_output=True, check=True)


def test_default_git_mode_checks_tracked_sources_and_targets(tmp_path: Path) -> None:
    root = make_tree(tmp_path)
    git(root, "init")
    (root / ".gitignore").write_text("ignored.md\n", encoding="utf-8")
    readme = root / "README.md"
    readme.write_text("[ok](/docs/index.md#index)\n", encoding="utf-8")
    git(root, "add", ".")
    for name in ("ignored.md", "untracked.md"):
        (root / name).write_text("[ignore](missing.md)\n", encoding="utf-8")
    assert check_docs.check_tree(root, use_git=True) == []
    readme.write_text(
        "[ignored](ignored.md)\n[local](untracked.md)\n", encoding="utf-8"
    )
    index = root / "docs" / "index.md"
    index.write_text(INDEX + "\nCite `untracked.md`.\n", encoding="utf-8")
    found = check_docs.check_tree(root, use_git=True)
    assert len(found) == 3 and all(
        "untracked" in problem or "ignored" in problem for problem in found
    )
    run = subprocess.run(
        [sys.executable, str(SCRIPT), "--root", str(root)],
        capture_output=True,
        text=True,
    )
    assert run.returncode == 1, run.stdout + run.stderr
    assert "missing cited path `untracked.md`" in run.stdout


def test_git_discovery_rejects_empty_markdown_result(tmp_path: Path) -> None:
    git(tmp_path, "init")
    root = make_tree(tmp_path / "ignored")
    (tmp_path / ".gitignore").write_text("ignored/\n", encoding="utf-8")
    assert any("no tracked Markdown" in item for item in check_docs.check_tree(root))


def test_git_discovery_reports_missing_git(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    root = make_tree(tmp_path)
    monkeypatch.setenv("PATH", str(tmp_path / "bin"))
    assert any(
        "git discovery failed" in problem.lower()
        for problem in check_docs.check_tree(root)
    )


def test_git_discovery_reports_deleted_and_invalid_utf8_documents(
    tmp_path: Path,
) -> None:
    root = make_tree(tmp_path)
    git(root, "init")
    (root / "deleted.md").touch()
    (root / "invalid.MD").write_bytes(b"\xff")
    git(root, "add", ".")
    (root / "deleted.md").unlink()
    found = check_docs.check_tree(root)
    assert any("deleted.md" in problem for problem in found)
    assert any("invalid.MD" in problem for problem in found)
