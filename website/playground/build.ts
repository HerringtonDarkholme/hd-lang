// Builds the browser playground with esbuild. `npm run website:build` calls
// `buildPlayground` to emit it as the site's playground/ directory, and
// `npm run website:dev` keeps a `watchPlayground` build up to date.
//
// Every URL in the output is relative, so it works at any base path.

import { copyFile, mkdir, readdir, readFile, rm } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import * as esbuild from "esbuild";

const playground = dirname(fileURLToPath(import.meta.url));
const root = resolve(playground, "../..");
const RUNTIME_DIR = join(root, "src", "emitter", "runtime");
const STD_DIR = join(root, "src", "std");
/** Directories whose files the compiler reads with `node:fs`, by shim path. */
const EMBEDDED: Readonly<Record<string, { readonly dir: string; readonly extension: string }>> = {
  "runtime-wat": { dir: RUNTIME_DIR, extension: ".wat" },
  "std-hd": { dir: STD_DIR, extension: ".hd" },
};

/**
 * Browser replacements for the Node APIs the compiler uses: the emitter's
 * runtime reads its `.wat` files and the checker its `std` `.hd` sources with
 * `node:fs`, and `instantiate` hashes
 * function sources with `node:crypto`. Binaryen imports Node modules only
 * behind a Node check, so they stay external.
 */
export function browserShims(): esbuild.Plugin {
  return {
    name: "hd-browser-shims",
    setup(build) {
      build.onResolve({ filter: /^node:fs$/ }, (args) => {
        const embedded = Object.entries(EMBEDDED).find(
          ([, { dir }]) => resolve(args.resolveDir) === dir,
        );
        return embedded ? { path: embedded[0], namespace: "hd-shim" } : undefined;
      });
      build.onResolve({ filter: /^node:crypto$/ }, () => ({
        path: join(playground, "src", "shims", "crypto.ts"),
      }));
      build.onResolve({ filter: /^node:/ }, (args) =>
        args.importer.includes(`${join("node_modules", "binaryen")}`)
          ? { path: args.path, external: true }
          : undefined,
      );
      build.onLoad({ filter: /^(runtime-wat|std-hd)$/, namespace: "hd-shim" }, async (args) => {
        const { dir, extension } = EMBEDDED[args.path]!;
        const files: Record<string, string> = {};
        for (const name of (await readdir(dir)).filter((file) => file.endsWith(extension)))
          files[name] = await readFile(join(dir, name), "utf8");
        return {
          contents: `const files = ${JSON.stringify(files)};
export function readFileSync(url) {
  const name = String(url).split("/").pop();
  if (!(name in files)) throw new Error("no embedded file " + name);
  return files[name];
}`,
          loader: "js",
        };
      });
    },
  };
}

/** esbuild options for the page and worker bundles, written to `outDir/assets/`. */
export function buildOptions(
  overrides: esbuild.BuildOptions = {},
  outDir?: string,
): esbuild.BuildOptions {
  return {
    entryPoints: {
      main: join(playground, "src", "main.ts"),
      worker: join(playground, "src", "worker.ts"),
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
    plugins: [browserShims()],
    logLevel: "info",
    ...overrides,
  };
}

async function copyStatic(outDir: string): Promise<void> {
  await rm(outDir, { recursive: true, force: true });
  await mkdir(outDir, { recursive: true });
  await copyFile(join(playground, "index.html"), join(outDir, "index.html"));
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
        } else onRebuild();
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
