"""Reproduce #826 probes on macOS; no production engine changes or full tests.

All generated files go in a new scratch directory. Supply an existing native hd
binary and a local public rusty_v8 Release archive; downloads are not timed.
"""

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import statistics
import subprocess
import time

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
NAMES = ["empty", "closures", "int-loop", "list-sort", "map-count",
         "string-build", "string-builder", "trait-dispatch"]


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def execute(command, cwd, env):
    started = time.perf_counter()
    result = subprocess.run([str(x) for x in command], cwd=cwd, env=env,
                            capture_output=True, check=False)
    return result, (time.perf_counter() - started) * 1000


def checked(command, cwd, env):
    result, elapsed = execute(command, cwd, env)
    if result.returncode:
        raise RuntimeError(result.stdout.decode() + result.stderr.decode())
    return result, elapsed


def save(path, value):
    path.write_text(json.dumps(value, indent=2) + "\n")


def build(project, target, feature, binary, env, scratch):
    assert not target.exists(), "cold build target must be new"
    build_env = dict(env, CARGO_TARGET_DIR=str(target))
    command = ["cargo", "build", "--manifest-path", project / "Cargo.toml",
               "--locked", "--features", feature, "--bin", binary]
    result, elapsed = execute(command, ROOT, build_env)
    (scratch / (target.name + "-build.log")).write_bytes(result.stdout + result.stderr)
    if result.returncode:
        raise RuntimeError("build failed; see preserved build log")
    output = target / "debug" / binary
    stripped = scratch / (target.name + "-stripped")
    shutil.copy2(output, stripped)
    checked(["strip", "-S", "-x", stripped], ROOT, env)
    return output, {"seconds": elapsed / 1000, "binaryBytes": output.stat().st_size,
                    "strippedBytes": stripped.stat().st_size, "sha256": sha(output)}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("mode", choices=["components", "runtime"])
    parser.add_argument("scratch", type=Path)
    parser.add_argument("--archive", type=Path, required=True)
    parser.add_argument("--hd", type=Path)
    args = parser.parse_args()
    scratch = args.scratch.resolve()
    scratch.mkdir(parents=True, exist_ok=False)
    env = dict(os.environ, HD_CACHE=str(scratch / "cache"),
               RUSTY_V8_ARCHIVE=str(args.archive.resolve()))
    receipt = {"mode": args.mode, "archiveSha256": sha(args.archive),
               "base": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT).decode().strip(),
               "profile": "Rust dev opt-level=1, default debug; prebuilt Release V8 C++",
               "sourceHashes": {str(f.relative_to(HERE)): sha(f) for f in HERE.rglob("*")
                                if f.is_file() and f.suffix in (".rs", ".js", ".mjs", ".py", ".toml", ".lock")}}
    report = scratch / "results.json"
    if args.mode == "components":
        receipt["builds"] = {}
        binaries = {}
        for engine, feature, binary in [("v8", "v8-engine", "v8-probe"),
                                         ("wasmtime", "wasmtime-engine", "wasmtime-probe")]:
            binaries[engine], row = build(HERE / "component", scratch / engine,
                                           feature, binary, env, scratch)
            receipt["builds"][engine] = row
            save(report, receipt)
        wasm = scratch / "empty.wasm"
        wasm.write_bytes(b"\0asm\x01\0\0\0")
        receipt["startup"] = []
        for trial in range(-1, 15):
            order = ["v8", "wasmtime"] if trial % 2 == 0 else ["wasmtime", "v8"]
            for engine in order:
                result, elapsed = checked([binaries[engine], wasm], ROOT, env)
                assert result.stdout == b"ok\n" and not result.stderr
                receipt["startup"].append({"trial": trial, "engine": engine, "ms": elapsed})
                save(report, receipt)
    else:
        if args.hd is None:
            parser.error("runtime needs --hd pointing to a native-feature CLI")
        hd = args.hd.resolve()
        receipt["hdSha256"] = sha(hd)
        v8, receipt["build"] = build(HERE / "hd", scratch / "hd-probe",
                                       "v8-engine", "v8-probe", env, scratch)
        checked(["node", HERE / "bundle.mjs", scratch / "js"], ROOT, env)
        script = scratch / "js/hd-run.js"
        receipt["scriptSha256"] = sha(script)
        receipt["cases"] = []
        for name in NAMES:
            source = (b"pub fn main() -> void:\n    if 1 + 1 != 2: panic(\"wrong\")\n"
                      if name == "empty" else (ROOT / "bench/runtime/progs" / (name + ".hd")).read_bytes())
            modules = {}
            for engine in ("node", "wasmtime"):
                package = scratch / "packages" / (name + "-" + engine)
                (package / "src").mkdir(parents=True)
                (package / "src/main.hd").write_bytes(source)
                (package / "hd.toml").write_text('[package]\nname = "' + name +
                                                '"\n[capabilities]\nConsole = true\nClock = true\n')
                checked([hd, "build", "--release", "--engine", engine], package, env)
                modules[engine] = package / "build/release" / (name + ".wasm")
            expected = b"" if name == "empty" else checked(
                ["node", ROOT / "bench/runtime/progs" / (name + ".js")], ROOT, env)[0].stdout
            commands = {"embedded-v8": [v8, modules["node"], script],
                        "node": ["node", ROOT / "host/run.mjs", modules["node"]],
                        "wasmtime": [hd, modules["wasmtime"], "--engine", "wasmtime"]}
            case = {"program": name, "sourceSha256": hashlib.sha256(source).hexdigest(),
                    "modules": {k: {"bytes": v.stat().st_size, "sha256": sha(v)} for k, v in modules.items()},
                    "expected": expected.decode(), "rows": [], "freshCacheRows": []}
            receipt["cases"].append(case)
            for trial in range(-1, 5):
                order = list(commands)
                order = order[trial % 3:] + order[:trial % 3]
                for engine in order:
                    result, elapsed = checked(["/usr/bin/time", "-l", *commands[engine]], ROOT, env)
                    assert result.stdout == expected
                    rss = re.search(rb"^\s*(\d+)\s+maximum resident set size$", result.stderr, re.M)
                    case["rows"].append({"trial": trial, "engine": engine, "ms": elapsed,
                                         "stdout": result.stdout.decode(), "status": result.returncode,
                                         "rssBytes": int(rss[1]) if rss else None,
                                         "stderrAndResourceUsage": result.stderr.decode()})
                    save(report, receipt)
            for trial in range(-1, 5):
                cache = scratch / "fresh-cache" / name / str(trial)
                assert not cache.exists()
                result, elapsed = checked(["/usr/bin/time", "-l", *commands["wasmtime"]],
                                           ROOT, dict(env, HD_CACHE=str(cache)))
                assert result.stdout == expected
                case["freshCacheRows"].append({"trial": trial, "ms": elapsed,
                                               "stdout": result.stdout.decode(), "status": result.returncode,
                                               "stderrAndResourceUsage": result.stderr.decode()})
                save(report, receipt)
            print(name, {k: statistics.median(x["ms"] for x in case["rows"]
                                              if x["trial"] >= 0 and x["engine"] == k)
                         for k in commands}, flush=True)
    save(report, receipt)
    print(report)


if __name__ == "__main__":
    main()
