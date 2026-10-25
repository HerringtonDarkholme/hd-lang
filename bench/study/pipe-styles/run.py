"""Build and time the equivalent #822 desugarings; never edits compiler/std."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import resource
import statistics
import subprocess
import time

ROOT = Path(__file__).resolve().parents[3]
FORMS = ["direct", "curried", "partial"]


def expected(dataset, n):
    values = (str(i * 3 + 7) for i in range(n) if i % 3 == 1) if dataset == "numeric" else (
        f"row{i % 17}!" for i in range(n) if "row1" in f"  row{i % 17}  "
    )
    values = list(values)
    text = ",".join(values)
    checksum = 0
    for byte in text.encode():
        checksum = (checksum * 131 + byte) % 1_000_000_007
    return f"{len(values)} {len(values)} {len(text)} {checksum}\n"


def uint(data, at):
    value = shift = 0
    while True:
        byte = data[at]
        at += 1
        value |= (byte & 127) << shift
        if byte < 128:
            return value, at
        shift += 7


def wasm_metrics(path):
    data = path.read_bytes()
    at = 8
    sections = {}
    while at < len(data):
        kind = data[at]
        size, at = uint(data, at + 1)
        sections[kind] = data[at:at + size]
        at += size
    return {
        "bytes": len(data), "functions": uint(sections[3], 0)[0],
        "code_bytes": len(sections[10]),
        "code_sha256": hashlib.sha256(sections[10]).hexdigest(),
        "sha256": hashlib.sha256(data).hexdigest(),
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--hd", type=Path, required=True)
    parser.add_argument("--work", type=Path, required=True)
    parser.add_argument("--runs", type=int, default=21)
    parser.add_argument("--n", type=int, default=1_000_000)
    parser.add_argument("--quick", action="store_true", help="V8 only, as bench/study --quick")
    parser.add_argument("--prepare-only", action="store_true")
    args = parser.parse_args()
    hd = args.hd.resolve()
    work = args.work.resolve()
    work.mkdir(parents=True, exist_ok=True)
    env = {**os.environ, "HD_CACHE": str(work / "cache")}
    env.pop("HD_JOBS", None)
    subprocess.run(["python3", str(Path(__file__).with_name("generate.py")), str(work)], check=True)
    engines = ["node"] if args.quick else ["node", "wasmtime"]
    artifacts = []
    for dataset in ["numeric", "string"]:
        for form in FORMS:
            package = work / f"{dataset}-{form}"
            for engine in engines:
                compiled = subprocess.run([str(hd), "build", "--release", "--engine", engine],
                    cwd=package, env=env, capture_output=True, text=True, timeout=60)
                if compiled.returncode:
                    raise RuntimeError(compiled.stdout + compiled.stderr)
                artifact = work / f"{dataset}-{form}-{engine}.wasm"
                artifact.write_bytes((package / "build/release/pipe_probe.wasm").read_bytes())
                command = (["node", str(ROOT / "host/run.mjs"), str(artifact)] if engine == "node"
                    else [str(hd), str(artifact), "--engine", engine, "--"])
                check = subprocess.run([*command, "100"], env=env, capture_output=True, text=True, timeout=60)
                assert check.returncode == 0 and not check.stderr and check.stdout == expected(dataset, 100), check
                row = {"dataset": dataset, "form": form, "engine": engine,
                    "artifact": str(artifact), "command": command, **wasm_metrics(artifact)}
                artifacts.append(row)
                print("prepared", dataset, form, engine, row["bytes"], flush=True)
    (work / "artifacts.json").write_text(json.dumps(artifacts, indent=2))
    if args.prepare_only:
        return
    answers = {dataset: expected(dataset, args.n) for dataset in ["numeric", "string"]}
    # A discarded fresh process verifies the million-input workload. It does
    # not warm the modules in the later fresh processes.
    for row in artifacts:
        checked = subprocess.run([*row["command"], str(args.n)], env=env, capture_output=True, text=True, timeout=120)
        assert checked.returncode == 0 and not checked.stderr and checked.stdout == answers[row["dataset"]], checked
    rows = []
    for repeat in range(args.runs):
        forms = FORMS[repeat % len(FORMS):] + FORMS[:repeat % len(FORMS)]
        for dataset in (["numeric", "string"] if repeat % 2 == 0 else ["string", "numeric"]):
            for form in forms:
                order = engines if (repeat + FORMS.index(form)) % 2 == 0 else list(reversed(engines))
                for engine in order:
                    row = next(x for x in artifacts if (x["dataset"], x["form"], x["engine"]) == (dataset, form, engine))
                    before = resource.getrusage(resource.RUSAGE_CHILDREN)
                    started = time.perf_counter_ns()
                    run = subprocess.run([*row["command"], str(args.n)], env=env, capture_output=True, text=True, timeout=120)
                    wall = (time.perf_counter_ns() - started) / 1_000_000
                    after = resource.getrusage(resource.RUSAGE_CHILDREN)
                    assert run.returncode == 0 and not run.stderr and run.stdout == answers[dataset], run
                    result = {"repeat": repeat, "dataset": dataset, "form": form, "engine": engine,
                        "wall_ms": wall, "user_ms": (after.ru_utime-before.ru_utime)*1000,
                        "system_ms": (after.ru_stime-before.ru_stime)*1000, "load": os.getloadavg()}
                    result["cpu_ms"] = result["user_ms"] + result["system_ms"]
                    rows.append(result)
                    (work / "timings.json").write_text(json.dumps(rows, indent=2))
        print("completed repeat", repeat + 1, flush=True)
    summary = []
    for artifact in artifacts:
        selected = [r for r in rows if all(r[k] == artifact[k] for k in ["dataset", "form", "engine"])]
        assert len(selected) == args.runs
        summary.append({**{k:artifact[k] for k in ["dataset", "form", "engine"]},
            **{k:statistics.median(r[k] for r in selected) for k in ["wall_ms", "cpu_ms", "user_ms", "system_ms"]}})
    (work / "summary.json").write_text(json.dumps(summary, indent=2))
    print(json.dumps(summary, indent=2))


if __name__ == "__main__":
    main()
