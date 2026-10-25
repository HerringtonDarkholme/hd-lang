"""Inventory explicit pub fn declarations, not implicit-public trait methods."""

import json
from pathlib import Path
import re

ROOT = Path(__file__).resolve().parents[3]


def declarations():
    rows = []
    for file in sorted((ROOT / "lib/std").rglob("*.hd")):
        text = file.read_text()
        for match in re.finditer(r"(?m)^([ \t]*)pub fn ([A-Za-z_]\w*)!?", text):
            at = match.end()
            if text[at:at + 1] == "[":
                depth = 1
                at += 1
                while depth:
                    depth += (text[at] == "[") - (text[at] == "]")
                    at += 1
            while text[at].isspace():
                at += 1
            assert text[at] == "(", (file, match[2])
            at += 1
            start, depth, args = at, 0, []
            while True:
                char = text[at]
                if char in "([{":
                    depth += 1
                elif char in ")]}" and (char != ")" or depth):
                    depth -= 1
                elif char == ")":
                    if text[start:at].strip():
                        args.append(text[start:at].strip())
                    break
                elif char == "," and not depth:
                    args.append(text[start:at].strip())
                    start = at + 1
                at += 1
            rows.append({
                "file": str(file.relative_to(ROOT)),
                "line": text.count("\n", 0, match.start()) + 1,
                "name": match[2], "indented": bool(match[1]), "args": args,
                "receiver": bool(args and re.search(r"\bself\b", args[0])),
            })
    return rows


def main():
    rows = declarations()
    summary = {}
    for scope in ["all", "non_rt"]:
        chosen = [row for row in rows if scope == "all" or not (
            row["file"] == "lib/std/rt.hd" or row["file"].startswith("lib/std/rt/")
        )]
        summary[scope] = {
            "explicit_pub_declarations": len(chosen),
            "top_level": sum(not row["indented"] for row in chosen),
            "multi_arg_top_level": sum(not row["indented"] and len(row["args"]) > 1 for row in chosen),
            "receiver_declarations": sum(row["receiver"] for row in chosen),
            "receiver_plus_args": sum(row["receiver"] and len(row["args"]) > 1 for row in chosen),
            "defaults": sum(any("=" in arg for arg in row["args"]) for row in chosen),
            "varargs": sum(any("..." in arg for arg in row["args"]) for row in chosen),
        }
    print(json.dumps({"summary": summary, "declarations": rows}, indent=2))


if __name__ == "__main__":
    main()
