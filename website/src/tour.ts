// Renders the interactive tour (website/src/tour-pages.ts reads its pages):
// the contents page, and for each page the prose column and the editor
// column of a split page. website/client/tour.ts runs the editor.

import type { MarkdownIt } from "markdown-it";

import { escapeHtml, highlightHd, type RenderEnv } from "./markdown.ts";
import { TOUR_INDEX, tourKey, tourOutput, type TourPage } from "./tour-pages.ts";

/** The site asset that runs a tour page's editor; website/build.ts bundles it. */
export const TOUR_SCRIPT = "assets/tour.js";

interface TourRender {
  readonly md: MarkdownIt;
  readonly env: RenderEnv;
  /** Site URL of a page given its output path. */
  readonly pageUrl: (output: string) => string;
  /** The compiler worker's URL, when the site includes the playground build. */
  readonly workerUrl?: string;
}

/** The sidebar's table of contents; `current` is the page shown, if any. */
export function tourContents(
  pages: readonly TourPage[],
  pageUrl: (output: string) => string,
  current?: TourPage,
): string {
  const items = pages
    .map((page) => {
      const active = page === current ? ' aria-current="page"' : "";
      return `<li><a href="${pageUrl(tourOutput(page))}"${active}><span class="tour-number">${page.number}</span>${escapeHtml(page.title)}</a></li>`;
    })
    .join("");
  const index = current ? "" : ' aria-current="page"';
  return `<details class="nav-section tour-contents" open><summary>Tour</summary><ul>
<li><a href="${pageUrl(TOUR_INDEX)}"${index}>Contents</a></li>${items}</ul></details>`;
}

function pager(pages: readonly TourPage[], page: TourPage, pageUrl: TourRender["pageUrl"]): string {
  const previous = pages[page.number - 2];
  const next = pages[page.number];
  const link = (target: TourPage | undefined, rel: "prev" | "next"): string => {
    const label = rel === "prev" ? "Previous" : "Next";
    if (!target)
      return rel === "prev"
        ? `<a class="tour-prev" rel="prev" href="${pageUrl(TOUR_INDEX)}"><span>${label}</span>Contents</a>`
        : "<span></span>";
    return `<a class="tour-${rel}" rel="${rel}" href="${pageUrl(tourOutput(target))}"><span>${label}</span>${escapeHtml(target.title)}</a>`;
  };
  return `<nav class="pager tour-pager" aria-label="Tour pages">${link(previous, "prev")}${link(next, "next")}</nav>`;
}

/** A tour page's prose column. */
export function tourProse(pages: readonly TourPage[], page: TourPage, input: TourRender): string {
  const { md, env, pageUrl } = input;
  return `<p class="tour-step"><a href="${pageUrl(TOUR_INDEX)}">Tour</a> · ${page.number} of ${pages.length}</p>
<h1>${md.renderInline(page.claim, env)}</h1>
${md.render(page.prose, env)}
<p class="tour-keys">${page.output ? "Run" : "Test"}: <kbd>Ctrl</kbd>+<kbd>Enter</kbd> · Pages: <kbd>Alt</kbd>+<kbd>←</kbd> <kbd>Alt</kbd>+<kbd>→</kbd></p>
${pager(pages, page, pageUrl)}`;
}

/** The output panel's text before the first run. */
function tourHint(page: TourPage): string {
  if (page.output && page.tests)
    return "Press Run to see the output here, or Test to run the tests.";
  return page.output ? "Press Run to see the output here." : "Press Test to run the tests.";
}

/**
 * A tour page's editor column. The snippet is shown highlighted until the
 * script replaces it with an editor; without the playground build it stays
 * a static listing. A page with an output block gets a Run button, and one
 * with a tests block a Test button; Ctrl+Enter presses the first of them.
 */
export function tourEditor(page: TourPage, input: TourRender): string {
  const source = JSON.stringify(page.code).replaceAll("<", "\\u003c");
  const static_ = `<pre class="code hd tour-static"><code>${highlightHd(page.code.replace(/\n$/, ""))}</code></pre>`;
  if (!input.workerUrl)
    return `<div class="tour-editor tour-offline">
<div class="tour-toolbar"><span class="tour-file">main.hd</span></div>
<div class="tour-code">${static_}</div>
<div class="tour-output"><p class="notice">Running code needs the playground build: <code>pnpm run website:build</code> includes it.</p></div>
</div>`;
  const primary = page.output ? "run" : "test";
  const button = (mode: "run" | "test", label: string): string => {
    const keys = mode === primary ? " (Ctrl+Enter)" : "";
    const style = mode === primary ? "button button-primary" : "button";
    return `<button type="button" class="${style}" id="tour-${mode}" title="${label}${keys}">${label}</button>`;
  };
  const buttons = [
    page.output ? button("run", "Run") : "",
    page.tests ? button("test", "Test") : "",
  ];
  return `<div class="tour-editor" id="tour-editor" data-tour-key="${escapeHtml(tourKey(page))}" data-worker="${escapeHtml(input.workerUrl)}" data-primary="${primary}">
<div class="tour-toolbar"><span class="tour-file">main.hd</span><span class="tour-status" id="tour-status" role="status"></span><button type="button" class="button button-quiet" id="tour-reset" title="Restore the original code">Reset</button>${buttons.join("")}</div>
<div class="tour-code" id="tour-code">${static_}</div>
<div class="tour-output" id="tour-output" aria-live="polite"><p class="tour-hint">${tourHint(page)}</p></div>
<script type="application/json" id="tour-source">${source}</script>
</div>`;
}

/** The tour's contents page. */
export function tourIndexBody(pages: readonly TourPage[], input: TourRender): string {
  const { md, env, pageUrl } = input;
  const items = pages
    .map(
      (page) =>
        `<li><a href="${pageUrl(tourOutput(page))}">${escapeHtml(page.title)}</a><span class="tour-claim">${md.renderInline(page.claim, env)}</span></li>`,
    )
    .join("\n");
  return `<h1>A Tour of hd</h1>
<p>Each page makes one claim about hd, explains in a few sentences why it matters, and gives you a program to run and change right in the page. Most pages also name a one-line edit that turns the program into a compile error, so you see the error the claim is about.</p>
<p><a class="button button-primary" href="${pageUrl(tourOutput(pages[0]!))}">Start the tour</a></p>
<h2 id="contents">Contents</h2>
<ol class="tour-index">
${items}
</ol>
<p>Want the whole language on one page? Read the <a href="${pageUrl("guide/language-tour.html")}">Language Tour</a> guide.</p>`;
}
