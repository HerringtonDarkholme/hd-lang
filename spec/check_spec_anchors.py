#!/usr/bin/env python3
"""Verify conformance references and relative Markdown links."""

from __future__ import annotations

from collections import defaultdict
from pathlib import Path
import re
import sys
from urllib.parse import unquote


HEADING = re.compile(r"^#{1,6}\s+(.+?)\s*#*\s*$", re.MULTILINE)
LINK = re.compile(r"(?<!!)\[[^]]*\]\(([^)]+)\)")


def github_slug(text: str) -> str:
    """Implement the GitHub heading-slug rules needed by this repository."""

    text = re.sub(r"<[^>]+>", "", text)
    text = re.sub(r"[`*_~]", "", text.strip().lower())
    text = re.sub(r"[^\w\- ]", "", text, flags=re.UNICODE)
    return text.replace(" ", "-")


def anchors(path: Path) -> set[str]:
    counts: dict[str, int] = defaultdict(int)
    result: set[str] = set()
    for match in HEADING.finditer(path.read_text(encoding="utf-8")):
        base = github_slug(match.group(1))
        index = counts[base]
        counts[base] += 1
        result.add(base if index == 0 else f"{base}-{index}")
    return result


def prose_without_fences(text: str) -> str:
    return re.sub(r"^```.*?^```\s*$", "", text, flags=re.MULTILINE | re.DOTALL)


def link_target(raw: str) -> str:
    raw = raw.strip()
    if raw.startswith("<") and ">" in raw:
        return raw[1 : raw.index(">")]
    return raw.split(maxsplit=1)[0]


def check_reference(
    source: Path,
    destination: str,
    cache: dict[Path, set[str]],
    failures: list[str],
) -> None:
    if not destination or destination.startswith(("http://", "https://", "mailto:")):
        return
    target_text, separator, fragment = destination.partition("#")
    target_text = unquote(target_text).split("?", 1)[0]
    if target_text and Path(target_text).suffix not in {".md", ".hd", ".tsv"}:
        # The grammar and type prose contains code-like `[T](value)` forms.
        # They are not Markdown file links; repository links use an explicit
        # extension or a same-document fragment.
        return
    target = source if not target_text else (source.parent / target_text).resolve()
    if not target.exists() or not target.is_file():
        failures.append(f"{source}: missing link target {destination}")
        return
    if separator and fragment:
        decoded = unquote(fragment)
        known = cache.setdefault(target, anchors(target))
        if decoded not in known:
            failures.append(f"{source}: missing link anchor {destination}")


def main() -> int:
    if len(sys.argv) != 3:
        print("usage: check_spec_anchors.py SPEC_DIR CASES_TSV", file=sys.stderr)
        return 2
    spec_dir = Path(sys.argv[1]).resolve()
    repo_dir = spec_dir.parent
    failures: list[str] = []
    cache: dict[Path, set[str]] = {}

    for row in Path(sys.argv[2]).read_text(encoding="utf-8").splitlines()[1:]:
        fixture, _, _, reference = row.split("\t")
        file_text, separator, fragment = reference.partition("#")
        target = spec_dir / file_text
        if not target.is_file():
            failures.append(f"{fixture}: missing specification {reference}")
        elif separator and fragment not in cache.setdefault(target, anchors(target)):
            failures.append(f"{fixture}: missing specification anchor {reference}")

    for directory in (spec_dir, repo_dir / "guide", repo_dir / "future-work"):
        for source in sorted(directory.rglob("*.md")):
            prose = prose_without_fences(source.read_text(encoding="utf-8"))
            for match in LINK.finditer(prose):
                check_reference(source, link_target(match.group(1)), cache, failures)

    if failures:
        print("unresolved specification links:", file=sys.stderr)
        for failure in failures:
            print(f"- {failure}", file=sys.stderr)
        return 1
    print("specification links and anchors passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
