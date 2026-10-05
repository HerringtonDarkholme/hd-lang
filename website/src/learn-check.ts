import { execFile } from "node:child_process";
import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { availableParallelism, tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";

import { runHd } from "../../test/hd-in-process.ts";
import { createMarkdown, fencedBlocks } from "./markdown.ts";
import { editedCode, TOUR_MAIN, type TourPage } from "./tour-pages.ts";

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

/** The located diagnostic lines of a failed command, without the file name. */
function errorLines(
  output: { readonly stdout: string; readonly stderr: string },
  file: string,
  tree: string | undefined,
  fallback: string,
): string[] {
  const lines = `${output.stdout}\n${output.stderr}`.split("\n").flatMap((line) => {
    if (line.startsWith(`${file}:`)) return [line.slice(file.length + 1)];
    if (tree && line.startsWith(`${tree}/`)) return [line.slice(tree.length + 1)];
    return [];
  });
  const crash = output.stderr.trim().split("\n")[0] || fallback;
  return lines.length ? lines : [crash];
}

/**
 * Runs `IMPL ACTION [OPTION]... FILE`; resolves to the located diagnostic
 * lines without the file name, or none on exit 0. With `tree`, a directory
 * that holds the other files of FILE's package at the package path
 * `TOUR_MAIN`, FILE runs in this repository's compiler through the
 * adapter's package tree option (an `hd` command line has none), and a
 * diagnostic in a tree file keeps its path in the tree, such as
 * `src/pricing.hd:3:5: ...`.
 */
function compilerErrors(action: readonly string[], file: string, tree?: string): Promise<string[]> {
  if (tree) {
    return runHd(
      [...action, file],
      { cwd: REPO_DIR },
      { packageTree: { directory: tree, path: TOUR_MAIN } },
    ).then((result) =>
      result.status === 0 ? [] : errorLines(result, file, tree, `exit status ${result.status}`),
    );
  }
  const [program, ...args] = compilerCommand();
  return new Promise((complete) => {
    execFile(program!, [...args, ...action, file], { cwd: REPO_DIR }, (error, stdout, stderr) => {
      if (!error) return complete([]);
      complete(errorLines({ stdout, stderr }, file, tree, error.message.split("\n")[0]!));
    });
  });
}

/** Runs `jobs` on at most eight workers at a time; the results keep the jobs' order. */
async function inParallel<T>(jobs: readonly (() => Promise<T>)[]): Promise<T[]> {
  const results: T[] = [];
  let next = 0;
  const worker = async (): Promise<void> => {
    while (next < jobs.length) {
      const index = next;
      next += 1;
      results[index] = await jobs[index]!();
    }
  };
  const count = Math.min(8, availableParallelism(), jobs.length);
  await Promise.all(Array.from({ length: count }, worker));
  return results;
}

/** Returns one message per ```hd block of `path` that the compiler's `parse` rejects. */
export async function checkHdBlocksParse(root: string, path: string): Promise<string[]> {
  const markdown = await readFile(`${root}/${path}`, "utf8");
  const blocks = fencedBlocks(createMarkdown(), markdown).filter((block) => block.info === "hd");
  if (blocks.length === 0) return [`${path}: no hd blocks found`];
  const scratch = await mkdtemp(join(tmpdir(), "hd-learn-check-"));
  try {
    const results = await inParallel(
      blocks.map((block, index) => async () => {
        const file = join(scratch, `block-${index}.hd`);
        await writeFile(file, block.code);
        return compilerErrors(["parse"], file);
      }),
    );
    return blocks.flatMap((block, index) =>
      results[index]!.length
        ? [`${path}:${block.line}: ${results[index]!.join("; ")} (at block line:col)`]
        : [],
    );
  } finally {
    await rm(scratch, { force: true, recursive: true });
  }
}

/**
 * Returns one message per problem with a tour page: its snippet does not
 * type-check (`IMPL check --tests FILE`, in a package tree when the page has
 * several files), its edit does not give the error it
 * names, a `failure:` edit does not type-check, or the snippet does not quote
 * the edit's error or failure in a comment. The command contract has no
 * action that runs a program with `main`, so the playground's runner test
 * runs and tests the snippets (website/playground/test/runner.test.ts).
 */
export async function checkTourSnippets(pages: readonly TourPage[]): Promise<string[]> {
  const scratch = await mkdtemp(join(tmpdir(), "hd-tour-check-"));
  const check = async (page: TourPage, code: string, name: string): Promise<string[]> => {
    const file = join(scratch, `${page.number}-${name}.hd`);
    await writeFile(file, code);
    const paths = Object.keys(page.files);
    if (paths.length === 0) return compilerErrors(["check", "--tests"], file);
    // The snippet is src/main.hd of a package that holds the page's other files.
    const tree = join(scratch, `${page.number}-${name}-tree`);
    for (const path of paths) {
      await mkdir(dirname(join(tree, path)), { recursive: true });
      await writeFile(join(tree, path), page.files[path]!);
    }
    return compilerErrors(["check", "--tests"], file, tree);
  };
  const snippet = (page: TourPage) => async (): Promise<string[]> =>
    (await check(page, page.code, "snippet")).map((error) => `${page.source}: snippet: ${error}`);
  const edited = (page: TourPage) => async (): Promise<string[]> => {
    const { edit } = page;
    if (!edit) return [];
    const problems: string[] = [];
    const quoted = edit.error ?? edit.failure;
    const comments = page.code.split("\n").filter((line) => line.startsWith("#"));
    if (!comments.some((line) => line.includes(quoted)))
      problems.push(`${page.source}: no comment in the snippet quotes '${quoted}'`);
    let code: string;
    try {
      code = editedCode(page);
    } catch (error) {
      return [...problems, (error as Error).message];
    }
    const errors = await check(page, code, "edit");
    if (edit.error === undefined) {
      // A failure: edit still compiles; Test reports the failure.
      if (errors.length > 0)
        problems.push(`${page.source}: the failure: edit does not compile: ${errors.join("; ")}`);
      return problems;
    }
    if (!errors.some((error) => error.includes(edit.error)))
      problems.push(
        `${page.source}: the edit gives ${errors.length ? errors.join("; ") : "no error"}, not '${edit.error}'`,
      );
    return problems;
  };
  try {
    const results = await inParallel(pages.flatMap((page) => [snippet(page), edited(page)]));
    return results.flat();
  } finally {
    await rm(scratch, { force: true, recursive: true });
  }
}
