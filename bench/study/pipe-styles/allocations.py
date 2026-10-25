"""Read the existing allocation-request counter using scratch Wasm wrappers.

The production compiler/runtime are untouched. Each of four fresh runs returns
one byte of the same 32-bit counter; stdout must still match the original run.
The pinned allocator increments GIVEN for reused slots and class fast paths too.
"""
import argparse
import json
import os
from pathlib import Path
import re
import subprocess
from run import ROOT, expected, uint


def leb(n):
    out = bytearray()
    while True:
        byte = n & 127
        n >>= 7
        out.append(byte | (128 if n else 0))
        if not n:
            return bytes(out)


def word(data, at):
    size, at = uint(data, at)
    return data[at:at + size].decode(), at + size


def instrument(data, shift):
    sections = []
    at = 8
    while at < len(data):
        kind = data[at]
        size, at = uint(data, at + 1)
        sections.append((kind, data[at:at + size]))
        at += size
    parts = dict(sections)
    imports, at = uint(parts[2], 0)
    for _ in range(imports):
        _, at = word(parts[2], at)
        _, at = word(parts[2], at)
        assert parts[2][at] == 0, "function-only imports required"
        _, at = uint(parts[2], at + 1)
    funcs, at = uint(parts[3], 0)
    types = []
    for _ in range(funcs):
        ty, at = uint(parts[3], at)
        types.append(ty)
    exports, at = uint(parts[7], 0)
    entries = []
    for _ in range(exports):
        name, at = word(parts[7], at)
        kind = parts[7][at]
        index, at = uint(parts[7], at + 1)
        entries.append((name, kind, index))
    poll = next(index for name, kind, index in entries if name == "hd.poll" and kind == 0)
    candidates = set(re.findall(rb"\x23([\x00-\x7f])\x23\1\x28\x42\x01\xf4\x01", parts[10]))
    assert len(candidates) == 1, "unrecognized allocator counter layout"
    table = next(iter(candidates))[0]
    wrapper = imports + funcs
    # Original poll first; propagate a pending/error status unchanged. On normal
    # completion read exactly one byte of GIVEN. Never treat the byte as u8's
    # truncated representation of a larger unchecked result.
    body = (b"\x01\x01\x7f\x20\0\x10" + leb(poll) +
        b"\x22\x01\x04\x40\x20\x01\x0f\x0b\x23" + leb(table) +
        b"\x45\x04\x7f\x41\0\x05\x23" + leb(table) +
        b"\x28\x42\x01\xf4\x01\x0b\x41" + leb(shift) + b"\x76\x41\xff\x01\x71\x0b")
    out = bytearray(data[:8])
    for kind, payload in sections:
        if kind == 3:
            _, at = uint(payload, 0)
            payload = leb(funcs + 1) + payload[at:] + leb(types[poll-imports])
        elif kind == 7:
            payload = bytearray(leb(exports))
            for name, export_kind, index in entries:
                encoded = name.encode()
                payload += leb(len(encoded)) + encoded + bytes([export_kind]) + leb(wrapper if name == "hd.poll" else index)
        elif kind == 10:
            count, at = uint(payload, 0)
            assert count == funcs
            payload = leb(count + 1) + payload[at:] + leb(len(body)) + body
        out += bytes([kind]) + leb(len(payload)) + payload
    return out


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--hd", type=Path, required=True)
    p.add_argument("--work", type=Path, required=True)
    p.add_argument("--n", type=int, nargs="+", default=[100, 1_000_000])
    args = p.parse_args()
    work = args.work.resolve()
    hd = args.hd.resolve()
    env = {**os.environ, "HD_CACHE": str(work / "counter-cache")}
    rows = []
    for artifact in json.loads((work / "artifacts.json").read_text()):
        original = Path(artifact["artifact"])
        for n in args.n:
            status_bytes = []
            for shift in [0, 8, 16, 24]:
                counter = original.with_name(original.stem + f"-count{shift}.wasm")
                counter.write_bytes(instrument(original.read_bytes(), shift))
                cmd = (["node", str(ROOT / "host/run.mjs"), str(counter), str(n)] if artifact["engine"] == "node"
                    else [str(hd), str(counter), "--engine", "wasmtime", "--", str(n)])
                run = subprocess.run(cmd, env=env, capture_output=True, text=True, timeout=120)
                assert 0 <= run.returncode <= 255 and not run.stderr and run.stdout == expected(artifact["dataset"], n), run
                status_bytes.append(run.returncode)
            total = sum(byte << shift for byte, shift in zip(status_bytes, [0, 8, 16, 24]))
            row = {**{k:artifact[k] for k in ["dataset", "form", "engine"]}, "n": n,
                "allocation_requests": total, "counter_bytes": status_bytes}
            rows.append(row)
            (work / "allocation-counts.json").write_text(json.dumps(rows, indent=2))
            print(row, flush=True)


if __name__ == "__main__":
    main()
