// The interactive tour's pages: one per Markdown file in website/tour/, each
// with a claim as its headline, a few sentences of prose, and one runnable
// snippet. A page file holds:
//
//   ---
//   title: Hello, hd          (the table-of-contents entry)
//   ---
//   # The claim               (the headline)
//   Prose...
//   ```hd                     (the snippet the editor opens with)
//   ```edit                   (optional: a one-line edit and the error it gives)
//   replace: <a whole line of the snippet>
//   with: <its replacement>
//   error: <code: message the check prints>
//   ```
//   ```output                 (the console output of a run)
//   ```
//
// website/src/learn-check.ts checks that each snippet compiles and that each
// edit gives its error; website/playground/test/runner.test.ts runs each
// snippet through the playground's pipeline and compares the output.
// website/src/tour.ts renders the pages.

import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";

/** Repository-relative directory of the tour's page files. */
export const TOUR_DIR = "website/tour";

/** The tour's contents page. */
export const TOUR_INDEX = "tour/index.html";

export interface TourEdit {
  /** A whole line of the snippet, as written. */
  readonly replace: string;
  readonly with: string;
  /** `code: message` of the diagnostic the edited snippet gives. */
  readonly error: string;
}

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
  readonly code: string;
  readonly edit?: TourEdit;
  /** Console lines a run prints. */
  readonly output: readonly string[];
}

/** Output path of a tour page relative to the site root. */
export function tourOutput(page: TourPage): string {
  return `tour/${page.number}-${page.slug}/index.html`;
}

/** The storage key a page's edits are kept under in the browser. */
export function tourKey(page: TourPage): string {
  return `${page.number}-${page.slug}`;
}

const FENCE = /^```(\w+)\n([\s\S]*?)^```[ \t]*\n?/gm;

function parseEdit(source: string, text: string): TourEdit {
  const fields = new Map<string, string>();
  for (const line of text.split("\n").filter((line) => line !== "")) {
    const match = /^(replace|with|error): (.*)$/.exec(line);
    if (!match) throw new Error(`${source}: edit block line '${line}' is not replace/with/error`);
    fields.set(match[1]!, match[2]!);
  }
  const [replace, replacement, error] = ["replace", "with", "error"].map((key) => fields.get(key));
  if (replace === undefined || replacement === undefined || error === undefined)
    throw new Error(`${source}: an edit block needs replace:, with:, and error:`);
  return { replace, with: replacement, error };
}

export function parseTourPage(source: string, text: string, number: number): TourPage {
  const slug = /^\d+-([a-z0-9-]+)\.md$/.exec(source.split("/").at(-1)!)?.[1];
  if (!slug) throw new Error(`${source}: name a tour page NN-slug.md`);
  const front = /^---\ntitle: (.+)\n---\n/.exec(text);
  if (!front) throw new Error(`${source}: a tour page starts with a title: front matter block`);
  const blocks = new Map<string, string>();
  const body = text.slice(front[0].length).replaceAll(FENCE, (_, info: string, code: string) => {
    if (blocks.has(info)) throw new Error(`${source}: more than one \`\`\`${info} block`);
    blocks.set(info, code);
    return "";
  });
  const code = blocks.get("hd");
  const output = blocks.get("output");
  if (code === undefined || output === undefined)
    throw new Error(`${source}: a tour page needs one \`\`\`hd and one \`\`\`output block`);
  const unknown = [...blocks.keys()].filter((info) => !["hd", "edit", "output"].includes(info));
  if (unknown.length > 0) throw new Error(`${source}: unknown block \`\`\`${unknown[0]}`);
  const heading = /^# (.+)\n/m.exec(body);
  if (!heading) throw new Error(`${source}: a tour page needs a # headline`);
  const edit = blocks.get("edit");
  return {
    number,
    slug,
    source,
    title: front[1]!.trim(),
    claim: heading[1]!.trim(),
    prose: body.slice(heading.index + heading[0].length).trim(),
    code,
    edit: edit === undefined ? undefined : parseEdit(source, edit),
    output: output.replace(/\n$/, "").split("\n"),
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
