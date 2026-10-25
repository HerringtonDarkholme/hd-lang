// Run from the repository root. Generated bundles stay in the supplied scratch directory.
import { build } from "esbuild";
import { readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { resolve, join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const output = resolve(process.argv[2]);
mkdirSync(output, { recursive: true });
const result = await build({
  entryPoints: ["host/core.mjs"],
  outfile: join(output, "core.js"),
  bundle: true,
  format: "iife",
  globalName: "HDCore",
  platform: "browser",
  metafile: true,
});
writeFileSync(join(output, "core-metafile.json"), JSON.stringify(result.metafile, null, 2));
writeFileSync(
  join(output, "hd-run.js"),
  ["shim.js", join(output, "core.js"), "run.js"]
    .map((file, index) => readFileSync(index === 1 ? file : join(here, file), "utf8"))
    .join("\n"),
);
