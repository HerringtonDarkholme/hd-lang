// JSON decode A/B (#1809): builds main.hd and types20.hd with each `hd`
// binary given, runs the decode program alternately (2 rounds each, 7
// batches per cell per round), and prints the median per cell and the
// release Wasm sizes as a Markdown table.
//
//   node bench/json-decode/run.ts BASE_HD NEW_HD [ROUNDS]
//
// Each binary's build goes to its own copy of this package, so the two
// never share a build directory. Set HD_CACHE to keep hd off the shared cache.
import { execFileSync } from "node:child_process";
import { cpSync, mkdtempSync, statSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const runner = join(here, "../../host/run.ts");
const [base, next, roundsText] = process.argv.slice(2);
if (!base || !next) {
  console.error("usage: node bench/json-decode/run.ts BASE_HD NEW_HD [ROUNDS]");
  process.exit(2);
}
const rounds = Number(roundsText ?? "2");

type Built = { name: string; main: string; mainSize: number; typesSize: number };

function build(name: string, hd: string): Built {
  const dir = mkdtempSync(join(tmpdir(), `json-decode-${name}-`));
  for (const file of ["hd.toml", "main.hd", "types20.hd"]) {
    cpSync(join(here, file), join(dir, file));
  }
  for (const file of ["main.hd", "types20.hd"]) {
    // cost: one release build per program per binary.
    execFileSync(hd, ["build", "--release", file], {
      cwd: dir,
      stdio: "inherit",
      timeout: 300_000,
    });
  }
  const out = join(dir, "build/release/files");
  return {
    name,
    main: join(out, "main.wasm"),
    mainSize: statSync(join(out, "main.wasm")).size,
    typesSize: statSync(join(out, "types20.wasm")).size,
  };
}

const builds = [build("base", base), build("new", next)];
// cell key -> variant -> nanoseconds per decode, one per batch
const samples = new Map<string, Map<string, number[]>>();
const cells: string[] = [];
for (let round = 0; round < rounds; round += 1) {
  for (const built of builds) {
    // cost: one run of every cell, about a minute.
    const out = execFileSync("node", [runner, built.main], { encoding: "utf8", timeout: 600_000 });
    for (const line of out.trim().split("\n")) {
      const [shape, order, objects, bytes, ns] = line.split("\t");
      const key = [shape, order, objects, bytes].join("\t");
      if (!samples.has(key)) {
        samples.set(key, new Map());
        cells.push(key);
      }
      const byVariant = samples.get(key)!;
      if (!byVariant.has(built.name)) {
        byVariant.set(built.name, []);
      }
      byVariant.get(built.name)!.push(Number(ns));
    }
  }
}

function median(values: number[]): number {
  const sorted = [...values].sort((left, right) => left - right);
  const mid = sorted.length >> 1;
  return sorted.length % 2 ? sorted[mid] : (sorted[mid - 1] + sorted[mid]) / 2;
}

const micros = (ns: number) =>
  ns >= 100_000 ? Math.round(ns / 1000).toLocaleString("en-US") : (ns / 1000).toFixed(1);
const rate = (bytes: number, ns: number) => Math.round((bytes / ns) * 1000);
console.log("| Shape | Order | Bytes | main µs (MB/s) | branch µs (MB/s) | change |");
console.log("| --- | --- | ---: | ---: | ---: | ---: |");
for (const key of cells) {
  const [shape, order, , bytesText] = key.split("\t");
  const bytes = Number(bytesText);
  const before = median(samples.get(key)!.get("base")!);
  const after = median(samples.get(key)!.get("new")!);
  const change = Math.round(((after - before) / before) * 100);
  console.log(
    `| ${shape} | ${order} | ${bytes.toLocaleString("en-US")} | ${micros(before)} (${rate(bytes, before)}) | ${micros(after)} (${rate(bytes, after)}) | ${change > 0 ? "+" : ""}${change}% |`,
  );
}
const pct = (before: number, after: number) =>
  `${after >= before ? "+" : ""}${(((after - before) / before) * 100).toFixed(1)}%`;
console.log("");
console.log("| Wasm size (release) | main | branch | change |");
console.log("| --- | ---: | ---: | ---: |");
const [old, now] = builds;
const sizeRow = (label: string, before: number, after: number) =>
  `| ${label} | ${before.toLocaleString("en-US")} | ${after.toLocaleString("en-US")} | ${pct(before, after)} |`;
console.log(sizeRow("decode program", old.mainSize, now.mainSize));
console.log(sizeRow("20 derived types", old.typesSize, now.typesSize));
