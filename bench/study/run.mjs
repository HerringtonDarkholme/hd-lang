// The runtime suite on the RC (linear-memory) backend: the eleven programs
// of the Rust model study (§8, in git history at b3f5eed1a, and its
// appendix), #634's iterator bench, bench/runtime/progs, bench/micro and
// the tokenizer and math cases. Each hd program is built with
// `hd build --release --engine E` for E in node and wasmtime, run on Node
// through host/run.mjs and on wasmtime as `hd FILE.wasm --engine wasmtime`
// (Cranelift compile included, as `hd run` pays it), and checked against
// its reference's output. JavaScript (Node) and Go references run the same
// way; Python references use python3. The existing wall-time table uses process wall time,
// the median of the measured runs, in integer milliseconds;
// peak RSS is the largest `/usr/bin/time -l` saw.
//
// Usage: node bench/study/run.mjs [--runs N] [--only a,b] [--engines node,wasmtime]
//                                 [--hd PATH] [--work DIR] [--no-refs] [--python-only]
//                                 [--backend B] [--quick] [--warm-module]
//
// Each study program's input is tuned so hd on V8 takes about 300 ms
// (https://matklad.github.io/2026/10/05/benchmark-milliseconds.html); times
// are integer milliseconds, the median of 5 fresh-process runs after a discarded
// preflight process. That preflight does not warm any measured Wasm module.
// `--warm-module` additionally compiles once on Node, discards three activations,
// then measures fresh hosts and instances of that same module in this process.
// `--quick` is hd on V8 only, no references, 3 runs. `--backend B` passes
// `--backend B` to `hd build` (the Wasm GC era CLI has `--backend wasm-gc`).
//        node bench/study/run.mjs --checked PATH [--only a,b] [--work DIR]
//
// `--checked PATH` names an `hd` whose disk build sets `Packages::checked`
// when HD_BENCH_CHECKED is set (a local patch; checked mode has no CLI
// flag) and whose checked runtime folds the allocation count into the high
// word of hd.retained and the heap pages into that of hd.released. It
// builds each program at the reduced input in SMALL and prints retains,
// releases, allocations and heap pages; checked mode never reuses a block,
// so the pages bound the bytes allocated.
import { execFileSync, spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { createHost } from "../../host/node.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));
const REPO = resolve(HERE, "..", "..");
const RUNNER = join(REPO, "host", "run.mjs");

const STUDY = [
  "spectral_norm",
  "fannkuch_redux",
  "mandelbrot",
  "nbody",
  "sieve",
  "binary_trees",
  "points",
  "closures",
  "map_count",
  "string_build",
  "json",
  "structure_walk",
];
// Fixed stdout for the study inputs, also allowing hd-only or Python-only
// measurements without launching another reference to obtain expected output.
const EXPECTED = {
  spectral_norm: "1274224152\n",
  fannkuch_redux: "73196 38\n",
  mandelbrot: "778803\n",
  nbody: "-169075163 -169008104\n",
  sieve: "2204262\n",
  binary_trees:
    "stretch 18 524287\n131072 4 4063232\n32768 6 4161536\n8192 8 4186112\n2048 10 4192256\n512 12 4193792\n128 14 4194176\n32 16 4194272\nlong 17 262143\n",
  points: "3631250395357\n",
  closures: "745614\n",
  map_count: "50021 124948550\n",
  string_build: "89888890\n",
  json: "13302414 85311667\n",
  structure_walk: "787500\n",
  dyn_iter:
    "dyn 66666653333334\nin-place 66666653333334\nloop 66666653333334\nrange 66666653333334\ndyn-short 653400000\nin-place-short 653400000\n",
};

const PROGS = [
  "closures",
  "int-loop",
  "list-sort",
  "map-count",
  "nested-display",
  "string-build",
  "string-builder",
  "trait-dispatch",
];
const MICRO = ["fib", "sum", "string_build", "map", "sort"];
// A program with no work: its time is start-up and instantiation.
const EMPTY = 'pub fn main() -> void:\n    if 1 + 1 != 2: panic("wrong")\n';

/** Every case: its name, hd source, and the references that print the same output. */
function cases() {
  const list = [];
  for (const n of STUDY) {
    list.push({
      name: n,
      hd: join(HERE, `${n}.hd`),
      js: join(HERE, `${n}.js`),
      go: join(HERE, "go", `${n}.go`),
      py: join(HERE, "py", `${n}.py`),
    });
  }
  list.push({
    name: "dyn_iter",
    hd: join(HERE, "dyn_iter.hd"),
    go: join(HERE, "go", "dyn_iter.go"),
    py: join(HERE, "py", "dyn_iter.py"),
  });
  for (const n of PROGS) {
    const base = join(REPO, "bench", "runtime", "progs", n);
    list.push({ name: `progs/${n}`, hd: `${base}.hd`, js: `${base}.js` });
  }
  for (const n of MICRO) {
    list.push({ name: `micro/${n}`, hd: join(REPO, "bench", "micro", `${n}.hd`) });
  }
  list.push({ name: "empty", src: EMPTY, emptyExpected: true });
  for (const n of ["tokenizer", "math"]) {
    const dir = join(REPO, "bench", n);
    list.push({ name: n, hd: join(dir, "main.hd"), expected: join(dir, "expected.out") });
  }
  return list;
}

// Reduced inputs for checked builds: [constant line, replacement].
const SMALL = {
  spectral_norm: [["N := 2800", "N := 250"]],
  fannkuch_redux: [["N := 10", "N := 8"]],
  mandelbrot: [["SIZE := 1400", "SIZE := 160"]],
  nbody: [["STEPS := 7_000_000", "STEPS := 50_000"]],
  sieve: [["N := 36_000_000", "N := 300_000"]],
  binary_trees: [["MAX := +17", "MAX := +10"]],
  points: [["N := 7_500_000", "N := 10_000"]],
  closures: [["N := +650_000", "N := +10_000"]],
  map_count: [["N := 2_500_000", "N := 20_000"]],
  string_build: [["N := 7_000_000", "N := 30_000"]],
  json: [["N := 170_000", "N := 4_000"]],
  dyn_iter: [
    ["N := 10_000_000i64", "N := 100_000i64"],
    ["CALLS := 100_000", "CALLS := 1_000"],
  ],
};

function parseArgs(argv) {
  const opts = { runs: 5, engines: ["node", "wasmtime"], refs: true };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === "--runs") {
      opts.runs = Number(argv[++i]);
    } else if (a === "--only") {
      opts.only = argv[++i].split(",");
    } else if (a === "--engines") {
      opts.engines = argv[++i].split(",");
    } else if (a === "--hd") {
      opts.hd = resolve(argv[++i]);
    } else if (a === "--work") {
      opts.work = resolve(argv[++i]);
    } else if (a === "--checked") {
      opts.checked = resolve(argv[++i]);
    } else if (a === "--no-refs") {
      opts.refs = false;
    } else if (a === "--warm-module") {
      opts.warmModule = true;
    } else if (a === "--backend") {
      opts.backend = argv[++i];
    } else if (a === "--quick") {
      opts.quick = true;
      opts.runs = 3;
      opts.engines = ["node"];
      opts.refs = false;
    } else if (a === "--python-only") {
      opts.pythonOnly = true;
    } else {
      throw new Error(`unknown flag ${a}`);
    }
  }
  if (opts.quick && argv.includes("--runs")) {
    opts.runs = Number(argv[argv.indexOf("--runs") + 1]);
  }
  if (!Number.isInteger(opts.runs) || opts.runs < 1) {
    throw new Error("--runs must be a positive integer");
  }
  if (opts.warmModule && (opts.checked || opts.pythonOnly || !opts.engines.includes("node"))) {
    throw new Error("--warm-module requires the Node engine and ordinary timing mode");
  }
  opts.hd ??= join(REPO, "target", "release", "hd");
  opts.work ??= join(REPO, "target", "bench-study");
  return opts;
}

/** Runs argv under /usr/bin/time -l: wall ms, peak RSS MB, stdout, status. */
function timed(argv, env = process.env) {
  const start = performance.now();
  const r = spawnSync("/usr/bin/time", ["-l", ...argv], {
    encoding: "utf8",
    env,
    maxBuffer: 1 << 28,
  });
  const ms = performance.now() - start;
  const rss = /(\d+)\s+maximum resident set size/.exec(r.stderr ?? "");
  return {
    ms,
    rssMb: rss ? Number(rss[1]) / 1048576 : null,
    out: r.stdout ?? "",
    err: r.stderr ?? "",
    status: r.status,
  };
}

const median = (xs) => {
  const s = [...xs].sort((a, b) => a - b);
  return s[Math.floor(s.length / 2)];
};

/**
 * Median fresh-process wall ms and peak RSS after a discarded preflight. A first run
 * over 20 s counts as measured, and one over 60 s is the only run.
 * Python-only mode disables the discarded preflight and requests exactly one run.
 */
export function measure(argv, runs, expect, preflight = true) {
  const first = timed(argv);
  const expected = expect ?? first.out;
  const failure = validate(first, expected);
  if (failure) {
    return { error: failure };
  }
  const samples = !preflight || first.ms > 20_000 ? [first] : [];
  const want = first.ms > 60_000 ? 1 : runs;
  while (samples.length < want) {
    const sample = timed(argv);
    const failure = validate(sample, expected);
    if (failure) {
      return { error: failure };
    }
    samples.push(sample);
  }
  return {
    ...summarize(samples),
    rssMb: Math.max(...samples.map((s) => s.rssMb ?? 0)),
    out: first.out,
    err: first.err,
  };
}

function validate(sample, expected) {
  if (sample.status !== 0) {
    return `exit ${sample.status}: ${sample.err.slice(-400)}`;
  }
  if (sample.out !== expected) {
    return `output ${JSON.stringify(sample.out.slice(0, 200))}`;
  }
  return null;
}

function summarize(samples) {
  return {
    runs: samples.length,
    ms: median(samples.map((s) => s.ms)),
    spread: Math.max(...samples.map((s) => s.ms)) - Math.min(...samples.map((s) => s.ms)),
  };
}

/** One compiled module; fresh host and instance for every discarded/measured activation.
 * Includes host creation, instantiation, init/poll and close; excludes compilation.
 * Every activation must match the expected stdout and succeed, including warm-ups.
 */
export async function measureWarmModule(module, runs, expect, warmups = 3) {
  const samples = [];
  for (let i = 0; i < warmups + runs; i++) {
    const out = [];
    const err = [];
    const start = performance.now();
    const host = createHost(
      { out: (text) => out.push(text), err: (text) => err.push(text) },
      { module },
    );
    let status;
    try {
      const instance = await WebAssembly.instantiate(module, host.imports);
      ({ status } = await host.run(instance, "hd.init", "hd.poll"));
    } finally {
      host.close();
    }
    const sample = { ms: performance.now() - start, status, out: out.join(""), err: err.join("") };
    const failure = validate(sample, expect);
    if (failure) {
      return { error: failure };
    }
    if (i >= warmups) {
      samples.push(sample);
    }
  }
  return { ...summarize(samples), warmups, outputValidation: "exact stdout, every activation" };
}

/** Builds `source` as package NAME in WORK/pkgs; returns the release Wasm path. */
function build(hd, work, name, source, engine, env = process.env, backend = undefined) {
  const flat = name.replaceAll("/", "-").replaceAll("_", "-");
  const dir = join(work, "pkgs", `${flat}-${engine}`);
  rmSync(dir, { recursive: true, force: true });
  mkdirSync(join(dir, "src"), { recursive: true });
  // Every study program's main uses Console, and one uses Clock (cli.cap.table.required).
  writeFileSync(
    join(dir, "hd.toml"),
    `[package]\nname = "${flat}"\n\n[capabilities]\nConsole = true\nClock = true\n`,
  );
  writeFileSync(join(dir, "src", "main.hd"), source);
  execFileSync(
    hd,
    ["build", "--release", "--engine", engine, ...(backend ? ["--backend", backend] : [])],
    {
      cwd: dir,
      env: { ...env, HD_CACHE: join(work, "cache") },
      stdio: ["ignore", "ignore", "inherit"],
    },
  );
  return join(dir, "build", "release", `${flat}.wasm`);
}

function checkedRun(opts, list) {
  const env = { ...process.env, HD_BENCH_CHECKED: "1" };
  const work = join(opts.work, "checked");
  mkdirSync(work, { recursive: true });
  const probe = join(work, "probe.mjs");
  writeFileSync(probe, PROBE.replace("RUNNER", JSON.stringify(RUNNER)));
  console.log("| Program | reduced input | retains | releases | allocations | heap pages |");
  console.log("| --- | --- | ---: | ---: | ---: | ---: |");
  for (const c of list) {
    let src = c.src ?? readFileSync(c.hd, "utf8");
    const small = SMALL[c.name] ?? [];
    for (const [from, to] of small) {
      src = src.replace(from, to);
    }
    const wasm = build(opts.checked, work, c.name, src, "node", env);
    const r = spawnSync("node", [probe, wasm], { encoding: "utf8", maxBuffer: 1 << 28 });
    const line = /PROBE (.*)/.exec(r.stderr ?? "");
    const p = line ? JSON.parse(line[1]) : {};
    const input = small.map(([, to]) => to).join(", ") || "full";
    const ok = r.status === 0 ? "" : ` (exit ${r.status}: ${(r.stderr ?? "").split("\n")[0]})`;
    console.log(
      `| ${c.name}${ok} | ${input} | ${p.retained ?? "?"} | ${p.released ?? "?"} | ${p.allocated ?? "?"} | ${p.pages ?? "?"} |`,
    );
  }
}

const PROBE = `
import { writeSync } from "node:fs";
const orig = WebAssembly.instantiate;
let inst = null;
WebAssembly.instantiate = async function (...a) {
  const r = await orig.apply(WebAssembly, a);
  inst = r.instance ?? r;
  return r;
};
process.on("exit", () => {
  const e = inst?.exports ?? {};
  const rep = {};
  if (e["hd.retained"]) {
    const r = BigInt(e["hd.retained"]());
    const l = BigInt(e["hd.released"]());
    rep.retained = Number(r & 0xffffffffn);
    rep.allocated = Number(r >> 32n);
    rep.released = Number(l & 0xffffffffn);
    rep.pages = Number(l >> 32n);
  }
  writeSync(2, "PROBE " + JSON.stringify(rep) + "\\n");
});
const runner = RUNNER;
process.argv = [process.argv[0], runner, ...process.argv.slice(2)];
await import("file://" + runner);
`;

async function main() {
  const startedAt = performance.now();
  const opts = parseArgs(process.argv.slice(2));
  let list = cases();
  if (opts.only) {
    list = list.filter((c) => opts.only.includes(c.name));
  }
  mkdirSync(join(opts.work, "go"), { recursive: true });
  if (opts.checked) {
    return checkedRun(opts, list);
  }
  if (opts.pythonOnly) {
    list = list.filter((c) => c.py);
  }
  let python = false;
  if ((opts.refs || opts.pythonOnly) && list.some((c) => c.py)) {
    const probe = spawnSync("python3", ["--version"], { encoding: "utf8" });
    python = probe.status === 0;
    if (!python) {
      console.error("Note: python3 unavailable; skipping Python references.");
    }
  }
  const rows = [];
  for (const c of list) {
    const row = { name: c.name };
    rows.push(row);
    // Known study outputs need no reference process, including --no-refs.
    let expect = EXPECTED[c.name];
    if (expect === undefined && c.js && !opts.pythonOnly) {
      expect = execFileSync("node", [c.js], { encoding: "utf8" });
    } else if (c.expected) {
      expect = readFileSync(c.expected, "utf8");
    } else if (c.emptyExpected) {
      expect = "";
    }
    if (opts.refs && !opts.pythonOnly && c.js) {
      row.js = measure(["node", c.js], opts.runs, expect);
    }
    if (opts.refs && !opts.pythonOnly && c.go) {
      const bin = join(opts.work, "go", c.name);
      execFileSync("go", ["build", "-o", bin, c.go]);
      row.go = measure([bin], opts.runs, expect);
    }
    if (python && c.py) {
      row.py = measure(
        ["python3", c.py],
        opts.pythonOnly ? 1 : opts.runs,
        expect,
        !opts.pythonOnly,
      );
    }
    for (const engine of opts.pythonOnly ? [] : opts.engines) {
      const wasm = build(
        opts.hd,
        opts.work,
        c.name,
        c.src ?? readFileSync(c.hd, "utf8"),
        engine,
        process.env,
        opts.backend,
      );
      row[`${engine}Bytes`] = statSync(wasm).size;
      const argv =
        engine === "node" ? ["node", RUNNER, wasm] : [opts.hd, wasm, "--engine", "wasmtime"];
      row[engine] = measure(argv, opts.runs, expect);
      if (engine === "node" && opts.warmModule && !row.node.error) {
        const module = await WebAssembly.compile(readFileSync(wasm));
        row.warmModule = await measureWarmModule(module, opts.runs, expect ?? row.node.out);
      }
    }
    console.error(JSON.stringify(row, (k, v) => (k === "out" || k === "err" ? undefined : v)));
  }
  writeFileSync(join(opts.work, "results.json"), JSON.stringify(rows, null, 1));
  const cell = (m) => (m ? (m.error ? "fail" : Math.round(m.ms).toString()) : "");
  const rss = (m) => (m && !m.error ? Math.round(m.rssMb).toString() : "");
  console.log(
    `Node ${process.version}; V8 ${process.versions.v8}. Process-wall samples each start a fresh process; a discarded preflight is not module warm-up.`,
  );
  console.log(
    "| Program | hd, V8 | hd, wasmtime | Node | Go | py | hd/Node | hd/Go | RSS MB: hd V8, wasmtime, Node, Go, py |",
  );
  console.log("| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |");
  for (const r of rows) {
    const ratio = (a, b) => (a && b && !a.error && !b.error ? (a.ms / b.ms).toFixed(1) : "");
    console.log(
      `| ${r.name} | ${cell(r.node)} | ${cell(r.wasmtime)} | ${cell(r.js)} | ${cell(r.go)} | ${cell(r.py)} | ${ratio(r.node, r.js)} | ${ratio(r.node, r.go)} | ${[r.node, r.wasmtime, r.js, r.go, r.py].map(rss).join(", ")} |`,
    );
  }
  for (const r of rows) {
    for (const k of ["node", "wasmtime", "js", "go", "py"]) {
      if (r[k]?.error) {
        console.log(`- ${r.name} ${k}: ${r[k].error}`);
      }
    }
  }
  if (opts.warmModule) {
    console.log("\nNode/V8 timings, ms (median):");
    console.log(
      "| Program | fresh-process wall | samples | warm-module activation | samples | discarded activations | output validation |",
    );
    console.log("| --- | ---: | ---: | ---: | ---: | ---: | --- |");
    for (const r of rows) {
      console.log(
        `| ${r.name} | ${cell(r.node)} | ${r.node?.runs ?? ""} | ${cell(r.warmModule)} | ${r.warmModule?.runs ?? ""} | ${r.warmModule?.warmups ?? ""} | ${r.warmModule && !r.warmModule.error ? "exact stdout, every sample" : "failed"} |`,
      );
      if (r.warmModule?.error) {
        console.log(`- ${r.name} warm-module: ${r.warmModule.error}`);
      }
    }
  }
  const dyn = rows.find((r) => r.name === "dyn_iter");
  if (dyn) {
    for (const k of ["node", "wasmtime", "go", "py"]) {
      if (dyn[k] && !dyn[k].error) {
        console.log(
          `- dyn_iter ${k}: ${dyn[k].err
            .split("\n")
            .filter((l) => /^\S+ \d+ms$/.test(l))
            .join("; ")}`,
        );
      }
    }
  }
  console.log(`\nTotal ${Math.round((performance.now() - startedAt) / 1000)} s.`);
  if (
    rows.some((r) => ["node", "wasmtime", "js", "go", "py", "warmModule"].some((k) => r[k]?.error))
  ) {
    process.exitCode = 1;
  }
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  if (!existsSync(RUNNER)) {
    throw new Error(`no ${RUNNER}`);
  }
  main().catch((error) => {
    console.error(error);
    process.exitCode = 1;
  });
}
