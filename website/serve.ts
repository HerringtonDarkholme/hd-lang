import { watch } from "node:fs";
import { readFile, stat } from "node:fs/promises";
import { createServer } from "node:http";
import { dirname, extname, join, normalize, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { parseArgs } from "node:util";

import { buildSite } from "./build.ts";
import { watchPlayground } from "./playground/build.ts";

// Local preview: builds the site for base `/`, serves it, and rebuilds when a
// Markdown source, a site asset, or a playground source changes. The
// playground is an unminified build with source maps, kept up to date in
// website/playground/dist/ and copied into the site. Changes to the build
// code itself need a restart.

const WEBSITE_DIR = dirname(fileURLToPath(import.meta.url));
const REPO_DIR = resolve(WEBSITE_DIR, "..");
const PLAYGROUND_DIST = join(WEBSITE_DIR, "playground", "dist");

const TYPES: Readonly<Record<string, string>> = {
  ".html": "text/html; charset=utf-8",
  ".css": "text/css; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".mjs": "text/javascript; charset=utf-8",
  ".json": "application/json",
  ".svg": "image/svg+xml",
  ".wasm": "application/wasm",
  ".png": "image/png",
  ".ico": "image/x-icon",
  ".map": "application/json",
  ".txt": "text/plain; charset=utf-8",
};

async function fileFor(root: string, urlPath: string): Promise<string | null> {
  const relative = normalize(decodeURIComponent(urlPath)).replace(/^(\.\.[/\\])+/, "");
  let path = join(root, relative);
  if (!path.startsWith(root)) return null;
  try {
    if ((await stat(path)).isDirectory()) path = join(path, "index.html");
    await stat(path);
    return path;
  } catch {
    return null;
  }
}

async function rebuild(outDir: string): Promise<void> {
  const started = Date.now();
  try {
    const result = await buildSite({ base: "/", outDir, playgroundDist: PLAYGROUND_DIST });
    console.log(`website: built ${result.pages} pages in ${Date.now() - started} ms`);
  } catch (error) {
    console.error(error instanceof Error ? error.message : error);
  }
}

async function main(): Promise<void> {
  const { values } = parseArgs({
    options: {
      port: { type: "string", default: process.env.PORT ?? "4173" },
      out: { type: "string", default: join(WEBSITE_DIR, "dist") },
    },
  });
  const outDir = resolve(values.out);

  let timer: ReturnType<typeof setTimeout> | undefined;
  const schedule = (): void => {
    clearTimeout(timer);
    timer = setTimeout(() => void rebuild(outDir), 150);
  };
  await watchPlayground(PLAYGROUND_DIST, schedule);
  await rebuild(outDir);
  for (const directory of ["spec", "guide", "future-work", "website/assets"])
    try {
      watch(join(REPO_DIR, directory), { recursive: true }, schedule);
    } catch {
      // A directory that does not exist yet is simply not watched.
    }
  watch(join(REPO_DIR, "README.md"), schedule);

  const server = createServer(async (request, response) => {
    const url = new URL(request.url ?? "/", "http://localhost");
    const file = await fileFor(outDir, url.pathname);
    const status = file ? 200 : 404;
    const served = file ?? join(outDir, "404.html");
    try {
      const body = await readFile(served);
      response.writeHead(status, {
        "content-type": TYPES[extname(served)] ?? "application/octet-stream",
        "cache-control": "no-store",
      });
      response.end(body);
    } catch {
      response.writeHead(404).end("not found");
    }
  });
  server.listen(Number(values.port), () => {
    console.log(`website: serving ${outDir} at http://localhost:${values.port}/`);
  });
}

await main();
