// The runtime suite on the RC (linear-memory) backend: the eleven programs
// of the Rust model study (docs/future-work/RUST_MODEL_STUDY.md §8 and its
// appendix), #634's iterator bench, bench/runtime/progs, bench/micro and
// the tokenizer and math cases. Each hd program is built with
// `hd build --release --engine E` for E in node and wasmtime, run on Node
// through host/run.mjs and on wasmtime as `hd FILE.wasm --engine wasmtime`
// (Cranelift compile included, as `hd run` pays it), and checked against
// its reference's output. JavaScript (Node) and Go references run the same
// way. Every time is process wall time, the median of the measured runs;
// peak RSS is the largest `/usr/bin/time -l` saw.
//
// Usage: node bench/study/run.mjs [--runs N] [--only a,b] [--engines node,wasmtime]
//                                 [--hd PATH] [--work DIR] [--no-refs]
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
];
const PROGS = [
  "closures",
  "int-loop",
  "list-sort",
  "map-count",
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
  for (const n of STUDY)
    list.push({
      name: n,
      hd: join(HERE, `${n}.hd`),
      js: join(HERE, `${n}.js`),
      go: join(HERE, "go", `${n}.go`),
    });
  list.push({ name: "dyn_iter", hd: join(HERE, "dyn_iter.hd"), go: join(HERE, "go", "dyn_iter.go") });
  for (const n of PROGS) {
    const base = join(REPO, "bench", "runtime", "progs", n);
    list.push({ name: `progs/${n}`, hd: `${base}.hd`, js: `${base}.js` });
  }
  for (const n of MICRO) list.push({ name: `micro/${n}`, hd: join(REPO, "bench", "micro", `${n}.hd`) });
  list.push({ name: "empty", src: EMPTY, emptyExpected: true });
  for (const n of ["tokenizer", "math"]) {
    const dir = join(REPO, "bench", n);
    list.push({ name: n, hd: join(dir, "main.hd"), expected: join(dir, "expected.out") });
  }
  return list;
}

// Reduced inputs for checked builds: [constant line, replacement].
const SMALL = {
  spectral_norm: [["N := 2500", "N := 250"]],
  fannkuch_redux: [["N := 10", "N := 8"]],
  mandelbrot: [["SIZE := 1600", "SIZE := 160"]],
  nbody: [["STEPS := 5_000_000", "STEPS := 50_000"]],
  sieve: [["N := 30_000_000", "N := 300_000"]],
  binary_trees: [["MAX := +20", "MAX := +10"]],
  points: [["N := 1_000_000", "N := 10_000"]],
  closures: [["N := +1_000_000", "N := +10_000"]],
  map_count: [["N := 2_000_000", "N := 20_000"]],
  string_build: [["N := 3_000_000", "N := 30_000"]],
  json: [["N := 400_000", "N := 4_000"]],
  dyn_iter: [
    ["N := 10_000_000i64", "N := 100_000i64"],
    ["CALLS := 100_000", "CALLS := 1_000"],
  ],
};

function parseArgs(argv) {
  const opts = { runs: 3, engines: ["node", "wasmtime"], refs: true };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === "--runs") opts.runs = Number(argv[++i]);
    else if (a === "--only") opts.only = argv[++i].split(",");
    else if (a === "--engines") opts.engines = argv[++i].split(",");
    else if (a === "--hd") opts.hd = resolve(argv[++i]);
    else if (a === "--work") opts.work = resolve(argv[++i]);
    else if (a === "--checked") opts.checked = resolve(argv[++i]);
    else if (a === "--no-refs") opts.refs = false;
    else throw new Error(`unknown flag ${a}`);
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
 * Median wall ms and peak RSS over `runs` runs after a warm-up. A first run
 * over 20 s counts as measured, and one over 60 s is the only run.
 */
function measure(argv, runs, expect) {
  const first = timed(argv);
  if (first.status !== 0) return { error: `exit ${first.status}: ${first.err.slice(-400)}` };
  if (expect !== undefined && first.out !== expect)
    return { error: `output ${JSON.stringify(first.out.slice(0, 200))}` };
  const samples = first.ms > 20_000 ? [first] : [];
  const want = first.ms > 60_000 ? 1 : runs;
  while (samples.length < want) samples.push(timed(argv));
  return {
    runs: samples.length,
    ms: median(samples.map((s) => s.ms)),
    spread: Math.max(...samples.map((s) => s.ms)) - Math.min(...samples.map((s) => s.ms)),
    rssMb: Math.max(...samples.map((s) => s.rssMb ?? 0)),
    out: first.out,
    err: first.err,
  };
}

/** Builds `source` as package NAME in WORK/pkgs; returns the release Wasm path. */
function build(hd, work, name, source, engine, env = process.env) {
  const flat = name.replaceAll("/", "-").replaceAll("_", "-");
  const dir = join(work, "pkgs", `${flat}-${engine}`);
  rmSync(dir, { recursive: true, force: true });
  mkdirSync(join(dir, "src"), { recursive: true });
  writeFileSync(join(dir, "hd.toml"), `[package]\nname = "${flat}"\n`);
  writeFileSync(join(dir, "src", "main.hd"), source);
  execFileSync(hd, ["build", "--release", "--engine", engine], {
    cwd: dir,
    env: { ...env, HD_CACHE: join(work, "cache") },
    stdio: ["ignore", "ignore", "inherit"],
  });
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
    for (const [from, to] of small) src = src.replace(from, to);
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

function main() {
  const opts = parseArgs(process.argv.slice(2));
  let list = cases();
  if (opts.only) list = list.filter((c) => opts.only.includes(c.name));
  mkdirSync(join(opts.work, "go"), { recursive: true });
  if (opts.checked) return checkedRun(opts, list);
  const rows = [];
  for (const c of list) {
    const row = { name: c.name };
    rows.push(row);
    // The reference output: the JavaScript program's, else expected.out.
    let expect;
    if (c.js) expect = execFileSync("node", [c.js], { encoding: "utf8" });
    else if (c.expected) expect = readFileSync(c.expected, "utf8");
    else if (c.emptyExpected) expect = "";
    if (opts.refs && c.js) row.js = measure(["node", c.js], opts.runs, expect);
    if (opts.refs && c.go) {
      const bin = join(opts.work, "go", c.name);
      execFileSync("go", ["build", "-o", bin, c.go]);
      row.go = measure([bin], opts.runs, c.name === "dyn_iter" ? undefined : expect);
    }
    for (const engine of opts.engines) {
      const wasm = build(opts.hd, opts.work, c.name, c.src ?? readFileSync(c.hd, "utf8"), engine);
      row[`${engine}Bytes`] = statSync(wasm).size;
      const argv =
        engine === "node"
          ? ["node", RUNNER, wasm]
          : [opts.hd, wasm, "--engine", "wasmtime"];
      row[engine] = measure(argv, opts.runs, c.name === "dyn_iter" ? row.go?.out : expect);
    }
    console.error(JSON.stringify(row, (k, v) => (k === "out" || k === "err" ? undefined : v)));
  }
  writeFileSync(join(opts.work, "results.json"), JSON.stringify(rows, null, 1));
  const cell = (m) => (m ? (m.error ? "fail" : Math.round(m.ms).toString()) : "");
  const rss = (m) => (m && !m.error ? Math.round(m.rssMb).toString() : "");
  console.log("| Program | hd, V8 | hd, wasmtime | Node | Go | hd/Node | hd/Go | RSS MB: hd V8, wasmtime, Node, Go |");
  console.log("| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- |");
  for (const r of rows) {
    const ratio = (a, b) => (a && b && !a.error && !b.error ? (a.ms / b.ms).toFixed(1) : "");
    console.log(
      `| ${r.name} | ${cell(r.node)} | ${cell(r.wasmtime)} | ${cell(r.js)} | ${cell(r.go)} | ${ratio(r.node, r.js)} | ${ratio(r.node, r.go)} | ${[r.node, r.wasmtime, r.js, r.go].map(rss).join(", ")} |`,
    );
  }
  for (const r of rows)
    for (const k of ["node", "wasmtime", "js", "go"])
      if (r[k]?.error) console.log(`- ${r.name} ${k}: ${r[k].error}`);
  const dyn = rows.find((r) => r.name === "dyn_iter");
  if (dyn)
    for (const k of ["node", "wasmtime", "go"])
      if (dyn[k] && !dyn[k].error) console.log(`- dyn_iter ${k}: ${dyn[k].err.split("\n").filter((l) => /^\S+ \d+ms$/.test(l)).join("; ")}`);
}

if (existsSync(RUNNER)) main();
else throw new Error(`no ${RUNNER}`);
