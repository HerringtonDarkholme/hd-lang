import { escapeHtml, type Heading } from "./markdown.ts";
import {
  navSections,
  PAGES,
  PLAYGROUND_APP_DIR,
  PLAYGROUND_PAGE,
  REPOSITORY_URL,
  type PageSource,
} from "./pages.ts";

/**
 * The page shells:
 *
 * - `docs`: sidebar, a reading-width article, and an on-page outline.
 * - `wide`: sidebar and a content area that fills the rest (the playground).
 * - `home`: no sidebar on wide screens; the body lays out its own sections.
 * - `split`: two columns, prose on the left and a sticky `aside` on the
 *   right, such as a live editor; they stack on narrow screens.
 */
export type PageShape = "docs" | "wide" | "home" | "split";

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
  /** The page shell; `docs` when omitted. */
  readonly shape?: PageShape;
  /** The right-hand column of a `split` page, such as an editor. */
  readonly aside?: string;
  /**
   * Adds the bottom REPL panel. It needs the playground build, whose compiler
   * worker it loads when the panel first opens.
   */
  readonly repl?: boolean;
  /** Sidebar contents in place of the site's page sections, such as the tour's. */
  readonly nav?: string;
  /** Module scripts the page loads, as paths under the base. */
  readonly scripts?: readonly string[];
}

/** The site asset that runs the REPL panel; website/build.ts bundles it. */
export const REPL_SCRIPT = "assets/repl.js";

/** The localStorage key of the theme toggle; site.js writes it. */
const THEME_KEY = "hd-theme";

const GITHUB_ICON =
  '<svg viewBox="0 0 16 16" width="20" height="20" aria-hidden="true"><path fill="currentColor" d="M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38 0-.19-.01-.82-.01-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82.64-.18 1.32-.27 2-.27.68 0 1.36.09 2 .27 1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8.013 8.013 0 0016 8c0-4.42-3.58-8-8-8z"/></svg>';

const THEME_ICON =
  '<svg viewBox="0 0 24 24" width="18" height="18" aria-hidden="true"><path class="icon-sun" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" d="M12 3v2m0 14v2M3 12h2m14 0h2M5.6 5.6l1.4 1.4m10 10 1.4 1.4M5.6 18.4 7 17M17 7l1.4-1.4M16 12a4 4 0 1 1-8 0 4 4 0 0 1 8 0z"/><path class="icon-moon" fill="currentColor" d="M20 14.5A8 8 0 0 1 9.5 4a8 8 0 1 0 10.5 10.5z"/></svg>';

const SEARCH_ICON =
  '<svg viewBox="0 0 24 24" width="16" height="16" aria-hidden="true"><path fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" d="m20 20-4.5-4.5M17 10.5a6.5 6.5 0 1 1-13 0 6.5 6.5 0 0 1 13 0z"/></svg>';

function link(base: string, output: string): string {
  return base + output.replace(/(^|\/)index\.html$/, "$1");
}

/** The top bar's links, each with the test that marks it as the current area. */
const PRIMARY: readonly [label: string, output: string, current: (output: string) => boolean][] = [
  ["Home", "index.html", (output) => output === "index.html"],
  ["Learn", "guide/learn-in-10-minutes.html", (output) => output.startsWith("guide/")],
  ["Tour", "tour/index.html", (output) => output.startsWith("tour/")],
  ["Playground", PLAYGROUND_PAGE, (output) => output === PLAYGROUND_PAGE],
  ["Spec", "spec/index.html", (output) => output.startsWith("spec/")],
];

function primaryLinks(input: LayoutInput): string {
  return PRIMARY.map(([label, output, current]) => {
    const active = current(input.output);
    return `<a href="${link(input.base, output)}"${active ? ' aria-current="page"' : ""}>${label}</a>`;
  }).join("");
}

/** Sections with more pages than this start closed unless they hold the current page. */
const OPEN_SECTION_LIMIT = 20;

function sidebar(input: LayoutInput): string {
  const item = (href: string, label: string, active: boolean): string =>
    `<li><a href="${href}"${active ? ' aria-current="page"' : ""}>${escapeHtml(label)}</a></li>`;
  const sections = input.nav
    ? [input.nav]
    : navSections().map((section) => {
        const current = section.pages.some((entry) => entry.output === input.output);
        const open = current || section.pages.length <= OPEN_SECTION_LIMIT;
        const items = section.pages
          .map((entry) =>
            item(link(input.base, entry.output), entry.navTitle, entry.output === input.output),
          )
          .join("");
        const playground =
          section.title === "Start"
            ? item(
                link(input.base, PLAYGROUND_PAGE),
                "Playground",
                input.output === PLAYGROUND_PAGE,
              )
            : "";
        return `<details class="nav-section"${open ? " open" : ""}><summary>${escapeHtml(section.title)}</summary><ul>${items}${playground}</ul></details>`;
      });
  return `<nav class="sidebar" id="sidebar" aria-label="Site">
<div class="sidebar-primary">${primaryLinks(input)}<a href="${REPOSITORY_URL}">GitHub</a></div>
${sections.join("")}</nav>`;
}

/** Chapters with fewer outline entries than this get no on-page outline. */
const OUTLINE_MINIMUM = 3;

function outline(headings: readonly Heading[]): string {
  const entries = headings.filter((heading) => heading.level === 2 || heading.level === 3);
  if (entries.length < OUTLINE_MINIMUM) return "";
  const items = entries
    .map(
      (heading) =>
        `<li class="toc-${heading.level}"><a href="#${escapeHtml(heading.id)}">${escapeHtml(heading.text)}</a></li>`,
    )
    .join("");
  return `<aside class="toc" aria-labelledby="toc-title"><h2 id="toc-title">On this page</h2><ul>${items}</ul></aside>`;
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

function main(input: LayoutInput, shape: PageShape): string {
  const sourceLink = input.source
    ? `<p class="source-link"><a href="${REPOSITORY_URL}/blob/main/${input.source}">View source on GitHub</a></p>`
    : "";
  if (shape === "home")
    return `<main id="content" class="content content-home">
${input.body}
</main>`;
  if (shape === "split")
    return `<main id="content" class="content content-split">
<div class="split">
<article class="prose split-prose">
${input.body}
${pager(input)}
</article>
<aside class="split-aside" aria-label="Live editor">
${input.aside ?? ""}
</aside>
</div>
</main>`;
  return `<main id="content" class="content">
<article class="${shape === "wide" ? "wide" : "prose"}">
${input.body}
</article>
${sourceLink}
${pager(input)}
</main>
${shape === "wide" ? "" : outline(input.headings)}`;
}

export function renderLayout(input: LayoutInput): string {
  const { base } = input;
  const shape = input.shape ?? "docs";
  const pageTitle = input.output === "index.html" ? "hd-lang" : `${input.title} · hd-lang`;
  return `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>${escapeHtml(pageTitle)}</title>
<meta name="description" content="${escapeHtml(input.description)}">
<meta name="color-scheme" content="light dark">
<script>try{var t=localStorage.getItem("${THEME_KEY}");if(t==="light"||t==="dark")document.documentElement.dataset.theme=t}catch(e){}</script>
<link rel="stylesheet" href="${base}assets/style.css">
<link rel="icon" href="${base}assets/favicon.svg" type="image/svg+xml">
<script src="${base}assets/site.js" defer></script>${[
    ...(input.repl ? [REPL_SCRIPT] : []),
    ...(input.scripts ?? []),
  ]
    .map((script) => `\n<script type="module" src="${base}${script}"></script>`)
    .join("")}
</head>
<body data-base="${escapeHtml(base)}"${
    input.repl
      ? ` data-repl-worker="${escapeHtml(`${base}${PLAYGROUND_APP_DIR}/assets/worker.js`)}"`
      : ""
  }>
<a class="skip-link" href="#content">Skip to content</a>
<header class="topbar">
  <div class="topbar-inner">
  <button class="menu-button" type="button" aria-controls="sidebar" aria-expanded="false" aria-label="Toggle navigation">
    <span></span><span></span><span></span>
  </button>
  <a class="brand" href="${base}" aria-label="hd-lang home"><span class="brand-mark" aria-hidden="true">hd</span><span class="brand-name">hd-lang</span></a>
  <nav class="topnav" aria-label="Primary">${primaryLinks(input)}</nav>
  <div class="search" role="search">
    ${SEARCH_ICON}
    <input type="search" id="search-input" placeholder="Search" aria-label="Search the documentation" autocomplete="off">
    <kbd class="search-key" aria-hidden="true">/</kbd>
    <ul id="search-results" hidden></ul>
  </div>
  <button class="theme-toggle" type="button" aria-label="Toggle dark theme" title="Toggle dark theme">${THEME_ICON}</button>
  <a class="github-link" href="${REPOSITORY_URL}" aria-label="hd-lang on GitHub">${GITHUB_ICON}</a>
  </div>
</header>
<div class="layout layout-${shape}">
${sidebar(input)}
${main(input, shape)}
</div>
</body>
</html>
`;
}
