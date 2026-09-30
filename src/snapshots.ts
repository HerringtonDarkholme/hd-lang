import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { basename, dirname, join, relative, sep } from "node:path";
import type { HostFunction } from "./host-functions.ts";
import type { RegressionStore } from "./property-tests.ts";

// The runner side of `std.testing.snapshot_file` (spec/10-modules.md#snapshots,
// Testing T53). The file is `<package root>/__snapshots__/<module>/<test-slug>-<n>.snap`;
// the slug, counter, and table-row rules are in
// future-work/RUNTIME_AND_LIBRARY.md#snapshot-tests. A missing file fails the
// test case, except in an update run, which writes it.

export interface SnapshotRun {
  /** Starts a test case, or one `it_each` row of it. */
  readonly begin: (name: string, row: number | undefined) => void;
  readonly hostFunctions: Readonly<Record<string, HostFunction>>;
}

/** The test name, lowercased, with each run of other characters turned into `-`. */
export function testSlug(name: string): string {
  return name.toLowerCase().replace(/[^a-z0-9]+/g, "-");
}

/** The package root (the nearest directory with `hd.toml`) and the file's module name. */
export function snapshotModule(file: string): { readonly root: string; readonly module: string } {
  let directory = dirname(file);
  for (;;) {
    if (existsSync(join(directory, "hd.toml"))) break;
    const parent = dirname(directory);
    if (parent === directory) return { root: dirname(file), module: basename(file, ".hd") };
    directory = parent;
  }
  const path = relative(directory, file).replace(/\.hd$/, "").split(sep);
  const parts = path[0] === "src" ? path.slice(1) : path;
  if (parts.at(-1) === "mod") parts.pop();
  return { root: directory, module: parts.join(".") };
}

export function snapshotRun(file: string, update: boolean): SnapshotRun {
  const { root, module } = snapshotModule(file);
  let slug = "";
  let count = 0;
  return {
    begin(name, row) {
      slug = `${testSlug(name)}${row === undefined ? "" : `.${row}`}`;
      count = 0;
    },
    hostFunctions: {
      snapshot_file_check(text) {
        count += 1;
        const path = join(root, "__snapshots__", module, `${slug}-${count}.snap`);
        const actual = String(text);
        if (existsSync(path) && readFileSync(path, "utf8") === actual) return "";
        if (update) {
          mkdirSync(dirname(path), { recursive: true });
          writeFileSync(path, actual);
          return "";
        }
        return existsSync(path)
          ? `snapshot_file: the text differs from ${relative(root, path)}; run hd test --update to accept it`
          : `snapshot_file: ${relative(root, path)} is missing; run hd test --update to record it`;
      },
    },
  };
}

// The property-test regression files (spec/std/testing.md#r-std-testing.prop.regression-file,
// Testing T37): a failing property's shrunk choice stream, one decimal draw
// per line, which the runner replays before new cases on the next run.

/** The streams under `<root>/__regressions__/<module>/<test-slug>` (T37). */
export function regressionStore(root: string, module: string): RegressionStore {
  const path = (name: string): string => join(root, "__regressions__", module, testSlug(name));
  return {
    load(name) {
      const file = path(name);
      if (!existsSync(file)) return undefined;
      return readFileSync(file, "utf8")
        .split("\n")
        .map((line) => line.trim())
        .filter((line) => /^\d+$/.test(line))
        .map((line) => BigInt(line));
    },
    save(name, stream) {
      const file = path(name);
      mkdirSync(dirname(file), { recursive: true });
      writeFileSync(file, stream.map((draw) => `${draw}\n`).join(""));
      return relative(root, file);
    },
  };
}
