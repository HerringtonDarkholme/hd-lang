// Builds the playground compiler and its optional WAT printer:
// `cargo rustc -p hd_web/hd_wat --lib --target wasm32-unknown-unknown --profile
// web`, copied to website/playground/assets/hd_{web,wat}.wasm, which the
// playground build copies next to its worker. The output is not committed.
// Prints the raw and gzip sizes. CARGO_TARGET_DIR is honored.
//
// `cargo rustc` hands the final link `--compress-relocations`: wasm-ld
// otherwise leaves every call and data address as a padded 5-byte LEB,
// and compacting them cut the gzip download by 28 KB (#49). The flag goes
// to each final module alone, so the dependencies' builds stay shared with
// `cargo build`; it cannot go in a profile or a target's rustflags, since
// the debug builds of wasm32 carry debug info, which the flag rejects.
//
// The compiler links a 2 MiB linear-memory stack (`-zstack-size`), up from
// the default 1 MiB: at 1 MiB, `f()()...` under nested blocks ran out of
// it near 620 links, below the browser build's 500-link chain limit
// (`MAX_CHAIN_LINKS`, grammar.limit.chain) with too little margin. At 2 MiB
// the engine's own stack fails first on every shape. Cost: one more MiB of
// initial memory per compiler instance; none at run time.
//
// No wasm-opt pass: -Oz made the gzip download larger (it optimizes raw
// size), and the cheap passes that do help (-O1 and duplicate function
// elimination, 56 KB) need a binaryen the repository does not declare.
//
// A Node script rather than a shell one: it measures gzip with node:zlib on
// every platform, and the smoke test (test-web-compiler.ts) imports its
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
/** The compiler wasm the playground serves. */
export const COMPILER_WASM = join(root, "website", "playground", "assets", "hd_web.wasm");
export const WAT_WASM = join(root, "website", "playground", "assets", "hd_wat.wasm");

/** The compiler's linear-memory stack, in bytes (see the header). */
const COMPILER_STACK = 2 * 1024 * 1024;

/** Raw and gzip (level 9) byte counts of a file. */
export function sizes(path: string): { raw: number; gzip: number } {
  const bytes = readFileSync(path);
  return { raw: bytes.length, gzip: gzipSync(bytes, { level: 9 }).length };
}

/** `1234567` as `1234567 B (1.23 MB)`. */
export function formatBytes(bytes: number): string {
  return `${bytes} B (${(bytes / 1e6).toFixed(2)} MB)`;
}

function report(label: string, { raw, gzip }: { raw: number; gzip: number }): void {
  console.log(`${label}: raw ${formatBytes(raw)}, gzip ${formatBytes(gzip)}`);
}

function build() {
  for (const crate of ["hd_web", "hd_wat"]) {
    // cost: two fixed crates, one cargo process each, once per build.
    execFileSync(
      "cargo",
      [
        "rustc",
        "-p",
        crate,
        "--lib",
        "--target",
        "wasm32-unknown-unknown",
        "--profile",
        "web",
        "--",
        "-C",
        "link-arg=--compress-relocations",
        ...(crate === "hd_web" ? ["-C", `link-arg=-zstack-size=${COMPILER_STACK}`] : []),
      ],
      { cwd: root, stdio: "inherit" },
    );
    mkdirSync(dirname(COMPILER_WASM), { recursive: true });
    const output = crate === "hd_web" ? COMPILER_WASM : WAT_WASM;
    copyFileSync(join(TARGET_DIR, "wasm32-unknown-unknown", "web", `${crate}.wasm`), output);
    report(`${crate}.wasm`, sizes(output));
  }
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  build();
}
