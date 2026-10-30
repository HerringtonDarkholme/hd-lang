import { readFile, mkdir, writeFile, rm } from "node:fs/promises";
import { stripTypeScriptTypes } from "node:module";
import { dirname, isAbsolute, relative, resolve } from "node:path";

const relativeMts = /(["'])(\.\.?\/[^"'\r\n]*?)\.mts((?:[?#][^"'\r\n]*)?)\1/g;

const fail = (message) => new Error(`host emission: ${message}`);

function inside(root, path) {
  const rel = relative(root, path);
  return (
    rel !== ".." &&
    !rel.startsWith(`..${process.platform === "win32" ? "\\" : "/"}`) &&
    !isAbsolute(rel)
  );
}

function checkedPath(root, name, kind) {
  if (typeof name !== "string" || name.length === 0) throw fail(`invalid ${kind} path`);
  const path = resolve(root, name);
  if (!inside(root, path)) throw fail(`${kind} path escapes its root: ${name}`);
  return path;
}

/** Turn one erasable `.mts` module into runnable ESM with `.mjs` dependencies. */
export function emitHostJavaScript(source, sourceName) {
  if (!sourceName.endsWith(".mts")) throw fail(`typed source must end in .mts: ${sourceName}`);
  let javascript;
  try {
    javascript = stripTypeScriptTypes(source, { mode: "strip" });
  } catch (error) {
    throw fail(`cannot strip ${sourceName}: ${error.message}`);
  }
  return javascript.replace(relativeMts, (_, quote, path, suffix) => {
    return `${quote}${path}.mjs${suffix}${quote}`;
  });
}

/** Read a named host graph, emitting `.mts` inputs under their `.mjs` output names. */
export async function readHostSources(root, sources) {
  const sourceRoot = resolve(root);
  const files = {};
  for (const name of Object.keys(sources).sort()) {
    const sourceName = sources[name];
    const input = checkedPath(sourceRoot, sourceName, "source");
    const text = (await readFile(input, "utf8")).replaceAll("\r\n", "\n");
    if (sourceName.endsWith(".mts")) {
      if (!name.endsWith(".mjs")) throw fail(`typed source output must end in .mjs: ${name}`);
      files[name] = emitHostJavaScript(text, sourceName);
    } else if (
      (sourceName.endsWith(".mjs") && name.endsWith(".mjs")) ||
      (sourceName.endsWith(".json") && name.endsWith(".json"))
    ) {
      files[name] = text;
    } else {
      throw fail(`unsupported source/output pair: ${sourceName} -> ${name}`);
    }
  }
  return files;
}

/** Emit a host graph into a fresh directory, leaving both sources and existing output untouched. */
export async function emitHostSources(directory, root, sources) {
  const destination = resolve(directory);
  const files = await readHostSources(root, sources);
  await mkdir(destination);
  try {
    for (const [name, text] of Object.entries(files)) {
      const output = checkedPath(destination, name, "output");
      await mkdir(dirname(output), { recursive: true });
      await writeFile(output, text, { encoding: "utf8", flag: "wx" });
    }
  } catch (error) {
    await rm(destination, { recursive: true, force: true });
    throw error;
  }
  return files;
}
