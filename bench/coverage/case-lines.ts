import { dirname, basename, join } from "node:path";
import { classifyRust } from "./rust-lines.ts";

const core = new Set([
  "hd_syntax",
  "hd_resolve",
  "hd_types",
  "hd_check",
  "hd_tir",
  "hd_mono",
  "hd_opt",
  "hd_layout",
  "hd_wasm",
]);

function records(lcov: string): Map<string, Map<number, number>> {
  const files = new Map<string, Map<number, number>>();
  let counts: Map<number, number> | undefined;
  for (const line of lcov.split("\n")) {
    if (line.startsWith("SF:")) {
      const match = /(?:^|\/)(crates\/([^/]+)\/src\/[^\n]+\.rs)$/.exec(line.slice(3));
      counts = undefined;
      if (match && core.has(match[2])) {
        counts = files.get(match[1]) ?? new Map();
        files.set(match[1], counts);
      }
    } else if (counts && line.startsWith("DA:")) {
      const [at, hits] = line.slice(3).split(",").map(Number);
      counts.set(at, Math.max(counts.get(at) ?? 0, hits));
    }
  }
  return files;
}

/** One exact binary's ordinary source-line dictionary, built once, never per case. */
export function caseLineIndex(lcov: string, read: (file: string) => string) {
  const files = records(lcov);
  const classified = new Map(
    [...files.keys()].map((file) => [file, classifyRust(read(file), file)]),
  );
  const testDirs = new Set<string>();
  for (const [file, lines] of classified) {
    const stem = file.slice(0, -3);
    if (lines.wholeFileTest) {
      testDirs.add(stem);
    }
    const folder = ["lib.rs", "main.rs", "mod.rs"].includes(basename(file)) ? dirname(file) : stem;
    for (const name of lines.testModules) {
      testDirs.add(join(folder, name));
    }
  }
  const lines: string[] = [];
  const ids = new Map<string, number>();
  for (const [file, counts] of files) {
    const stem = file.endsWith("/mod.rs") ? dirname(file) : file.slice(0, -3);
    let test = testDirs.has(stem);
    // Check path prefixes, not every test module for every source file.
    for (let slash = stem.indexOf("/"); !test && slash >= 0; slash = stem.indexOf("/", slash + 1)) {
      test = testDirs.has(stem.slice(0, slash));
    }
    if (test) {
      continue;
    }
    for (const at of counts.keys()) {
      if (classified.get(file)?.kinds.get(at) !== "code") {
        continue;
      }
      const key = `${file}:${at}`;
      ids.set(key, lines.length);
      lines.push(key);
    }
  }
  return {
    lines,
    // Cost: one scan of this case's LCOV; no source reread or history scan.
    covered(text: string): number[] {
      const covered: number[] = [];
      for (const [file, counts] of records(text)) {
        for (const [at, hits] of counts) {
          const id = ids.get(`${file}:${at}`);
          if (id !== undefined && hits > 0) {
            covered.push(id);
          }
        }
      }
      return covered.sort((left, right) => left - right);
    },
  };
}
