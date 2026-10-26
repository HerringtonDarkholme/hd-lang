// hd release Wasm vs MoonBit's linear-memory wasm backend, same programs.
//
//   node bench/study/moonbit/compare.mjs --build          build both sides
//   node bench/study/moonbit/compare.mjs [--runs 11] [--only a,b]
//
// Every run is a fresh `node` process: a child (`--child`) loads one module,
// instantiates it, then times only the call of its entry. The parent also
// times the whole process (wall). Per program and tier, one discarded
// warm-up, then --runs measured runs with hd and MoonBit interleaved.
// Tiers: V8 default, and `--no-liftoff` (optimizing compiler only).
import { execFileSync, spawnSync } from "node:child_process";
import { mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import os from "node:os";

const HERE = dirname(fileURLToPath(import.meta.url));
const REPO = resolve(HERE, "..", "..", "..");
const WORK = join(HERE, "work");
const MOON = process.env.MOON ?? "/Users/hd/.moon/bin/moon";
const HD = join(REPO, "target", "release", "hd");
const MBT_BUILD = join(HERE, "_build", "wasm", "release", "build");

const EMPTY = 'pub fn main() -> void:\n    if 1 + 1 != 2: panic("wrong")\n';
const study = [
  "binary_trees",
  "closures",
  "fannkuch_redux",
  "json",
  "mandelbrot",
  "map_count",
  "nbody",
  "points",
  "sieve",
  "spectral_norm",
  "string_build",
];
const progs = [
  "closures",
  "int-loop",
  "list-sort",
  "map-count",
  "nested-display",
  "string-build",
  "string-builder",
  "trait-dispatch",
];
// name = MoonBit package name; hd = source path.
const CASES = [
  ...study.map((n) => ({ name: `study_${n}`, hd: join(REPO, "bench", "study", `${n}.hd`) })),
  ...progs.map((n) => ({
    name: `progs_${n.replaceAll("-", "_")}`,
    hd: join(REPO, "bench", "runtime", "progs", `${n}.hd`),
  })),
];
const EMPTY_CASE = { name: "empty", src: EMPTY };
const hdWasm = (c) => join(WORK, c.name, "build", "release", `${c.name.replaceAll("_", "-")}.wasm`);
const mbtWasm = (c) => join(MBT_BUILD, c.name, `${c.name}.wasm`);

function build(cases, hd) {
  for (const c of cases) {
    const dir = join(WORK, c.name);
    rmSync(dir, { recursive: true, force: true });
    mkdirSync(join(dir, "src"), { recursive: true });
    writeFileSync(
      join(dir, "hd.toml"),
      `[package]\nname = "${c.name.replaceAll("_", "-")}"\n\n[capabilities]\nConsole = true\nClock = true\n`,
    );
    writeFileSync(join(dir, "src", "main.hd"), c.src ?? readFileSync(c.hd, "utf8"));
    execFileSync("nice", [hd, "build", "--release", "--engine", "node"], {
      cwd: dir,
      env: { ...process.env, HD_CACHE: join(WORK, "cache") },
      stdio: ["ignore", "ignore", "inherit"],
    });
  }
  execFileSync(
    "nice",
    [MOON, "build", "--target", "wasm", "--release", ...cases.map((c) => c.name)],
    {
      cwd: HERE,
      stdio: ["ignore", "ignore", "inherit"],
    },
  );
}

// ---- child: one module, one run ----
async function child(kind, file) {
  const bytes = readFileSync(file);
  const out = [];
  let ms;
  if (kind === "hd") {
    const { createHost } = await import(join(REPO, "host", "node.mjs"));
    const host = createHost({ out: (t) => out.push(t), err: (t) => out.push(t) }, {});
    const { instance } = await WebAssembly.instantiate(bytes, host.imports);
    const t = performance.now();
    const { status } = await host.run(instance, "hd.init", "hd.poll");
    ms = performance.now() - t;
    host.close();
    if (status !== 0) throw new Error(`hd status ${status}`);
  } else {
    // The whole host MoonBit needs: fd_write for println.
    let mem;
    const chunks = [];
    const imports = {
      wasi_snapshot_preview1: {
        fd_write(_fd, iov, n, nw) {
          const dv = new DataView(mem.buffer);
          let total = 0;
          for (let i = 0; i < n; i++) {
            const p = dv.getUint32(iov + i * 8, true);
            const l = dv.getUint32(iov + i * 8 + 4, true);
            chunks.push(Buffer.from(new Uint8Array(mem.buffer, p, l)));
            total += l;
          }
          dv.setUint32(nw, total, true);
          return 0;
        },
      },
    };
    const { instance } = await WebAssembly.instantiate(bytes, imports);
    mem = instance.exports.memory;
    const t = performance.now();
    instance.exports._start();
    ms = performance.now() - t;
    out.push(Buffer.concat(chunks).toString());
  }
  process.stdout.write(JSON.stringify({ ms, out: out.join("") }));
}

const median = (xs) => [...xs].sort((a, b) => a - b)[Math.floor(xs.length / 2)];
const gmean = (xs) => Math.exp(xs.reduce((s, x) => s + Math.log(x), 0) / xs.length);

function once(kind, file, flags) {
  const t = performance.now();
  const r = spawnSync("node", [...flags, fileURLToPath(import.meta.url), "--child", kind, file], {
    encoding: "utf8",
    maxBuffer: 1 << 26,
  });
  const wall = performance.now() - t;
  if (r.status !== 0) throw new Error(`${kind} ${file}: ${r.stderr.slice(-400)}`);
  const { ms, out } = JSON.parse(r.stdout);
  return { ms, wall, out };
}

function measure(c, flags, runs) {
  const files = { hd: hdWasm(c), mbt: mbtWasm(c) };
  const res = { hd: { ms: [], wall: [] }, mbt: { ms: [], wall: [] } };
  const outs = {};
  for (let i = -1; i < runs; i++) {
    for (const k of ["hd", "mbt"]) {
      const r = once(k, files[k], flags);
      outs[k] ??= r.out;
      if (r.out !== outs[k]) throw new Error(`${c.name} ${k}: output changed between runs`);
      if (i >= 0) {
        res[k].ms.push(r.ms);
        res[k].wall.push(r.wall);
      }
    }
  }
  if (outs.hd !== outs.mbt)
    throw new Error(
      `${c.name}: stdout differs\n hd:  ${JSON.stringify(outs.hd.slice(0, 200))}\n mbt: ${JSON.stringify(outs.mbt.slice(0, 200))}`,
    );
  return {
    hd: median(res.hd.ms),
    mbt: median(res.mbt.ms),
    hdWall: median(res.hd.wall),
    mbtWall: median(res.mbt.wall),
  };
}

async function main() {
  const argv = process.argv.slice(2);
  if (argv[0] === "--child") return child(argv[1], argv[2]);
  let runs = 11;
  let only;
  let hd = HD;
  for (let i = 0; i < argv.length; i++) {
    if (argv[i] === "--build") continue;
    else if (argv[i] === "--hd" && argv[i + 1]) hd = resolve(argv[++i]);
    else if (argv[i] === "--runs") runs = Number(argv[++i]);
    else if (argv[i] === "--only") only = argv[++i].split(",");
    else throw new Error(`unknown flag ${argv[i]}`);
  }
  const cases = [...CASES, EMPTY_CASE].filter((c) => !only || only.includes(c.name));
  if (!cases.length) throw new Error("no matching program");
  if (argv.includes("--build")) return build(cases, hd);
  const tiers = [[], ["--no-liftoff"]];
  const rows = [];
  for (const c of cases) {
    const r = tiers.map((f) => measure(c, f, runs));
    rows.push({ name: c.name, def: r[0], nl: r[1] });
    console.error(`done ${c.name}`);
  }
  const f = (x) => x.toFixed(x < 10 ? 2 : 1);
  const ratio = (a, b) => a / b;
  const table = (title, key) => {
    console.log(`\n${title}\n`);
    console.log(
      "| program | hd default ms | MoonBit default ms | hd/MBT | hd --no-liftoff ms | MBT --no-liftoff ms | hd/MBT |",
    );
    console.log("| --- | ---: | ---: | ---: | ---: | ---: | ---: |");
    const g = [[], []];
    for (const r of rows) {
      const [h1, m1, h2, m2] =
        key === "ms"
          ? [r.def.hd, r.def.mbt, r.nl.hd, r.nl.mbt]
          : [r.def.hdWall, r.def.mbtWall, r.nl.hdWall, r.nl.mbtWall];
      console.log(
        `| ${r.name} | ${f(h1)} | ${f(m1)} | ${ratio(h1, m1).toFixed(2)} | ${f(h2)} | ${f(m2)} | ${ratio(h2, m2).toFixed(2)} |`,
      );
      if (r.name !== "empty") {
        g[0].push(ratio(h1, m1));
        g[1].push(ratio(h2, m2));
      }
    }
    console.log(
      `| **geomean (excl. empty)** | | | ${gmean(g[0]).toFixed(2)} | | | ${gmean(g[1]).toFixed(2)} |`,
    );
  };
  console.log(
    `node ${process.version}, runs ${runs} (median, 1 discarded warm-up), load avg at end ${os
      .loadavg()
      .map((x) => x.toFixed(2))
      .join(" ")}`,
  );
  table("Run time (entry call only, after instantiation)", "ms");
  table("Wall time (whole node process)", "wall");
}
await main();
