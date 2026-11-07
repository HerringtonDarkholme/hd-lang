#!/usr/bin/env python3
"""Audits emitted WAT and own-view IR for wasteful code, without running it.

    scripts/ir-audit.py [DIR ...]            summary table, worst first
    scripts/ir-audit.py --list NAME [DIR]    every site of one finding
    scripts/ir-audit.py --json [DIR ...]     machine-readable counts

The verdict on any change is runtime and module size. A finding here is a lead:
a static site, not a measured cost, and V8 may already remove it. Vet this script
against measurements (scripts/ir-audit-verify.py checks it finds what it claims,
not that what it finds matters).

With no DIR it reads the stage baselines of hd_driver and hd_wasm_linear. It
simulates the Wasm stack inside each basic block, so a finding is a fact about
that block. Elided code (`;; ...`) ends a block, and a function with elided code
skips the whole-function checks.
"""

import argparse
import collections
import glob
import json
import os
import re
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
DEFAULT = ["crates/hd_driver/tests/baselines", "crates/hd_wasm_linear/tests/baselines"]

DESCRIBE = {
    # WAT
    "load-dropped": "a value loaded from memory and dropped unused",
    "pure-dropped": "a pure value (const, local, arithmetic) computed and dropped",
    "repeat-load": "the same load twice in a block with no store or call between",
    "repeat-bounds": "the same bounds check twice in a block with nothing that could change the length",
    "count-pair": "a retain and a release of one value in a block with no other call between",
    "zero-init": "a store of zero into a local that is still zero",
    "empty-if": "an `if` with nothing in it",
    "const-branch": "a branch on a constant",
    "dead-store": "a local set and never read in its function",
    "br-to-end": "`br 0` straight before the `end` it jumps to",
    "empty-block": "a `block` with nothing in it",
    "empty-else": "an `else` with nothing in it",
    "set-then-get": "`local.set $x` straight before `local.get $x` (a tee saves bytes)",
    # IR
    "ir-ref-drop": "a Drop of a reference type, which counts nothing unless it is a view (#1360)",
    "ir-copy-only-lends": "a counted Read copy whose only uses lend it, then a Drop: one retain and one release",
    "ir-store-then-read": "a Store to a local straight before a Read of it",
    "ir-unused-read": "a counted Read that nothing uses",
    "ir-conservative": "a count the compiler could not name a reason for",
}
INFO_ONLY = {"set-then-get", "br-to-end", "empty-block", "empty-else", "ir-store-then-read"}

COUNT_HELPERS = re.compile(r"^(retain|rt:retain|rt:release|rt:coll_copy|rt:drop)")
RETAINS = re.compile(r"^(retain|rt:retain|rt:coll_copy)")
RELEASES = re.compile(r"^(rt:release|rt:drop)")
PANIC = re.compile(r"^rt:panic")
# An allocation changes no existing object's count or length.
ALLOCATES = re.compile(r"^rt:(new|list_buf|str\\b)")
BIN = re.compile(r"^[if](32|64)\.(add|sub|mul|div_[su]|div|rem_[su]|and|or|xor|shl|shr_[su]|rotl|rotr|eq|ne|lt_[su]|gt_[su]|le_[su]|ge_[su]|lt|gt|le|ge|min|max|copysign)$")
UN = re.compile(r"^([if](32|64)\.(eqz|clz|ctz|popcnt|abs|neg|sqrt|ceil|floor|trunc|nearest|extend8_s|extend16_s|extend32_s|wrap_i64|extend_i32_[su]|trunc_[fi].*|convert_.*|promote_.*|demote_.*|reinterpret_.*))$")


class Value:
    __slots__ = ("expr", "pure", "kind")

    def __init__(self, expr, pure=True, kind="pure"):
        self.expr, self.pure, self.kind = expr, pure, kind


def signatures(text):
    sigs = {}
    for m in re.finditer(r'\(func \$("[^"]+"|[^\s()]+)((?: \(param [^)]*\))*)((?: \(result [^)]*\))*)', text):
        params = sum(len(re.sub(r"\$\S+", "", p).split()) - 1 for p in re.findall(r"\(param [^)]*\)", m.group(2)))
        results = sum(len(r.split()) - 1 for r in re.findall(r"\(result [^)]*\)", m.group(3)))
        sigs[m.group(1).strip('"')] = (params, results)
    return sigs


def functions(text):
    """Yields (name, params, lines) of each function a WAT text defines."""
    name, params, body = None, set(), []
    for raw in text.splitlines():
        line = raw.strip()
        m = re.match(r'\(func \$("[^"]+"|[^\s()]+)', line)
        if raw.startswith("  (func ") and m:
            name = m.group(1).strip('"')
            params = set(re.findall(r"\(param \$(\S+) ", line))
            body = []
        elif name is not None and raw.startswith("  )"):
            yield name, params, body
            name = None
        elif name is not None:
            body.append(line)
    if name is not None:
        yield name, params, body


def unwrap_count_tests(code):
    out, i = [], 0
    ops = [c for c, _ in code]
    while i < len(code):
        m = re.match(r"local\.tee \$(\S+)$", ops[i])
        if m and ops[i + 1 : i + 2] == ["if"]:
            t = f"local.get ${m.group(1)}"
            known = [t, "i32.const 8", "i32.sub", "i32.load $heap"]
            j = i + 2
            nested = ops[j : j + 4] == known
            if nested:
                j += 4
                if ops[j : j + 2] == ["global.get $tracing", "i32.or"]:
                    j += 2
                nested = ops[j : j + 1] == ["if"]
                j += 1
            if ops[j : j + 1] == [t] and ops[j + 1 : j + 2] and ops[j + 1].startswith("call "):
                closes = ["end", "end"] if nested else ["end"]
                if ops[j + 2 : j + 2 + len(closes)] == closes:
                    out.append((ops[j + 1], False))
                    i = j + 2 + len(closes)
                    continue
        out.append(code[i])
        i += 1
    return out


def audit_function(name, params, body, sigs, emit):
    def hit(kind, detail=""):
        emit(kind, name, detail)

    code = [(l.split(";;")[0].strip(), l.strip() == ";; ...") for l in body]
    code = [(c, e) for c, e in code if c or e]
    elided = any(e for _, e in code)
    # A count call behind its call-site test (`count_tests.rs`) is still one call:
    # without this, the test's `if` would end the block and hide every pair.
    code = unwrap_count_tests(code)

    ops = [c for c, e in code if not e]
    for a, b in zip(ops, ops[1:]):
        if a.startswith("if") and b == "end":
            hit("empty-if")
        if a == "br 0" and b == "end":
            hit("br-to-end")
        if a == "block" and b == "end":
            hit("empty-block")
        if a == "else" and b == "end":
            hit("empty-else")
        if re.match(r"i(32|64)\.const ", a) and (b.startswith("if") or b.startswith("br_if")):
            hit("const-branch", a)
        m = re.match(r"local\.set \$(\S+)", a)
        if m and b == f"local.get ${m.group(1)}":
            hit("set-then-get", m.group(1))

    if not elided:
        gets = set(re.findall(r"local\.get \$(\S+)", "\n".join(ops)))
        for var in sorted(set(re.findall(r"local\.(?:set|tee) \$(\S+)", "\n".join(ops))) - gets):
            hit("dead-store", var)
        seen, depth_loop = set(), 0
        previous = ""
        for c in ops:
            if c.startswith("loop"):
                depth_loop += 1
            m = re.match(r"local\.(get|set|tee) \$(\S+)", c)
            if m:
                var = m.group(2)
                if (
                    m.group(1) != "get"
                    and var not in seen
                    and var not in params
                    and depth_loop == 0
                    and re.match(r"(i32|i64)\.const 0$|(f32|f64)\.const 0$", previous)
                ):
                    hit("zero-init", var)
                seen.add(var)
            previous = c

    # Value numbering inside one basic block.
    stack, env, epoch, loads, checks, retained = [], {}, [0], {}, {}, {}

    def reset():
        stack.clear()
        env.clear()
        loads.clear()
        checks.clear()
        retained.clear()
        epoch[0] += 1

    def pop():
        return stack.pop() if stack else Value("?", False, "unknown")

    i = 0
    while i < len(code):
        c, e = code[i]
        i += 1
        if e:
            reset()
            continue
        head = c.split(" ")[0]
        if head in ("block", "loop", "else", "end", "br", "unreachable", "return", "br_table"):
            reset()
        elif head in ("if", "br_if"):
            pop()
            reset()
        elif head == "local.get":
            var = c.split("$")[1]
            stack.append(Value(env.get(var, f"${var}"), True, "pure"))
        elif head in ("local.set", "local.tee"):
            var = c.split("$")[1]
            v = pop()
            env[var] = v.expr
            for key in [k for k in checks if f"${var}" in k]:
                del checks[key]
            if head == "local.tee":
                stack.append(Value(v.expr, v.pure, v.kind))
        elif head.endswith(".const"):
            stack.append(Value(c.split(" ", 1)[1] if " " in c else "0", True, "pure"))
        elif re.match(r"[if](32|64)\.load", head):
            addr = pop()
            off = re.search(r"offset=(\d+)", c)
            key = f"{head}[{addr.expr}+{off.group(1) if off else 0}]@{epoch[0]}"
            if addr.expr != "?" and key in loads:
                hit("repeat-load", key.split("@")[0])
            loads[key] = True
            stack.append(Value(key, True, "load"))
        elif re.match(r"[if](32|64)\.store", head):
            pop()
            pop()
            epoch[0] += 1
        elif head == "drop":
            v = pop()
            if v.kind == "load":
                hit("load-dropped", v.expr.split("@")[0])
            elif v.pure and v.kind == "pure" and v.expr != "?":
                hit("pure-dropped", v.expr)
        elif head == "select":
            pop()
            b, a = pop(), pop()
            stack.append(Value(f"sel({a.expr},{b.expr})", a.pure and b.pure, "pure"))
        elif BIN.match(head):
            b, a = pop(), pop()
            expr = f"{head}({a.expr},{b.expr})"
            stack.append(Value(expr, a.pure and b.pure, "load" if "load" in a.kind + b.kind else "pure"))
            if head.endswith("ge_u") or head.endswith("lt_u"):
                following = code[i][0] if i < len(code) else ""
                after = code[i + 1][0] if i + 1 < len(code) else ""
                closes = [code[j][0] for j in range(i + 2, min(i + 4, len(code)))]
                if following.startswith("if") and "$rt:panic" in after and closes == ["unreachable", "end"]:
                    # A check that panics or falls through ends no block.
                    pop()
                    if "?" not in expr:
                        if expr in checks:
                            hit("repeat-bounds", expr)
                        checks[expr] = True
                    i += 4
        elif UN.match(head):
            a = pop()
            stack.append(Value(f"{head}({a.expr})", a.pure, a.kind))
        elif head == "global.get":
            stack.append(Value(c, False, "global"))
        elif head == "global.set":
            pop()
        elif head == "call":
            callee = c.split("$", 1)[1].strip('"')
            n, r = sigs.get(callee, (None, None))
            if n is None:
                reset()
                continue
            args = [pop() for _ in range(n)]
            if PANIC.match(callee) or ALLOCATES.match(callee):
                pass
            elif RETAINS.match(callee) and args:
                retained[args[0].expr] = True
            elif RELEASES.match(callee) and args:
                if args[0].expr in retained and args[0].expr != "?":
                    hit("count-pair", args[0].expr.split("@")[0])
                    del retained[args[0].expr]
            else:
                retained.clear()
                epoch[0] += 1
                checks.clear()
            for k in range(r):
                stack.append(Value(f"call:{callee}#{i}.{k}", False, "call"))
        elif head == "call_indirect":
            n = sum(len(p.split()) - 1 for p in re.findall(r"\(param [^)]*\)", c)) + 1
            r = sum(len(p.split()) - 1 for p in re.findall(r"\(result [^)]*\)", c))
            for _ in range(n):
                pop()
            retained.clear()
            checks.clear()
            epoch[0] += 1
            for k in range(r):
                stack.append(Value(f"indirect#{i}.{k}", False, "call"))
        else:
            reset()


SCALAR = re.compile(r"^(i8|i16|i32|i64|u8|u16|u32|u64|usize|isize|f32|f64|bool|char|\(\)|never)\??$")
REF_TYPE = re.compile(r"^(Ref|Counted)\[")


def audit_own(text, emit):
    func = None
    body = {}
    order = []

    def flush():
        if func is None:
            return
        uses = collections.defaultdict(list)
        for n in order:
            tag, rest = body[n]
            refs = re.findall(r"%(\d+)", rest)
            for r in refs:
                if r != n:
                    uses[r].append((n, tag))
        for k, n in enumerate(order):
            tag, rest = body[n]
            ty = rest.rsplit(" : ", 1)[-1].strip()
            if tag == "Drop":
                m = re.match(r"\[%(\d+)\]", rest)
                if m and m.group(1) in body:
                    source = body[m.group(1)][1].rsplit(" : ", 1)[-1].strip()
                    if REF_TYPE.match(source):
                        emit("ir-ref-drop", func, source)
            if tag == "Read" and not SCALAR.match(ty) and not REF_TYPE.match(ty):
                used = uses.get(n, [])
                if rest.startswith("copy") and used:
                    tags = [t for _, t in used]
                    if "Drop" in tags and all(t in ("Drop", "Call", "CallValue", "CallDyn") for t in tags):
                        emit("ir-copy-only-lends", func, f"%{n} {rest[:60]}")
                if not used and not rest.startswith("borrow"):
                    emit("ir-unused-read", func, f"%{n} {rest[:60]}")
            if tag == "Store" and k + 1 < len(order):
                next_tag, next_rest = body[order[k + 1]]
                m = re.match(r"\[(\$\d+)\]", rest)
                if m and next_tag == "Read" and f"[{m.group(1)}]" in next_rest:
                    emit("ir-store-then-read", func, m.group(1))

    for raw in text.splitlines():
        line = raw[1:] if raw.startswith(">") else raw
        s = line.strip()
        if re.match(r"^[a-z_]+(/[\w.\- \[\]<>?&,:]+)+$", s) or re.match(r"^app/", s) or re.match(r"^std/", s):
            flush()
            func, body, order = s, {}, []
            continue
        m = re.match(r"%(\d+) = (\w+) ?(.*)$", s)
        if m and func:
            body[m.group(1)] = (m.group(2), m.group(3))
            order.append(m.group(1))
    flush()


def audit_counts(text, emit):
    func = None
    for raw in text.splitlines():
        s = (raw[1:] if raw.startswith(">") else raw).strip()
        if re.match(r"^(app|std)/", s):
            func = s
        m = re.search(r"\(conservative: ([^)]+)\)", s)
        if m and func:
            emit("ir-conservative", func, m.group(1))


def self_test():
    """Each WAT finding fires on its smallest input and not on its near miss."""
    head = '  (import "f" "rt:panic" (func $rt:panic))\n  (import "f" "rt:new" (func $rt:new (param i32) (result i32)))\n  (import "f" "retain" (func $retain (param i32)))\n  (import "f" "rt:release_ref" (func $rt:release_ref (param i32)))\n  (import "f" "g" (func $g))\n'
    cases = [
        ("load-dropped", "local.get $p\ni32.load $heap\ndrop", True),
        ("load-dropped", "local.get $p\ni32.load $heap\nlocal.set $q\nlocal.get $q", False),
        ("load-dropped", "local.get $p\ni32.load $heap\nlocal.get $p\ni32.load $heap offset=4\nlocal.set $q\ndrop", True),
        ("pure-dropped", "i32.const 1\ndrop", True),
        ("repeat-load", "local.get $p\ni32.load $heap\nlocal.get $p\ni32.load $heap\ni32.add\ndrop", True),
        ("repeat-load", "local.get $p\ni32.load $heap\nlocal.get $p\ni32.const 1\ni32.store $heap\nlocal.get $p\ni32.load $heap\ni32.add\ndrop", False),
        ("repeat-load", "local.get $p\ni32.load $heap\ncall $g\nlocal.get $p\ni32.load $heap\ni32.add\ndrop", False),
        ("repeat-bounds", "local.get $i\nlocal.get $p\ni32.load $heap offset=4\ni32.ge_u\nif\ncall $rt:panic\nunreachable\nend", False),
        ("repeat-bounds", "local.get $i\nlocal.get $p\ni32.load $heap offset=4\ni32.ge_u\nif\ncall $rt:panic\nunreachable\nend\nlocal.get $i\nlocal.get $p\ni32.load $heap offset=4\ni32.ge_u\nif\ncall $rt:panic\nunreachable\nend", True),
        ("repeat-bounds", "local.get $i\nlocal.get $p\ni32.load $heap offset=4\ni32.ge_u\nif\ncall $rt:panic\nunreachable\nend\ncall $g\nlocal.get $i\nlocal.get $p\ni32.load $heap offset=4\ni32.ge_u\nif\ncall $rt:panic\nunreachable\nend", False),
        ("count-pair", "local.get $p\ncall $retain\nlocal.get $p\ncall $rt:release_ref", True),
        ("count-pair", "local.get $p\ncall $retain\ncall $g\nlocal.get $p\ncall $rt:release_ref", False),
        ("count-pair", "local.get $p\ncall $retain\ni32.const 4\ncall $rt:new\ndrop\nlocal.get $p\ncall $rt:release_ref", True),
        ("count-pair", "local.get $p\nlocal.tee $t\nif\nlocal.get $t\ni32.const 8\ni32.sub\ni32.load $heap\nif\nlocal.get $t\ncall $retain\nend\nend\nlocal.get $p\nlocal.tee $t\nif\nlocal.get $t\ncall $rt:release_ref\nend", True),
        ("zero-init", "i32.const 0\nlocal.set $q\nlocal.get $q\ndrop", True),
        ("zero-init", "i32.const 1\nlocal.set $q\ni32.const 0\nlocal.set $q\nlocal.get $q\ndrop", False),
        ("zero-init", "loop\ni32.const 0\nlocal.set $q\nend\nlocal.get $q\ndrop", False),
        ("empty-if", "local.get $p\nif\nend", True),
        ("empty-else", "local.get $p\nif\ncall $g\nelse\nend", True),
        ("const-branch", "i32.const 1\nif\ncall $g\nend", True),
        ("dead-store", "local.get $p\nlocal.set $q", True),
        ("dead-store", "local.get $p\nlocal.set $q\n;; ...", False),
        ("br-to-end", "block\nbr 0\nend", True),
        ("empty-block", "block\nend", True),
        ("set-then-get", "local.get $p\nlocal.set $q\nlocal.get $q\ndrop", True),
    ]
    bad = 0
    for kind, code, want in cases:
        text = head + "  (func $f (param $p i32) (param $i i32)\n" + "\n".join("    " + l for l in code.split("\n")) + "\n  )\n"
        found = []
        sigs = signatures(text)
        for name, params, body in functions(text):
            audit_function(name, params, body, sigs, lambda k, f, d="": found.append(k))
        if (kind in found) != want:
            bad += 1
            print(f"FAIL {kind} want {want} got {found}\n{code}")
    print(f"self-test: {len(cases) - bad} of {len(cases)} pass")
    return 1 if bad else 0


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("dirs", nargs="*")
    ap.add_argument("--list", metavar="NAME")
    ap.add_argument("--json", action="store_true")
    ap.add_argument("--self-test", action="store_true")
    args = ap.parse_args()
    if args.self_test:
        return self_test()
    dirs = args.dirs or [os.path.join(ROOT, d) for d in DEFAULT]

    sites = collections.defaultdict(list)
    files = 0
    for d in dirs:
        for path in sorted(glob.glob(os.path.join(d, "**", "*"), recursive=True)):
            ext = os.path.splitext(path)[1]
            if ext not in (".wat", ".own", ".counts"):
                continue
            files += 1
            text = open(path, errors="replace").read()
            label = path.split("baselines/")[-1]

            def emit(kind, func, detail="", label=label):
                sites[kind].append((label, func, detail))

            if ext == ".wat":
                sigs = signatures(text)
                for name, params, body in functions(text):
                    audit_function(name, params, body, sigs, emit)
            elif ext == ".own":
                audit_own(text, emit)
            else:
                audit_counts(text, emit)

    if args.json:
        print(json.dumps({k: len(v) for k, v in sorted(sites.items())}, indent=2))
        return 0
    if args.list:
        for label, func, detail in sites.get(args.list, []):
            print(f"{label}  {func}  {detail}")
        return 0
    print(f"{files} files")
    print(f"{'finding':22} {'sites':>6} {'files':>6}  what")
    for kind, found in sorted(sites.items(), key=lambda kv: (kv[0] in INFO_ONLY, -len(kv[1]))):
        print(f"{kind:22} {len(found):6} {len({f[0] for f in found}):6}  {DESCRIBE.get(kind, '')}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
