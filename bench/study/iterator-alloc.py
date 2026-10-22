"""Count #634's chain allocations without changing the compiler or runtime.

Usage: python3 bench/study/iterator-alloc.py target/debug/hd [WORK]
Builds optimized Node/Wasmtime modules. A scratch-only hd.poll wrapper
returns the allocator's block counter as the process status (must fit u8).
The original poll and guest computation are preserved; stdout is checked.
Per-call slopes remove fixed Args, formatting and loop setup overhead.
Counts at two input lengths distinguish per-element allocations; counts
at one, two and eight calls identify per-chain costs.
The wrapper assumes this backend's heap memory 1 and GIVEN offset 244;
it refuses an unfamiliar allocator layout instead of guessing an address.
"""
import json
import os
from pathlib import Path
import re
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[2]


def uleb(n):
    out = bytearray()
    while True:
        b = n & 127
        n >>= 7
        out.append(b | (128 if n else 0))
        if not n:
            return bytes(out)


def uint(data, at):
    value = shift = 0
    while True:
        b = data[at]
        at += 1
        value |= (b & 127) << shift
        if not b & 128:
            return value, at
        shift += 7


def text(data, at):
    size, at = uint(data, at)
    return data[at:at + size].decode(), at + size


def counted(data):
    assert data[:8] == b"\0asm\x01\0\0\0"
    sections = []
    at = 8
    while at < len(data):
        kind = data[at]
        size, at = uint(data, at + 1)
        sections.append((kind, data[at:at + size]))
        at += size
    payloads = dict(sections)
    imports, at = uint(payloads[2], 0)
    for _ in range(imports):
        _, at = text(payloads[2], at)
        _, at = text(payloads[2], at)
        assert payloads[2][at] == 0, "expected function-only imports"
        _, at = uint(payloads[2], at + 1)
    funcs, at = uint(payloads[3], 0)
    types = []
    for _ in range(funcs):
        value, at = uint(payloads[3], at)
        types.append(value)
    exports, at = uint(payloads[7], 0)
    entries = []
    for _ in range(exports):
        name, at = text(payloads[7], at)
        kind = payloads[7][at]
        index, at = uint(payloads[7], at + 1)
        entries.append((name, kind, index))
    poll = next(index for name, kind, index in entries if name == "hd.poll" and kind == 0)
    # Two global.get operations preceding the allocator's increment load.
    # Memory-1 i32.load: align=2 | memory-index flag, index=1, offset=244.
    candidates = set(re.findall(rb"\x23([\x00-\x7f])\x23\1\x28\x42\x01\xf4\x01", payloads[10]))
    assert len(candidates) == 1, "unfamiliar allocator counter pattern"
    table = next(iter(candidates))[0]
    wrapper = imports + funcs
    # One i32 local: retain the original status and return it unless done.
    body = (b"\x01\x01\x7f\x20\0\x10" + uleb(poll) +
            b"\x22\x01\x04\x40\x20\x01\x0f\x0b\x23" + uleb(table) +
            b"\x45\x04\x7f\x41\0\x05\x23" + uleb(table) +
            b"\x28\x42\x01\xf4\x01\x0b\x22\x01\x41\xff\x01\x4f\x04\x40\x00\x0b\x20\x01\x0b")
    out = bytearray(data[:8])
    for kind, payload in sections:
        if kind == 3:
            _, at = uint(payload, 0)
            payload = uleb(funcs + 1) + payload[at:] + uleb(types[poll - imports])
        elif kind == 7:
            payload = bytearray(uleb(exports))
            for name, export_kind, index in entries:
                encoded = name.encode()
                payload.extend(uleb(len(encoded)) + encoded + bytes([export_kind]) +
                               uleb(wrapper if name == "hd.poll" else index))
        elif kind == 10:
            count, at = uint(payload, 0)
            assert count == funcs
            payload = uleb(count + 1) + payload[at:] + uleb(len(body)) + body
        out.extend(bytes([kind]) + uleb(len(payload)) + payload)
    return out


def main():
    HD = Path(sys.argv[1]).resolve()
    WORK = Path(sys.argv[2]).resolve() if len(sys.argv) > 2 else ROOT / "target/iterator-alloc"
    os.environ["HD_CACHE"] = str(WORK / "cache")

    source = (ROOT / "bench/study/dyn_iter.hd").read_text().split("pub fn main()", 1)[0]
    rows = []
    for engine in ["node", "wasmtime"]:
        for variant, function in [("dyn", "via_dyn"), ("in-place", "in_place"), ("loop", "plain")]:
            package = WORK / f"{variant}-{engine}"
            (package / "src").mkdir(parents=True, exist_ok=True)
            (package / "hd.toml").write_text('[package]\nname = "iterator-count"\n')
            main = f'''use std.host.Args
    use std.text.Parse

    pub fn main() -> void $ Console + Args:
        arguments := $.use(Args).list()
        n := arguments[0].parse::[i64]().expect("input length")
        calls := arguments[1].parse::[usize]().expect("call count")
        let total: i64 = 0
        for _ in 0..calls:
            total = total + {function}(n)
        println(total)
    '''
            (package / "src/main.hd").write_text(source + main)
            subprocess.run([str(HD), "build", "--release", "--engine", engine], cwd=package,
                           check=True, stdout=subprocess.DEVNULL)
            wasm = package / "build/release/iterator-count.wasm"
            probe = wasm.with_name("counted.wasm")
            probe.write_bytes(counted(wasm.read_bytes()))
            for n in [100, 100_000]:
                k = (n - 1) // 3
                expected_sum = n * (n - 1) - 3 * k * (k + 1)
                for calls in [1, 2, 8]:
                    args = [str(n), str(calls)]
                    command = (["node", str(ROOT / "host/run.mjs"), str(probe), *args]
                               if engine == "node" else [str(HD), str(probe), "--engine", engine, "--", *args])
                    run = subprocess.run(command, capture_output=True, text=True)
                    assert run.stdout == f"{expected_sum * calls}\n", run
                    assert not run.stderr, run.stderr
                    assert 0 <= run.returncode < 255, "counter must fit the exit-status probe"
                    row = dict(engine=engine, variant=variant, n=n, calls=calls, allocations=run.returncode)
                    rows.append(row)
                    print(json.dumps(row), flush=True)
            if variant == "dyn" and (rows[-2]["allocations"] - rows[-3]["allocations"]) > 0:
                # A native exit status is only u8: prove the guest-side guard
                # refuses a larger total instead of letting it wrap or saturate.
                per_call = rows[-2]["allocations"] - rows[-3]["allocations"]
                args = ["100", str(255 // per_call + 200)]
                command = (["node", str(ROOT / "host/run.mjs"), str(probe), *args]
                           if engine == "node" else [str(HD), str(probe), "--engine", engine, "--", *args])
                run = subprocess.run(command, capture_output=True, text=True)
                assert run.returncode == 3 and "unreachable" in run.stderr, run

    (WORK / "allocations.json").write_text(json.dumps(rows, indent=2) + "\n")

    for engine in ["node", "wasmtime"]:
        for variant in ["dyn", "in-place", "loop"]:
            slopes = []
            for n in [100, 100_000]:
                selected = {r["calls"]: r["allocations"] for r in rows
                            if r["engine"] == engine and r["variant"] == variant and r["n"] == n}
                slope = selected[2] - selected[1]
                assert selected[8] - selected[2] == slope * 6, "nonlinear per-call allocation counts"
                slopes.append(slope)
            print(json.dumps(dict(engine=engine, variant=variant, per_call=slopes,
                                  constant_across_input_lengths=slopes[0] == slopes[1])), flush=True)


if __name__ == "__main__":
    main()
