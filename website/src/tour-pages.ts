// The interactive tour's pages: one per Markdown file in website/tour/, each
// with a claim as its headline, a few sentences of prose, and one runnable
// snippet. A page file holds:
//
//   ---
//   title: Hello, hd          (the table-of-contents entry)
//   ---
//   # The claim               (the headline)
//   Prose...
//   ```hd                     (the snippet the editor opens with, src/main.hd)
//   ```hd src/pricing.hd      (optional: more files of the package, as tabs)
//   ```edit                   (optional: a one-line edit and what it breaks)
//   replace: <a whole line of the snippet>
//   with: <its replacement, or nothing to delete the line>
//   error: <code: message the check prints>
//   ```
//   ```output                 (the console output of a run)
//   ```
//   ```tests                  (the summary Test prints, such as `2 tests passed`)
//   ```
//
// A page has an output block, a tests block, or both; a tests block gives
// the editor a Test button. A page with more files than src/main.hd shows
// each as an editor tab, and its edit applies to src/main.hd. An edit names either the compile error it gives
// (`error:`) or, on a page with a tests block, the failure Test reports for
// code that still compiles (`failure:`).
//
// website/src/learn-check.ts checks that each snippet compiles and that each
// edit gives its error; website/playground/test/runner.test.ts runs and
// tests each snippet through the playground's pipeline and compares the
// output and the summary.
// website/src/tour.ts renders the pages.

import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";

/** Repository-relative directory of the tour's page files. */
export const TOUR_DIR = "website/tour";

/** The tour's contents page. */
export const TOUR_INDEX = "tour/index.html";

/** The package path of a page's snippet, the entry module. */
export const TOUR_MAIN = "src/main.hd";

export type TourEdit = {
  /** A whole line of the snippet, as written. */
  readonly replace: string;
  readonly with: string;
} & (
  | {
      /** `code: message` of the diagnostic the edited snippet gives. */
      readonly error: string;
      readonly failure?: undefined;
    }
  | {
      /** Text of the failing summary Test gives for the edited snippet. */
      readonly failure: string;
      readonly error?: undefined;
    }
);

export interface TourPage {
  /** 1-based position in the tour. */
  readonly number: number;
  readonly slug: string;
  /** Repository-relative path of the page file. */
  readonly source: string;
  /** Table-of-contents title. */
  readonly title: string;
  /** The headline, as inline Markdown. */
  readonly claim: string;
  /** The prose under the headline, as Markdown. */
  readonly prose: string;
  /** The source of src/main.hd. */
  readonly code: string;
  /** The package's other files: package path to source, in tab order. */
  readonly files: Readonly<Record<string, string>>;
  readonly edit?: TourEdit;
  /** Console lines a run prints, when the page has an output block. */
  readonly output?: readonly string[];
  /** The summary Test prints, when the page has a tests block. */
  readonly tests?: string;
}

/** Output path of a tour page relative to the site root. */
export function tourOutput(page: TourPage): string {
  return `tour/${page.number}-${page.slug}/index.html`;
}

/** The storage key a page's edits are kept under in the browser. */
export function tourKey(page: TourPage): string {
  return `${page.number}-${page.slug}`;
}

const FENCE = /^```(\w+)(?: (\S+))?\n([\s\S]*?)^```[ \t]*\n?/gm;

/** A tour file's path: src/, identifier names, and .hd, as the playground's projects take. */
const FILE_PATH = /^src\/(?:[a-z_][a-z0-9_]*\/)*[a-z_][a-z0-9_]*\.hd$/;

function parseEdit(source: string, text: string): TourEdit {
  const fields = new Map<string, string>();
  for (const line of text.split("\n").filter((line) => line !== "")) {
    // An empty `with:` deletes the line.
    const match = /^(replace|with|error|failure):(?: (.*))?$/.exec(line);
    if (!match)
      throw new Error(`${source}: edit block line '${line}' is not replace/with/error/failure`);
    fields.set(match[1]!, match[2] ?? "");
  }
  const [replace, replacement, error, failure] = ["replace", "with", "error", "failure"].map(
    (key) => fields.get(key),
  );
  if (
    replace === undefined ||
    replacement === undefined ||
    (error === undefined) === (failure === undefined)
  )
    throw new Error(
      `${source}: an edit block needs replace:, with:, and one of error: or failure:`,
    );
  return error === undefined
    ? { replace, with: replacement, failure: failure! }
    : { replace, with: replacement, error };
}

export function parseTourPage(source: string, text: string, number: number): TourPage {
  const slug = /^\d+-([a-z0-9-]+)\.md$/.exec(source.split("/").at(-1)!)?.[1];
  if (!slug) throw new Error(`${source}: name a tour page NN-slug.md`);
  const front = /^---\ntitle: (.+)\n---\n/.exec(text);
  if (!front) throw new Error(`${source}: a tour page starts with a title: front matter block`);
  const blocks = new Map<string, string>();
  const files: Record<string, string> = {};
  const body = text
    .slice(front[0].length)
    .replaceAll(FENCE, (_, info: string, path: string | undefined, code: string) => {
      if (path !== undefined) {
        if (info !== "hd" || !FILE_PATH.test(path) || path === TOUR_MAIN)
          throw new Error(`${source}: \`\`\`${info} ${path} is not \`\`\`hd src/<name>.hd`);
        if (path in files) throw new Error(`${source}: more than one \`\`\`hd ${path} block`);
        files[path] = code;
        return "";
      }
      if (blocks.has(info)) throw new Error(`${source}: more than one \`\`\`${info} block`);
      blocks.set(info, code);
      return "";
    });
  const code = blocks.get("hd");
  const output = blocks.get("output");
  const tests = blocks.get("tests")?.trim();
  if (code === undefined || (output === undefined && tests === undefined))
    throw new Error(
      `${source}: a tour page needs one \`\`\`hd block and an \`\`\`output or \`\`\`tests block`,
    );
  const unknown = [...blocks.keys()].filter(
    (info) => !["hd", "edit", "output", "tests"].includes(info),
  );
  if (unknown.length > 0) throw new Error(`${source}: unknown block \`\`\`${unknown[0]}`);
  const heading = /^# (.+)\n/m.exec(body);
  if (!heading) throw new Error(`${source}: a tour page needs a # headline`);
  const editText = blocks.get("edit");
  const edit = editText === undefined ? undefined : parseEdit(source, editText);
  if (edit?.failure !== undefined && tests === undefined)
    throw new Error(`${source}: a failure: edit needs a \`\`\`tests block`);
  return {
    number,
    slug,
    source,
    title: front[1]!.trim(),
    claim: heading[1]!.trim(),
    prose: body.slice(heading.index + heading[0].length).trim(),
    code,
    files,
    edit,
    output: output?.replace(/\n$/, "").split("\n"),
    tests,
  };
}

/** Every tour page, in order. */
export function loadTour(repoDir: string): TourPage[] {
  return readdirSync(join(repoDir, TOUR_DIR))
    .filter((name) => name.endsWith(".md"))
    .sort()
    .map((name, index) => {
      const source = `${TOUR_DIR}/${name}`;
      return parseTourPage(source, readFileSync(join(repoDir, source), "utf8"), index + 1);
    });
}

/** The page's package, with `code` as src/main.hd. */
export function tourProject(
  page: TourPage,
  code = page.code,
): { files: Record<string, string>; main: string } {
  return { files: { [TOUR_MAIN]: code, ...page.files }, main: TOUR_MAIN };
}

/** The snippet with the page's edit applied; throws unless it names exactly one line. */
export function editedCode(page: TourPage): string {
  const { edit } = page;
  if (!edit) throw new Error(`${page.source}: the page has no edit`);
  const lines = page.code.split("\n");
  const at = lines.flatMap((line, index) => (line === edit.replace ? [index] : []));
  if (at.length !== 1)
    throw new Error(
      `${page.source}: the edit's replace: line occurs ${at.length} times in the snippet, not once`,
    );
  lines[at[0]!] = edit.with;
  return lines.join("\n");
}

/** The headline as plain text, for titles and search. */
export function plainClaim(page: TourPage): string {
  return page.claim.replaceAll(/[`*_]/g, "");
}
