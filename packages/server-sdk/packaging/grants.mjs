import { readFile } from "node:fs/promises";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { parse } from "./vendor/smol-toml/parse.js";

const unscoped = new Set(["Console", "ConsoleInput", "Args", "Clock", "Random"]);
const scoped = new Set(["Env", "FsRead", "FsWrite", "Process", "Http", "Net", "Sys"]);
const identifier = /^[_\p{XID_Start}][_\p{XID_Continue}]*$/u;
const utf8 = new TextDecoder("utf-8", { fatal: true });
const invalid = (message) => new Error(`hd.toml: ${message}`);

function packageSelector(key) {
  const parts = key.split(".");
  return (
    ((parts[0] === "pkg" && parts.length >= 2) || (parts[0] === "dep" && parts.length >= 3)) &&
    parts.every((part) => part !== "_" && part.normalize("NFC") === part && identifier.test(part))
  );
}

/** Read only production entries. Unlisted capabilities stay unlisted. */
export function parseProductionGrants(text) {
  let manifest;
  try {
    manifest = parse(text, { integersAsBigInt: true });
  } catch {
    // Parser errors can include source excerpts; never echo manifest values.
    throw invalid("invalid TOML");
  }
  if (!Object.hasOwn(manifest, "capabilities")) {
    return Object.freeze({});
  }
  const table = manifest.capabilities;
  if (
    table === null ||
    typeof table !== "object" ||
    Array.isArray(table) ||
    ![null, Object.prototype].includes(Object.getPrototypeOf(table))
  ) {
    throw invalid("`capabilities` must be a table");
  }
  const entries = Object.entries(table).sort(([a], [b]) => a.localeCompare(b));
  for (const entry of entries) {
    const [key, value] = entry;
    const builtin = unscoped.has(key) || scoped.has(key);
    const selector = !builtin && packageSelector(key);
    if (!builtin && !selector) {
      throw invalid(`unknown capability ${JSON.stringify(key)}`);
    }
    if (typeof value === "boolean") {
      continue;
    }
    if (!Array.isArray(value) || !value.every((scope) => typeof scope === "string")) {
      throw invalid(`capability ${JSON.stringify(key)} needs true, false or a string list`);
    }
    if (selector || unscoped.has(key)) {
      throw invalid(`capability ${JSON.stringify(key)} takes no scope list`);
    }
    entry[1] = Object.freeze([...value]);
  }
  return Object.freeze(Object.fromEntries(entries));
}

/** The caller selects the root package; this never searches ancestors. */
export async function readProductionGrants(rootDirectory) {
  const bytes = await readFile(resolve(rootDirectory, "hd.toml"));
  let text;
  try {
    text = utf8.decode(bytes);
  } catch {
    throw invalid("invalid UTF-8");
  }
  return parseProductionGrants(text);
}

// Internal SDK task bridge. It reads a manifest, not the command's grants.
if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  try {
    if (process.argv.length !== 3) {
      throw new Error("usage: grants.mjs ROOT-PACKAGE");
    }
    console.log(JSON.stringify(await readProductionGrants(process.argv[2])));
  } catch (error) {
    console.error(error.message);
    process.exitCode = 1;
  }
}
