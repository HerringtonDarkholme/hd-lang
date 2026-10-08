import { execFileSync } from "node:child_process";
import { mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));
const REPO = resolve(HERE, "..", "..", "..");
const TS_HD = ["node", "--experimental-strip-types", join(REPO, "bin", "hd.js")];
const NEW_HD = join(REPO, "compiler", "target", "release", "hd");
const RUNS = 5;
const TIMEOUT_MS = 60_000;

const args = process.argv.slice(2);
const workRoot = args[0] ?? join(tmpdir(), `hd-compare-${process.pid}`);

function sh(cmd, argv, opts = {}) {
  const start = performance.now();
  let ok = true;
  let code = 0;
  let out = "";
  try {
    out = execFileSync(cmd, argv, {
      encoding: "utf8",
      timeout: TIMEOUT_MS,
      ...opts,
    });
  } catch (err) {
    ok = false;
    code = err.status ?? 1;
    out = (err.stdout ?? "") + (err.stderr ?? "");
  }
  return { ms: performance.now() - start, ok, code, out: String(out).slice(0, 2000) };
}

function loadavg() {
  try {
    return execFileSync("uptime", [], { encoding: "utf8", timeout: 5000 }).trim();
  } catch {
    return "uptime unavailable";
  }
}

function stats(samples) {
  const s = [...samples].sort((a, b) => a - b);
  return { p50: s[2], p95: s[4], min: s[0], max: s[4] };
}

function measure(fn) {
  const samples = [];
  let last = null;
  for (let i = 0; i < RUNS; i++) last = fn(i);
  for (let i = 0; i < RUNS; i++) samples.push(fn(i).ms);
  return { ...stats(samples), last };
}

function mkpkg(name, source) {
  const dir = join(workRoot, name);
  rmSync(dir, { recursive: true, force: true });
  mkdirSync(join(dir, "src"), { recursive: true });
  writeFileSync(join(dir, "hd.toml"), `[package]\nname = "${name}"\n`);
  writeFileSync(join(dir, "src", "main.hd"), source);
  return dir;
}

function tsBuildClean(dir) {
  rmSync(join(dir, "build"), { recursive: true, force: true });
}

function ts(cmdArgs, dir) {
  return sh(TS_HD[0], [...TS_HD.slice(1), ...cmdArgs], { cwd: dir });
}

function nw(cmdArgs, dir, cacheDir) {
  return sh(NEW_HD, cmdArgs, {
    cwd: dir,
    env: { ...process.env, HD_CACHE: cacheDir },
  });
}

const tinySrc = readFileSync(join(HERE, "inputs", "tiny.hd"), "utf8");
const midSrc = readFileSync(join(HERE, "inputs", "mid.hd"), "utf8");
const calcSrc = readFileSync(join(REPO, "examples", "dogfood", "calc.hd"), "utf8");
const testSrc = (n) => readFileSync(join(HERE, "inputs", `test${n}.hd`), "utf8");

const row = (o) => `${o.p50.toFixed(0)} / ${o.p95.toFixed(0)}`;

function main() {
  console.log(`# hd compare: TS prototype vs new compiler`);
  console.log(`\nLoad at start: ${loadavg()}`);
  console.log(`Repo: ${REPO}\nTS: \`node bin/hd.js\` (src/)\nNew: \`compiler/target/release/hd\``);
  console.log(`\nMethod: ${RUNS} runs each; p50 = median of ${RUNS}, p95 = max of ${RUNS}.`);
  console.log(
    `Cold TS = \`rm -rf build\` before each run; warm TS = build dir reused after one warmup.`,
  );
  console.log(
    `Cold new = fresh empty HD_CACHE per run; warm new = shared HD_CACHE after one warmup.`,
  );
  console.log(
    `New has no \`check\` and no \`--version\`; nearest no-ops are \`--help\` (new) vs \`help\` (TS).`,
  );
  console.log(
    `Checked-in bench programs are stale on both compilers (see Skips); \`mid.hd\` is the fixed math kernel both accept.\n`,
  );

  mkdirSync(workRoot, { recursive: true });

  // Startup.
  const tsStart = measure(() => ts(["help"], REPO));
  const nwStart = measure(() => nw(["--help"], REPO, join(workRoot, "cache-startup-warm")));
  console.log(`## Startup (nearest no-op)\n`);
  console.log(`| Compiler | Command | p50 / p95 (ms) |`);
  console.log(`| --- | --- | ---: |`);
  console.log(`| TS | \`hd help\` | ${row(tsStart)} |`);
  console.log(`| new | \`hd --help\` | ${row(nwStart)} |\n`);

  // Check (TS only).
  console.log(`## hd check (TS only; new has no check command)\n`);
  console.log(`| Input | Cold p50 / p95 (ms) | Warm p50 / p95 (ms) |`);
  console.log(`| --- | ---: | ---: |`);
  for (const [name, src] of [
    ["tiny", tinySrc],
    ["mid", midSrc],
    ["calc", calcSrc],
  ]) {
    const dir = mkpkg(`check-${name}`, src);
    const cold = measure(() => {
      tsBuildClean(dir);
      return ts(["check"], dir);
    });
    ts(["check"], dir);
    const warm = measure(() => ts(["check"], dir));
    console.log(`| ${name} | ${row(cold)} | ${row(warm)} |`);
  }
  console.log(``);

  // Build.
  console.log(`## hd build\n`);
  console.log(`| Input | TS cold | TS warm | new cold | new warm |`);
  console.log(`| --- | ---: | ---: | ---: | ---: |`);
  for (const [name, src] of [
    ["tiny", tinySrc],
    ["mid", midSrc],
  ]) {
    const dir = mkpkg(`build-${name}`, src);
    const tsCold = measure(() => {
      tsBuildClean(dir);
      return ts(["build"], dir);
    });
    ts(["build"], dir);
    const tsWarm = measure(() => ts(["build"], dir));
    const outCold = join(workRoot, `build-${name}-cold.wasm`);
    const nwCold = measure((i) => {
      const cache = join(workRoot, `cache-build-${name}-cold-${i}`);
      rmSync(cache, { recursive: true, force: true });
      mkdirSync(cache, { recursive: true });
      rmSync(outCold, { force: true });
      return nw(["build", "src/main.hd", "-o", outCold], dir, cache);
    });
    const nwCache = join(workRoot, `cache-build-${name}-warm`);
    rmSync(nwCache, { recursive: true, force: true });
    mkdirSync(nwCache, { recursive: true });
    const outWarm = join(workRoot, `build-${name}-warm.wasm`);
    nw(["build", "src/main.hd", "-o", outWarm], dir, nwCache);
    const nwWarm = measure(() => nw(["build", "src/main.hd", "-o", outWarm], dir, nwCache));
    console.log(`| ${name} | ${row(tsCold)} | ${row(tsWarm)} | ${row(nwCold)} | ${row(nwWarm)} |`);
  }
  {
    const dir = mkpkg("build-calc", calcSrc);
    const tsCold = measure(() => {
      tsBuildClean(dir);
      return ts(["build"], dir);
    });
    console.log(
      `| calc (TS only) | ${row(tsCold)} | n/a (new: unknown-import it_prop + unsupported list spread) | — | — |`,
    );
  }
  console.log(``);

  // Run.
  console.log(`## hd run (bare FILE; same sources)\n`);
  console.log(`| Input | TS cold | TS warm | new cold | new warm |`);
  console.log(`| --- | ---: | ---: | ---: | ---: |`);
  for (const [name, src] of [
    ["tiny", tinySrc],
    ["mid", midSrc],
  ]) {
    const dir = mkpkg(`run-${name}`, src);
    const tsCold = measure(() => {
      tsBuildClean(dir);
      return ts(["src/main.hd"], dir);
    });
    ts(["src/main.hd"], dir);
    const tsWarm = measure(() => ts(["src/main.hd"], dir));
    const nwCold = measure((i) => {
      const cache = join(workRoot, `cache-run-${name}-cold-${i}`);
      rmSync(cache, { recursive: true, force: true });
      mkdirSync(cache, { recursive: true });
      return nw(["run", "src/main.hd"], dir, cache);
    });
    const nwCache = join(workRoot, `cache-run-${name}-warm`);
    rmSync(nwCache, { recursive: true, force: true });
    mkdirSync(nwCache, { recursive: true });
    nw(["run", "src/main.hd"], dir, nwCache);
    const nwWarm = measure(() => nw(["run", "src/main.hd"], dir, nwCache));
    const ok = tsCold.last.out.trim() === nwCold.last.out.trim() ? "same stdout" : "STDOUT DIFFERS";
    console.log(
      `| ${name} | ${row(tsCold)} | ${row(tsWarm)} | ${row(nwCold)} | ${row(nwWarm)} | ${ok} |`,
    );
  }
  {
    const dir = mkpkg("run-calc", calcSrc);
    const tsCold = measure(() => {
      tsBuildClean(dir);
      return ts(["src/main.hd"], dir);
    });
    console.log(`| calc (TS only) | ${row(tsCold)} | n/a | new: fails (see Skips) | — |`);
  }
  console.log(``);

  // Test.
  console.log(`## hd test (package; total ms and per-case ms)\n`);
  console.log(`| Cases | TS total | TS per-case | new total | new per-case |`);
  console.log(`| ---: | ---: | ---: | ---: | ---: |`);
  for (const n of [1, 30, 300]) {
    const dir = mkpkg(`test-${n}`, testSrc(n));
    ts(["test"], dir);
    const tsR = measure(() => ts(["test"], dir));
    const nwCache = join(workRoot, `cache-test-${n}`);
    rmSync(nwCache, { recursive: true, force: true });
    mkdirSync(nwCache, { recursive: true });
    nw(["test"], dir, nwCache);
    const nwR = measure(() => nw(["test"], dir, nwCache));
    console.log(
      `| ${n} | ${row(tsR)} | ${(tsR.p50 / n).toFixed(1)} / ${(tsR.p95 / n).toFixed(1)} | ${row(nwR)} | ${(nwR.p50 / n).toFixed(1)} / ${(nwR.p95 / n).toFixed(1)} |`,
    );
  }
  console.log(``);

  // Skips (single probing runs, not timed).
  console.log(`## Skips (what each compiler rejects)\n`);
  const benchFiles = ["tokenizer", "math", "records", "traits"].map(
    (b) => `compiler/bench/${b}/main.hd`,
  );
  for (const f of benchFiles) {
    const src = readFileSync(join(REPO, f), "utf8");
    const dir = mkpkg(`skip-${f.replaceAll("/", "-")}`, src);
    const t = ts(["check", "src/main.hd"], dir);
    const c = join(workRoot, "cache-skip");
    mkdirSync(c, { recursive: true });
    const n = nw(["run", "src/main.hd"], dir, c);
    console.log(
      `- \`${f}\`: TS check exit ${t.ok ? 0 : 101} (${t.out.split("\n")[0].slice(0, 120)}); new run ${n.ok ? "exit 0" : "fails"} (${n.out.split("\n")[0].slice(0, 120)})`,
    );
  }
  {
    const dir = mkpkg("skip-calc-new", calcSrc);
    const c = join(workRoot, "cache-skip");
    mkdirSync(c, { recursive: true });
    const n = nw(["run", "src/main.hd"], dir, c);
    console.log(
      `- \`examples/dogfood/calc.hd\` on new: fails (${n.out.split("\n")[0].slice(0, 160)})`,
    );
  }
  console.log(`\nLoad at end: ${loadavg()}`);
  console.log(`\nWork dir kept at: ${workRoot}`);
}

main();
