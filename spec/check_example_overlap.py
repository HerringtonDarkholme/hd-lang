#!/usr/bin/env python3
"""Verify that runnable text-fence mappings share salient source identifiers."""

from pathlib import Path
import re
import sys


STOP = set(
    "fn let mut pub data enum trait impl type use as for in if else match while "
    "return break continue pass true false nil self Self where reified annotate "
    "shape and or not is string bool void i8 i16 i32 i64 u8 u16 u32 u64 f32 "
    "f64 list map Result Ok Err Any".split()
)


def identifiers(text: str) -> set[str]:
    return {
        value
        for value in re.findall(r"[^\W\d]\w*", text, re.UNICODE)
        if len(value) >= 3 and value not in STOP
    }


def salient_identifiers(text: str) -> set[str]:
    """Names that characterize an example rather than incidental prose/locals."""

    source = re.sub(r'"(?:\\.|[^"\\])*"', '""', text)
    source = re.sub(r"#.*", "", source)

    declared = set(
        re.findall(
            r"\b(?:fn|data|enum|trait|impl|type)\s+(?:\[[^]]+\]\s*)?([^\W\d]\w*)",
            source,
            re.UNICODE,
        )
    )
    members = set(re.findall(r"\.([^\W\d]\w*)\s*(?:\[|\()", source, re.UNICODE))
    associated = set(re.findall(r"::([^\W\d]\w*)\s*\(", source, re.UNICODE))
    capitals = {
        value
        for value in re.findall(r"\b[A-Z][A-Za-z0-9_]*\b", source)
        if value not in STOP
    }
    result = (declared | members | associated | capitals) & identifiers(source)
    return result or identifiers(source)


def text_blocks(path: Path) -> list[str]:
    return [
        match.group(1)
        for match in re.finditer(
            r"^```text\s*\n(.*?)^```", path.read_text(encoding="utf-8"), re.M | re.S
        )
    ]


def main() -> int:
    if len(sys.argv) != 3:
        print("usage: check_example_overlap.py SPEC_DIR EXAMPLES_TSV", file=sys.stderr)
        return 2
    spec_dir = Path(sys.argv[1])
    examples = Path(sys.argv[2])
    failures: list[str] = []
    cache: dict[str, list[str]] = {}
    for row in examples.read_text(encoding="utf-8").splitlines()[1:]:
        specification, block_text, classification, fixture_text = row.split("\t")
        if classification not in {"accept", "mixed"}:
            continue
        blocks = cache.setdefault(
            specification, text_blocks(spec_dir / specification)
        )
        block = blocks[int(block_text) - 1]
        block_ids = salient_identifiers(block)
        if not block_ids:
            continue
        fixture_ids: set[str] = set()
        for fixture in fixture_text.split("|"):
            fixture_ids.update(
                identifiers(
                    (spec_dir / "conformance" / fixture).read_text(encoding="utf-8")
                )
            )
        overlap = block_ids & fixture_ids
        coverage = len(overlap) / len(block_ids)
        if coverage < 0.60:
            failures.append(
                f"{specification} block {block_text} covers {len(overlap)}/"
                f"{len(block_ids)} salient identifiers ({coverage:.0%}) with {fixture_text}; "
                f"missing {', '.join(sorted(block_ids - overlap))}"
            )
    if failures:
        print("example inventory overlap failures:", file=sys.stderr)
        for failure in failures:
            print(f"- {failure}", file=sys.stderr)
        return 1
    print("example inventory overlap passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
