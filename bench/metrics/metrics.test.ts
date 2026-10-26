// Unit tests of the metric harness helpers. They start no process: no hd,
// no metric script, no `ps`. Tests that start one live in integration/,
// outside the `pnpm test` glob.

import assert from "node:assert/strict";
import {
  chmodSync,
  existsSync,
  linkSync,
  mkdirSync,
  readFileSync,
  realpathSync,
  writeFileSync,
} from "node:fs";
import { join } from "node:path";
import { describe, it } from "node:test";

import { fixtureChecks, markerProblem, mistakeProblem } from "./check-fixtures.ts";
import {
  freedBytes,
  MICRO_CASES,
  passedCount,
  perUnit,
  pickArtifact,
  printed,
} from "./lib/artifact.ts";
import {
  appendMetricsHistory,
  formatTable,
  metricsChanges,
  metricsReadme,
  selectMetrics,
  storeDailyMetrics,
  type MetricsRun,
} from "./run.ts";
import { findCacheCap, findThreadControl } from "./lib/capability.ts";
import { loadMistakes, parseMistake } from "./lib/corpus.ts";
import { applyEdits, fixOf, hasFixField } from "./lib/fixit.ts";
import {
  FIXTURES,
  fixture,
  fixturePaths,
  manifest,
  paramsOf,
  PROGRAMS,
  programFiles,
  render,
  withParams,
  type TemplateScope,
} from "./lib/fixtures.ts";
import {
  dependentsOf,
  docTestPackage,
  editBody,
  editSignature,
  generatePackage,
  helperTemplates,
  integrationPackage,
  makeRandom,
  reachEveryModule,
  unitPackage,
} from "./lib/gen.ts";
import { parseHdCommand, parsePsRss, parseTimeReport, splitCommand } from "./lib/hd.ts";
import { formatValue, judge } from "./lib/metric.ts";
import {
  estimateTokens,
  geomean,
  growthExponent,
  p50,
  p95,
  percentile,
  spread,
} from "./lib/stats.ts";
import { makeTempDir, removeTempDir, treeBytes } from "./lib/tmp.ts";
import { parseStrace } from "./lib/trace.ts";
import { cacheShape } from "./scripts/cache-contention.ts";
import { latencyBound } from "./scripts/concurrency.ts";
import { toolchainOf } from "./scripts/disk.ts";
import { CASES, generate } from "./lib/pathological.ts";
import { FILES, markedLines } from "./scripts/errors-per-run.ts";
import { newErrors } from "./scripts/fixit-safety.ts";
import { METRICS } from "./scripts/index.ts";
import { under } from "./scripts/io-per-check.ts";
import { growthBound, growthFrom, parseSamples, replInput } from "./scripts/long-session.ts";
import { conformanceCounts, tsvRows } from "./scripts/conformance.ts";
import { stdModules, stdNamesIn } from "./scripts/dead-code.ts";
import { applyEdit, difference, newEditState, outcome } from "./scripts/incremental-soundness.ts";
import { flatGrowth } from "./scripts/long-run-memory.ts";
import { nodeBody } from "./scripts/runtime.ts";
import { nodeOnce } from "./scripts/size-startup-heap.ts";

describe("stats", () => {
  it("takes nearest-rank percentiles", () => {
    const values = Array.from({ length: 20 }, (_, index) => 20 - index);
    assert.equal(p50(values), 10);
    assert.equal(p95(values), 19);
    assert.equal(percentile(values, 100), 20);
    assert.equal(percentile([7], 50), 7);
    assert.throws(() => percentile([], 50));
  });

  it("measures growth exponents", () => {
    assert.equal(growthExponent({ size: 2, cost: 100 }, { size: 4, cost: 200 }), 1);
    assert.equal(growthExponent({ size: 2, cost: 100 }, { size: 4, cost: 400 }), 2);
    assert.ok(Number.isNaN(growthExponent({ size: 2, cost: 0 }, { size: 4, cost: 400 })));
  });

  it("estimates four bytes per token", () => {
    assert.equal(estimateTokens("abcdefgh"), 2);
    assert.equal(estimateTokens("abcdefghi"), 3);
  });
});

describe("generator", () => {
  it("is deterministic for a seed", () => {
    const first = generatePackage({ seed: "s", lines: 2_000 });
    const second = generatePackage({ seed: "s", lines: 2_000 });
    assert.deepEqual([...first.files], [...second.files]);
    const other = generatePackage({ seed: "t", lines: 2_000 });
    assert.notDeepEqual([...first.files], [...other.files]);
    assert.equal(makeRandom("x")(), makeRandom("x")());
  });

  it("reaches the requested size", () => {
    const pkg = generatePackage({ seed: "size", lines: 10_000 });
    const counted = [...pkg.files.values()].reduce((sum, text) => sum + text.split("\n").length, 0);
    assert.ok(pkg.lines >= 9_900 && pkg.lines <= 10_400, `${pkg.lines} lines`);
    assert.ok(Math.abs(counted - pkg.lines) < pkg.files.size * 2);
    assert.ok(pkg.files.has("hd.toml") && pkg.files.has("src/lib.hd"));
  });

  it("has edit sites and dependents in every module", () => {
    const pkg = generatePackage({ seed: "edit", lines: 1_000 });
    for (const module of pkg.modules) {
      const text = pkg.files.get(module.file)!;
      assert.match(editBody(text), /let bump = \+2$/m);
      assert.match(editSignature(text), /extra: i32 = 0\) -> i32:/);
    }
    assert.deepEqual(
      dependentsOf(pkg, "m000").map((module) => module.name),
      pkg.modules.filter((module) => module.imports.includes("m000")).map((module) => module.name),
    );
    assert.ok(dependentsOf(pkg, "m000").length >= 1);
  });
});

describe("hd command", () => {
  it("splits a command with quotes", () => {
    assert.deepEqual(splitCommand(`node "a b.js" --x 'y z'`), ["node", "a b.js", "--x", "y z"]);
    assert.throws(() => splitCommand(`node "open`));
  });

  it("reads macOS and GNU time reports", () => {
    const bsd =
      "        0.52 real         0.43 user         0.08 sys\n  77627392  maximum resident set size\n";
    assert.deepEqual(parseTimeReport(bsd), { cpuMs: 510, rssBytes: 77627392 });
    const gnu = [
      '\tCommand being timed: "hd check"',
      "\tUser time (seconds): 0.40",
      "\tSystem time (seconds): 0.10",
      "\tMaximum resident set size (kbytes): 1000",
    ].join("\n");
    assert.deepEqual(parseTimeReport(gnu), { cpuMs: 500, rssBytes: 1_024_000 });
    assert.equal(parseTimeReport("nothing"), undefined);
  });
});

describe("fix-its", () => {
  it("applies UTF-8 byte edits without shifting earlier offsets", () => {
    const text = "αβ\nlet café = 1\n";
    assert.equal(
      applyEdits(text, [
        { file: "case.hd", start: 9, end: 14, text: "coffee" },
        { file: "case.hd", start: 0, end: 0, text: "# note\n" },
      ]),
      "# note\nαβ\nlet coffee = 1\n",
    );
  });

  it("reads the specified single-fix shape, never an alternative", () => {
    const edit = { file: "case.hd", start: 0, end: 1, text: "" };
    const fix = { message: "remove the prefix", edits: [edit] };
    assert.deepEqual(fixOf({ file: "case.hd", fixes: [fix] }), [edit]);
    assert.equal(fixOf({ fixes: [fix, fix] }), undefined);
    assert.equal(fixOf({ fixes: [] }), undefined);
    assert.equal(fixOf({}), undefined);
    assert.equal(fixOf({ fix }), undefined);
    assert.equal(hasFixField({ fixes: [] }), true);
    assert.equal(hasFixField({ fix: null }), false);
  });

  it("leaves edits for other files to a multi-file applicator", () => {
    const edit = { file: "case.hd", start: 0, end: 1, text: "" };
    assert.equal(
      fixOf({ file: "other.hd", fixes: [{ message: "remove", edits: [edit] }] }),
      undefined,
    );
    assert.equal(
      fixOf({ fixes: [{ message: "remove", edits: [edit, { ...edit, file: "other.hd" }] }] }),
      undefined,
    );
    assert.throws(() => applyEdits("x", [edit, { ...edit, file: "other.hd" }]), /multiple files/);
  });

  it("rejects malformed ranges and edits inside a UTF-8 character", () => {
    const edit = { file: "case.hd", start: 0, end: 1, text: "" };
    for (const invalid of [
      { ...edit, start: -1 },
      { ...edit, end: 0.5 },
      { ...edit, start: 2 },
      { ...edit, text: null },
    ]) {
      assert.equal(fixOf({ fixes: [{ message: "remove", edits: [invalid] }] }), undefined);
    }
    assert.throws(() => applyEdits("éx", [{ ...edit, start: 1, end: 2 }]), /UTF-8/);
    assert.throws(() => applyEdits("x", [{ ...edit, end: 2 }]), /range/);
    assert.throws(
      () =>
        applyEdits("abcd", [
          { ...edit, end: 3 },
          { ...edit, start: 2, end: 4 },
        ]),
      /overlapping/,
    );
  });

  it("counts error codes added by a fix", () => {
    const before = [{ code: "a", severity: "error" }];
    const after = [
      { code: "a", severity: "error" },
      { code: "b", severity: "error" },
      { code: "c", severity: "warning" },
    ];
    assert.deepEqual(newErrors(before, after), ["b"]);
    assert.deepEqual(newErrors(before, []), []);
  });
});

describe("fixtures", () => {
  it("replaces the literal of each param line, and only there", () => {
    const text = 'let n: i64 = 1_000  # param: n\nf("1000 ${n}", 3)  # param: m\nx := 5\n';
    assert.deepEqual(
      [...paramsOf(text)],
      [
        ["n", 1000],
        ["m", 3],
      ],
    );
    assert.equal(
      withParams(text, { n: 7, m: 40 }),
      'let n: i64 = 7  # param: n\nf("1000 ${n}", 40)  # param: m\nx := 5\n',
    );
    assert.throws(() => withParams(text, { k: 1 }), /no param k/);
    assert.throws(() => paramsOf("f(1, 2)  # param: n"), /exactly one integer/);
  });

  it("renders fields, functions, blocks and comments of a template", () => {
    const template = [
      "{{! a comment }}",
      "fn f{{k}}() -> i32: {{twice 4}}",
      "{{#each items}}",
      "    v{{i}}{{tail}}",
      "{{/each}}",
      "{{#if on}}",
      "on",
      "{{/if}}",
      "{{#unless on}}",
      "off",
      "{{/unless}}",
    ].join("\n");
    const scope: TemplateScope = {
      k: 3,
      twice: (n: string) => 2 * Number(n),
      items: [{ i: 1 }, { i: 2, tail: "!" }],
      tail: "",
      on: false,
    };
    assert.equal(render(template, scope), "fn f3() -> i32: 8\n    v1\n    v2!\noff");
    assert.throws(() => render("{{missing}}", {}), /unset/);
    assert.throws(() => render("{{#each items}}\nx", { items: [] }), /not closed/);
  });

  it("writes capability tables into a manifest", () => {
    assert.equal(manifest("a"), '[package]\nname = "a"\n');
    assert.equal(
      manifest("b", { capabilities: ["Console"], devCapabilities: ["FsRead"] }),
      '[package]\nname = "b"\n\n[capabilities]\nConsole = true\n\n[dev-capabilities]\nFsRead = true\n',
    );
  });

  it("builds every program package with its default and its own params", () => {
    for (const name of Object.keys(PROGRAMS) as (keyof typeof PROGRAMS)[]) {
      const files = programFiles(name);
      assert.ok(files.has("hd.toml") && [...files.keys()].some((path) => path.startsWith("src/")));
      for (const source of Object.values(PROGRAMS[name].files)) assert.ok(fixture(source));
    }
    assert.match(
      programFiles("serde-throughput/encode", { repeat: 32 }).get("src/main.hd")!,
      /= 32 /,
    );
    assert.throws(() => programFiles("runtime/baseline", { repeat: 1 }), /no param repeat/);
  });

  it("uses every fixture file in some check", () => {
    const { unused, checks } = fixtureChecks(parseHdCommand("hd"));
    assert.deepEqual(unused, []);
    assert.ok(checks.length > 100, `${checks.length} checks`);
    assert.ok(fixturePaths().every((path) => existsSync(join(FIXTURES, path))));
  });

  it("draws helpers from every paragraph of the helper template", () => {
    assert.equal(helperTemplates().length, 7);
    assert.ok(helperTemplates().every((text) => text.startsWith("fn ")));
  });
});

describe("corpora", () => {
  it("loads the mistake corpus", () => {
    const corpus = loadMistakes();
    assert.ok(corpus.length >= 40 && corpus.length <= 80, `${corpus.length} programs`);
    for (const mistake of corpus) {
      assert.ok(mistake.line > 1, mistake.name);
      assert.ok(mistake.text.split("\n")[mistake.line - 1]!.includes(mistake.code), mistake.name);
    }
  });

  it("rejects a program whose marker and header disagree", () => {
    const text = "# mistake: m\n# log: l\n# expect: a\n\nx  # diagnostic: b\n";
    assert.throws(() => parseMistake("bad", "bad.hd", text), /differs/);
  });

  it("builds the fixed variant of a program", () => {
    const text = "# mistake: m\n# log: l\n# expect: a\n# fixed: y\\nz\n\nx  # diagnostic: a\nw\n";
    assert.equal(
      parseMistake("p", "p.hd", text).fixedText,
      text.replace("x  # diagnostic: a", "y\nz"),
    );
  });

  it("generates pathological cases that grow with scale", () => {
    for (const entry of CASES) {
      assert.equal(generate(entry, 1), generate(entry, 1), entry.name);
      assert.ok(generate(entry, 4).length > generate(entry, 2).length, entry.name);
    }
  });

  it("marks ten mistakes in each errors-per-run file", () => {
    for (const label of FILES)
      assert.equal(markedLines(fixture(`errors-per-run/${label}.hd`)).length, 10, label);
  });

  it("reads the cascade codes a mistake allows", () => {
    const text = "# mistake: m\n# log: l\n# expect: a\n# cascade: b, c\n\nx  # diagnostic: a\n";
    const mistake = parseMistake("p", "p.hd", text);
    assert.deepEqual(mistake.cascade, ["b", "c"]);
    const at = (code: string, line = 6) => ({ code, line });
    assert.equal(mistakeProblem(mistake, [at("a"), at("b", 9)]), undefined);
    assert.match(mistakeProblem(mistake, [at("b")])!, /expected a/);
    assert.match(mistakeProblem(mistake, [at("a"), at("d")])!, /also got d@6/);
  });

  it("judges an errors-per-run report against its markers", () => {
    const text = "f  # diagnostic: unknown-name\ng  # diagnostic: syntax-error\nh\n";
    const at = (code: string, line: number) => ({ code, line });
    assert.equal(markerProblem(text, [at("syntax-error", 2)]), undefined);
    assert.match(markerProblem(text, [at("syntax-error", 3)])!, /unmarked/);
    assert.match(markerProblem(text, [])!, /not reported: syntax-error@2/);
    const parsing = "f  # diagnostic: unknown-name\n";
    assert.match(markerProblem(parsing, [])!, /not reported: unknown-name@1/);
  });
});

describe("runner", () => {
  it("selects metrics by name and pillar", () => {
    assert.deepEqual(
      selectMetrics(METRICS, ["fmt,mistakes"], undefined).map((metric) => metric.name),
      ["mistakes", "fmt"],
    );
    assert.equal(selectMetrics(METRICS, [], "1").length, 15);
    assert.deepEqual(
      selectMetrics(METRICS, [], "2").map((metric) => metric.name),
      [
        "resources",
        "long-session",
        "startup",
        "concurrency",
        "disk",
        "parallel-speedup",
        "cache-contention",
        "cache-growth",
        "io-per-check",
        "fetch-dedup",
      ],
    );
    assert.deepEqual(
      selectMetrics(METRICS, [], "3").map((metric) => metric.name),
      [
        "proptest-perf",
        "unit-test-perf",
        "integration-test-perf",
        "runtime",
        "allocations",
        "size-startup-heap",
        "host-call-overhead",
        "suspension-overhead",
        "serde-throughput",
        "text-throughput",
        "dead-code",
        "long-run-memory",
      ],
    );
    assert.deepEqual(
      selectMetrics(METRICS, [], "gate").map((metric) => metric.name),
      ["conformance", "incremental-soundness"],
    );
    assert.throws(() => selectMetrics(METRICS, ["nope"], undefined), /unknown metric/);
  });

  it("formats a table and judges targets", () => {
    assert.equal(
      formatTable([
        ["a", "bb"],
        ["ccc", "d"],
      ]),
      "a   | bb\n----|---\nccc | d",
    );
    assert.equal(judge("m", "t", 40, 50, "ms").status, "pass");
    assert.equal(judge("m", "t", 0.9, 0.95, "%", "at-least").status, "fail");
    assert.equal(judge("m", "t", Number.NaN, 1, "x").status, "fail");
  });
});

describe("pillar 2 helpers", () => {
  it("sums the RSS of one process group from ps", () => {
    const text = "    1  18112\n  367  16672\n  367   9920\n 3670     10\n";
    assert.equal(parsePsRss(text, 367), (16672 + 9920) * 1024);
    assert.equal(parsePsRss(text, 42), undefined);
  });

  it("counts hard-linked files once and removes read-only trees", () => {
    const dir = makeTempDir("unit");
    mkdirSync(join(dir, "entry"));
    writeFileSync(join(dir, "entry", "a.txt"), "12345");
    linkSync(join(dir, "entry", "a.txt"), join(dir, "b.txt"));
    writeFileSync(join(dir, "c.txt"), "123");
    assert.deepEqual(treeBytes(dir), { bytes: 8, files: 2 });
    chmodSync(join(dir, "entry"), 0o555);
    removeTempDir(dir);
    assert.equal(existsSync(dir), false);
  });

  it("finds documented thread controls and cache caps", () => {
    const flag = findThreadControl("flags:\n  -j, --jobs N  threads to use\n", "");
    assert.deepEqual(flag?.args(4), ["--jobs=4"]);
    const variable = findThreadControl("flags:\n  --tests\n", "HD_THREADS sets the thread count");
    assert.deepEqual(variable?.env(2), { HD_THREADS: "2" });
    const both = findThreadControl("  --jobs N  use N threads\n", "the default is HD_JOBS");
    assert.deepEqual(both?.args(3), ["--jobs=3"]);
    assert.deepEqual(both?.fallback?.env(3), { HD_JOBS: "3" });
    assert.deepEqual(both?.fallback?.args(3), []);
    assert.equal(findThreadControl("  -p, --package NAME\n", "HD_CACHE"), undefined);
    assert.equal(findCacheCap("HD_CACHE_MAX_BYTES caps the cache"), "HD_CACHE_MAX_BYTES");
    assert.equal(findCacheCap("Fetched versions go to HD_CACHE when it is set."), undefined);
  });

  it("reads file calls from an strace log", () => {
    const log = [
      '101 openat(AT_FDCWD, "/p/src/m001.hd", O_RDONLY|O_CLOEXEC) = 3',
      '101 openat(AT_FDCWD, "/p/build/out", O_WRONLY|O_CREAT, 0644) = 4',
      '[pid 102] newfstatat(AT_FDCWD, "/p/src/m002.hd", {st_mode=S_IFREG|0644}, 0) = 0',
      '102 openat(AT_FDCWD, "/p/src/gone.hd", O_RDONLY) = -1 ENOENT (No such file or directory)',
      '102 stat("src/m003.hd", {st_mode=S_IFREG|0644}) = 0',
      '102 newfstatat(3, "", {st_mode=S_IFREG|0644}, AT_EMPTY_PATH) = 0',
      "102 <... openat resumed>) = 5",
    ].join("\n");
    const access = parseStrace(log);
    assert.deepEqual([...access.reads], ["/p/src/m001.hd"]);
    assert.deepEqual([...access.stats].sort(), [
      "/p/build/out",
      "/p/src/gone.hd",
      "/p/src/m002.hd",
      "src/m003.hd",
    ]);
    assert.deepEqual(under(access.stats, ["/p/src"], "/p"), [
      "/p/src/gone.hd",
      "/p/src/m002.hd",
      "/p/src/m003.hd",
    ]);
  });

  it("matches hash-named cache files by their place", () => {
    const pack = (hash: string) => `obj/pack-v1/${hash}.pack`;
    assert.deepEqual(cacheShape([pack("5bab78bb9d0fea6a2297b731bf7942b7"), "obj/pack-v1/index"]), [
      "obj/pack-v1/*.pack",
      "obj/pack-v1/index",
    ]);
    assert.deepEqual(
      cacheShape([pack("8220cddcb8057615261e169576720938")]),
      cacheShape([pack("525ef66a5bc28d6893f8dfa60ea5495f")]),
    );
  });

  it("bounds concurrent latency by the core count", () => {
    assert.equal(latencyBound(4, 8), 1.5);
    assert.equal(latencyBound(8, 8), 1.5);
    assert.equal(latencyBound(16, 8), 3);
  });

  it("marks every REPL input with its own index", () => {
    assert.equal(replInput(0), "w0 := +1000000");
    assert.equal(replInput(7), "w6 + 1");
  });

  it("reads the session test's samples and bounds the growth by page size", () => {
    const samples = parseSamples(
      [
        "LONG-SESSION-START rss_kib=48704",
        "LONG-SESSION inputs=100 rss_kib=1000 build_us=5 run_us=6 mean_build_us=7 mean_run_us=8 journal=9",
        "LONG-SESSION inputs=150 rss_kib=1500 build_us=5 run_us=6 mean_build_us=70 mean_run_us=8 journal=9",
      ].join("\n"),
    );
    assert.equal(samples.length, 2);
    assert.equal(samples[1]!.rssBytes, 1500 * 1024);
    assert.equal(samples[1]!.buildUs, 70);
    assert.equal(growthFrom(samples, 100), 500 * 1024);
    assert.ok(Number.isNaN(growthFrom(samples, 50)));
    assert.equal(growthBound(100, 1000, 16384), 900 * 100 * 1024);
    assert.equal(growthBound(100, 1000, 4096), 900 * 25 * 1024);
  });

  it("takes node_modules as the toolchain of a Node hd", () => {
    // Any script of this repository stands in for an hd written for Node.
    const root = join(import.meta.dirname, "..", "..");
    const toolchain = toolchainOf(["node", join(root, "scripts", "test-web-compiler.js")]);
    assert.equal(toolchain.kind, "node_modules");
  });

  it("takes a native hd as its own toolchain, even in a checkout with node_modules", () => {
    const toolchain = toolchainOf([process.execPath]);
    assert.deepEqual(toolchain, { kind: "files", paths: [realpathSync(process.execPath)] });
  });
});

describe("pillar 3 helpers", () => {
  it("takes spreads and geometric means", () => {
    const values = [5, 1, 4, 2, 3, 9, 8, 7, 6, 10];
    assert.deepEqual(spread(values), { p10: 1, p50: 5, p90: 9 });
    assert.equal(geomean([2, 8]), 4);
    assert.ok(Number.isNaN(geomean([1, 0])));
    assert.ok(Number.isNaN(geomean([])));
  });

  it("formats rates and small costs", () => {
    assert.equal(formatValue(0.5, "us"), "0.50 µs");
    assert.equal(formatValue(93.4, "ns"), "93 ns");
    assert.equal(formatValue(2_500_000, "per-s"), "2.5M/s");
    assert.equal(formatValue(45_000, "per-s"), "45k/s");
    assert.equal(formatValue(3.25, "MB/s"), "3.3 MB/s");
    assert.equal(formatValue(80, "B/iter"), "80 B/iter");
  });

  it("picks the release Wasm file of the package, else a native executable", () => {
    const files = ["debug/bench.wasm", "release/other.wasm", "release/bench.wasm"];
    assert.deepEqual(pickArtifact(files, true), { path: "release/bench.wasm", kind: "wasm" });
    assert.deepEqual(pickArtifact(files, false), { path: "debug/bench.wasm", kind: "wasm" });
    assert.deepEqual(
      pickArtifact(["release/bench", "release/bench.d"], true, (path) => path === "release/bench"),
      { path: "release/bench", kind: "native" },
    );
    assert.equal(pickArtifact(["release/bench.d"], true), undefined);
  });

  it("pairs timings into costs per unit", () => {
    assert.deepEqual(perUnit([110, 120], [10, 20], 10), [10, 10]);
  });

  it("reads printed counts and test totals", () => {
    assert.equal(printed("bytes=45488 total=10", "bytes"), 45488);
    assert.throws(() => printed("nothing", "bytes"), /no bytes=N/);
    assert.equal(passedCount("src/a.hd: 5 passed\nsrc/b.hd: 3 passed, 1 failed\n"), 8);
  });

  it("sums the bytes a GC log freed", () => {
    const log = [
      "[1:0x1] 9 ms: pause=0.2 gc=s start_object_size=4000 end_object_size=1000 allocated=4000",
      "program output",
      "[1:0x1] 12 ms: pause=0.1 gc=mc start_object_size=3000 end_object_size=2500",
    ].join("\n");
    assert.equal(freedBytes(log), 3500);
    assert.equal(freedBytes("no collections"), 0);
  });

  it("turns each micro case into a self-timing Node program", () => {
    const root = join(import.meta.dirname, "..", "..");
    for (const name of MICRO_CASES) {
      const source = readFileSync(join(root, "bench", "micro", `${name}.js`), "utf8");
      assert.match(
        nodeBody(source),
        /console\.log\(JSON\.stringify\(\{ ms: measure\(main\) \}\)\);\n$/,
      );
      assert.match(nodeOnce(source), /\nmain\(\);\n$/);
      assert.ok(!nodeBody(source).includes("const times"), name);
    }
    assert.throws(() => nodeBody("console.log(1)"), /not a micro case/);
  });

  it("generates unit, integration and doc test packages of the asked size", () => {
    const units = unitPackage(100);
    const cases = [...units.values()].join("\n").match(/^\s*it\(/gm) ?? [];
    assert.equal(cases.length, 100);
    assert.ok(units.has("src/m9_test.hd"));
    const integration = integrationPackage(3);
    assert.equal([...integration.keys()].filter((path) => path.startsWith("tests/")).length, 3);
    assert.match(integration.get("tests/export0.hd")!, /temp_dir\(\)/);
    const docs = docTestPackage(4).get("src/lib.hd")!;
    assert.equal(docs.match(/^## ```hd$/gm)?.length, 4);
  });

  it("calls into every module of a generated package", () => {
    const pkg = generatePackage({ seed: "reach", lines: 1_000 });
    const main = reachEveryModule(pkg);
    for (const [k, module] of pkg.modules.entries()) {
      assert.ok(main.includes(`use pkg.${module.name}.`), module.name);
      assert.ok(main.includes(`shape${k}(`), module.name);
    }
  });

  it("finds std module names among Wasm names, but not runtime helpers", () => {
    const modules = stdModules(join(import.meta.dirname, "..", ".."));
    assert.ok(modules.includes("json") && modules.includes("std") && !modules.includes("host"));
    assert.deepEqual(
      stdNamesIn(
        ["hd.host_string_new", "hd.panic", "hd.json_encode", "stdConsoleWrite", "main"],
        modules,
      ),
      ["hd.json_encode", "stdConsoleWrite"],
    );
  });

  it("measures flat memory from the samples after warm-up", () => {
    const mb = 1024 * 1024;
    const flat = [50, 80, 100, 100, 101, 100, 99, 100, 101, 100, 100, 102].map((v) => v * mb);
    assert.ok(flatGrowth(flat) <= 2 * mb);
    const growing = Array.from({ length: 12 }, (_, i) => (100 + i * 5) * mb);
    assert.equal(flatGrowth(growing), 30 * mb);
    assert.ok(Number.isNaN(flatGrowth([1, 2, 3])));
  });

  it("reads a conformance summary and a TSV", () => {
    const output =
      "........\ndriver conformance on node, dev pipeline: pass 90, known 1, fail 9, unsupported 0, total 100\n  unsupported:Link (1):\nCLI conformance on node: pass 4, known 0, fail 0, unsupported 0, total 4\n";
    assert.deepEqual(conformanceCounts(output), { passed: 94, selected: 103 });
    assert.equal(conformanceCounts("nothing"), undefined);
    assert.deepEqual(tsvRows("path\treason\na\tb\n\nc\td\n"), ["a\tb", "c\td"]);
  });

  it("applies each edit of a soundness script and undoes breakage", () => {
    const pkg = generatePackage({ seed: "soundness", lines: 1_000 });
    const text = pkg.files.get(pkg.modules[1]!.file)!;
    const state = newEditState();
    assert.equal(applyEdit("repair", 1, text, state), undefined);
    const broken = applyEdit("break", 1, text, state)!;
    assert.match(broken, /x \+ bump \+ true$/m);
    assert.equal(applyEdit("break", 1, broken, state), undefined);
    assert.equal(applyEdit("repair", 1, broken, state), text);
    const failing = applyEdit("break-test", 1, text, state)!;
    assert.equal(applyEdit("repair-test", 1, failing, state), text);
    const signed = applyEdit("signature", 1, text, state)!;
    assert.equal(applyEdit("signature", 1, signed, state), undefined);
    assert.match(applyEdit("add", 1, text, state)!, /pub fn added1_1\(x: i32\) -> i32:/);
  });

  it("compares runs by status, diagnostics and test outcomes in any order", () => {
    const diag = (file: string, line: number) =>
      JSON.stringify({
        kind: "diagnostic",
        code: "c",
        severity: "error",
        file,
        line,
        column: 1,
        message: "m",
      });
    const test = (name: string, result: string) =>
      JSON.stringify({ kind: "test", name, outcome: result });
    const a = outcome(1, [diag("/w/src/a.hd", 2), test("t1", "passed")].join("\n"), "/w");
    const b = outcome(1, [test("t1", "passed"), diag("/f/src/a.hd", 2)].join("\n"), "/f");
    assert.equal(difference(a, b), undefined);
    const c = outcome(1, [test("t1", "failed"), diag("/f/src/a.hd", 2)].join("\n"), "/f");
    assert.match(
      difference(a, c) ?? "",
      /incremental only: test \| t1 \| passed; clean only: test \| t1 \| failed/,
    );
    assert.match(difference(outcome(0, "", "/w"), outcome(1, "", "/f")) ?? "", /status 0/);
  });
});

describe("daily metrics", () => {
  const run = (
    value = "100 ms",
    status: "pass" | "fail" | "n/a" = "pass",
    finished = "2026-10-25T06:00:00.000Z",
  ): MetricsRun => ({
    hd: "hd",
    commit: "abc123",
    finished,
    machine: { runner: "ubuntu-latest", cpus: 4, cpu: "test CPU" },
    results: [{ metric: "fmt", name: "latency", value, target: "≤ 200 ms", status }],
  });

  it("keeps same-day runs and expands columns without changing earlier values", () => {
    const first = run();
    const second = {
      ...run("120 ms", "fail", "2026-10-25T07:00:00.000Z"),
      results: [
        ...run("120 ms", "fail").results,
        { metric: "new", name: "target", value: "1 B", target: "≤ 2 B", status: "pass" as const },
      ],
    };
    const history = appendMetricsHistory(appendMetricsHistory("", first), second);
    const [header, one, two] = history
      .trimEnd()
      .split("\n")
      .map((line) => line.split("\t"));
    assert.equal(one![header!.indexOf('value ["fmt","latency"]')], "100 ms");
    assert.equal(one![header!.indexOf('value ["new","target"]')], "");
    assert.equal(two![header!.indexOf('value ["new","target"]')], "1 B");
    assert.equal(two![header!.indexOf("fail")], "1");
    assert.equal(appendMetricsHistory(history, second), history);
    const absent = appendMetricsHistory(history, {
      ...run(),
      finished: "2026-10-26T06:00:00.000Z",
      results: [],
    });
    assert.equal(absent.trimEnd().split("\n").length, 4);
  });

  it("reports flips and movements strictly greater than ten percent", () => {
    assert.match(metricsChanges(run()), /First recorded/);
    assert.doesNotMatch(metricsChanges(run("110 ms"), run().results), /→/);
    assert.match(metricsChanges(run("111 ms"), run().results), /pass 100 ms → pass 111 ms/);
    assert.match(metricsChanges(run("100 ms", "fail"), run().results), /pass 100 ms → fail 100 ms/);
    assert.match(metricsChanges(run("-", "n/a"), run().results), /→ n\/a -/);
    assert.match(metricsChanges(run("1 ms"), run("0 ms").results), /→/);
  });

  it("normalizes time, bytes, throughput and composite measurements", () => {
    for (const [before, after] of [
      ["1 s", "1000 ms"],
      ["1 MB", "1048576 B"],
      ["1M/s", "1000k/s"],
      ["100 ms, 2 MB", "110 ms, 2 MB"],
    ])
      assert.doesNotMatch(metricsChanges(run(after), run(before).results), /→/);
    assert.match(metricsChanges(run("100 ms, 3 MB"), run("100 ms, 2 MB").results), /→/);
    assert.doesNotMatch(metricsChanges(run("identical"), run("same").results), /→/);
  });

  it("groups pillars, marks small timing changes as noise and limits daily links", () => {
    const days = Array.from(
      { length: 30 },
      (_, i) => `2026-10-${String(Math.floor(i / 2) + 1).padStart(2, "0")}T${i % 2 ? "12" : "00"}`,
    );
    const text = metricsReadme(run("105 ms"), run().results, days);
    assert.match(text, /## Pillar 1/);
    assert.match(text, /ubuntu-latest, 4 CPUs/);
    assert.match(text, /Timing change ≤10%: noise/);
    assert.equal(text.match(/\]\(daily\//g)?.length, 28);
    assert.match(text, /daily\/2026-10-15T12.json/);
    assert.doesNotMatch(text, /daily\/2026-10-01T12.json/);
  });

  it("escapes metric text in Markdown and keeps TSV cells on one line", () => {
    const current = {
      ...run(),
      results: [{ ...run().results[0]!, name: "a|<b>\nline", note: "one\ttwo" }],
    };
    const text = metricsReadme(current, undefined, []);
    assert.match(text, /a&#124;&lt;b> line/);
    const history = appendMetricsHistory("", current);
    assert.equal(history.trimEnd().split("\n").length, 2);
    assert.match(history, /b>\\nline/);
  });

  it("stores a first run and compares a second run on the same date", () => {
    const directory = makeTempDir("daily-store");
    try {
      assert.match(storeDailyMetrics(run(), directory), /First recorded run/);
      const second = run("150 ms", "fail", "2026-10-25T07:00:00.000Z");
      const summary = storeDailyMetrics(second, directory);
      assert.match(summary, /pass 100 ms → fail 150 ms/);
      assert.match(
        readFileSync(join(directory, "README.md"), "utf8"),
        new RegExp(summary.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")),
      );
      const daily = JSON.parse(
        readFileSync(join(directory, "daily/2026-10-25T07.json"), "utf8"),
      ) as MetricsRun;
      assert.deepEqual(daily, second);
      assert.equal(
        readFileSync(join(directory, "history.tsv"), "utf8").trimEnd().split("\n").length,
        3,
      );
      storeDailyMetrics(second, directory);
      assert.equal(
        readFileSync(join(directory, "history.tsv"), "utf8").trimEnd().split("\n").length,
        3,
      );
      assert.throws(
        () => storeDailyMetrics({ ...second, finished: "../../escape" }, directory),
        /ISO finished/,
      );
    } finally {
      removeTempDir(directory);
    }
  });
});
