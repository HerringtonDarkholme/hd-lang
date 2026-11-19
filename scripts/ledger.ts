// The decision ledger (crates/hd_wasm/src/ledger.rs): builds the bench and
// stage corpora with a compiler that has the `ledger` feature and prints how
// often each emitter decision fired, as one table: counter, bench, stage.
// A step of docs/future-work/BACKEND_NEUTRAL_TIR.md is done when the
// counters it names read zero in both columns.
//
// Bench: the programs of bench/study and bench/runtime/progs, built
// `--release`. Stage: every hd_driver and hd_wasm stage case with a `main`,
// built dev, each as a one-file package; a case that does not build that
// way (a diagnostic or multi-file case) is counted as not built.
//
// Cost per run: one cargo build of `hd` into target/ledger (incremental),
// then one `hd build` process per program (about 510), JOBS at a time, each
// with its own cache under the work directory.
//
// Usage: node scripts/ledger.ts [--hd <hd built with --features hd_driver/ledger>]

import { execFile, execFileSync } from "node:child_process";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { availableParallelism, tmpdir } from "node:os";
import { join } from "node:path";

const root = join(import.meta.dirname, "..");
const JOBS = Math.max(1, Math.floor(availableParallelism() / 2));
const TIMEOUT_MS = 120_000;

function ledgerHd(): string {
  const at = process.argv.indexOf("--hd");
  if (at >= 0) {
    return process.argv[at + 1];
  }
  const target = join(root, "target", "ledger");
  const args = [
    "build",
    "--release",
    "-p",
    "hd_cli",
    "--bin",
    "hd",
    "--features",
    "hd_driver/ledger",
  ];
  // cost: one incremental cargo build a run.
  execFileSync("cargo", [...args, "--target-dir", target], { cwd: root, stdio: "inherit" });
  return join(target, "release", "hd");
}

function corpus(globs: string[], main: boolean): string[] {
  // cost: one `git ls-files` a corpus; the stage filter reads each listed case once.
  const files = execFileSync("git", ["ls-files", "-z", "--", ...globs], {
    cwd: root,
    encoding: "utf8",
  })
    .split("\0")
    .filter(Boolean);
  return main
    ? files.filter((file) => /^(pub )?fn main\b/m.test(readFileSync(join(root, file), "utf8")))
    : files;
}

type Run = { built: number; failed: number; counts: Map<string, number> };

/** Builds one program as a one-file package and returns its ledger rows, or null when it does not build. */
function build(
  hd: string,
  work: string,
  file: string,
  release: boolean,
): Promise<Map<string, number> | null> {
  const dir = mkdtempSync(join(work, "p"));
  mkdirSync(join(dir, "src"));
  writeFileSync(
    join(dir, "hd.toml"),
    '[package]\nname = "ledger"\n\n[capabilities]\nConsole = true\nClock = true\n',
  );
  writeFileSync(join(dir, "src", "main.hd"), readFileSync(join(root, file)));
  const args = ["build", "--engine", "node", ...(release ? ["--release"] : [])];
  const env = { ...process.env, HD_CACHE: join(dir, "cache"), HD_LEDGER: "1" };
  return new Promise((done) => {
    // cost: one `hd build` process per corpus program, JOBS in flight (see the header).
    execFile(
      hd,
      args,
      { cwd: dir, env, timeout: TIMEOUT_MS, maxBuffer: 1 << 26 },
      (error, _out, stderr) => {
        rmSync(dir, { recursive: true, force: true });
        const rows = new Map<string, number>();
        for (const line of stderr.split("\n")) {
          const [tag, key, count] = line.split("\t");
          if (tag === "LEDGER") {
            rows.set(key, (rows.get(key) ?? 0) + Number(count));
          }
        }
        done(error || rows.size === 0 ? null : rows);
      },
    );
  });
}

async function measure(hd: string, work: string, files: string[], release: boolean): Promise<Run> {
  const run: Run = { built: 0, failed: 0, counts: new Map() };
  let next = 0;
  const worker = async () => {
    while (next < files.length) {
      const rows = await build(hd, work, files[next++], release);
      if (!rows) {
        run.failed++;
        continue;
      }
      run.built++;
      for (const [key, count] of rows) {
        run.counts.set(key, (run.counts.get(key) ?? 0) + count);
      }
    }
  };
  await Promise.all(Array.from({ length: JOBS }, worker));
  return run;
}

const started = Date.now();
const hd = ledgerHd();
const work = mkdtempSync(join(tmpdir(), "hd-ledger-"));
try {
  const bench = await measure(
    hd,
    work,
    corpus(["bench/study/*.hd", "bench/runtime/progs/*.hd"], false),
    true,
  );
  const stage = await measure(
    hd,
    work,
    corpus(["crates/hd_driver/tests/cases/*.hd", "crates/hd_wasm/tests/cases/*.hd"], true),
    false,
  );
  const keys = [...new Set([...bench.counts.keys(), ...stage.counts.keys()])].sort();
  const lines = [
    `| Counter | Bench, release (${bench.built} built, ${bench.failed} not) | Stage, dev (${stage.built} built, ${stage.failed} not) |`,
    "| --- | ---: | ---: |",
    ...keys.map((k) => `| \`${k}\` | ${bench.counts.get(k) ?? 0} | ${stage.counts.get(k) ?? 0} |`),
  ];
  process.stdout.write(`${lines.join("\n")}\n`);
  process.stderr.write(`ledger: ${((Date.now() - started) / 1000).toFixed(0)} s\n`);
} finally {
  rmSync(work, { recursive: true, force: true });
}
