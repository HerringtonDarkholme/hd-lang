import { readFile } from "node:fs/promises";

import { parseSource } from "../../spec/reference-parser/parser.ts";
import { createMarkdown, fencedBlocks } from "./markdown.ts";

/** The learn page's source; every ```hd block in it must parse. */
export const LEARN_PAGE = "guide/LEARN_IN_10_MINUTES.md";

/** Returns one message per ```hd block of `path` that the reference parser rejects. */
export async function checkHdBlocksParse(root: string, path: string): Promise<string[]> {
  const markdown = await readFile(`${root}/${path}`, "utf8");
  const failures: string[] = [];
  let count = 0;
  for (const block of fencedBlocks(createMarkdown(), markdown)) {
    if (block.info !== "hd") continue;
    count += 1;
    const diagnostics = parseSource(block.code);
    if (diagnostics.length > 0) {
      const rendered = diagnostics.map(({ code, line }) => `${code} at block line ${line}`);
      failures.push(`${path}:${block.line}: ${rendered.join(", ")}`);
    }
  }
  if (count === 0) failures.push(`${path}: no hd blocks found`);
  return failures;
}
