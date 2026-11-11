// Writes the Node host that `hd` embeds as plain JavaScript, so no
// `hd run` or `hd test` pays Node's type stripping (#1697). hd_cli's build
// script runs it once per change to the host.
//
// It walks the module graph from the entry points `hd` starts. Each
// module is emitted by `emitHostJavaScript`: its types are erased in
// place, so a host stack trace's line and column still point into the
// `.ts` source, and its `./x.ts` references become `./x.js`. A reference
// that only a type uses is erased with it, so the graph holds exactly the
// modules a run can load. A `package.json` marks the `.js` files as ES
// modules.
//
// Usage: node scripts/ship-host.ts OUT
// Writes OUT/<module>.js and OUT/package.json, and prints each written
// path relative to OUT, then each source read relative to the repository,
// as `out PATH` and `src PATH` lines.
import { existsSync } from "node:fs";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { emitHostJavaScript } from "./host-emission.ts";

const repository = fileURLToPath(new URL("../", import.meta.url));
const host = join(repository, "host");

/** The modules `hd` starts: `hd run`, `hd test`, and the REPL's session. */
const entries = ["run.ts", "test.ts", "node.ts"];

/**
 * Modules a run can name but `hd` never loads, with the reason. A reference
 * to any other missing module fails the build.
 */
const notShipped: Record<string, string> = {
  // `run.ts` imports it only for a configuration's `profile`, which only
  // the conformance runner writes.
  "profiles.ts": "conformance runtime profiles",
};

/** A relative reference to an emitted module: `"./x.js"`, `'../x.js?q'`. */
const emittedReference = /(["'])(\.\.?\/[^"'\r\n]*?\.js)(?:[?#][^"'\r\n]*)?\1/g;

const out = process.argv[2];
if (!out) {
  throw new Error("usage: node scripts/ship-host.ts OUT");
}
const shipped = new Map<string, string>();
const pending = [...entries];
while (pending.length > 0) {
  const name = pending.pop()!;
  if (shipped.has(name)) {
    continue;
  }
  const source = join(host, name);
  const javascript = emitHostJavaScript(
    (await readFile(source, "utf8")).replaceAll("\r\n", "\n"),
    name,
  );
  shipped.set(name, javascript);
  for (const [, , reference] of javascript.matchAll(emittedReference)) {
    const target = relative(host, resolve(dirname(source), reference)).replaceAll("\\", "/");
    const typed = target.replace(/\.js$/, ".ts");
    if (target.startsWith("../")) {
      throw new Error(`host/${name} names ${reference}, outside host/`);
    }
    if (typed in notShipped) {
      continue;
    }
    if (!existsSync(join(host, typed))) {
      throw new Error(`host/${name} names ${reference}, but host/${typed} does not exist`);
    }
    pending.push(typed);
  }
}
const lines: string[] = [];
for (const [name, javascript] of [...shipped].sort(([left], [right]) => (left < right ? -1 : 1))) {
  const path = join(out, name.replace(/\.ts$/, ".js"));
  await mkdir(dirname(path), { recursive: true });
  await writeFile(path, javascript);
  lines.push(`out ${name.replace(/\.ts$/, ".js")}`, `src host/${name}`);
}
await writeFile(join(out, "package.json"), '{ "type": "module" }\n');
lines.push("out package.json");
for (const helper of [
  "scripts/ship-host.ts",
  "scripts/host-emission.ts",
  "packages/server-sdk/packaging/typescript-emission.ts",
]) {
  lines.push(`src ${helper}`);
}
process.stdout.write(lines.join("\n") + "\n");
