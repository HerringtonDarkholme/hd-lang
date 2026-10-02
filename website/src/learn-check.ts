import { execFile } from "node:child_process";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { availableParallelism, tmpdir } from "node:os";
import { join, resolve } from "node:path";

import { createMarkdown, fencedBlocks } from "./markdown.ts";

/** The learn page's source; every ```hd block in it must parse. */
export const LEARN_PAGE = "guide/LEARN_IN_10_MINUTES.md";

/** The repository root, where the compiler command runs. */
const REPO_DIR = resolve(import.meta.dirname, "../..");

/**
 * The compiler, called through the conformance command contract
 * (spec/conformance/README.md, Command Contract): $HD_TEST_COMMAND, else the
 * repository compiler.
 */
function compilerCommand(): string[] {
  const text = process.env.HD_TEST_COMMAND ?? "node --experimental-strip-types bin/hd.js";
  return [...text.matchAll(/"([^"]*)"|'([^']*)'|(\S+)/g)].map((m) => m[1] ?? m[2] ?? m[3]!);
}

/** Runs `IMPL parse FILE`; resolves to the located diagnostic lines, or none on exit 0. */
function parseErrors(file: string): Promise<string[]> {
  const [program, ...args] = compilerCommand();
  return new Promise((complete) => {
    execFile(program!, [...args, "parse", file], { cwd: REPO_DIR }, (error, stdout, stderr) => {
      if (!error) return complete([]);
      const lines = `${stdout}\n${stderr}`.split("\n").filter((line) => line.startsWith(file));
      const crash = `${stderr}`.trim().split("\n")[0] || error.message.split("\n")[0]!;
      complete(lines.length ? lines.map((line) => line.slice(file.length + 1)) : [crash]);
    });
  });
}

/** Returns one message per ```hd block of `path` that the compiler's `parse` rejects. */
export async function checkHdBlocksParse(root: string, path: string): Promise<string[]> {
  const markdown = await readFile(`${root}/${path}`, "utf8");
  const blocks = fencedBlocks(createMarkdown(), markdown).filter((block) => block.info === "hd");
  if (blocks.length === 0) return [`${path}: no hd blocks found`];
  const scratch = await mkdtemp(join(tmpdir(), "hd-learn-check-"));
  try {
    const results: string[][] = [];
    let next = 0;
    const worker = async (): Promise<void> => {
      while (next < blocks.length) {
        const index = next;
        next += 1;
        const file = join(scratch, `block-${index}.hd`);
        await writeFile(file, blocks[index]!.code);
        results[index] = await parseErrors(file);
      }
    };
    const jobs = Math.min(8, availableParallelism(), blocks.length);
    await Promise.all(Array.from({ length: jobs }, worker));
    return blocks.flatMap((block, index) =>
      results[index]!.length
        ? [`${path}:${block.line}: ${results[index]!.join("; ")} (at block line:col)`]
        : [],
    );
  } finally {
    await rm(scratch, { force: true, recursive: true });
  }
}
