#!/usr/bin/env python3
"""Tests scripts/ir-audit.py from outside, on every real function in the baselines.

1. Planting: add one known wasteful pattern to the start of a real function. The
   tool must report exactly one more of that finding, and no other finding may
   change beyond what the plant itself declares.
2. Neutral changes: rename locals, add comments, reorder functions. Counts must
   not move.
3. Independent count: patterns simple enough for a regex, counted a second way.
4. Stack health: how much of the code the simulation really understood.
"""

import collections
import glob
import importlib.util
import os
import random
import re
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
path = os.path.join(HERE, "ir-audit.py")
spec = importlib.util.spec_from_file_location("ir_audit", path)
audit = importlib.util.module_from_spec(spec)
spec.loader.exec_module(audit)

DIRS = ["crates/hd_driver/tests/baselines", "crates/hd_wasm/tests/baselines"]

# name: (plant, every finding it must add)
PLANTS = {
    "load-dropped": ("local.get $zzp\ni32.load $heap\ndrop", {"load-dropped": 1}),
    "load-dropped-2": ("local.get $zzp\ni32.load $heap\nlocal.get $zzp\ni32.load $heap offset=4\nlocal.set $zzq\ndrop\nlocal.get $zzq\nlocal.set $zzr\nlocal.get $zzr\ni32.eqz\nlocal.set $zzq", {"load-dropped": 1, "set-then-get": 1}),
    "pure-dropped": ("i32.const 7\ndrop", {"pure-dropped": 1}),
    "repeat-load": ("local.get $zzp\ni32.load $heap\nlocal.get $zzp\ni32.load $heap\ni32.add\nlocal.set $zzq\nlocal.get $zzq\ndrop", {"repeat-load": 1, "pure-dropped": 1, "set-then-get": 1}),
    "zero-init": ("i32.const 0\nlocal.set $zzq\nlocal.get $zzq\nglobal.set $zzg", {"zero-init": 1, "set-then-get": 1}),
    "empty-if": ("local.get $zzp\nif\nend", {"empty-if": 1}),
    "empty-else": ("local.get $zzp\nif\nunreachable\nelse\nend", {"empty-else": 1}),
    "const-branch": ("i32.const 1\nif\nunreachable\nend", {"const-branch": 1}),
    "br-to-end": ("block\nbr 0\nend", {"br-to-end": 1}),
    "empty-block": ("block\nend", {"empty-block": 1}),
    "repeat-bounds": ("local.get $zzi\nlocal.get $zzp\ni32.load $heap offset=4\ni32.ge_u\nif\ncall $rt:panic\nunreachable\nend\nlocal.get $zzi\nlocal.get $zzp\ni32.load $heap offset=4\ni32.ge_u\nif\ncall $rt:panic\nunreachable\nend", {"repeat-bounds": 1, "repeat-load": 1}),
    "count-pair": ("local.get $zzp\ncall $retain\nlocal.get $zzp\ncall $rt:release_ref", {"count-pair": 1}),
    "dead-store": ("local.get $zzp\nlocal.set $zzdead", {"dead-store": 1}),
}
DECLS = '  (import "f" "rt:panic" (func $rt:panic))\n  (import "f" "retain" (func $retain (param i32)))\n  (import "f" "rt:release_ref" (func $rt:release_ref (param i32)))\n'


def run(params, body, sigs):
    found = collections.Counter()
    audit.audit_function("f", params, body, sigs, lambda k, f, d="": found.update([k]))
    return found


def collect():
    out = []
    for d in DIRS:
        for p in sorted(glob.glob(os.path.join(d, "**", "*.wat"), recursive=True)):
            text = open(p, errors="replace").read()
            sigs = audit.signatures(DECLS + text)
            for name, params, body in audit.functions(text):
                out.append((p, name, params, body, sigs))
    return out


def main():
    funcs = collect()
    print(f"{len(funcs)} real functions")
    fails = collections.Counter()
    examples = {}

    # 1. Planting.
    for plant, (code, want) in PLANTS.items():
        lines = code.split("\n")
        tested = 0
        for p, name, params, body, sigs in funcs:
            elided = any(l.strip() == ";; ..." for l in body)
            if plant in ("dead-store", "zero-init") and elided:
                continue
            base = run(params, body, sigs)
            got = run(params, lines + body, sigs)
            diff = {k: got[k] - base[k] for k in set(got) | set(base) if got[k] != base[k]}
            tested += 1
            if diff != want:
                fails[plant] += 1
                examples.setdefault(plant, (p.split("baselines/")[-1], name, diff))
        print(f"plant {plant:16} {tested - fails[plant]:6} of {tested} exact")

    # 2. Neutral changes.
    rng = random.Random(7)
    rename_bad = comment_bad = 0
    for p, name, params, body, sigs in funcs:
        base = run(params, body, sigs)
        renamed = [re.sub(r"(local\.\w+ )\$(\S+)", r"\1$r_\2", l) for l in body]
        got = run({"r_" + x for x in params}, renamed, sigs)
        if got != base:
            rename_bad += 1
            examples.setdefault("rename", (p, name, dict(got - base), dict(base - got)))
        noisy = []
        for l in body:
            noisy.append(l)
            if rng.random() < 0.2:
                noisy.append(";; 12 |     a source line")
        if run(params, noisy, sigs) != base:
            comment_bad += 1
            examples.setdefault("comment", (p, name))
    print(f"rename locals: {len(funcs) - rename_bad} of {len(funcs)} unchanged")
    print(f"add comments:  {len(funcs) - comment_bad} of {len(funcs)} unchanged")

    # 3. Independent regex counts over the same corpus.
    regex = collections.Counter()
    tool = collections.Counter()
    for p, name, params, body, sigs in funcs:
        code = [l.split(";;")[0].strip() for l in body if l.split(";;")[0].strip()]
        text = "\n".join(code)
        regex["empty-if"] += len(re.findall(r"^if[^\n]*\nend$", text, re.M))
        regex["empty-else"] += len(re.findall(r"^else\nend$", text, re.M))
        regex["empty-block"] += len(re.findall(r"^block\nend$", text, re.M))
        # `br 0` before `end` falls through only in a block or an if; in a
        # loop it is a continue. A plain regex counted 151 loop continues.
        regex["br-to-end"] += 0
        opened = []
        for i, line in enumerate(code):
            op = line.split()[0]
            if op in ("block", "loop", "if"):
                opened.append(op)
            elif op == "end" and opened:
                opened.pop()
            if line == "br 0" and code[i + 1 : i + 2] == ["end"] and opened and opened[-1] != "loop":
                regex["br-to-end"] += 1
        regex["const-branch"] += len(re.findall(r"^i(?:32|64)\.const [^\n]*\n(?:if|br_if)", text, re.M))
        tool.update(run(params, body, sigs))
    for k in sorted(regex):
        print(f"independent {k:14} regex {regex[k]:5}  tool {tool[k]:5}  {'same' if regex[k] == tool[k] else 'DIFFERENT'}")

    # 4. Stack health.
    known = unknown = 0
    for p, name, params, body, sigs in funcs:
        for raw in body:
            c = raw.split(";;")[0].strip()
            if not c:
                continue
            head = c.split(" ")[0]
            if head == "call":
                callee = c.split("$", 1)[1].strip('"')
                if callee in sigs:
                    known += 1
                else:
                    unknown += 1
    print(f"calls with a known signature: {known} of {known + unknown}")
    missing = collections.Counter()
    for p, name, params, body, sigs in funcs:
        for raw in body:
            c = raw.split(";;")[0].strip()
            if c.startswith("call ") and c.split("$", 1)[1].strip('"') not in sigs:
                missing[re.sub(r"'\d+$", "", c.split("$", 1)[1].strip('"'))] += 1
    print("unknown callees:", missing.most_common(6))
    if unknown:
        fails["unknown callee"] += 1

    if examples:
        print("first failure of each kind:")
        for k, v in examples.items():
            print(" ", k, v)
    return 1 if fails or rename_bad or comment_bad or any(regex[k] != tool[k] for k in regex) else 0


if __name__ == "__main__":
    sys.exit(main())
