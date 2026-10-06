// End-to-end check of the built site in a headless Chromium: the bottom REPL
// panel, the spec page layout, the playground page, and then the playground
// itself (website/playground/e2e.ts):
//
//   pnpm run website:build && pnpm run website:e2e
//
// website/dist/ is served under /hd-lang/, the GitHub Pages base it is built
// for. The panel's worker is the playground's compiler worker. Set
// CHROME_PATH to pick the browser, and E2E_SCREENSHOTS to a directory to save
// screenshots there.

import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { createServer } from "node:http";
import { dirname, extname, join, normalize } from "node:path";
import { fileURLToPath } from "node:url";

import { chromium, type Page } from "playwright-core";

import { PAGES_BASE } from "./build.ts";
import { playgroundSteps } from "./playground/e2e.ts";
import { encodeBase64Url } from "./playground/src/share.ts";
import { browserPath } from "./playground/test/chrome.ts";
import { PLAYGROUND_APP_DIR, PLAYGROUND_PAGE } from "./src/pages.ts";

const DIST = join(dirname(fileURLToPath(import.meta.url)), "dist");
const SCREENSHOTS = process.env.E2E_SCREENSHOTS;
const TYPES: Record<string, string> = {
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".css": "text/css; charset=utf-8",
  ".json": "application/json",
  ".svg": "image/svg+xml",
  ".txt": "text/plain; charset=utf-8",
};
/** How long the first evaluation may take: it waits for the 15 MB worker. */
const FIRST_REPLY_MS = 60_000;

const server = createServer(async (request, response) => {
  const url = new URL(request.url ?? "/", "http://localhost");
  if (!url.pathname.startsWith(PAGES_BASE)) {
    response.writeHead(404).end();
    return;
  }
  let relative = normalize(decodeURIComponent(url.pathname.slice(PAGES_BASE.length)));
  if (relative === "." || relative.endsWith("/")) relative = join(relative, "index.html");
  try {
    const body = await readFile(join(DIST, relative));
    response.writeHead(200, {
      "content-type": TYPES[extname(relative)] ?? "application/octet-stream",
    });
    response.end(body);
  } catch {
    response.writeHead(404).end();
  }
});
await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
const address = server.address();
const origin = `http://127.0.0.1:${typeof address === "object" && address ? address.port : 0}`;

const browser = await chromium.launch({ executablePath: browserPath(), headless: true });
let passed = 0;

async function step(name: string, body: () => Promise<void>): Promise<void> {
  await body();
  passed += 1;
  console.log(`ok - ${name}`);
}

interface Opened {
  readonly page: Page;
  /** URLs of the worker scripts the page has requested so far. */
  readonly workers: string[];
}

async function openPage(
  path: string,
  options: { colorScheme?: "light" | "dark"; width?: number; ready?: string } = {},
): Promise<Opened> {
  const context = await browser.newContext({
    colorScheme: options.colorScheme ?? "light",
    viewport: { width: options.width ?? 1280, height: 800 },
  });
  const page = await context.newPage();
  const workers: string[] = [];
  page.on("pageerror", (error) => console.error("page error:", error.message));
  page.on("request", (request) => {
    if (request.url().endsWith("/worker.js")) workers.push(request.url());
  });
  await page.goto(`${origin}${PAGES_BASE}${path}`);
  await page.locator(options.ready ?? "#repl-toggle").waitFor();
  return { page, workers };
}

/** Types one input line into the panel and presses Enter. */
async function enter(page: Page, text: string): Promise<void> {
  await page.locator(".repl-input").pressSequentially(text);
  await page.locator(".repl-input").press("Enter");
}

const last = (page: Page, selector: string) => page.locator(`.repl-log ${selector}`).last();

try {
  await step("the REPL opens on a spec page and evaluates x * 2 as 42 : usize", async () => {
    const { page, workers } = await openPage("spec/04-type-system.html");
    assert.equal(await page.locator("#repl-panel").isVisible(), false);
    assert.deepEqual(workers, [], "the worker loads only when the panel opens");
    await page.keyboard.press("Control+Backquote");
    await page.locator("#repl-panel").waitFor();
    assert.ok(
      await page.locator(".repl-input").evaluate((node) => node === document.activeElement),
    );
    assert.equal(workers.length, 1);
    await enter(page, "x := 21");
    await enter(page, "x * 2");
    await last(page, ".repl-value").waitFor({ timeout: FIRST_REPLY_MS });
    assert.equal(await last(page, ".repl-value").textContent(), "42 : usize");
    assert.equal(await last(page, ".repl-value .repl-type").textContent(), " : usize");
    assert.equal(await page.locator(".repl-status").textContent(), "Ready");

    // Echoed input is highlighted with the repository highlighter's classes.
    const echo = page.locator(".repl-log .repl-echo").nth(1);
    assert.equal(await echo.textContent(), "hd> x * 2");
    assert.equal(await echo.locator(".hl-number").textContent(), "2");
    assert.equal(await echo.locator(".hl-operator").first().textContent(), "*");

    // Ctrl+` toggles the panel closed and open again without losing the session.
    await page.keyboard.press("Control+Backquote");
    assert.equal(await page.locator("#repl-panel").isVisible(), false);
    await page.click("#repl-toggle");
    await enter(page, "x + 1");
    await page.locator(".repl-log .repl-value").nth(1).waitFor();
    assert.equal(await last(page, ".repl-value").textContent(), "22 : usize");
    assert.equal(workers.length, 1, "reopening reuses the worker");
    if (SCREENSHOTS) await page.screenshot({ path: join(SCREENSHOTS, "repl-open.png") });
    await page.context().close();
  });

  await step("blocks continue, history recalls, and commands answer", async () => {
    const { page } = await openPage("guide/index.html");
    await page.click("#repl-toggle");
    await enter(page, "n := 3");
    // A line ending in ':' continues; the empty line after the block ends it.
    await enter(page, "if n > 1:");
    assert.equal(await page.locator(".repl-input").inputValue(), "if n > 1:\n    ");
    assert.equal(await page.locator(".repl-prompt").textContent(), "hd>\n...");
    await page.locator(".repl-input").pressSequentially('println("big")');
    await page.locator(".repl-input").press("Enter");
    await page.locator(".repl-input").press("Enter");
    await last(page, ".repl-output").waitFor({ timeout: FIRST_REPLY_MS });
    assert.equal(await last(page, ".repl-output").textContent(), "big");
    // An open bracket continues too.
    await enter(page, "[n,");
    await enter(page, "n * 2]");
    await last(page, ".repl-value").waitFor();
    assert.equal(await last(page, ".repl-value").textContent(), "[3, 6] : List[usize]");

    await page.locator(".repl-input").press("ArrowUp");
    assert.equal(await page.locator(".repl-input").inputValue(), "[n,\nn * 2]");
    await page.locator(".repl-input").press("ArrowDown");
    assert.equal(await page.locator(".repl-input").inputValue(), "");

    await enter(page, ":type [n]");
    await last(page, ".repl-code").waitFor();
    assert.equal(await last(page, ".repl-code").textContent(), "List[usize]");
    await enter(page, ":help");
    await page.locator(".repl-info", { hasText: ":type EXPR" }).waitFor();
    await enter(page, ":reset");
    await page.locator(".repl-info", { hasText: "session reset" }).waitFor();
    await enter(page, "n");
    await last(page, ".repl-error").waitFor();
    assert.equal(
      await last(page, ".repl-error").textContent(),
      "1:1: unknown-name: unknown name 'n'",
    );
    await page.context().close();
  });

  await step("Stop ends an endless loop and keeps the session", async () => {
    const { page, workers } = await openPage("index.html");
    await page.click("#repl-toggle");
    await enter(page, "kept := 5");
    await enter(page, "kept");
    await last(page, ".repl-value").waitFor({ timeout: FIRST_REPLY_MS });
    await enter(page, "while true:");
    await page.locator(".repl-input").pressSequentially("pass");
    await page.locator(".repl-input").press("Enter");
    await page.locator(".repl-input").press("Enter");
    await page.locator(".repl-stop").waitFor();
    await page.waitForTimeout(300);
    await page.click(".repl-stop");
    await page.locator(".repl-error", { hasText: "stopped" }).waitFor();
    // The restarted worker replays the accepted inputs before the next one.
    await enter(page, "kept + 1");
    await page.locator(".repl-log .repl-value").nth(1).waitFor({ timeout: FIRST_REPLY_MS });
    assert.equal(await last(page, ".repl-value").textContent(), "6 : usize");
    assert.equal(workers.length, 2, "Stop replaced the worker");
    await page.context().close();
  });

  await step("an error shows in red with its position", async () => {
    const { page } = await openPage("spec/04-type-system.html", { colorScheme: "dark" });
    await page.click("#repl-toggle");
    await enter(page, "y := missing + 1");
    await last(page, ".repl-error").waitFor({ timeout: FIRST_REPLY_MS });
    assert.equal(
      await last(page, ".repl-error").textContent(),
      "1:6: unknown-name: unknown name 'missing'",
    );
    const color = await last(page, ".repl-error").evaluate((node) => getComputedStyle(node).color);
    assert.equal(color, "rgb(255, 123, 114)");
    await enter(page, "1 / 0");
    await page
      .locator(".repl-error", { hasText: "panic at 1:1: integer-division-by-zero" })
      .waitFor();
    await page.context().close();
  });

  await step(
    "Try in REPL evaluates a code block; whole programs link to the playground",
    async () => {
      const { page, workers } = await openPage("guide/learn-in-10-minutes.html");
      assert.deepEqual(workers, []);
      const block = page.locator(".code-block", { hasText: "fn greet(name: string)" }).first();
      await block.hover();
      await block.locator(".try-repl").click();
      await page.locator("#repl-panel").waitFor();
      await last(page, ".repl-output").waitFor({ timeout: FIRST_REPLY_MS });
      assert.equal(await last(page, ".repl-output").textContent(), "hello, Ada");
      const echoes = await page.locator(".repl-log .repl-echo").allTextContents();
      assert.deepEqual(echoes, [
        'hd> fn greet(name: string) -> void $ Console:\n...     println("hello, $name")',
        "hd> fn shout(name: string) -> void $ Console: println(name.upper())",
        'hd> greet("Ada")',
      ]);
      // `string.upper()` comes from std.text, so the whole snippet is accepted.
      assert.equal(await page.locator(".repl-log .repl-error").count(), 0);
      // The session keeps the snippet's declarations.
      await enter(page, 'greet("hd")');
      await page.locator(".repl-log .repl-output").nth(1).waitFor();
      assert.equal(await last(page, ".repl-output").textContent(), "hello, hd");

      const hello = page.locator(".code-block", { hasText: 'println("hello, hd-lang")' }).first();
      await hello.locator(".try-repl").click();
      await page.locator(".repl-output", { hasText: "hello, hd-lang" }).waitFor();

      const program = page.locator(".code-block", { hasText: "pub fn main!()" }).first();
      assert.equal(await program.locator(".try-repl").count(), 0);
      const link = program.locator("a.try-playground");
      assert.equal(await link.textContent(), "Open in playground");
      assert.match((await link.getAttribute("href")) ?? "", /playground\.html#code=/);
      await page.context().close();
    },
  );

  await step("the panel fits a phone-width page", async () => {
    const { page } = await openPage("spec/04-type-system.html", { width: 375 });
    await page.click("#repl-toggle");
    await page.locator("#repl-panel").waitFor();
    const overflow = await page.evaluate(
      () => document.documentElement.scrollWidth - document.documentElement.clientWidth,
    );
    assert.equal(overflow, 0);
    const box = await page.locator("#repl-panel").boundingBox();
    assert.ok(box && box.width === 375 && box.y > 0, "the panel spans the bottom of the page");
    if (SCREENSHOTS) await page.screenshot({ path: join(SCREENSHOTS, "repl-phone.png") });
    await page.context().close();
  });
  await step("spec rule IDs, callouts, and error examples fit light, dark, and phone", async () => {
    const views = [
      ["light", 1280],
      ["dark", 1280],
      ["light", 375],
      ["dark", 375],
    ] as const;
    for (const [colorScheme, width] of views) {
      const { page } = await openPage("spec/08-data-and-enums.html#r-data.embed.width", {
        colorScheme,
        width,
      });
      const view = `${colorScheme} at ${width}px`;
      const overflow = await page.evaluate(
        () => document.documentElement.scrollWidth - document.documentElement.clientWidth,
      );
      assert.equal(overflow, 0, `${view}: no horizontal page scroll`);
      const target = page.locator("#r-data\\.embed\\.width");
      assert.ok(await target.evaluate((node) => node.matches(":target")), `${view}: rule anchor`);
      assert.equal(await target.textContent(), "data.embed.width");
      // A rule ID never covers its rule's text.
      const item = page.locator("li", { has: page.locator("#r-data\\.field\\.unique") });
      const [chip, text] = await item.evaluate((node) => {
        const anchor = node.querySelector(".rule-id")!.getBoundingClientRect();
        const range = document.createRange();
        range.setStart(node.lastChild!, 0);
        range.setEnd(node.lastChild!, 1);
        return [anchor.toJSON(), range.getBoundingClientRect().toJSON()];
      });
      assert.ok(
        chip.right <= text.left || chip.left >= text.right || chip.bottom <= text.top,
        `${view}: the rule ID does not overlap the rule`,
      );
      const tint = await page
        .locator(".line-error")
        .first()
        .evaluate((node) => getComputedStyle(node).backgroundColor);
      assert.notEqual(tint, "rgba(0, 0, 0, 0)", `${view}: error lines are tinted`);
      const label = page.locator(".error-example .example-label").first();
      assert.equal(await label.textContent(), "Error example");
      const why = page.locator("blockquote.callout-why").first();
      const [border, text2] = await why.evaluate((node) => [
        getComputedStyle(node).borderLeftColor,
        getComputedStyle(node).color,
      ]);
      assert.notEqual(border, text2, `${view}: the Why callout has an accent border`);
      if (SCREENSHOTS) {
        const name = `spec-rules-${colorScheme}-${width}`;
        await page.screenshot({ path: join(SCREENSHOTS, `${name}-limits.png`) });
        await page.locator("#fields").scrollIntoViewIfNeeded();
        await page.evaluate(() => window.scrollBy(0, -70));
        await page.screenshot({ path: join(SCREENSHOTS, `${name}-fields.png`) });
      }
      // A footnote reference is small, and the notes at the page end are muted.
      const ref = page.locator(".callout-note sup.footnote-ref a").first();
      const [refSize, textSize] = await ref.evaluate((node) => [
        Number.parseFloat(getComputedStyle(node).fontSize),
        Number.parseFloat(getComputedStyle(node.closest("p")!).fontSize),
      ]);
      assert.ok(refSize < textSize, `${view}: the footnote reference is smaller than its text`);
      const [noteColor, bodyColor] = await page
        .locator("section.footnotes")
        .evaluate((node) => [getComputedStyle(node).color, getComputedStyle(document.body).color]);
      assert.notEqual(noteColor, bodyColor, `${view}: the footnotes are muted`);
      await ref.click();
      const note = page.locator("#fn-miku");
      assert.ok(await note.evaluate((node) => node.matches(":target")), `${view}: note target`);
      if (SCREENSHOTS)
        await page.screenshot({
          path: join(SCREENSHOTS, `spec-rules-${colorScheme}-${width}-footnotes.png`),
        });
      await note.locator(".footnote-backref").click();
      assert.ok(await ref.evaluate((node) => node.matches(":target")), `${view}: back-link target`);
      await page.context().close();
    }
  });
  await step("the landing page fits a phone, and its menu and theme toggle work", async () => {
    const { page } = await openPage("index.html", { width: 375 });
    const overflow = await page.evaluate(
      () => document.documentElement.scrollWidth - document.documentElement.clientWidth,
    );
    assert.equal(overflow, 0, "no horizontal page scroll");
    assert.equal(await page.locator(".topnav").isVisible(), false);
    await page.click(".menu-button");
    const spec = page.locator("#sidebar .sidebar-primary a", { hasText: "Spec" });
    await spec.waitFor({ state: "visible" });
    await page.keyboard.press("Escape");
    const background = () => page.evaluate(() => getComputedStyle(document.body).backgroundColor);
    assert.equal(await background(), "rgb(255, 255, 255)");
    await page.click(".theme-toggle");
    assert.equal(await background(), "rgb(17, 19, 24)");
    await page.reload();
    assert.equal(await background(), "rgb(17, 19, 24)", "the choice persists");
    if (SCREENSHOTS) await page.screenshot({ path: join(SCREENSHOTS, "home-phone-dark.png") });
    await page.context().close();
  });

  await step("the tour runs a page, keeps edits, resets, and moves between pages", async () => {
    const home = await openPage("index.html");
    await home.page.locator(".hero-actions a", { hasText: "Take the tour" }).click();
    await home.page.waitForURL(`${origin}${PAGES_BASE}tour/`);
    await home.page.locator(".topnav a[aria-current=page]", { hasText: "Tour" }).waitFor();
    await home.page.locator(".tour-index a").first().click();
    await home.page.waitForURL(`${origin}${PAGES_BASE}tour/1-hello/`);
    await home.page.context().close();

    const { page, workers } = await openPage("tour/1-hello/", { ready: "#tour-editor .cm-editor" });
    const output = page.locator("#tour-output");
    await page.locator("#tour-status", { hasText: "Ready" }).waitFor({ timeout: FIRST_REPLY_MS });
    assert.deepEqual(workers, [`${origin}${PAGES_BASE}${PLAYGROUND_APP_DIR}/assets/worker.js`]);
    await page.click("#tour-run");
    await output.locator(".outcome.passed").waitFor({ timeout: FIRST_REPLY_MS });
    assert.equal(
      await output.locator(".stdout").textContent(),
      "Hello, Ada! Your order has shipped.\n",
    );
    assert.equal(
      await page.locator("#sidebar .tour-contents a[aria-current=page]").textContent(),
      "1Hello, hd",
      "the contents mark the current page",
    );

    await page.locator(".tour-pager a[rel=next]").click();
    await page.waitForURL(`${origin}${PAGES_BASE}tour/2-values-and-mut/`);
    await page.locator("#tour-editor .cm-editor").waitFor();
    // Change the discount from 10 to 50 percent, one line, as a reader would.
    await page.locator(".cm-line", { hasText: "apply_discount(order, 10)" }).click();
    await page.keyboard.press("End");
    await page.keyboard.press("Backspace");
    await page.keyboard.press("Backspace");
    await page.keyboard.press("Backspace");
    await page.keyboard.type("50)");
    await page.keyboard.press("Control+Enter");
    await output.locator(".outcome.passed").waitFor({ timeout: FIRST_REPLY_MS });
    assert.equal(await output.locator(".stdout").textContent(), "A-1042: 2500 cents\n");

    // The edit survives a reload; Reset brings back the original.
    await page.reload();
    await page.locator(".cm-line", { hasText: "apply_discount(order, 50)" }).waitFor();
    await page.click("#tour-reset");
    await page.locator(".cm-line", { hasText: "apply_discount(order, 10)" }).waitFor();
    await page.locator("#tour-status", { hasText: "Ready" }).waitFor({ timeout: FIRST_REPLY_MS });
    await page.click("#tour-run");
    await output.locator(".outcome.passed").waitFor({ timeout: FIRST_REPLY_MS });
    assert.equal(await output.locator(".stdout").textContent(), "A-1042: 4500 cents\n");
    await page.reload();
    await page.locator(".cm-line", { hasText: "apply_discount(order, 10)" }).waitFor();

    // The page's edit, deleting `mut` from the parameter, lists its diagnostic.
    await page
      .locator(".cm-line", { hasText: "fn apply_discount(order: mut Order" })
      .locator(".hd-keyword", { hasText: /^mut$/ })
      .dblclick();
    await page.keyboard.press("Backspace");
    assert.ok(
      await page.locator(".cm-line", { hasText: "fn apply_discount(order:  Order" }).isVisible(),
    );
    await page.click("#tour-run");
    await output.locator(".outcome.failed").waitFor({ timeout: FIRST_REPLY_MS });
    assert.equal(await output.locator(".diagnostic .code").textContent(), "readonly-root");
    await page.click("#tour-reset");

    // Alt+Left goes back a page when the focus is outside the editor.
    await page.locator("h1").click();
    await page.keyboard.press("Alt+ArrowLeft");
    await page.waitForURL(`${origin}${PAGES_BASE}tour/1-hello/`);
    await page.keyboard.press("Alt+ArrowRight");
    await page.waitForURL(`${origin}${PAGES_BASE}tour/2-values-and-mut/`);
    if (SCREENSHOTS) await page.screenshot({ path: join(SCREENSHOTS, "tour-desktop.png") });
    await page.context().close();
  });

  await step("a tour page fits a phone in light and dark", async () => {
    for (const colorScheme of ["light", "dark"] as const) {
      const { page } = await openPage("tour/1-hello/", {
        colorScheme,
        width: 375,
        ready: "#tour-editor .cm-editor",
      });
      const overflow = await page.evaluate(
        () => document.documentElement.scrollWidth - document.documentElement.clientWidth,
      );
      assert.equal(overflow, 0, `${colorScheme}: no horizontal page scroll`);
      const editor = await page.locator(".split-aside").boundingBox();
      assert.ok(
        editor && editor.width <= 375 && editor.height >= 300,
        `${colorScheme}: editor box`,
      );
      const background = await page
        .locator(".tour-code .cm-editor")
        .evaluate((node) => getComputedStyle(node).backgroundColor);
      assert.equal(
        background,
        colorScheme === "dark" ? "rgb(27, 30, 39)" : "rgb(246, 246, 249)",
        `${colorScheme}: the editor uses the site's code background`,
      );
      await page.locator("#tour-status", { hasText: "Ready" }).waitFor({ timeout: FIRST_REPLY_MS });
      await page.click("#tour-run");
      await page.locator("#tour-output .outcome.passed").waitFor({ timeout: FIRST_REPLY_MS });
      if (SCREENSHOTS) {
        await page.locator(".split-aside").scrollIntoViewIfNeeded();
        await page.screenshot({ path: join(SCREENSHOTS, `tour-phone-${colorScheme}.png`) });
      }
      await page.context().close();
    }
  });

  await step("a tour page with tests has a Test button, and wrapped lines hang", async () => {
    const { page } = await openPage("tour/12-requirements/", { ready: "#tour-editor .cm-editor" });
    const output = page.locator("#tour-output");
    await page.locator("#tour-status", { hasText: "Ready" }).waitFor({ timeout: FIRST_REPLY_MS });
    await page.click("#tour-test");
    await output.locator(".outcome.passed", { hasText: "1 test passed" }).waitFor({
      timeout: FIRST_REPLY_MS,
    });
    await page.click("#tour-run");
    await output.locator(".outcome.passed").waitFor({ timeout: FIRST_REPLY_MS });
    assert.equal(await output.locator(".stdout").textContent(), "Reminder: invoice INV-7 is due\n");

    // A wrapped line hangs one step under its own indentation: 8 spaces + 4.
    const hang = await page
      .locator(".cm-line", { hasText: "assert_equal(console.output()" })
      .evaluate((line) => {
        const style = getComputedStyle(line);
        const rows = Math.round(line.getBoundingClientRect().height / parseFloat(style.lineHeight));
        return {
          padding: parseFloat(style.paddingLeft),
          indent: parseFloat(style.textIndent),
          rows,
        };
      });
    assert.ok(hang.rows > 1, "the long assertion line wraps in the editor column");
    assert.ok(hang.indent < 0 && Math.abs(hang.padding + hang.indent - 6) < 0.5, "hang");
    await page.context().close();

    // A page with only tests has no Run button, and Ctrl+Enter tests.
    const tests = await openPage("tour/13-tests/", { ready: "#tour-editor .cm-editor" });
    assert.equal(await tests.page.locator("#tour-run").count(), 0, "no Run button");
    await tests.page.locator("#tour-status", { hasText: "Ready" }).waitFor({
      timeout: FIRST_REPLY_MS,
    });
    await tests.page.locator("h1").click();
    await tests.page.keyboard.press("Control+Enter");
    await tests.page
      .locator("#tour-output .outcome.passed", { hasText: "3 tests passed" })
      .waitFor({ timeout: FIRST_REPLY_MS });
    await tests.page.context().close();
  });

  await step(
    "a tour page with several files shows a tab per file and runs them together",
    async () => {
      const { page } = await openPage("tour/14-modules/", { ready: "#tour-editor .cm-editor" });
      const output = page.locator("#tour-output");
      const tab = (name: string) => page.locator(".tour-tab", { hasText: name });
      assert.equal(await tab("main.hd").getAttribute("aria-selected"), "true");
      await page.locator("#tour-status", { hasText: "Ready" }).waitFor({ timeout: FIRST_REPLY_MS });
      await page.click("#tour-run");
      await output.locator(".outcome.passed").waitFor({ timeout: FIRST_REPLY_MS });
      assert.equal(await output.locator(".stdout").textContent(), "total: 4149 cents\n");

      // Break pricing.hd: its fee becomes a string.
      await tab("pricing.hd").click();
      assert.equal(await tab("pricing.hd").getAttribute("aria-selected"), "true");
      await page.locator(".cm-line", { hasText: "if subtotal >= 5000: 0 else: 499" }).click();
      await page.keyboard.press("End");
      for (let index = 0; index < 3; index += 1) await page.keyboard.press("Backspace");
      await page.keyboard.type('"free"');
      await tab("main.hd").click();
      await page.locator(".cm-line", { hasText: "use pkg.pricing.{Line, total}  #" }).waitFor();
      await page.click("#tour-run");
      await output.locator(".outcome.failed").waitFor({ timeout: FIRST_REPLY_MS });
      const location = await output.locator(".diagnostic .location").first().textContent();
      assert.match(location ?? "", /^src\/pricing\.hd:/);
      // The diagnostic's jump opens the file it is in.
      await output.locator(".diagnostic .jump").first().click();
      assert.equal(await tab("pricing.hd").getAttribute("aria-selected"), "true");

      // Each file's edit survives a reload; Reset restores every file.
      await page.reload();
      await page.locator("#tour-editor .cm-editor").waitFor();
      await tab("pricing.hd").click();
      await page.locator(".cm-line", { hasText: 'else: "free"' }).waitFor();
      await page.click("#tour-reset");
      assert.equal(await tab("main.hd").getAttribute("aria-selected"), "true");
      await tab("pricing.hd").click();
      await page.locator(".cm-line", { hasText: "else: 499" }).waitFor();
      await page.locator("#tour-status", { hasText: "Ready" }).waitFor({ timeout: FIRST_REPLY_MS });
      await page.click("#tour-run");
      await output.locator(".outcome.passed").waitFor({ timeout: FIRST_REPLY_MS });
      await page.context().close();
    },
  );

  await step("the Playground link opens the playground with the code it carries", async () => {
    const home = await openPage("index.html");
    await home.page.locator(".topnav a", { hasText: "Playground" }).click();
    await home.page.waitForURL(`${origin}${PAGES_BASE}${PLAYGROUND_PAGE}`);
    await home.page.context().close();
    const source = 'pub fn main() -> void $ Console:\n    println("from the site")\n';
    const hash = `#code=${encodeBase64Url(source)}`;
    const { page } = await openPage(`${PLAYGROUND_PAGE}${hash}`);
    const frame = page.frameLocator("#playground-frame");
    await frame.getByText("Compiler ready").waitFor({ timeout: FIRST_REPLY_MS });
    const app = `${PAGES_BASE}${PLAYGROUND_APP_DIR}/${hash}`;
    // The page has no title bar; the framed app's Full screen link carries the code.
    assert.equal(await page.locator(".sidebar").isVisible(), false);
    assert.equal(await frame.locator("#fullscreen").getAttribute("href"), `${origin}${app}`);
    await frame.locator("#run").click();
    await frame.locator(".outcome.passed").waitFor();
    assert.equal(await frame.locator(".stdout").textContent(), "from the site\n");
    await page.context().close();
  });

  await playgroundSteps({
    browser,
    origin,
    base: `${PAGES_BASE}${PLAYGROUND_APP_DIR}/`,
    step,
    screenshots: SCREENSHOTS,
  });
  console.log(`${passed} passed`);
} finally {
  await browser.close();
  server.close();
}
