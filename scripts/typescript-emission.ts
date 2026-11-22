import { readFile } from "node:fs/promises";
import { stripTypeScriptTypes } from "node:module";

const relativeTypeScript = /(["'])(\.\.?\/[^"'\r\n]*?)\.ts((?:[?#][^"'\r\n]*)?)\1/g;

/** Turn one erasable `.ts` module into runnable ESM with `.js` dependencies. */
export function emitTypeScriptJavaScript(source: string, sourceName: string): string {
  if (!sourceName.endsWith(".ts")) {
    throw new Error(`TypeScript emission: typed source must end in .ts: ${sourceName}`);
  }
  let javascript: string;
  try {
    javascript = stripTypeScriptTypes(source, { mode: "strip" });
  } catch (error) {
    throw new Error(
      `TypeScript emission: cannot strip ${sourceName}: ${error instanceof Error ? error.message : String(error)}`,
    );
  }
  return javascript.replace(relativeTypeScript, (_, quote, path, suffix) => {
    return `${quote}${path}.js${suffix}${quote}`;
  });
}

export async function readTypeScriptJavaScript(source: URL, sourceName: string): Promise<Buffer> {
  return Buffer.from(emitTypeScriptJavaScript(await readFile(source, "utf8"), sourceName));
}
