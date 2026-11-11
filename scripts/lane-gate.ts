// The gate a lane passes before it is pushed (#1505). It runs what the diff can
// break and nothing more; scripts/lane-gate.sh is the entry point.
//
//   scripts/lane-gate.sh STEP [CASES]
//   scripts/lane-gate.sh --plan [A..B] [CASES]    print the selected steps, run none
//
// STEP names the #1399 step whose evidence plancheck prints. CASES is a
// comma-separated list of conformance path or rule parts, or `none`; fixtures
// the diff adds or changes always run too. The diff is `origin/main...HEAD`
// (GATE_RANGE or the --plan range overrides it). GATE_STAGES_DONE says the
// caller has just run the `--test stages` targets on this tree.
//
// Selection, from `git diff --name-status` and one `cargo metadata`:
//   touched crates    crates/<name>/ files other than .md; Cargo.toml, Cargo.lock
//                     or the toolchain file select every default member; lib/std
//                     also selects hd_driver.
//   selected crates   touched, plus every workspace crate that depends on one
//                     (normal and build edges, transitively), plus the direct
//                     dev-dependents of a touched crate. Crates outside the default
//                     members (hd_run_wasmtime) are CI's.
//   tests of a crate  hd_driver, hd_check, hd_wasm: `--lib --test stages`; hd_check's
//                     also run when any compiler crate is selected, since its
//                     stage baselines dump later stages through hd_testkit;
//                     hd_cli: `--bins` when touched, plus `--test all` for the test
//                     modules the diff changes; every other touched crate: all its targets; a
//                     selected crate that is only a dependent: clippy's type check
//                     of all its targets, no test build (each test binary links the
//                     whole compiler, and CI runs them all), except hd_lint,
//                     whose one test binary asserts on checker facts. Never bare
//                     `-p hd_driver`, never `--test conformance` without
//                     HD_CONFORMANCE_ONLY.
//   golden std TIR    hd_check, hd_tir or lib/std touched.
//   server SDK schema lib/std/{console,http,http_wire,time}.hd or packages/server-sdk.
//   metrics fixtures  bench/metrics fixtures changed (CI runs them for every
//                     diagnostic change).
//   conformance       the named CASES plus runtime/typing fixtures the diff adds or
//                     changes (dev, checked, optimized); changed spec/conformance/cli
//                     case directories run through hd_cli's cli_conformance.
//   std_rt            a compiler crate selected (hd_check, hd_tir, hd_mono, hd_opt,
//                     hd_wasm) or lib/std changed.
//   spec-sync, cost   any file outside crates/*/src and crates/*/tests (spec-sync);
//                     .rs, .ts, the cost allowlist or the ast-grep rules (cost).
//   cargo fmt/clippy  .rs changed (clippy on the selected crates, all targets).
//   oxlint, oxfmt     the changed .ts and .js files.
// A docs-only diff therefore runs plancheck, spec-sync and nothing heavy.
//
// Stops at the first failing step and prints its log tail. Logs:
// target/lane-gate/STEP/NAME.log.
//
// Cost per run: one `git diff`, one `cargo metadata` (read once), then one
// process per planned step; steps grow with the crates the change reaches.

import { spawnSync } from "node:child_process";
import { closeSync, existsSync, mkdirSync, openSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, relative } from "node:path";

type Env = Record<string, string>;
type Step = { name: string; why: string; argv: string[]; env?: Env };
type Change = { status: string; path: string };
type Workspace = {
  byDir: Map<string, string>;
  defaults: Set<string>;
  normalRev: Map<string, Set<string>>;
  devRev: Map<string, Set<string>>;
};
type Context = {
  root: string;
  paths: string[];
  live: string[];
  touched: Set<string>;
  selected: Set<string>;
  std: boolean;
  cargoRoot: boolean;
  named: string;
  stagesDone: boolean;
};
type Rule = (ctx: Context) => Step[];

const STAGE_CRATES = new Set(["hd_driver", "hd_check", "hd_wasm"]);
const OBSERVERS = new Set(["hd_lint"]);
const COMPILER_CRATES = ["hd_check", "hd_tir", "hd_mono", "hd_opt", "hd_wasm"];
const SDK_STD = new Set(["console", "http", "http_wire", "time"]);
const CONFORMANCE = "specification_conformance_passes_except_known_failures";

function git(root: string, args: string[]): string {
  const out = spawnSync("git", args, { cwd: root, encoding: "utf8", maxBuffer: 1 << 28 });
  if (out.status !== 0) {
    console.error(`git ${args.join(" ")}: ${out.stderr}`);
    process.exit(2);
  }
  return out.stdout;
}

function readChanges(root: string, range: string): Change[] {
  const text = git(root, ["diff", "--name-status", "--no-renames", range]);
  return text
    .split("\n")
    .filter(Boolean)
    .map((line) => {
      const [status, ...rest] = line.split("\t");
      return { status: status!, path: rest.join("\t") };
    });
}

function readWorkspace(root: string): Workspace {
  const out = spawnSync("cargo", ["metadata", "--no-deps", "--format-version", "1", "--offline"], {
    cwd: root,
    encoding: "utf8",
    maxBuffer: 1 << 28,
  });
  if (out.status !== 0) {
    console.error(`cargo metadata: ${out.stderr}`);
    process.exit(2);
  }
  const meta = JSON.parse(out.stdout) as {
    packages: {
      id: string;
      name: string;
      manifest_path: string;
      dependencies: { name: string; kind: string | null }[];
    }[];
    workspace_default_members: string[];
  };
  const names = new Set(meta.packages.map((pkg) => pkg.name));
  const byDir = new Map<string, string>();
  const idName = new Map<string, string>();
  const normalRev = new Map<string, Set<string>>();
  const devRev = new Map<string, Set<string>>();
  for (const pkg of meta.packages) {
    byDir.set(relative(root, dirname(pkg.manifest_path)), pkg.name);
    idName.set(pkg.id, pkg.name);
    for (const dep of pkg.dependencies.filter((entry) => names.has(entry.name))) {
      const table = dep.kind === "dev" ? devRev : normalRev;
      table.set(dep.name, (table.get(dep.name) ?? new Set()).add(pkg.name));
    }
  }
  const defaults = new Set(meta.workspace_default_members.map((id) => idName.get(id)!));
  return { byDir, defaults, normalRev, devRev };
}

function selectCrates(ws: Workspace, touched: Set<string>, extra: string[]): Set<string> {
  const selected = new Set([...touched, ...extra]);
  for (const name of touched) {
    for (const dependent of ws.devRev.get(name) ?? []) {
      selected.add(dependent);
    }
  }
  const queue = [...touched];
  while (queue.length > 0) {
    for (const dependent of ws.normalRev.get(queue.pop()!) ?? []) {
      if (!selected.has(dependent)) {
        selected.add(dependent);
        queue.push(dependent);
      }
    }
  }
  return new Set([...selected].filter((name) => ws.defaults.has(name)));
}

function findBin(root: string, name: string): string {
  const local = join(root, "node_modules/.bin", name);
  if (existsSync(local)) {
    return local;
  }
  // A lane worktree has no node_modules; the main checkout's binaries are the same tools.
  const common = git(root, ["rev-parse", "--path-format=absolute", "--git-common-dir"]).trim();
  const shared = join(dirname(common), "node_modules/.bin", name);
  return existsSync(shared) ? shared : name;
}

const has = (ctx: Context, pattern: RegExp) => ctx.paths.some((path) => pattern.test(path));
const anyOf = (set: Set<string>, names: string[]) => names.some((name) => set.has(name));
const step = (name: string, why: string, argv: string[], env?: Env): Step => ({
  name,
  why,
  argv,
  env,
});
const cargoTest = (...args: string[]) => ["cargo", "test", "-q", ...args];

/** The first capture group of `pattern` over the live files, deduplicated and sorted. */
function captures(ctx: Context, pattern: RegExp): string[] {
  const found = ctx.live.map((path) => pattern.exec(path)?.[1]);
  return [...new Set(found.filter((name): name is string => Boolean(name)))].sort();
}

const planRule: Rule = (ctx) => [
  step("plancheck", "always: the #1399 step's evidence", ["python3", "scripts/plancheck.py"]),
  ...(has(ctx, /^(?!crates\/[^/]+\/(src|tests)\/)/)
    ? [
        step("spec_sync", "a file outside crate sources changed", [
          "sh",
          "-c",
          "node scripts/check-rule-citations.ts && node scripts/check-removed-forms.ts",
        ]),
      ]
    : []),
  ...(has(ctx, /\.(rs|ts)$|^scripts\/cost-allowlist\.tsv$|^\.ast-grep\//)
    ? [
        step("cost", ".rs, .ts, allowlist or ast-grep rules changed", [
          "node",
          "scripts/check-cost.ts",
        ]),
      ]
    : []),
];

const scriptRule: Rule = (ctx) => {
  const files = ctx.live.filter((path) => /\.(ts|js)$/.test(path));
  if (files.length === 0) {
    return [];
  }
  const why = `${files.length} .ts/.js file(s) changed`;
  return [
    step("oxlint", why, [findBin(ctx.root, "oxlint"), ...files]),
    step("oxfmt", why, [findBin(ctx.root, "oxfmt"), "--check", ...files]),
  ];
};

const lintRule: Rule = (ctx) => {
  if (!has(ctx, /\.rs$/) && !ctx.cargoRoot) {
    return [];
  }
  // Clippy checks every target of the touched crates and of every crate
  // that depends on one: a dependent's API break shows here, type-checked
  // without a debug build or link, and CI runs the dependents' tests.
  const crates = [...ctx.selected].sort();
  return [
    step("fmt", ".rs files changed", ["cargo", "fmt", "--check"]),
    ...(crates.length === 0
      ? []
      : [
          step("clippy", `touched and their dependents: ${crates.join(" ")}`, [
            "cargo",
            "clippy",
            "-q",
            ...crates.flatMap((name) => ["-p", name]),
            "--all-targets",
            "--",
            "-D",
            "warnings",
          ]),
        ]),
  ];
};

const crateRule: Rule = (ctx) => {
  const rest = [...ctx.selected].filter((name) => !STAGE_CRATES.has(name) && name !== "hd_cli");
  if (rest.length === 0) {
    return [];
  }
  // A touched crate runs every test target. A crate selected only as a
  // dependent is type-checked by the clippy step: its tests are one debug
  // link of the whole compiler each (42 for an hd_syntax change), and CI
  // runs them.
  const touched = rest.filter((name) => ctx.touched.has(name)).sort();
  // hd_lint's tests assert on the checker's facts and run in one binary
  // (about 18 s), so a dependent hd_lint runs them: type-checking it
  // missed d8e7fd385's nine lint failures.
  const observers = rest.filter((name) => !ctx.touched.has(name) && OBSERVERS.has(name)).sort();
  const steps: Step[] = [];
  if (touched.length > 0) {
    steps.push(
      step(
        "crates",
        `touched: ${touched.join(" ")}`,
        cargoTest(...touched.flatMap((name) => ["-p", name])),
      ),
    );
  }
  for (const name of observers) {
    steps.push(
      step(
        name,
        "dependent whose tests observe checker output",
        cargoTest("-p", name, "--test", "all"),
      ),
    );
  }
  return steps;
};

const stageRule: Rule = (ctx) => {
  const steps: Step[] = [];
  const stages = (crate: string, name: string, why = `${crate} selected`) => {
    if (!ctx.stagesDone) {
      steps.push(step(name, why, cargoTest("-p", crate, "--lib", "--test", "stages")));
    }
  };
  // hd_check's stage cases dump own, counts and wat through hd_testkit, a
  // dev-dependency, so a later stage's change reaches its baselines without
  // selecting it.
  if (ctx.selected.has("hd_check")) {
    stages("hd_check", "check_stages");
  } else if (anyOf(ctx.selected, COMPILER_CRATES)) {
    stages("hd_check", "check_stages", "a compiler stage selected: hd_check's baselines dump it");
  }
  if (ctx.selected.has("hd_wasm")) {
    stages("hd_wasm", "wasm_stages");
  }
  return steps;
};

const driverRule: Rule = (ctx) => {
  if (!ctx.selected.has("hd_driver")) {
    return [];
  }
  const targets = captures(ctx, /^crates\/hd_driver\/tests\/(analysis|count_reasons)\.rs$/);
  const stages = ctx.stagesDone ? [] : ["--test", "stages"];
  const extra = targets.flatMap((name) => ["--test", name]);
  const why = ctx.stagesDone
    ? "hd_driver selected (stages done by the caller)"
    : "hd_driver selected";
  const steps = [
    step("hd_driver", why, cargoTest("-p", "hd_driver", "--lib", ...stages, ...extra)),
  ];
  const modules = captures(ctx, /^crates\/hd_driver\/tests\/all\/([^/.]+)(?:\.rs|\/)/);
  if (modules.length > 0) {
    steps.push(
      step(
        "driver_all",
        `changed test modules: ${modules.join(" ")}`,
        cargoTest("-p", "hd_driver", "--test", "all", "--", ...modules.map((name) => `${name}::`)),
      ),
    );
  }
  return steps;
};

const goldenRule: Rule = (ctx) => {
  if (!anyOf(ctx.touched, ["hd_check", "hd_tir"]) && !ctx.std) {
    return [];
  }
  return [
    step(
      "golden_tir",
      "hd_check, hd_tir or lib/std changed",
      cargoTest(
        "-p",
        "hd_driver",
        "--test",
        "all",
        "--",
        "--exact",
        "checker::std_bodies_match_golden_tir",
      ),
    ),
  ];
};

const sdkRule: Rule = (ctx) => {
  const stdFiles = captures(ctx, /^lib\/std\/([^/]+)\.hd$/);
  if (!has(ctx, /^packages\/server-sdk\//) && !stdFiles.some((name) => SDK_STD.has(name))) {
    return [];
  }
  return [
    step("hd_build", "the schema test runs target/debug/hd", [
      "cargo",
      "build",
      "-q",
      "-p",
      "hd_cli",
    ]),
    step("sdk_schema", "lib/std server modules or packages/server-sdk changed", [
      "node",
      "--test",
      "--experimental-strip-types",
      "packages/server-sdk/test/server-schema.test.ts",
    ]),
  ];
};

const cliRule: Rule = (ctx) => {
  // hd_cli's test binaries are debug links of the whole compiler. When hd_cli
  // is only a dependent, clippy type-checks it and CI runs its tests; the gate
  // runs them for a change to hd_cli itself, its test modules, or the metrics
  // fixtures.
  const steps: Step[] = [];
  if (ctx.touched.has("hd_cli")) {
    steps.push(step("cli_bins", "hd_cli touched", cargoTest("-p", "hd_cli", "--bins")));
  }
  const filters = captures(ctx, /^crates\/hd_cli\/tests\/all\/([^/.]+)(?:\.rs|\/)/).map(
    (name) => `${name}::`,
  );
  const metrics = has(ctx, /^bench\/metrics\/(fixtures\/|check-fixtures\.ts)/);
  if (metrics) {
    filters.push("metrics_fixtures");
  }
  if (filters.length > 0) {
    const why = metrics ? "metrics fixtures changed" : "changed test modules";
    steps.push(step("cli_all", why, cargoTest("-p", "hd_cli", "--test", "all", "--", ...filters)));
  }
  return steps;
};

const cliCaseRule: Rule = (ctx) => {
  const cases = captures(ctx, /^spec\/conformance\/cli\/([^/]+)\//);
  if (cases.length === 0) {
    return [];
  }
  return [
    step(
      "cli_conf",
      `${cases.length} changed cli case(s)`,
      cargoTest("-p", "hd_cli", "--test", "cli_conformance"),
      { HD_CONFORMANCE_ONLY: cases.join(",") },
    ),
  ];
};

const conformanceRule: Rule = (ctx) => {
  const fixtures = captures(ctx, /^spec\/conformance\/(?:runtime|typing)\/.*?([^/]+)\.hd$/);
  const cases = [ctx.named, ...fixtures]
    .join(",")
    .replace(/(^|,)none(?=,|$)/g, "$1")
    .replace(/,{2,}/g, ",")
    .replace(/^,|,$/g, "");
  if (cases === "") {
    return [];
  }
  const argv = cargoTest("-p", "hd_driver", "--test", "conformance", CONFORMANCE, "--", "--exact");
  const why = `${cases.split(",").length} case(s): named plus changed fixtures`;
  const only = { HD_CONFORMANCE_ONLY: cases };
  return [
    step("conf_dev", why, argv, only),
    step("conf_checked", why, argv, { ...only, HD_CONFORMANCE_CHECKED: "1" }),
    step("conf_opt", why, argv, { ...only, HD_CONFORMANCE_PIPELINE: "optimized" }),
  ];
};

const stdRtRule: Rule = (ctx) => {
  if (!anyOf(ctx.selected, COMPILER_CRATES) && !ctx.std) {
    return [];
  }
  return [
    step(
      "std_rt",
      "a compiler crate is selected or lib/std changed",
      cargoTest(
        "-p",
        "hd_driver",
        "--test",
        "conformance",
        "--",
        "--exact",
        "std_runtime_modules_pass_their_hd_tests_on_the_linear_backend",
      ),
    ),
  ];
};

// Cheap checks first, the heaviest last.
const RULES: Rule[] = [
  planRule,
  scriptRule,
  lintRule,
  crateRule,
  stageRule,
  driverRule,
  goldenRule,
  sdkRule,
  cliRule,
  cliCaseRule,
  conformanceRule,
  stdRtRule,
];

function context(root: string, changes: Change[], named: string, ws: Workspace): Context {
  const paths = changes.map((change) => change.path);
  const touched = new Set<string>();
  for (const path of paths) {
    const match = /^(crates\/[^/]+)\/(.+)$/.exec(path);
    const crate = match && ws.byDir.get(match[1]!);
    if (crate && ws.defaults.has(crate) && !match[2]!.endsWith(".md")) {
      touched.add(crate);
    }
  }
  const cargoRoot = paths.some((path) =>
    /^(Cargo\.(toml|lock)|rust-toolchain(\.toml)?|\.cargo\/)/.test(path),
  );
  if (cargoRoot) {
    ws.defaults.forEach((name) => touched.add(name));
  }
  const std = paths.some((path) => path.startsWith("lib/std/"));
  return {
    root,
    paths,
    live: changes.filter((change) => change.status !== "D").map((change) => change.path),
    touched,
    selected: selectCrates(ws, touched, std ? ["hd_driver"] : []),
    std,
    cargoRoot,
    named,
    stagesDone: Boolean(process.env.GATE_STAGES_DONE),
  };
}

function showPlan(range: string, count: number, ctx: Context, steps: Step[]) {
  console.log(`plan for ${range}: ${count} changed file(s)`);
  console.log(`touched:  ${[...ctx.touched].sort().join(" ") || "none"}`);
  console.log(`selected: ${[...ctx.selected].sort().join(" ") || "none"}`);
  const clip = (text: string, max: number) =>
    text.length > max ? `${text.slice(0, max - 3)}...` : text;
  for (const entry of steps) {
    const env = Object.entries(entry.env ?? {}).map(([key, value]) => `${key}=${clip(value, 60)}`);
    const argv = entry.argv.map((arg) => clip(arg, 120));
    console.log(`${entry.name.padEnd(14)} ${entry.why}`);
    console.log(`${" ".repeat(15)}${[...env, ...argv].join(" ")}`);
  }
}

function fetchPlan(logs: string): string {
  // cost: one gh call per run.
  const body = spawnSync("gh", ["issue", "view", "1399", "--json", "body", "-q", ".body"], {
    encoding: "utf8",
  });
  if (body.status !== 0) {
    console.log("cannot fetch #1399");
    process.exit(2);
  }
  const file = join(logs, "plan.md");
  writeFileSync(file, body.stdout);
  return file;
}

/** Runs one step into its log; returns false after printing the log tail on failure. */
function runStep(ctx: Context, entry: Step, logs: string, stepName: string): boolean {
  const argv =
    entry.name === "plancheck" ? [...entry.argv, fetchPlan(logs), "--show", stepName] : entry.argv;
  process.stdout.write(entry.name.padEnd(16));
  const started = Date.now();
  const log = join(logs, `${entry.name}.log`);
  const fd = openSync(log, "w");
  // cost: one process per planned step; the plan grows with the crates the diff reaches.
  const run = spawnSync(argv[0]!, argv.slice(1), {
    cwd: ctx.root,
    env: {
      ...process.env,
      RUST_TEST_THREADS: "2",
      CARGO_BUILD_JOBS: "2",
      HD_TEST_JOBS: "4",
      ...entry.env,
    },
    stdio: ["ignore", fd, fd],
  });
  closeSync(fd);
  const text = readFileSync(log, "utf8");
  const seconds = Math.round((Date.now() - started) / 1000);
  if (run.status === 0 && !/FAILED|panicked/.test(text)) {
    console.log(`ok   ${seconds}s`);
    return true;
  }
  console.log(`FAIL ${seconds}s  ${log}`);
  console.log(text.split("\n").slice(-21).join("\n"));
  return false;
}

function writeEvidence(logs: string, stepName: string) {
  const text = readFileSync(join(logs, "plancheck.log"), "utf8");
  const lines = text.split("\n").filter((line) => line.startsWith("  "));
  writeFileSync(join(logs, "evidence.txt"), lines.map((line) => `${line}\n`).join(""));
  console.log(`evidence: ${lines.length} lines for ${stepName} in ${join(logs, "evidence.txt")}`);
}

function main() {
  const args = process.argv.slice(2);
  const planOnly = args[0] === "--plan";
  if (planOnly) {
    args.shift();
  }
  let range = process.env.GATE_RANGE ?? "origin/main...HEAD";
  if (planOnly && args[0]?.includes("..")) {
    range = args.shift()!;
  }
  const stepName = planOnly ? "STEP" : args[0];
  if (!stepName) {
    console.error("usage: scripts/lane-gate.sh STEP [CASES] | --plan [A..B] [CASES]");
    process.exit(2);
  }
  const root = git(process.cwd(), ["rev-parse", "--show-toplevel"]).trim();
  const changes = readChanges(root, range);
  const named = (planOnly ? args[0] : args[1]) ?? "none";
  const ctx = context(root, changes, named, readWorkspace(root));
  const steps = RULES.flatMap((rule) => rule(ctx));
  if (planOnly) {
    showPlan(range, changes.length, ctx, steps);
    return;
  }
  const logs = join(root, "target/lane-gate", stepName);
  mkdirSync(logs, { recursive: true });
  const started = Date.now();
  for (const entry of steps) {
    if (!runStep(ctx, entry, logs, stepName)) {
      process.exit(1);
    }
    if (entry.name === "plancheck") {
      writeEvidence(logs, stepName);
    }
  }
  console.log(`PASS ${Math.round((Date.now() - started) / 1000)}s`);
}

main();
