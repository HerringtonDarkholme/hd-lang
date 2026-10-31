// The generator of the metrics' synthetic packages, from the templates in
// fixtures/package/, fixtures/unit-test-perf/ and
// fixtures/integration-test-perf/.
//
// `generatePackage({ seed, lines })` returns the files of one package of
// about `lines` lines: modules `src/m000.hd`, `src/m001.hd`, ... of
// fixtures/package/module.hd.tmpl, each with structs, enums, traits, impls,
// matches, `?`, loops, a `tests:` block and 6 to 12 helpers drawn from
// fixtures/package/helpers.hd.tmpl, plus `src/lib.hd`, one integration test
// and `hd.toml`. The same seed and size always give byte-identical files.
//
// Each module carries two edit sites the metrics use:
// - a private body edit: the line `    let bump = +N` in `fn tweakK`;
// - a public signature edit: `pub fn shapeK(x: i32) -> i32:`, which gains a
//   defaulted parameter, so its callers still type-check.
// Module K calls `shape(K-1)` and `total(K-1)`, so module K+1 is the direct
// dependent of module K.
//
// `unitPackage`, `integrationPackage` and `docTestPackage` write the test
// packages of unit-test-perf and integration-test-perf.

import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

import { fixture, manifest, render, renderFixture, writePackage, type Files } from "./fixtures.ts";
import { copyTree, makeTempDir } from "./tmp.ts";

export interface GeneratedModule {
  /** The module path segment, as `m007`. */
  readonly name: string;
  /** The file, relative to the package root, as `src/m007.hd`. */
  readonly file: string;
  /** Modules this module imports. */
  readonly imports: readonly string[];
  /** The name of one test case in this module, unique in the package. */
  readonly testName: string;
}

export interface GeneratedPackage {
  readonly files: ReadonlyMap<string, string>;
  readonly modules: readonly GeneratedModule[];
  readonly lines: number;
}

export const SIZES = { small: 1_000, "10k": 10_000, "50k": 50_000 } as const;
export type SizeName = keyof typeof SIZES;

/** A 32-bit FNV-1a hash of a string, to seed the generator. */
export function hashSeed(seed: string): number {
  let hash = 0x811c9dc5;
  for (let index = 0; index < seed.length; index++) {
    hash ^= seed.charCodeAt(index);
    hash = Math.imul(hash, 0x01000193) >>> 0;
  }
  return hash;
}

/** mulberry32: a small, fast, deterministic PRNG over 0..1. */
export function makeRandom(seed: string): () => number {
  let state = hashSeed(seed);
  return () => {
    state = (state + 0x6d2b79f5) >>> 0;
    let mixed = state;
    mixed = Math.imul(mixed ^ (mixed >>> 15), mixed | 1);
    mixed ^= mixed + Math.imul(mixed ^ (mixed >>> 7), mixed | 61);
    return ((mixed ^ (mixed >>> 14)) >>> 0) / 4294967296;
  };
}

const WORDS = [
  "pen",
  "cup",
  "lamp",
  "desk",
  "book",
  "coat",
  "shoe",
  "ring",
  "card",
  "seed",
  "tile",
  "rope",
];

const pad = (index: number): string => String(index).padStart(3, "0");
export const moduleName = (index: number): string => `m${pad(index)}`;

interface Random {
  int(low: number, high: number): number;
  pick<Value>(items: readonly Value[]): Value;
}

function wrap(next: () => number): Random {
  return {
    int: (low, high) => low + Math.floor(next() * (high - low + 1)),
    pick: (items) => items[Math.floor(next() * items.length)]!,
  };
}

/** The helper templates: one per paragraph of helpers.hd.tmpl, comments left out. */
export const helperTemplates = (): string[] =>
  fixture("package/helpers.hd.tmpl")
    .split(/\n\s*\n/)
    .map((paragraph) =>
      paragraph
        .split("\n")
        .filter((line) => !/^\s*\{\{!.*\}\}\s*$/.test(line))
        .join("\n")
        .trim(),
    )
    .filter(Boolean);

function generateModule(k: number, random: Random, deps: readonly number[]): string {
  const int = (low: string, high: string) => random.int(Number(low), Number(high));
  const word = random.pick(WORDS);
  const templates = helperTemplates();
  const helpers = Array.from({ length: random.int(6, 12) }, (_, helper) =>
    render(random.pick(templates), { k, h: helper, int }, "package/helpers.hd.tmpl"),
  );
  return renderFixture("package/module.hd.tmpl", {
    k,
    word,
    test: `t${pad(k)}`,
    deps: deps.map((j) => ({ j, jname: moduleName(j) })),
    helpers: helpers.map((text) => `${text}\n`).join("\n"),
    int,
  });
}

export interface GenerateOptions {
  readonly seed: string;
  /** The approximate line count of the package. */
  readonly lines: number;
  /** The package name in hd.toml. Default `gen`. */
  readonly name?: string;
}

export function generatePackage(options: GenerateOptions): GeneratedPackage {
  const random = wrap(makeRandom(options.seed));
  const files = new Map<string, string>();
  const modules: GeneratedModule[] = [];
  let total = 0;
  for (let k = 0; total < options.lines - 20; k++) {
    let deps: number[];
    if (k === 0) {
      deps = [];
    } else if (k >= 3 && random.int(0, 1) === 1) {
      deps = [k - 1, k - 3];
    } else {
      deps = [k - 1];
    }
    const text = generateModule(k, random, deps);
    total += text.split("\n").length;
    const file = `src/${moduleName(k)}.hd`;
    files.set(file, text);
    modules.push({
      name: moduleName(k),
      file,
      imports: deps.map(moduleName),
      testName: `t${pad(k)}a`,
    });
  }
  const lib = fixture("package/lib.hd");
  const smoke = fixture("package/smoke.hd");
  files.set("src/lib.hd", lib);
  files.set("tests/smoke.hd", smoke);
  files.set("hd.toml", manifest(options.name ?? "gen", { capabilities: ["Console"] }));
  total += lib.split("\n").length + smoke.split("\n").length + 2;
  return { files, modules, lines: total };
}

/** The dependents of module `name`: the modules that import it directly. */
export const dependentsOf = (pkg: GeneratedPackage, name: string): GeneratedModule[] =>
  pkg.modules.filter((module) => module.imports.includes(name));

/** The executable `src/main.hd` of a generated package: it prints one item's total. */
export const executable = (): string => fixture("package/main.hd");

/** An executable for a generated package that calls into every module. */
export function reachEveryModule(pkg: GeneratedPackage): string {
  return renderFixture("package/reach-all.hd.tmpl", {
    modules: pkg.modules.map((module, k) => ({ k, name: module.name, price: k + 1 })),
  });
}

// ------------------------------------------------------------- edits

function replaceOnce(text: string, pattern: RegExp, replacement: string, what: string): string {
  const next = text.replace(pattern, replacement);
  if (next === text) {
    throw new Error(`no ${what} in the module`);
  }
  return next;
}

/** Edits the private body of `tweakK`: `let bump = +N` becomes +(N+1). */
export function editBody(text: string): string {
  const next = text.replace(
    /^( {4}let bump = \+)(\d+)$/m,
    (_, head: string, number: string) => `${head}${Number(number) + 1}`,
  );
  if (next === text) {
    throw new Error("no body edit site in the module");
  }
  return next;
}

/** Edits the public signature of `shapeK`: it gains a defaulted parameter. */
export const editSignature = (text: string): string =>
  replaceOnce(
    text,
    /^(pub fn shape\d+\(x: i32)\) -> i32:$/m,
    "$1, extra: i32 = 0) -> i32:",
    "signature edit site",
  );

/** Puts a type error into the body of `tweakK`. */
export const breakBody = (text: string): string =>
  replaceOnce(text, /^ {4}x \+ bump$/m, "    x + bump + true", "body to break");

/** Takes the type error of `breakBody` out again. */
export const repairBody = (text: string): string =>
  replaceOnce(text, /^ {4}x \+ bump \+ true$/m, "    x + bump", "broken body");

/** Breaks the expectation of the module's first test case: 6 becomes 7. */
export const breakTest = (text: string): string =>
  replaceOnce(text, /(assert_equal\(total\d+\(items\), )6,/, "$17,", "test expectation to break");

/** Repairs the expectation `breakTest` broke. */
export const repairTest = (text: string): string =>
  replaceOnce(text, /(assert_equal\(total\d+\(items\), )7,/, "$16,", "broken test expectation");

/** Appends the public function `addedK_N`. */
export const addFunction = (text: string, k: number, number: number): string =>
  `${text.trimEnd()}\n\n## An added function.\npub fn added${k}_${number}(x: i32) -> i32:\n    x + ${number}\n`;

// --------------------------------------------- packages on disk

/** Writes a generated package of a named size into a fresh temporary directory. */
export function materialize(
  size: SizeName,
  seed: string,
): { readonly dir: string; readonly pkg: GeneratedPackage } {
  const pkg = generatePackage({ seed, lines: SIZES[size] });
  return { dir: writePackage(`pkg-${size}`, pkg.files), pkg };
}

/** Adds the executable `src/main.hd` to a generated package, so `hd build` has a program. */
export function addExecutable(dir: string): void {
  writeFileSync(join(dir, "src", "main.hd"), executable());
}

/** A fresh temporary copy of a package directory. */
export function copyPackage(source: string, label: string): string {
  const dir = makeTempDir(label);
  copyTree(source, dir);
  return dir;
}

/** Rewrites one file of a package directory in place. */
export function editFile(dir: string, file: string, edit: (text: string) => string): void {
  const path = join(dir, file);
  writeFileSync(path, edit(readFileSync(path, "utf8")));
}

/**
 * A small generated package whose test case `t001a` fails: its expected
 * total is off by one. Returns the directory and the failing test's filter.
 */
export function failingPackage(seed: string): { readonly dir: string; readonly filter: string } {
  const { dir, pkg } = materialize("small", seed);
  const module = pkg.modules[1]!;
  editFile(dir, module.file, breakTest);
  return { dir, filter: module.testName };
}

// ------------------------------------------------------ test packages

/** The modules of a unit-test package. */
export const UNIT_MODULES = 10;

/**
 * A package of `tests` unit tests over UNIT_MODULES modules: each module
 * has a pure function and half its tests in its `tests:` block, the other
 * half in its test module `src/mK_test.hd`.
 */
export function unitPackage(tests: number): Files {
  const files = new Map<string, string>([["hd.toml", manifest("units")]]);
  const perModule = tests / UNIT_MODULES;
  const half = Math.ceil(perModule / 2);
  for (let k = 0; k < UNIT_MODULES; k++) {
    const factor = k + 2;
    const cases = (from: number, to: number) =>
      Array.from({ length: to - from }, (_, index) => ({
        j: from + index,
        expected: (from + index) * factor + 1,
      }));
    files.set(
      `src/m${k}.hd`,
      renderFixture("unit-test-perf/module.hd.tmpl", { k, factor, cases: cases(0, half) }),
    );
    files.set(
      `src/m${k}_test.hd`,
      renderFixture("unit-test-perf/module_test.hd.tmpl", { k, cases: cases(half, perModule) }),
    );
  }
  return files;
}

/** The library of an integration-test package, with `docs` doc tests. */
const integrationLib = (docs: number): string =>
  renderFixture("integration-test-perf/lib.hd.tmpl", {
    docs: Array.from({ length: docs }, (_, doc) => ({ d: doc })),
  });

const integrationManifest = (): string =>
  manifest("shop", { devCapabilities: ["FsRead", "FsWrite"] });

/**
 * A package with `programs` integration test programs under `tests/`,
 * each with 2 cases that write a file into `temp_dir()` and read it back.
 */
export function integrationPackage(programs: number): Files {
  const files = new Map<string, string>([
    ["hd.toml", integrationManifest()],
    ["src/lib.hd", integrationLib(0)],
  ]);
  for (let program = 0; program < programs; program++) {
    files.set(
      `tests/export${program}.hd`,
      renderFixture("integration-test-perf/export.hd.tmpl", {
        p: program,
        cases: [0, 1].map((testCase) => ({ c: testCase })),
      }),
    );
  }
  return files;
}

/** A package whose library function carries `count` doc tests. */
export const docTestPackage = (count: number): Files =>
  new Map([
    ["hd.toml", integrationManifest()],
    ["src/lib.hd", integrationLib(count)],
  ]);
