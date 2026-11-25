// CI smoke check for every bench package, using the already-built compiler.
// Cost: O(bench tree entries + package compilation), one glob and one check
// per package, plus one run with expected.out; no compiler rebuild/timing loop.
import { spawnSync } from "node:child_process";
import { existsSync, globSync, readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { parseArgs } from "node:util";

const { values } = parseArgs({ options: { hd: { type: "string", default: "target/tools/hd" } } });
const hd = resolve(values.hd!);
const manifests = globSync("bench/**/hd.toml").sort();
if (manifests.length === 0) {
  throw new Error("no bench packages found under bench/");
}

const started = performance.now();
let passed = 0;

function run(packageDir: string, args: string[], capture = false): string | undefined {
  // cost: one invocation for this package; compilation scales with its source.
  const result = spawnSync(hd, args, {
    cwd: packageDir,
    encoding: "utf8",
    stdio: ["ignore", capture ? "pipe" : "inherit", "inherit"],
    timeout: 10_000,
  });
  if (result.error || result.status !== 0) {
    if (capture && result.stdout) {
      process.stdout.write(result.stdout);
    }
    console.error(
      `${packageDir}: hd ${args.join(" ")} failed: ${result.error?.message ?? result.status ?? result.signal}`,
    );
    return undefined;
  }
  return result.stdout ?? "";
}

for (const manifest of manifests) {
  const packageDir = dirname(manifest);
  console.log(`Checking ${packageDir}`);
  if (run(packageDir, ["check"]) === undefined) {
    continue;
  }
  const expected = join(packageDir, "expected.out");
  if (existsSync(expected)) {
    const stdout = run(
      packageDir,
      ["run", "--release", "--engine", "node", "--time-limit", "5s"],
      true,
    );
    if (stdout === undefined) {
      continue;
    }
    const wanted = readFileSync(expected, "utf8");
    if (stdout !== wanted) {
      console.error(
        `${packageDir}: stdout differs from expected.out\nexpected: ${JSON.stringify(wanted)}\nactual: ${JSON.stringify(stdout)}`,
      );
      continue;
    }
  }
  passed++;
}

console.log(
  `Bench packages: ${passed}/${manifests.length} passed in ${((performance.now() - started) / 1000).toFixed(2)} s`,
);
if (passed !== manifests.length) {
  process.exitCode = 1;
}
