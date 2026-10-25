import { readFile, mkdir, writeFile, rm } from "node:fs/promises";
import { resolve, join } from "node:path";
import { pathToFileURL } from "node:url";
import { digest, graphFingerprint, runtimeSources } from "./runtime-model.mjs";

const archive = new URL("../runtime/runtime-sources.json", import.meta.url);
const fail = (message) => new Error(`SDK portable runtime: ${message}`);
const hash = /^[0-9a-f]{64}$/;

/** Validate the SDK-owned snapshot before any output or guest code runs. */
export async function readPortableRuntime(source = archive) {
  let snapshot;
  try {
    snapshot = JSON.parse(await readFile(source, "utf8"));
  } catch {
    throw fail("cannot read snapshot JSON");
  }
  if (
    snapshot?.format !== 1 ||
    !Number.isInteger(snapshot.packageHostABI) ||
    snapshot.packageHostABI < 1 ||
    snapshot.packageHostABI > 0xffffffff ||
    typeof snapshot.baseRevision !== "string" ||
    !/^[0-9a-f]{40,64}$/.test(snapshot.baseRevision)
  )
    throw fail("unsupported snapshot metadata");
  const names = Object.keys(runtimeSources).sort();
  if (
    !snapshot.files ||
    typeof snapshot.files !== "object" ||
    Array.isArray(snapshot.files) ||
    JSON.stringify(Object.keys(snapshot.files).sort()) !== JSON.stringify(names)
  )
    throw fail("unexpected snapshot file list");
  const files = {};
  for (const name of names) {
    const file = snapshot.files[name];
    if (
      typeof file?.source !== "string" ||
      typeof file.sha256 !== "string" ||
      !hash.test(file.sha256) ||
      digest(file.source) !== file.sha256
    )
      throw fail(`snapshot checksum mismatch: ${name}`);
    files[name] = file.source;
  }
  if (
    typeof snapshot.fingerprint !== "string" ||
    graphFingerprint(files, snapshot.packageHostABI, snapshot.baseRevision) !== snapshot.fingerprint
  )
    throw fail("snapshot graph fingerprint mismatch");
  return Object.freeze({
    files: Object.freeze(files),
    fingerprint: snapshot.fingerprint,
    packageHostABI: snapshot.packageHostABI,
    baseRevision: snapshot.baseRevision,
  });
}

/** Emit only into a fresh task-owned directory; existing output is untouched. */
export async function emitPortableRuntime(directory, source = archive) {
  const snapshot = await readPortableRuntime(source);
  const destination = resolve(directory);
  await mkdir(destination);
  try {
    await mkdir(join(destination, "gen"));
    for (const [name, text] of Object.entries(snapshot.files))
      await writeFile(join(destination, name), text, { encoding: "utf8", flag: "wx" });
  } catch (error) {
    await rm(destination, { recursive: true, force: true });
    throw error;
  }
  return Object.freeze({
    directory: destination,
    fingerprint: snapshot.fingerprint,
    packageHostABI: snapshot.packageHostABI,
    baseRevision: snapshot.baseRevision,
    coreURL: pathToFileURL(join(destination, "core.mjs")).href,
    packageProvidersURL: pathToFileURL(join(destination, "package-providers.mjs")).href,
  });
}
