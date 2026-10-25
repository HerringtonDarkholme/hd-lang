// Repository maintenance only. Installed deploy tasks consume the snapshot.
import { readFile, mkdir, open, rename, rm } from "node:fs/promises";
import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { digest, graphFingerprint, runtimeSources } from "./runtime-model.mjs";

const root = new URL("../../../", import.meta.url);
const target = new URL("../runtime/runtime-sources.json", import.meta.url);
const files = {};
const sources = {};
for (const name of Object.keys(runtimeSources).sort()) {
  const text = (await readFile(new URL(runtimeSources[name], root), "utf8")).replaceAll(
    "\r\n",
    "\n",
  );
  files[name] = { source: text, sha256: digest(text) };
  sources[name] = text;
}
const schema = await readFile(new URL("crates/hd_host_abi/src/schema.rs", root), "utf8");
const version = schema.match(/^pub const VERSION: u32 = (\d+);$/m);
if (!version) throw new Error("cannot find the package-host ABI version");
const baseRevision = execFileSync("git", ["rev-parse", "HEAD"], {
  cwd: fileURLToPath(root),
  encoding: "utf8",
}).trim();
const packageHostABI = Number(version[1]);
const snapshot = {
  format: 1,
  baseRevision,
  packageHostABI,
  fingerprint: graphFingerprint(sources, packageHostABI, baseRevision),
  files,
};
await mkdir(new URL("../runtime/", import.meta.url), { recursive: true });
const temporary = new URL(`runtime-sources.${process.pid}.tmp`, target);
const handle = await open(temporary, "wx");
try {
  await handle.writeFile(JSON.stringify(snapshot, null, 2) + "\n", "utf8");
  await handle.close();
  await rename(temporary, target);
} finally {
  await handle.close();
  await rm(temporary, { force: true });
}
console.log(`SDK portable runtime: ${Object.keys(files).length} files, ${snapshot.fingerprint}`);
