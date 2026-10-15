// Builds the playground's compiler wasm (crates/hd_web/DESIGN.md §7, W3):
// `cargo build -p hd_web --target wasm32-unknown-unknown --profile web`,
// copied to website/playground/assets/hd_web.wasm, which the playground
// build copies next to its worker. The output is not committed. Prints the
// raw and gzip sizes.
//
// No wasm-opt pass: through binaryen.js it took 90-120 s and cut 0.6 MB raw
// but made the gzip download 9 KB larger, and gzip is what the page fetches.
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
const CARGO_OUTPUT = join(root, "target", "wasm32-unknown-unknown", "web", "hd_web.wasm");
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
    ["build", "-p", "hd_web", "--target", "wasm32-unknown-unknown", "--profile", "web"],
    { cwd: root, stdio: "inherit" },
  );
  mkdirSync(dirname(COMPILER_WASM), { recursive: true });
  copyFileSync(CARGO_OUTPUT, COMPILER_WASM);
  report("hd_web.wasm", sizes(COMPILER_WASM));
}

if (process.argv[1] === fileURLToPath(import.meta.url)) build();
