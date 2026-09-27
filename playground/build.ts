// Builds the browser playground into playground/dist/ with esbuild.
//
//   node --experimental-strip-types playground/build.ts          # production build
//   node --experimental-strip-types playground/build.ts --serve  # rebuild + serve
//
// Every URL in the output is relative, so dist/ works at any base path.

import { copyFile, mkdir, readdir, readFile, rm } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import * as esbuild from "esbuild";

const playground = dirname(fileURLToPath(import.meta.url));
const root = resolve(playground, "..");
export const DIST = join(playground, "dist");
const RUNTIME_DIR = join(root, "src", "emitter", "runtime");

/**
 * Browser replacements for the Node APIs the compiler uses: the emitter's
 * runtime reads its `.wat` files with `node:fs`, and `instantiate` hashes
 * function sources with `node:crypto`. Binaryen imports Node modules only
 * behind a Node check, so they stay external.
 */
export function browserShims(): esbuild.Plugin {
  return {
    name: "hd-browser-shims",
    setup(build) {
      build.onResolve({ filter: /^node:fs$/ }, (args) =>
        resolve(args.resolveDir) === RUNTIME_DIR
          ? { path: "runtime-wat", namespace: "hd-shim" }
          : undefined,
      );
      build.onResolve({ filter: /^node:crypto$/ }, () => ({
        path: join(playground, "src", "shims", "crypto.ts"),
      }));
      build.onResolve({ filter: /^node:/ }, (args) =>
        args.importer.includes(`${join("node_modules", "binaryen")}`)
          ? { path: args.path, external: true }
          : undefined,
      );
      build.onLoad({ filter: /^runtime-wat$/, namespace: "hd-shim" }, async () => {
        const files: Record<string, string> = {};
        for (const name of (await readdir(RUNTIME_DIR)).filter((file) => file.endsWith(".wat")))
          files[name] = await readFile(join(RUNTIME_DIR, name), "utf8");
        return {
          contents: `const files = ${JSON.stringify(files)};
export function readFileSync(url) {
  const name = String(url).split("/").pop();
  if (!(name in files)) throw new Error("no embedded runtime file " + name);
  return files[name];
}`,
          loader: "js",
        };
      });
    },
  };
}

export function buildOptions(overrides: esbuild.BuildOptions = {}): esbuild.BuildOptions {
  return {
    entryPoints: {
      main: join(playground, "src", "main.ts"),
      worker: join(playground, "src", "worker.ts"),
    },
    outdir: join(DIST, "assets"),
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

async function copyStatic(): Promise<void> {
  await mkdir(DIST, { recursive: true });
  await copyFile(join(playground, "index.html"), join(DIST, "index.html"));
}

async function main(): Promise<void> {
  const serve = process.argv.includes("--serve");
  await rm(DIST, { recursive: true, force: true });
  await copyStatic();
  if (!serve) {
    await esbuild.build(buildOptions());
    return;
  }
  const context = await esbuild.context(buildOptions({ minify: false, sourcemap: true }));
  await context.watch();
  const { hosts, port } = await context.serve({ servedir: DIST, port: 8000 });
  console.log(`playground at http://${hosts[0] ?? "localhost"}:${port}/`);
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) await main();
