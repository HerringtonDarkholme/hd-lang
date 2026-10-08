// Q22: runtime of new-compiler programs vs hand-written JS on the same Node.
//
// For each program in progs/: build the .hd with `hd build`, run the .wasm
// through the checked-in host runner (compiler/host/run.mjs), run the .js
// directly, 1 warmup + 5 measured runs each, p50/p95. Prints a markdown
// report to stdout. Fails loudly when hd and JS outputs differ.
//
// Usage: node compiler/bench/runtime/run.mjs [workdir]
import { execFileSync } from "node:child_process";
import { mkdirSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));
const REPO = resolve(HERE, "..", "..", "..");
const NEW_HD = join(REPO, "compiler", "target", "release", "hd");
const RUNNER = join(REPO, "compiler", "host", "run.mjs");
const RUNS = 5;
const TIMEOUT_MS = 300_000;

const args = process.argv.slice(2);
const workRoot = args[0] ?? join(tmpdir(), `hd-runtime-${process.pid}`);

function sh(cmd, argv, opts = {}) {
  const start = performance.now();
  let ok = true;
  let code = 0;
  let out = "";
  try {
    out = execFileSync(cmd, argv, { encoding: "utf8", timeout: TIMEOUT_MS, ...opts });
  } catch (err) {
    ok = false;
    code = err.status ?? 1;
    out = (err.stdout ?? "") + (err.stderr ?? "");
  }
  return { ms: performance.now() - start, ok, code, out: String(out) };
}

function stats(samples) {
  const s = [...samples].sort((a, b) => a - b);
  return { p50: s[2], p95: s[4], min: s[0], max: s[4] };
}

function measure(fn) {
  fn(); // warmup, excluded
  const samples = [];
  let last = null;
  for (let i = 0; i < RUNS; i++) last = fn();
  for (let i = 0; i < RUNS; i++) samples.push(fn().ms);
  return { ...stats(samples), last };
}

function gitHash() {
  try {
    return execFileSync("git", ["rev-parse", "--short", "HEAD"], {
      encoding: "utf8",
      cwd: REPO,
    }).trim();
  } catch {
    return "unknown";
  }
}

function nodeVersion() {
  try {
    return execFileSync("node", ["--version"], { encoding: "utf8" }).trim();
  } catch {
    return "unknown";
  }
}

function main() {
  const names = readdirSync(join(HERE, "progs"))
    .filter((f) => f.endsWith(".hd"))
    .map((f) => f.slice(0, -3))
    .sort();
  const hash = gitHash();
  const rows = [];
  let logRatioSum = 0;

  mkdirSync(workRoot, { recursive: true });
  for (const name of names) {
    const src = readFileSync(join(HERE, "progs", `${name}.hd`), "utf8");
    const dir = join(workRoot, name);
    rmSync(dir, { recursive: true, force: true });
    mkdirSync(join(dir, "src"), { recursive: true });
    writeFileSync(join(dir, "hd.toml"), `[package]\nname = "${name}"\n`);
    writeFileSync(join(dir, "src", "main.hd"), src);
    const cache = join(workRoot, `cache-${name}`);
    rmSync(cache, { recursive: true, force: true });
    mkdirSync(cache, { recursive: true });
    const built = sh(NEW_HD, ["build"], {
      cwd: dir,
      env: { ...process.env, HD_CACHE: cache },
    });
    if (!built.ok) {
      console.log(`| ${name} | BUILD FAILED |\n${built.out}`);
      process.exitCode = 1;
      continue;
    }
    const wasm = join(dir, "build", "debug", `${name}.wasm`);
    const hd = sh("node", [RUNNER, wasm]);
    const js = sh("node", [join(HERE, "progs", `${name}.js`)]);
    if (hd.out !== js.out) {
      console.log(`| ${name} | OUTPUT MISMATCH: hd=${hd.out.trim()} js=${js.out.trim()} |`);
      process.exitCode = 1;
      continue;
    }
    const hdT = measure(() => sh("node", [RUNNER, wasm]));
    const jsT = measure(() => sh("node", [join(HERE, "progs", `${name}.js`)]));
    const ratio = hdT.p50 / jsT.p50;
    logRatioSum += Math.log(ratio);
    rows.push({ name, hdT, jsT, ratio, out: hd.out.trim() });
  }

  console.log(`# Runtime vs Node`);
  console.log(``);
  console.log(`- hd commit: \`${hash}\`; binary: \`compiler/target/release/hd build\` (debug profile)`);
  console.log(`- runner: \`node ${nodeVersion()} compiler/host/run.mjs\` for hd, \`node\` directly for JS`);
  console.log(`- method: build once per program; 1 warmup + ${RUNS} measured runs each; p50 = median, p95 = max`);
  console.log(`- outputs verified equal before timing (checksum printed by each program)`);
  console.log(``);
  console.log(`| Program | hd p50 / p95 (ms) | JS p50 / p95 (ms) | hd/JS | Checksum |`);
  console.log(`| --- | ---: | ---: | ---: | ---: |`);
  for (const r of rows) {
    console.log(
      `| ${r.name} | ${r.hdT.p50.toFixed(0)} / ${r.hdT.p95.toFixed(0)} | ${r.jsT.p50.toFixed(0)} / ${r.jsT.p95.toFixed(0)} | ${r.ratio.toFixed(2)}x | ${r.out} |`,
    );
  }
  console.log(``);
  console.log(`Geomean hd/JS: ${Math.exp(logRatioSum / rows.length).toFixed(2)}x`);
}

main();
