// Builds the publishable package into dist/: bin/ and src/ compiled from
// TypeScript to JavaScript, plus the files the compiler reads at run time.
// The layout under dist/ mirrors the repository, so every
// `import.meta.url`-relative path (lib/std, runtime *.wat, spec/) still
// resolves. The repository itself keeps running the TypeScript directly.
import { chmod, cp, mkdir, readdir, readFile, rm, writeFile } from "node:fs/promises";
import { dirname, join, relative, resolve } from "node:path";
import { transform } from "esbuild";

const root = resolve(import.meta.dirname, "..");
const dist = join(root, "dist");

async function* walk(dir) {
  for (const entry of await readdir(dir, { withFileTypes: true })) {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) yield* walk(path);
    else yield path;
  }
}

// `./x.ts` and `../x.ts` specifiers become `.js`; no other string ends so.
function rewriteSpecifiers(code) {
  return code.replace(/(["'])(\.{1,2}\/[^"'\n]*)\.ts\1/g, "$1$2.js$1");
}

async function compile(file, out) {
  const source = await readFile(file, "utf8");
  const { code } = await transform(source, {
    loader: file.endsWith(".ts") ? "ts" : "js",
    format: "esm",
    target: "node24",
    sourcefile: file,
  });
  await mkdir(dirname(out), { recursive: true });
  await writeFile(out, rewriteSpecifiers(code));
}

// Copies the files under `dir` that pass `keep`, at the same path in dist/.
async function copyFiles(dir, keep) {
  for await (const file of walk(join(root, dir))) {
    if (!keep(file)) continue;
    const out = join(dist, relative(root, file));
    await mkdir(dirname(out), { recursive: true });
    await cp(file, out);
  }
}

await rm(dist, { recursive: true, force: true });

for (const dir of ["bin", "src"]) {
  for await (const file of walk(join(root, dir))) {
    if (file.endsWith(".ts") || file.endsWith(".js")) {
      await compile(file, join(dist, relative(root, file).replace(/\.ts$/, ".js")));
    }
  }
}
await copyFiles("src/emitter/runtime", (f) => f.endsWith(".wat"));
await copyFiles("lib/std", () => true);
await copyFiles("spec/lang", (f) => f.endsWith(".md"));
await copyFiles("spec/std", (f) => f.endsWith(".md"));
await mkdir(join(dist, "spec"), { recursive: true });
await cp(join(root, "spec/README.md"), join(dist, "spec/README.md"));
await chmod(join(dist, "bin/hd.js"), 0o755);
