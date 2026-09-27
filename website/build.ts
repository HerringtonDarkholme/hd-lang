import { cp, mkdir, readFile, rm, writeFile } from "node:fs/promises";
import { existsSync, statSync } from "node:fs";
import { dirname, join, posix, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { parseArgs } from "node:util";

import * as esbuild from "esbuild";

import { buildOptions } from "../playground/build.ts";
import { renderLayout, REPL_SCRIPT } from "./src/layout.ts";
import { checkHdBlocksParse, LEARN_PAGE } from "./src/learn-check.ts";
import { checkLinks } from "./src/links.ts";
import { buildGrammarIndex, type GrammarIndex } from "./src/ebnf.ts";
import {
  createMarkdown,
  escapeHtml,
  fencedBlocks,
  type Heading,
  type RenderEnv,
} from "./src/markdown.ts";
import {
  pageBySource,
  PAGES,
  PLAYGROUND_APP_DIR,
  PLAYGROUND_PAGE,
  REPOSITORY_URL,
} from "./src/pages.ts";

const WEBSITE_DIR = dirname(fileURLToPath(import.meta.url));
const REPO_DIR = resolve(WEBSITE_DIR, "..");

/** The base path GitHub Pages serves the project site under. */
export const PAGES_BASE = "/hd-lang/";

export interface BuildOptions {
  /** URL path the site is served under, such as `/hd-lang/` or `/`. */
  readonly base: string;
  /** Output directory. */
  readonly outDir: string;
  /** Static playground build to serve at `playground/`; defaults to `playground/dist`. */
  readonly playgroundDist?: string;
}

export interface BuildResult {
  readonly pages: number;
  /** Whether the playground build was found; it also enables the REPL panel. */
  readonly playground: boolean;
  /** The ```ebnf rule index the pages were rendered with. */
  readonly grammar: GrammarIndex;
}

/** The chapter whose rule definitions are canonical when a rule is restated elsewhere. */
const GRAMMAR_CHAPTER = "spec/02-grammar.md";

function normalizeBase(base: string): string {
  const trimmed = base.replace(/^\/+|\/+$/g, "");
  return trimmed === "" ? "/" : `/${trimmed}/`;
}

function siteLink(base: string, output: string): string {
  return base + output.replace(/(^|\/)index\.html$/, "$1");
}

/** Rewrites a link written in a repository Markdown file to its site URL. */
function linkResolver(base: string, errors: string[]): RenderEnv["resolveLink"] {
  return (href, source) => {
    if (/^(?:[a-z][a-z0-9+.-]*:|\/\/|#)/i.test(href)) return href;
    const hash = href.indexOf("#");
    const path = decodeURIComponent(hash < 0 ? href : href.slice(0, hash));
    const fragment = hash < 0 ? "" : href.slice(hash);
    const target = posix.normalize(posix.join(posix.dirname(source), path)).replace(/\/$/, "");
    const page = pageBySource.get(target);
    if (page) return siteLink(base, page.output) + fragment;
    const onDisk = join(REPO_DIR, target);
    if (target.startsWith("..") || !existsSync(onDisk)) {
      errors.push(`${source}: link ${href} names a missing repository path`);
      return href;
    }
    const kind = statSync(onDisk).isDirectory() ? "tree" : "blob";
    return `${REPOSITORY_URL}/${kind}/main/${target}${fragment}`;
  };
}

function playgroundUrl(base: string): (code: string) => string {
  return (code) =>
    `${siteLink(base, PLAYGROUND_PAGE)}#code=${Buffer.from(code, "utf8").toString("base64url")}`;
}

/**
 * Bundles the REPL panel, website/client/repl.ts, into the site's assets.
 * The panel is small; the compiler worker it starts comes from the playground
 * build and loads only when the panel first opens.
 */
async function bundleRepl(outfile: string): Promise<void> {
  await esbuild.build(
    buildOptions({
      entryPoints: [join(WEBSITE_DIR, "client", "repl.ts")],
      outdir: undefined,
      outfile,
      logLevel: "warning",
    }),
  );
}

function firstParagraph(markdown: string): string {
  const paragraph = markdown
    .split(/\n\s*\n/)
    .map((block) => block.trim())
    .find((block) => block !== "" && !/^(?:#|```|\||- |\d+\. )/.test(block));
  const text = (paragraph ?? "")
    .replaceAll(/\[([^\]]*)\]\([^)]*\)/g, "$1")
    .replaceAll(/[`*_]/g, "")
    .replaceAll(/\s+/g, " ");
  return text.length > 200 ? `${text.slice(0, 197)}...` : text;
}

const HOME_CARDS: readonly [output: string, title: string, blurb: string][] = [
  [
    "guide/learn-in-10-minutes.html",
    "Learn in 10 minutes",
    "A fast tour of the syntax and core ideas.",
  ],
  ["guide/language-tour.html", "Language Tour", "Every feature, with examples and design notes."],
  ["spec/index.html", "Specification", "The normative reference, chapter by chapter."],
  [PLAYGROUND_PAGE, "Playground", "Write and run hd code in the browser."],
];

function homeCards(base: string): string {
  const cards = HOME_CARDS.map(
    ([output, title, blurb]) =>
      `<a class="card" href="${siteLink(base, output)}"><strong>${escapeHtml(title)}</strong><span>${escapeHtml(blurb)}</span></a>`,
  );
  return `<div class="cards">${cards.join("")}</div>`;
}

function playgroundBody(base: string, available: boolean): string {
  if (available)
    return `<div class="playground-bar"><h1>Playground</h1><a id="playground-open" href="${base}${PLAYGROUND_APP_DIR}/">Open full screen</a></div>
<div class="playground-frame"><iframe id="playground-frame" src="${base}${PLAYGROUND_APP_DIR}/" data-src="${base}${PLAYGROUND_APP_DIR}/" title="hd-lang playground"></iframe></div>`;
  return `<div class="prose"><h1>Playground</h1>
<p class="notice">The playground is not part of this build. Build it with <code>npm run playground:build</code> before <code>npm run website:build</code>, and it will be served here.</p>
<div id="playground-code-wrap" hidden><p>The code you opened:</p><pre class="code hd"><code id="playground-code"></code></pre></div>
<p>Until then, read <a href="${siteLink(base, "guide/learn-in-10-minutes.html")}">Learn hd-lang in 10 Minutes</a> or run examples locally with <code>npm run hd -- run examples/core.hd</code>.</p></div>`;
}

interface SearchEntry {
  readonly title: string;
  readonly page: string;
  readonly url: string;
}

function searchEntries(
  base: string,
  output: string,
  title: string,
  headings: readonly Heading[],
): SearchEntry[] {
  const url = siteLink(base, output);
  return [
    { title, page: title, url },
    ...headings
      .filter((heading) => heading.level === 2 || heading.level === 3)
      .map((heading) => ({ title: heading.text, page: title, url: `${url}#${heading.id}` })),
  ];
}

export async function buildSite(options: BuildOptions): Promise<BuildResult> {
  const base = normalizeBase(options.base);
  const outDir = resolve(options.outDir);
  const failures = await checkHdBlocksParse(REPO_DIR, LEARN_PAGE);
  if (failures.length > 0)
    throw new Error(`learn page examples do not parse:\n${failures.join("\n")}`);

  await rm(outDir, { recursive: true, force: true });
  await mkdir(outDir, { recursive: true });
  await cp(join(WEBSITE_DIR, "assets"), join(outDir, "assets"), { recursive: true });

  const playgroundDist = options.playgroundDist ?? join(REPO_DIR, "playground", "dist");
  const playground = existsSync(join(playgroundDist, "index.html"));
  if (playground) await bundleRepl(join(outDir, REPL_SCRIPT));

  const md = createMarkdown();
  const linkErrors: string[] = [];
  const resolveLink = linkResolver(base, linkErrors);
  const search: SearchEntry[] = [];
  const written: string[] = [];
  const write = async (output: string, html: string): Promise<void> => {
    await mkdir(dirname(join(outDir, output)), { recursive: true });
    await writeFile(join(outDir, output), html);
    written.push(output);
  };

  const sources = await Promise.all(
    PAGES.map(async (entry) => ({
      entry,
      markdown: await readFile(join(REPO_DIR, entry.source), "utf8"),
    })),
  );
  const grammar = buildGrammarIndex(
    sources.map(({ entry, markdown }) => ({
      source: entry.source,
      blocks: fencedBlocks(md, markdown).filter((block) => block.info === "ebnf"),
    })),
    GRAMMAR_CHAPTER,
  );

  for (const { entry, markdown } of sources) {
    const env: RenderEnv = {
      source: entry.source,
      resolveLink,
      playgroundUrl: playgroundUrl(base),
      headings: [],
      slugCounts: new Map(),
      grammar,
    };
    let body = md.render(markdown, env);
    if (entry.output === "index.html") body += homeCards(base);
    const title = env.headings.find((heading) => heading.level === 1)?.text ?? entry.navTitle;
    search.push(...searchEntries(base, entry.output, title, env.headings));
    await write(
      entry.output,
      renderLayout({
        base,
        output: entry.output,
        title,
        description: firstParagraph(markdown),
        body,
        headings: env.headings,
        source: entry.source,
        repl: playground,
      }),
    );
  }
  if (linkErrors.length > 0) throw new Error(`broken source links:\n${linkErrors.join("\n")}`);

  if (playground) await cp(playgroundDist, join(outDir, PLAYGROUND_APP_DIR), { recursive: true });
  await write(
    PLAYGROUND_PAGE,
    renderLayout({
      base,
      output: PLAYGROUND_PAGE,
      title: "Playground",
      description: "Write and run hd-lang code in the browser.",
      body: playgroundBody(base, playground),
      headings: [],
      width: "wide",
      repl: playground,
    }),
  );
  search.push({ title: "Playground", page: "Playground", url: siteLink(base, PLAYGROUND_PAGE) });

  await write(
    "404.html",
    renderLayout({
      base,
      output: "404.html",
      title: "Page not found",
      description: "This page does not exist.",
      body: `<h1>Page not found</h1><p>This page does not exist. Start from the <a href="${base}">home page</a> or the <a href="${siteLink(base, "spec/index.html")}">specification</a>.</p>`,
      headings: [],
    }),
  );
  await writeFile(join(outDir, "search-index.json"), JSON.stringify(search));

  const broken = await checkLinks(outDir, base, written);
  if (broken.length > 0) throw new Error(`broken site links:\n${broken.join("\n")}`);
  return { pages: written.length, playground, grammar };
}

async function main(): Promise<void> {
  const { values } = parseArgs({
    options: {
      base: { type: "string", default: process.env.SITE_BASE ?? PAGES_BASE },
      out: { type: "string", default: join(WEBSITE_DIR, "dist") },
    },
  });
  const result = await buildSite({ base: values.base, outDir: values.out });
  const playground = result.playground ? "with the playground" : "without a playground build";
  console.log(`website: ${result.pages} pages, base ${normalizeBase(values.base)}, ${playground}`);
  const { grammar } = result;
  console.log(
    `grammar: ${grammar.definitions.size} rules (${grammar.definitionCount} definitions), ` +
      `${grammar.linkedCount} of ${grammar.referenceCount} references linked`,
  );
  if (grammar.unresolved.length > 0)
    console.warn(
      `warning: ${grammar.unresolved.length} grammar references name no rule:\n` +
        grammar.unresolved
          .map(({ name, source, line }) => `  ${source}:${line}: ${name}`)
          .join("\n"),
    );
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  main().catch((error: unknown) => {
    console.error(error instanceof Error ? error.message : error);
    process.exitCode = 1;
  });
}
