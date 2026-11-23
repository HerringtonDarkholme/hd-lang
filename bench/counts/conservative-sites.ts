// Static conservative sites by reason. The Rust corpus reader compiles only benchmarks.
import { execFileSync } from "node:child_process";
import { mkdirSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { gate } from "./baseline.ts";
import { compareSites, readSiteReport, readSites } from "./conservative-baseline.ts";

const root = fileURLToPath(new URL("../../", import.meta.url));
const options = new Map<string, string>();
const args = process.argv.slice(2);
for (let i = 0; i < args.length; i++) {
  const arg = args[i];
  if (["--base", "--cost-ok", "--output", "--input", "--runner"].includes(arg) && args[i + 1]) {
    options.set(arg, args[++i]);
  } else {
    throw new Error(`unknown/missing conservative-site argument: ${arg}`);
  }
}
const output = resolve(options.get("--output") ?? `${root}/target/conservative-sites`);
const input = resolve(options.get("--input") ?? `${output}/reports`);
mkdirSync(output, { recursive: true });
if (!options.has("--input")) {
  // Never inherit a developer's corpus sampling or dev-profile selection.
  const env = { ...process.env };
  delete env.HD_CORPUS_DEV;
  env.HD_CORPUS_EVERY = "1";
  env.HD_CORPUS_PARTS = "bench";
  env.HD_CORPUS_OUT = input;
  env.HD_CACHE = `${output}/cache`;
  env.CARGO_BUILD_JOBS = "2";
  env.RUST_TEST_THREADS = "2";
  env.HD_TEST_JOBS = "2";
  try {
    // `--runner` names a count_reasons test binary built already (CI's one
    // build); it runs from hd_driver's directory, as cargo runs it.
    const runner = options.get("--runner");
    const filter = ["count_reasons", "--exact", "--ignored", "--nocapture"];
    const cargo = ["test", "-q", "-j", "2", "-p", "hd_driver", "--test", "count_reasons", "--"];
    const log = execFileSync(
      runner ? resolve(runner) : "cargo",
      runner ? filter : [...cargo, ...filter],
      {
        cwd: runner ? `${root}/crates/hd_driver` : root,
        env,
        encoding: "utf8",
        timeout: 600_000,
        maxBuffer: 64 * 1024 * 1024,
      },
    );
    writeFileSync(`${output}/build.log`, log);
  } catch (error) {
    const failed = error as { stdout?: string; stderr?: string };
    writeFileSync(`${output}/build.log`, `${failed.stdout ?? ""}\n${failed.stderr ?? ""}`);
    throw error;
  }
}
const programs = ["bench/study", "bench/runtime/progs"]
  .flatMap((dir) =>
    readdirSync(`${root}/${dir}`)
      .filter((name) => name.endsWith(".hd"))
      .map((name) => `${dir}/${name}`),
  )
  .sort();
const report = readSiteReport(
  readFileSync(`${input}/reasons.tsv`, "utf8"),
  readFileSync(`${input}/digests.tsv`, "utf8"),
  programs,
);
writeFileSync(`${output}/actual.json`, JSON.stringify(report, null, 2) + "\n");
gate(
  output,
  options.get("--base"),
  readSites,
  (base) => compareSites(report, base),
  options.get("--cost-ok"),
);
