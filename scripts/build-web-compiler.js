// Builds the playground's compiler wasm (crates/hd_web/DESIGN.md §7, W3):
// `cargo rustc -p hd_web --lib --target wasm32-unknown-unknown --profile
// web`, copied to website/playground/assets/hd_web.wasm, which the
// playground build copies next to its worker. The output is not committed.
// Prints the raw and gzip sizes. CARGO_TARGET_DIR is honored.
//
// `cargo rustc` hands the final link `--compress-relocations`: wasm-ld
// otherwise leaves every call and data address as a padded 5-byte LEB,
// and compacting them cut the gzip download by 28 KB (#49). The flag goes
// to hd_web alone, so the dependencies' builds stay shared with
// `cargo build`; it cannot go in a profile or a target's rustflags, since
// the debug builds of wasm32 carry debug info, which the flag rejects.
//
// No wasm-opt pass: -Oz made the gzip download larger (it optimizes raw
// size), and the cheap passes that do help (-O1 and duplicate function
// elimination, 56 KB) need a binaryen the repository does not declare.
//
// A Node script rather than a shell one: it measures gzip with node:zlib on
// every platform, and the smoke test (test-web-compiler.js) imports its
// paths.

import { execFileSync } from "node:child_process";
import { copyFileSync, mkdirSync, readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { gzipSync } from "node:zlib";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const TARGET_DIR = process.env.CARGO_TARGET_DIR
  ? resolve(root, process.env.CARGO_TARGET_DIR)
  : join(root, "target");
const CARGO_OUTPUT = join(TARGET_DIR, "wasm32-unknown-unknown", "web", "hd_web.wasm");
/** The compiler wasm the playground serves. */
export const COMPILER_WASM = join(root, "website", "playground", "assets", "hd_web.wasm");

/** Raw and gzip (level 9) byte counts of a file. */
export function sizes(path) {
  const bytes = readFileSync(path);
  return { raw: bytes.length, gzip: gzipSync(bytes, { level: 9 }).length };
}

/** `1234567` as `1234567 B (1.23 MB)`. */
export function formatBytes(n) {
  return `${n} B (${(n / 1e6).toFixed(2)} MB)`;
}

function report(label, { raw, gzip }) {
  console.log(`${label}: raw ${formatBytes(raw)}, gzip ${formatBytes(gzip)}`);
}

function build() {
  execFileSync(
    "cargo",
    [
      "rustc",
      "-p",
      "hd_web",
      "--lib",
      "--target",
      "wasm32-unknown-unknown",
      "--profile",
      "web",
      "--",
      "-C",
      "link-arg=--compress-relocations",
    ],
    { cwd: root, stdio: "inherit" },
  );
  mkdirSync(dirname(COMPILER_WASM), { recursive: true });
  copyFileSync(CARGO_OUTPUT, COMPILER_WASM);
  report("hd_web.wasm", sizes(COMPILER_WASM));
}

if (process.argv[1] === fileURLToPath(import.meta.url)) build();
