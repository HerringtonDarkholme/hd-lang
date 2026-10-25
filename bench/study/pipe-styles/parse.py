"""Parse every hd/text example in the pipe-style report; syntax only."""

import argparse
import json
from pathlib import Path
import re
import subprocess

ROOT = Path(__file__).resolve().parents[3]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--parser", type=Path, required=True)
    parser.add_argument("--work", type=Path, required=True)
    args = parser.parse_args()
    work = args.work.resolve()
    work.mkdir(parents=True, exist_ok=True)
    report = ROOT / "docs/future-work/PIPE_STYLES.md"
    blocks = list(re.finditer(r"^```(?:text|hd)\n(.*?)^```", report.read_text(), re.M | re.S))
    results = []
    for i, block in enumerate(blocks, 1):
        file = work / f"block-{i:02}.hd"
        file.write_text(block[1])
        run = subprocess.run(
            [str(args.parser.resolve()), str(file)], capture_output=True, text=True, timeout=20
        )
        results.append({"block": i, "status": run.returncode, "output": run.stdout + run.stderr})
        print(f"block {i}: {'parse' if run.returncode == 0 else run.stdout.strip()}")
    (work / "parse.json").write_text(json.dumps(results, indent=2))
    if any(row["status"] for row in results):
        raise SystemExit(1)


if __name__ == "__main__":
    main()
