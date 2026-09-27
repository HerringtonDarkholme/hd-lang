// End-to-end check of the site's bottom REPL panel in a headless Chromium:
//
//   npm run playground:build && npm run website:build && npm run website:e2e
//
// website/dist/ is served under /hd-lang/, the GitHub Pages base it is built
// for. The panel's worker is the playground build's compiler worker. Set
// CHROME_PATH to pick the browser, and E2E_SCREENSHOTS to a directory to save
// screenshots there.

import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { createServer } from "node:http";
import { dirname, extname, join, normalize } from "node:path";
import { fileURLToPath } from "node:url";

import { chromium, type Page } from "playwright-core";

import { browserPath } from "../playground/test/chrome.ts";
import { PAGES_BASE } from "./build.ts";

const DIST = join(dirname(fileURLToPath(import.meta.url)), "dist");
const SCREENSHOTS = process.env.E2E_SCREENSHOTS;
const TYPES: Record<string, string> = {
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".css": "text/css; charset=utf-8",
  ".json": "application/json",
  ".svg": "image/svg+xml",
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
  options: { colorScheme?: "light" | "dark"; width?: number } = {},
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
  await page.locator("#repl-toggle").waitFor();
  return { page, workers };
}

/** Types one input line into the panel and presses Enter. */
async function enter(page: Page, text: string): Promise<void> {
  await page.locator(".repl-input").pressSequentially(text);
  await page.locator(".repl-input").press("Enter");
}

const last = (page: Page, selector: string) => page.locator(`.repl-log ${selector}`).last();

try {
  await step("the REPL opens on a spec page and evaluates x * 2 as 42 : i32", async () => {
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
    assert.equal(await last(page, ".repl-value").textContent(), "42 : i32");
    assert.equal(await last(page, ".repl-value .repl-type").textContent(), " : i32");
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
    assert.equal(await last(page, ".repl-value").textContent(), "22 : i32");
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
    assert.equal(await last(page, ".repl-value").textContent(), "[3, 6] : List[i32]");

    await page.locator(".repl-input").press("ArrowUp");
    assert.equal(await page.locator(".repl-input").inputValue(), "[n,\nn * 2]");
    await page.locator(".repl-input").press("ArrowDown");
    assert.equal(await page.locator(".repl-input").inputValue(), "");

    await enter(page, ":type [n]");
    await last(page, ".repl-code").waitFor();
    assert.equal(await last(page, ".repl-code").textContent(), "List[i32]");
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
    assert.equal(await last(page, ".repl-value").textContent(), "6 : i32");
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
    await page.locator(".repl-error", { hasText: "panic: integer-division-by-zero" }).waitFor();
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
      // The prototype has no string.upper(), so that input is rejected, in red
      // with its position, and the rest of the snippet still runs.
      assert.equal(
        await page.locator(".repl-log .repl-error").first().textContent(),
        "1:51: unknown-method: type 'string' has no supported method 'upper'",
      );
      // The session keeps the snippet's accepted declarations.
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
      await page.context().close();
    }
  });
  console.log(`${passed} passed`);
} finally {
  await browser.close();
  server.close();
}
