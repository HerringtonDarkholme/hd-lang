// hd release Wasm vs MoonBit, same programs. MoonBit's target is its
// linear-memory `wasm` backend, or `wasm-gc` with --target wasm-gc.
//
//   node bench/study/moonbit/compare.ts --build [--target wasm-gc]   build both sides
//   node bench/study/moonbit/compare.ts [--target wasm-gc] [--runs 11] [--only a,b]
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

type Dynamic = ReturnType<typeof JSON.parse>;

const HERE = dirname(fileURLToPath(import.meta.url));
const REPO = resolve(HERE, "..", "..", "..");
const WORK = join(HERE, "work");
const MOON = process.env.MOON ?? "/Users/hd/.moon/bin/moon";
const HD = join(REPO, "target", "release", "hd");
const TARGET = process.argv.includes("--target")
  ? process.argv[process.argv.indexOf("--target") + 1]
  : "wasm";
if (TARGET !== "wasm" && TARGET !== "wasm-gc") {
  throw new Error(`unknown --target ${TARGET}`);
}
const MBT_BUILD = join(HERE, "_build", TARGET, "release", "build");

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
  "map-count-long",
  "nested-display",
  "string-build",
  "string-builder",
  "trait-dispatch",
];
// name = MoonBit package name; hd = source path.
const CASES = [
  ...study.map((item) => ({
    name: `study_${item}`,
    hd: join(REPO, "bench", "study", `${item}.hd`),
  })),
  ...progs.map((item) => ({
    name: `progs_${item.replaceAll("-", "_")}`,
    hd: join(REPO, "bench", "runtime", "progs", `${item}.hd`),
  })),
];
const EMPTY_CASE = { name: "empty", src: EMPTY };
const hdWasm = (context: Dynamic): string =>
  join(WORK, context.name, "build", "release", `${context.name.replaceAll("_", "-")}.wasm`);
const mbtWasm = (context: Dynamic): string => join(MBT_BUILD, context.name, `${context.name}.wasm`);

function build(cases: Dynamic[], hd: string): void {
  for (const context of cases) {
    const dir = join(WORK, context.name);
    rmSync(dir, { recursive: true, force: true });
    mkdirSync(join(dir, "src"), { recursive: true });
    writeFileSync(
      join(dir, "hd.toml"),
      `[package]\nname = "${context.name.replaceAll("_", "-")}"\n\n[capabilities]\nConsole = true\nClock = true\n`,
    );
    writeFileSync(join(dir, "src", "main.hd"), context.src ?? readFileSync(context.hd, "utf8"));
    execFileSync("nice", [hd, "build", "--release", "--engine", "node"], {
      cwd: dir,
      env: { ...process.env, HD_CACHE: join(WORK, "cache") },
      stdio: ["ignore", "ignore", "inherit"],
    });
  }
  execFileSync(
    "nice",
    [MOON, "build", "--target", TARGET, "--release", ...cases.map((item) => item.name)],
    {
      cwd: HERE,
      stdio: ["ignore", "ignore", "inherit"],
    },
  );
}

// ---- child: one module, one run ----
async function child(kind: string, file: string): Promise<void> {
  const bytes = readFileSync(file);
  const out: string[] = [];
  let ms: number;
  if (kind === "hd") {
    const { createStartupHost } = await import(join(REPO, "host", "startup.ts"));
    const module = await WebAssembly.compile(bytes);
    const host = await createStartupHost(
      module,
      { out: (text: string) => out.push(text), err: (text: string) => out.push(text) },
      {},
    );
    const instance = new WebAssembly.Instance(module, host.imports);
    const started = performance.now();
    const { status } = await host.run(instance, "hd.init", "hd.poll");
    ms = performance.now() - started;
    host.close();
    if (status !== 0) {
      throw new Error(`hd status ${status}`);
    }
  } else {
    // The whole host MoonBit needs: fd_write for println (wasm), or
    // print_char, one UTF-16 unit per call (wasm-gc).
    let mem: WebAssembly.Memory | undefined;
    const chunks: Buffer[] = [];
    const units: number[] = [];
    const imports = {
      spectest: { print_char: (character: number) => units.push(character) },
      wasi_snapshot_preview1: {
        fd_write(_fd: number, iov: number, value: number, nw: number) {
          const dv = new DataView(mem!.buffer);
          let total = 0;
          for (let i = 0; i < value; i++) {
            const offset = dv.getUint32(iov + i * 8, true);
            const length = dv.getUint32(iov + i * 8 + 4, true);
            chunks.push(Buffer.from(new Uint8Array(mem!.buffer, offset, length)));
            total += length;
          }
          dv.setUint32(nw, total, true);
          return 0;
        },
      },
    };
    const { instance } = await WebAssembly.instantiate(bytes, imports);
    mem = instance.exports.memory as WebAssembly.Memory;
    const started = performance.now();
    (instance.exports._start as () => void)();
    ms = performance.now() - started;
    out.push(Buffer.concat(chunks).toString());
    for (let i = 0; i < units.length; i += 4096) {
      out.push(String.fromCharCode(...units.slice(i, i + 4096)));
    }
  }
  process.stdout.write(JSON.stringify({ ms, out: out.join("") }));
}

const median = (samples: number[]): number =>
  [...samples].sort((left, right) => left - right)[Math.floor(samples.length / 2)];
const gmean = (samples: number[]): number =>
  Math.exp(samples.reduce((sum, value) => sum + Math.log(value), 0) / samples.length);

function once(kind: string, file: string, flags: string[]): Dynamic {
  const started = performance.now();
  const execution = spawnSync(
    "node",
    [...flags, fileURLToPath(import.meta.url), "--child", kind, file],
    {
      encoding: "utf8",
      maxBuffer: 1 << 26,
    },
  );
  const wall = performance.now() - started;
  if (execution.status !== 0) {
    throw new Error(`${kind} ${file}: ${execution.stderr.slice(-400)}`);
  }
  const { ms, out } = JSON.parse(execution.stdout);
  return { ms, wall, out };
}

function measure(context: Dynamic, flags: string[], runs: number): Dynamic {
  const files: Record<string, string> = { hd: hdWasm(context), mbt: mbtWasm(context) };
  const res: Record<string, { ms: number[]; wall: number[] }> = {
    hd: { ms: [], wall: [] },
    mbt: { ms: [], wall: [] },
  };
  const outs: Record<string, string> = {};
  for (let i = -1; i < runs; i++) {
    for (const k of ["hd", "mbt"]) {
      const measurement = once(k, files[k], flags);
      outs[k] ??= measurement.out;
      if (measurement.out !== outs[k]) {
        throw new Error(`${context.name} ${k}: output changed between runs`);
      }
      if (i >= 0) {
        res[k].ms.push(measurement.ms);
        res[k].wall.push(measurement.wall);
      }
    }
  }
  if (outs.hd !== outs.mbt) {
    throw new Error(
      `${context.name}: stdout differs\n hd:  ${JSON.stringify(outs.hd.slice(0, 200))}\n mbt: ${JSON.stringify(outs.mbt.slice(0, 200))}`,
    );
  }
  return {
    hd: median(res.hd.ms),
    mbt: median(res.mbt.ms),
    hdWall: median(res.hd.wall),
    mbtWall: median(res.mbt.wall),
  };
}

async function main() {
  const argv = process.argv.slice(2);
  if (argv[0] === "--child") {
    return child(argv[1]!, argv[2]!);
  }
  let runs = 11;
  let only: string[] | undefined;
  let hd = HD;
  for (let i = 0; i < argv.length; i++) {
    if (argv[i] === "--build") {
      continue;
    } else if (argv[i] === "--target") {
      i++;
    } else if (argv[i] === "--hd" && argv[i + 1]) {
      hd = resolve(argv[++i]);
    } else if (argv[i] === "--runs") {
      runs = Number(argv[++i]);
    } else if (argv[i] === "--only") {
      only = argv[++i].split(",");
    } else {
      throw new Error(`unknown flag ${argv[i]}`);
    }
  }
  const cases = [...CASES, EMPTY_CASE].filter((item) => !only || only.includes(item.name));
  if (!cases.length) {
    throw new Error("no matching program");
  }
  if (argv.includes("--build")) {
    return build(cases, hd);
  }
  const tiers: string[][] = [[], ["--no-liftoff"]];
  const rows: Dynamic[] = [];
  for (const context of cases) {
    const measurements = tiers.map((flags) => measure(context, flags, runs));
    rows.push({ name: context.name, def: measurements[0], nl: measurements[1] });
    console.error(`done ${context.name}`);
  }
  const format = (value: number): string => value.toFixed(value < 10 ? 2 : 1);
  const ratio = (left: number, right: number): number => left / right;
  const table = (title: string, key: string): void => {
    console.log(`\n${title}\n`);
    console.log(
      "| program | hd default ms | MoonBit default ms | hd/MBT | hd --no-liftoff ms | MBT --no-liftoff ms | hd/MBT |",
    );
    console.log("| --- | ---: | ---: | ---: | ---: | ---: | ---: |");
    const value: number[][] = [[], []];
    for (const result of rows) {
      const [h1, m1, h2, m2] =
        key === "ms"
          ? [result.def.hd, result.def.mbt, result.nl.hd, result.nl.mbt]
          : [result.def.hdWall, result.def.mbtWall, result.nl.hdWall, result.nl.mbtWall];
      console.log(
        `| ${result.name} | ${format(h1)} | ${format(m1)} | ${ratio(h1, m1).toFixed(2)} | ${format(h2)} | ${format(m2)} | ${ratio(h2, m2).toFixed(2)} |`,
      );
      if (result.name !== "empty") {
        value[0].push(ratio(h1, m1));
        value[1].push(ratio(h2, m2));
      }
    }
    console.log(
      `| **geomean (excl. empty)** | | | ${gmean(value[0]).toFixed(2)} | | | ${gmean(value[1]).toFixed(2)} |`,
    );
  };
  console.log(
    `MoonBit target ${TARGET}; node ${process.version}, runs ${runs} (median, 1 discarded warm-up), load avg at end ${os
      .loadavg()
      .map((item) => item.toFixed(2))
      .join(" ")}`,
  );
  table("Run time (entry call only, after instantiation)", "ms");
  table("Wall time (whole node process)", "wall");
}
await main();
