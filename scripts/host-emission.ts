import { readFile, mkdir, writeFile, rm } from "node:fs/promises";
import { dirname, isAbsolute, relative, resolve } from "node:path";
import { emitTypeScriptJavaScript } from "../packages/server-sdk/packaging/typescript-emission.ts";

const fail = (message: string): Error => new Error(`host emission: ${message}`);

function inside(root: string, path: string): boolean {
  const rel = relative(root, path);
  return (
    rel !== ".." &&
    !rel.startsWith(`..${process.platform === "win32" ? "\\" : "/"}`) &&
    !isAbsolute(rel)
  );
}

function checkedPath(root: string, name: string, kind: string): string {
  if (typeof name !== "string" || name.length === 0) {
    throw fail(`invalid ${kind} path`);
  }
  const path = resolve(root, name);
  if (!inside(root, path)) {
    throw fail(`${kind} path escapes its root: ${name}`);
  }
  return path;
}

export function emitHostJavaScript(source: string, sourceName: string): string {
  try {
    return emitTypeScriptJavaScript(source, sourceName);
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    throw fail(message.replace(/^TypeScript emission: /, ""));
  }
}

/** Read a named host graph, emitting `.ts` inputs under their `.js` output names. */
export async function readTypeScriptSources(
  root: string,
  sources: Record<string, string>,
): Promise<Record<string, string>> {
  const sourceRoot = resolve(root);
  const files: Record<string, string> = {};
  for (const name of Object.keys(sources).sort()) {
    const sourceName = sources[name];
    const input = checkedPath(sourceRoot, sourceName, "source");
    const text = (await readFile(input, "utf8")).replaceAll("\r\n", "\n");
    if (sourceName.endsWith(".ts")) {
      if (!name.endsWith(".js")) {
        throw fail(`typed source output must end in .js: ${name}`);
      }
      files[name] = emitHostJavaScript(text, sourceName);
    } else if (sourceName.endsWith(".json") && name.endsWith(".json")) {
      files[name] = text;
    } else {
      throw fail(`unsupported source/output pair: ${sourceName} -> ${name}`);
    }
  }
  return files;
}

export const readHostSources = readTypeScriptSources;

/** Emit a host graph into a fresh directory, leaving both sources and existing output untouched. */
export async function emitHostSources(
  directory: string,
  root: string,
  sources: Record<string, string>,
): Promise<Record<string, string>> {
  const destination = resolve(directory);
  const files = await readTypeScriptSources(root, sources);
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
