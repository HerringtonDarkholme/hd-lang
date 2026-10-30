// Builds the browser playground with esbuild. `pnpm run website:build` calls
// `buildPlayground` to emit it as the site's playground/ directory, and
// `pnpm run website:dev` keeps a `watchPlayground` build up to date.
//
// Every URL in the output is relative, so it works at any base path.

import { existsSync } from "node:fs";
import { copyFile, mkdir, rm } from "node:fs/promises";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import * as esbuild from "esbuild";

const playground = dirname(fileURLToPath(import.meta.url));

/** esbuild options for the page and worker bundles, written to `outDir/assets/`. */
export function buildOptions(
  overrides: esbuild.BuildOptions = {},
  outDir?: string,
): esbuild.BuildOptions {
  return {
    entryPoints: {
      main: join(playground, "src", "main.ts"),
      worker: join(playground, "src", "worker.ts"),
      // Runs what the compiler worker builds (#254).
      "run-worker": join(playground, "src", "run-worker.ts"),
    },
    outdir: outDir === undefined ? undefined : join(outDir, "assets"),
    bundle: true,
    format: "esm",
    platform: "browser",
    target: ["es2022", "chrome119", "firefox120", "safari18"],
    minify: true,
    sourcemap: false,
    legalComments: "linked",
    loader: { ".hd": "text" },
    logLevel: "info",
    ...overrides,
  };
}

/** The compiler wasm `pnpm run build:web-compiler` writes; Run and Check need it. */
export const COMPILER_WASM = join(playground, "assets", "hd_web.wasm");
export const WAT_WASM = join(playground, "assets", "hd_wat.wasm");

async function copyStatic(outDir: string): Promise<void> {
  for (const path of [COMPILER_WASM, WAT_WASM]) {
    if (!existsSync(path)) {
      throw new Error(
        `${path} is missing: run \`pnpm run build:web-compiler\` before building the playground`,
      );
    }
  }
  await rm(outDir, { recursive: true, force: true });
  await mkdir(join(outDir, "assets"), { recursive: true });
  await copyFile(join(playground, "index.html"), join(outDir, "index.html"));
  // Next to worker.js, which loads it relative to its own URL.
  await copyFile(COMPILER_WASM, join(outDir, "assets", "hd_web.wasm"));
  await copyFile(WAT_WASM, join(outDir, "assets", "hd_wat.wasm"));
}

/** Writes a production build of the playground to `outDir`: index.html and assets/. */
export async function buildPlayground(outDir: string): Promise<void> {
  await copyStatic(outDir);
  await esbuild.build(buildOptions({ logLevel: "warning" }, outDir));
}

/**
 * Writes an unminified build with source maps to `outDir`, then rebuilds it
 * whenever a source changes and calls `onRebuild`. Resolves after the first
 * build.
 */
export async function watchPlayground(
  outDir: string,
  onRebuild: () => void,
): Promise<esbuild.BuildContext> {
  await copyStatic(outDir);
  let first: (() => void) | undefined;
  const built = new Promise<void>((resolve) => (first = resolve));
  const notify: esbuild.Plugin = {
    name: "hd-playground-rebuilt",
    setup(build) {
      build.onEnd(() => {
        if (first) {
          first();
          first = undefined;
        } else {
          onRebuild();
        }
      });
    },
  };
  const options = buildOptions({ minify: false, sourcemap: true, logLevel: "warning" }, outDir);
  const context = await esbuild.context({
    ...options,
    plugins: [...(options.plugins ?? []), notify],
  });
  await context.watch();
  await built;
  return context;
}
