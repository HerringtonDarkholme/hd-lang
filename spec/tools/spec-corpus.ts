// The specification as the spec text tools (spec/tools/spec.ts) see it: every
// chapter, numbered or stdlib, with its rule inventory and its sections.
//
// It builds on rule-inventory.ts and spec-prose.ts and adds no Markdown
// parsing of its own. It imports only Node built-ins and spec/. A corpus is
// read from a directory, or from a git revision through `git show`.
import { execFileSync } from "node:child_process";
import { readFileSync, readdirSync } from "node:fs";
import { resolve } from "node:path";

import { inventory, type Inventory, type Rule } from "./rule-inventory.ts";
import {
  blocks,
  type Block,
  CHAPTER_PREFIXES,
  CLI_DIRECTORY,
  knownCodes,
  LANG_DIRECTORY,
  legacyChapterPath,
  STD_DIRECTORY,
} from "./spec-prose.ts";

export const SPEC_ROOT = resolve(import.meta.dirname, "..");
export const REPO_ROOT = resolve(SPEC_ROOT, "..");

export type Tier = "language" | "std" | "cli";

/** A heading and the lines it governs, up to the next heading of any level. */
interface Section {
  readonly title: string;
  readonly level: number;
  readonly line: number;
  /** The last line of the section, inclusive. */
  readonly end: number;
  /** The titles of the enclosing headings, outermost first, ending with this one. */
  readonly trail: readonly string[];
}

export interface Chapter {
  /** The path under spec/, as in `lang/08-data-and-enums.md` or `std/iter.md`. */
  readonly name: string;
  readonly tier: Tier;
  /** The rule-ID prefix from CHAPTER_PREFIXES; undefined for a chapter the table lacks. */
  readonly prefix: string | undefined;
  readonly text: string;
  readonly blocks: readonly Block[];
  readonly inventory: Inventory;
  readonly sections: readonly Section[];
}

export interface Corpus {
  /** The spec directory, or `REV:spec` for a corpus read from git. */
  readonly specRoot: string;
  readonly readme: string;
  /** spec/std/README.md, which holds the stdlib glossary. */
  readonly stdReadme: string;
  readonly controlFlow: string;
  /** README Diagnostics codes and the panic categories, as rule-inventory.ts reads them. */
  readonly known: ReadonlySet<string>;
  readonly chapters: readonly Chapter[];
}

/** How a corpus reads the spec: the names in a directory under spec/, and one file's text. */
interface SpecReader {
  /** The file names in `directory`, relative to spec/ ("." is spec/ itself); [] when it is missing. */
  readonly list: (directory: string) => string[];
  /** A file's text, by its path under spec/; "" when it is missing. */
  readonly read: (name: string) => string;
}

function directoryReader(specRoot: string): SpecReader {
  return {
    list: (directory) => {
      try {
        return readdirSync(resolve(specRoot, directory));
      } catch {
        return [];
      }
    },
    read: (name) => {
      try {
        return readFileSync(resolve(specRoot, name), "utf8");
      } catch {
        return "";
      }
    },
  };
}

/** Reads spec/ as git revision `rev` has it. Throws when git cannot resolve `rev`. */
function gitReader(repoRoot: string, rev: string): SpecReader {
  const git = (...args: string[]): string =>
    execFileSync("git", ["-C", repoRoot, ...args], {
      encoding: "utf8",
      maxBuffer: 1 << 28,
      stdio: ["ignore", "pipe", "ignore"],
    });
  git("rev-parse", "--verify", "--quiet", `${rev}^{commit}`);
  return {
    list: (directory) => {
      const path = directory === "." ? "spec/" : `spec/${directory}/`;
      try {
        return git("ls-tree", "--name-only", rev, path)
          .split("\n")
          .filter((line) => line !== "")
          .map((line) => line.slice(path.length));
      } catch {
        return [];
      }
    },
    read: (name) => {
      try {
        return git("show", `${rev}:spec/${name}`);
      } catch {
        return "";
      }
    },
  };
}

/**
 * A reader for a revision from before task #175, which kept the language
 * chapters and cli.md directly in spec/. It serves each chapter at its
 * current path, as `lang/08-data-and-enums.md` or `cli/command-line.md`, so
 * a rule keeps its chapter across the move. A current layout reads as is.
 */
function currentLayout(reader: SpecReader): SpecReader {
  const top = reader.list(".");
  if (top.includes(LANG_DIRECTORY)) return reader;
  return {
    list: (directory) => {
      if (directory === LANG_DIRECTORY) return top.filter((name) => /^\d\d-.*\.md$/.test(name));
      if (directory === CLI_DIRECTORY) return top.includes("cli.md") ? ["command-line.md"] : [];
      return reader.list(directory);
    },
    read: (name) => reader.read(legacyChapterPath(name)),
  };
}

/**
 * The chapter paths under `specRoot`: the numbered language chapters in
 * spec/lang/, then spec/std/ and spec/cli/, each without its README.
 */
export function chapterNames(specRoot: string, reader = directoryReader(specRoot)): string[] {
  const list = (directory: string): string[] =>
    reader
      .list(directory)
      .filter((name) => name.endsWith(".md") && name !== "README.md")
      .sort()
      .map((name) => `${directory}/${name}`);
  return [
    ...list(LANG_DIRECTORY).filter((name) => /^[^/]+\/\d\d-.*\.md$/.test(name)),
    ...list(STD_DIRECTORY),
    ...list(CLI_DIRECTORY),
  ];
}

/** The sections of a chapter, from its heading blocks. */
function sectionsOf(text: string, parsed: readonly Block[]): Section[] {
  const lines = text.split(/\r?\n/);
  const headings = parsed.filter((block) => block.kind === "heading");
  const stack: { level: number; title: string }[] = [];
  return headings.map((heading, index) => {
    const level = /^#+/.exec(lines[heading.line - 1] ?? "")?.[0].length ?? 1;
    while (stack.length > 0 && stack.at(-1)!.level >= level) stack.pop();
    stack.push({ level, title: heading.text.trim() });
    const next = headings[index + 1];
    return {
      title: heading.text.trim(),
      level,
      line: heading.line,
      end: next ? next.line - 1 : lines.length,
      trail: stack.map((entry) => entry.title),
    };
  });
}

/** The innermost section holding `line`. */
export function sectionAt(chapter: Chapter, line: number): Section | undefined {
  return chapter.sections.findLast((section) => section.line <= line);
}

function loadChapter(
  specRoot: string,
  name: string,
  known: ReadonlySet<string>,
  text = readFileSync(resolve(specRoot, name), "utf8"),
): Chapter {
  const parsed = blocks(text);
  return {
    name,
    tier: name.startsWith(`${STD_DIRECTORY}/`)
      ? "std"
      : name.startsWith(`${CLI_DIRECTORY}/`)
        ? "cli"
        : "language",
    prefix: CHAPTER_PREFIXES[name],
    text,
    blocks: parsed,
    inventory: inventory(name, text, known),
    sections: sectionsOf(text, parsed),
  };
}

/** Loads the corpus from `specRoot`, or through `reader`, such as a gitReader. */
export function loadCorpus(specRoot = SPEC_ROOT, given = directoryReader(specRoot)): Corpus {
  const reader = currentLayout(given);
  const readme = reader.read("README.md");
  const controlFlow = reader.read(`${LANG_DIRECTORY}/06-control-flow.md`);
  const known = knownCodes(readme, controlFlow);
  return {
    specRoot,
    readme,
    stdReadme: reader.read(`${STD_DIRECTORY}/README.md`),
    controlFlow,
    known,
    chapters: chapterNames(specRoot, reader).map((name) =>
      loadChapter(specRoot, name, known, reader.read(name)),
    ),
  };
}

/** The corpus at git revision `rev`, labeled `REV:spec`. */
export function loadCorpusAt(repoRoot: string, rev: string): Corpus {
  return loadCorpus(`${rev}:spec`, gitReader(repoRoot, rev));
}

/** Every rule in the corpus with the chapter that holds it. */
export function allRules(corpus: Corpus): { chapter: Chapter; rule: Rule }[] {
  return corpus.chapters.flatMap((chapter) =>
    chapter.inventory.rules.map((rule) => ({ chapter, rule })),
  );
}
