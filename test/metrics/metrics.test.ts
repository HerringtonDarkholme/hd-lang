// Unit tests of the metric harness helpers. They start no process: no hd,
// no metric script, no `ps`. Tests that start one live in integration/,
// outside the `pnpm test` glob.

import assert from "node:assert/strict";
import { chmodSync, existsSync, linkSync, mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { describe, it } from "node:test";

import { CASES } from "./pathological/cases.ts";
import { formatTable, selectMetrics } from "./run.ts";
import { findCacheCap, findThreadControl } from "./lib/capability.ts";
import { loadMistakes, parseMistake } from "./lib/corpus.ts";
import { applyEdits, fixOf } from "./lib/fixit.ts";
import { dependentsOf, editBody, editSignature, generatePackage, makeRandom } from "./lib/gen.ts";
import { parsePsRss, parseTimeReport, splitCommand } from "./lib/hd.ts";
import { judge } from "./lib/metric.ts";
import { estimateTokens, growthExponent, p50, p95, percentile } from "./lib/stats.ts";
import { makeTempDir, removeTempDir, treeBytes } from "./lib/tmp.ts";
import { parseStrace } from "./lib/trace.ts";
import { latencyBound } from "./scripts/concurrency.ts";
import { toolchainOf } from "./scripts/disk.ts";
import { assemble, FILES } from "./scripts/errors-per-run.ts";
import { newErrors } from "./scripts/fixit-safety.ts";
import { METRICS } from "./scripts/index.ts";
import { under } from "./scripts/io-per-check.ts";
import { growthFrom, replInput, replSteps } from "./scripts/long-session.ts";

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
  it("applies edits by offset or by line and column", () => {
    const text = "fn f() -> i32:\n    x: i32 = 5\n    x\n";
    const byOffset = {
      start: { line: 2, column: 5, offset: 19 },
      end: { line: 2, column: 5, offset: 19 },
    };
    assert.equal(
      applyEdits(text, [{ ...byOffset, replacement: "let " }]),
      text.replace("    x:", "    let x:"),
    );
    const byLine = { start: { line: 3, column: 5 }, end: { line: 3, column: 6 } };
    assert.equal(
      applyEdits(text, [{ ...byLine, replacement: "0" }]),
      text.replace("    x\n", "    0\n"),
    );
  });

  it("reads one fix, never a choice of several", () => {
    const edit = {
      span: { start: { line: 1, column: 1 }, end: { line: 1, column: 2 } },
      replacement: "",
    };
    assert.equal(fixOf({ fix: { message: "m", edits: [edit] } })!.length, 1);
    assert.equal(fixOf({ fix: null, fixes: [{ edits: [edit] }, { edits: [edit] }] }), undefined);
    assert.equal(fixOf({}), undefined);
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

describe("corpora", () => {
  it("loads the mistake corpus", () => {
    const corpus = loadMistakes(join(import.meta.dirname, "mistakes"));
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
      assert.equal(entry.generate(1), entry.generate(1), entry.name);
      assert.ok(entry.generate(4).length > entry.generate(2).length, entry.name);
    }
  });

  it("marks ten mistakes in each errors-per-run file", () => {
    for (const functions of Object.values(FILES))
      assert.equal(assemble(functions).lines.length, 10);
  });
});

describe("runner", () => {
  it("selects metrics by name and pillar", () => {
    assert.deepEqual(
      selectMetrics(METRICS, ["fmt,mistakes"], undefined).map((metric) => metric.name),
      ["mistakes", "fmt"],
    );
    assert.equal(selectMetrics(METRICS, [], "1").length, 14);
    assert.deepEqual(
      selectMetrics(METRICS, [], "2").map((metric) => metric.name),
      [
        "resources",
        "long-session",
        "startup",
        "concurrency",
        "disk",
        "suite-cpu",
        "parallel-speedup",
        "cache-contention",
        "cache-growth",
        "io-per-check",
        "fetch-dedup",
      ],
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

  it("bounds concurrent latency by the core count", () => {
    assert.equal(latencyBound(4, 8), 1.5);
    assert.equal(latencyBound(8, 8), 1.5);
    assert.equal(latencyBound(16, 8), 3);
  });

  it("marks every REPL input with its own index", () => {
    assert.equal(replInput(0), "w0 := +1000000");
    assert.equal(replInput(7), "w6 + 1");
    const steps = replSteps(120, 50);
    assert.equal(steps.length, 3);
    assert.equal(steps[2]!.input.split("\n").filter(Boolean).length, 20);
    assert.ok(steps[0]!.done("1000049 : i32\n"));
    assert.ok(!steps[0]!.done("1000048 : i32\n"));
    assert.equal(growthFrom([5, 9, 12, 10], 1), 3);
    assert.ok(Number.isNaN(growthFrom([5], 1)));
  });

  it("takes node_modules as the toolchain of a Node hd", () => {
    const root = join(import.meta.dirname, "..", "..");
    const toolchain = toolchainOf([
      "node",
      "--experimental-strip-types",
      join(root, "bin", "hd.js"),
    ]);
    assert.equal(toolchain.kind, "node_modules");
  });
});
