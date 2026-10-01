import { escapeHtml, type Heading } from "./markdown.ts";
import {
  navSections,
  PAGES,
  PLAYGROUND_APP_DIR,
  PLAYGROUND_PAGE,
  REPOSITORY_URL,
  type PageSource,
} from "./pages.ts";

interface LayoutInput {
  /** Site base path, beginning and ending with `/`. */
  readonly base: string;
  /** Output path of this page relative to the site root. */
  readonly output: string;
  readonly title: string;
  readonly description: string;
  /** Rendered main content. */
  readonly body: string;
  /** Outline shown beside the content; empty for pages without one. */
  readonly headings: readonly Heading[];
  /** Repository-relative Markdown source, when the page has one. */
  readonly source?: string;
  /** `wide` pages fill the content area instead of using the reading width. */
  readonly width?: "prose" | "wide";
  /**
   * Adds the bottom REPL panel. It needs the playground build, whose compiler
   * worker it loads when the panel first opens.
   */
  readonly repl?: boolean;
}

/** The site asset that runs the REPL panel; website/build.ts bundles it. */
export const REPL_SCRIPT = "assets/repl.js";

const GITHUB_ICON =
  '<svg viewBox="0 0 16 16" width="20" height="20" aria-hidden="true"><path fill="currentColor" d="M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38 0-.19-.01-.82-.01-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82.64-.18 1.32-.27 2-.27.68 0 1.36.09 2 .27 1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8.013 8.013 0 0016 8c0-4.42-3.58-8-8-8z"/></svg>';

function link(base: string, output: string): string {
  return base + output.replace(/(^|\/)index\.html$/, "$1");
}

function sidebar(input: LayoutInput): string {
  const item = (href: string, label: string, active: boolean): string =>
    `<li><a href="${href}"${active ? ' aria-current="page"' : ""}>${escapeHtml(label)}</a></li>`;
  const sections = navSections().map(
    (section) =>
      `<section><h2>${escapeHtml(section.title)}</h2><ul>${section.pages
        .map((entry) =>
          item(link(input.base, entry.output), entry.navTitle, entry.output === input.output),
        )
        .join("")}${
        section.title === "Start"
          ? item(link(input.base, PLAYGROUND_PAGE), "Playground", input.output === PLAYGROUND_PAGE)
          : ""
      }</ul></section>`,
  );
  return `<nav class="sidebar" id="sidebar" aria-label="Site">${sections.join("")}</nav>`;
}

function outline(headings: readonly Heading[]): string {
  const entries = headings.filter((heading) => heading.level === 2 || heading.level === 3);
  if (entries.length < 2) return "";
  const items = entries
    .map(
      (heading) =>
        `<li class="toc-${heading.level}"><a href="#${escapeHtml(heading.id)}">${escapeHtml(heading.text)}</a></li>`,
    )
    .join("");
  return `<aside class="toc" aria-label="On this page"><h2>On this page</h2><ul>${items}</ul></aside>`;
}

function pager(input: LayoutInput): string {
  const index = PAGES.findIndex((entry) => entry.output === input.output);
  if (index < 0) return "";
  const neighbor = (entry: PageSource | undefined, rel: "prev" | "next"): string =>
    entry
      ? `<a class="pager-${rel}" rel="${rel}" href="${link(input.base, entry.output)}"><span>${rel === "prev" ? "Previous" : "Next"}</span>${escapeHtml(entry.navTitle)}</a>`
      : "<span></span>";
  return `<nav class="pager" aria-label="Pages">${neighbor(PAGES[index - 1], "prev")}${neighbor(PAGES[index + 1], "next")}</nav>`;
}

export function renderLayout(input: LayoutInput): string {
  const { base } = input;
  const sourceLink = input.source
    ? `<p class="source-link"><a href="${REPOSITORY_URL}/blob/main/${input.source}">View source on GitHub</a></p>`
    : "";
  const pageTitle = input.output === "index.html" ? "hd-lang" : `${input.title} · hd-lang`;
  return `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>${escapeHtml(pageTitle)}</title>
<meta name="description" content="${escapeHtml(input.description)}">
<meta name="color-scheme" content="light dark">
<link rel="stylesheet" href="${base}assets/style.css">
<link rel="icon" href="${base}assets/favicon.svg" type="image/svg+xml">
<script src="${base}assets/site.js" defer></script>${
    input.repl ? `\n<script type="module" src="${base}${REPL_SCRIPT}"></script>` : ""
  }
</head>
<body data-base="${escapeHtml(base)}"${
    input.repl
      ? ` data-repl-worker="${escapeHtml(`${base}${PLAYGROUND_APP_DIR}/assets/worker.js`)}"`
      : ""
  }>
<a class="skip-link" href="#content">Skip to content</a>
<header class="topbar">
  <button class="menu-button" type="button" aria-controls="sidebar" aria-expanded="false" aria-label="Toggle navigation">
    <span></span><span></span><span></span>
  </button>
  <a class="brand" href="${base}"><span class="brand-mark">hd</span>-lang</a>
  <nav class="topnav" aria-label="Primary">
    <a href="${link(base, "guide/learn-in-10-minutes.html")}">Learn</a>
    <a href="${link(base, "guide/index.html")}">Guide</a>
    <a href="${link(base, "spec/index.html")}">Reference</a>
    <a href="${link(base, PLAYGROUND_PAGE)}">Playground</a>
  </nav>
  <div class="search" role="search">
    <input type="search" id="search-input" placeholder="Search" aria-label="Search the documentation" autocomplete="off">
    <ul id="search-results" hidden></ul>
  </div>
  <a class="github-link" href="${REPOSITORY_URL}" aria-label="hd-lang on GitHub">${GITHUB_ICON}<span>GitHub</span></a>
</header>
<div class="layout${input.width === "wide" ? " layout-wide" : ""}">
${sidebar(input)}
<main id="content" class="content">
<article class="${input.width === "wide" ? "wide" : "prose"}">
${input.body}
</article>
${sourceLink}
${pager(input)}
</main>
${input.width === "wide" ? "" : outline(input.headings)}
</div>
</body>
</html>
`;
}
