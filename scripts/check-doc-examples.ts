// Checks the hd blocks of the docs outside spec/ against docs/examples.tsv, as
// spec/check-examples.ts does for the chapters: every `hd` fence of README.md,
// website/guide/, website/tour/, and docs/compiler/ has one row, each
// `complete` block passes `check` as printed, and a tour page's program runs
// and prints its output block. A `known-failure` block must still fail.
// (website/build.ts also checks the tour snippets and their edits.)
//
// The compiler is $HD_TEST_COMMAND, else scripts/hd-conformance.sh (built first,
// as spec/check.sh does, so the timed runs never include a build).
//
// Usage: check-doc-examples.ts REPO_DIR DOCS_TSV [COMPILER]
import { spawnSync } from "node:child_process";
import { mkdtempSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { parseTourPage, type TourPage } from "../website/src/tour-pages.ts";
import { type Checked, type Example, readInventory, runAndJudge } from "../spec/check-examples.ts";

// Docs outside spec/ (docs/examples.tsv): every `hd` fence of the files below
// has one row, keyed by its position among the file's `hd` fences, whose
// info string may carry a word (`hd example`, `hd src/pricing.hd`).
const DOC_FILES = ["README.md", "website/guide/*.md", "website/tour/*.md", "docs/compiler/*.md"];

/** The files of DOC_FILES that exist, relative to `repoDir`, in order. */
function docFiles(repoDir: string): string[] {
  const files: string[] = [];
  for (const pattern of DOC_FILES) {
    if (!pattern.includes("*")) {
      files.push(pattern);
      continue;
    }
    const directory = pattern.slice(0, pattern.lastIndexOf("/"));
    for (const name of readdirSync(join(repoDir, directory)).sort()) {
      if (name.endsWith(".md")) {
        files.push(`${directory}/${name}`);
      }
    }
  }
  return files;
}

/** The `hd` fences of a document, with their info word. */
export function docBlocks(text: string): (Example & { readonly word: string })[] {
  return [...text.matchAll(/^```hd(?:[ \t]+([^\n]*))?\n(.*?)^```/gms)].map((match) => ({
    info: "hd",
    word: (match[1] ?? "").trim(),
    body: match[2]!,
    line: text.slice(0, match.index).split("\n").length + 1,
  }));
}

/** A tour page's program as a runtime case: `# expect-stdout:` lines for its output block. */
function tourProgram(page: TourPage): string {
  const expected = page.code.split("\n").some((line) => line.startsWith("tests:"))
    ? []
    : (page.output ?? []).map((line) => `# expect-stdout: ${line.replaceAll("\\", "\\\\")}`);
  return [...expected, page.code].join("\n");
}

function writeDocCases(
  repoDir: string,
  inventory: Map<string, Map<number, string>>,
  directory: string,
): { checked: Map<string, Checked>; failures: string[] } {
  const manifest = ["path\tphase\texpectation\tspecification"];
  const checked = new Map<string, Checked>();
  const failures: string[] = [];
  const documents = docFiles(repoDir);
  for (const document of inventory.keys()) {
    if (!documents.includes(document)) {
      failures.push(`${document} is inventoried but is not a checked document`);
    }
  }
  for (const document of documents) {
    const text = readFileSync(join(repoDir, document), "utf8");
    const blocks = docBlocks(text);
    const indexed = inventory.get(document) ?? new Map<number, string>();
    if (indexed.size !== blocks.length) {
      failures.push(
        `${document} has ${blocks.length} hd blocks but ${indexed.size} inventory entries`,
      );
    }
    // A tour page's program (the ```hd block with no word) runs, and prints its output block.
    const tour = /^website\/tour\/\d+-/.test(document)
      ? parseTourPage(document, text, 0)
      : undefined;
    for (const [index, block] of blocks.entries()) {
      const classification = indexed.get(index + 1) ?? "";
      const where = `${document}:${block.line} (block ${index + 1}, ${classification})`;
      if (classification !== "complete" && classification !== "known-failure") {
        if (classification !== "fragment") {
          failures.push(`${where}: unknown classification`);
        }
        continue;
      }
      const path = `${document.replace(/\.md$/, "").replaceAll("/", "__")}-${index + 1}.hd`;
      const program = tour !== undefined && block.word === "";
      if (program && classification === "complete") {
        writeFileSync(join(directory, path), tourProgram(tour));
        manifest.push(`${path}\truntime\taccept\t-`);
      } else {
        writeFileSync(join(directory, path), block.body);
        manifest.push(`${path}\ttype\taccept\t-`);
      }
      checked.set(path, { where, classification });
    }
  }
  writeFileSync(join(directory, "cases.tsv"), manifest.join("\n") + "\n");
  return { checked, failures };
}

function main(args: readonly string[]): number {
  if (args.length < 2 || args.length > 3) {
    console.error("usage: check-doc-examples.ts REPO_DIR DOCS_TSV [COMPILER]");
    return 2;
  }
  const repoDir = resolve(args[0]!);
  let compiler = args[2] ?? process.env.HD_TEST_COMMAND;
  if (compiler === undefined) {
    const script = join(repoDir, "scripts/hd-conformance.sh");
    const built = spawnSync(script, ["--build"], { stdio: "inherit" });
    if (built.status !== 0) {
      return 2;
    }
    compiler = `"${script}"`;
  }
  const directory = mkdtempSync(join(tmpdir(), "hd-doc-examples-"));
  try {
    const inventory = readInventory(resolve(args[1]!));
    const { checked, failures } = writeDocCases(repoDir, inventory, directory);
    return runAndJudge({ directory, checked, failures, compiler, label: "doc example check" });
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
}

const invokedPath = process.argv[1] ? resolve(process.argv[1]) : "";
if (invokedPath === fileURLToPath(import.meta.url)) {
  process.exitCode = main(process.argv.slice(2));
}
