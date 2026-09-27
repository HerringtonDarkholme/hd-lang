// End-to-end check of the built playground in a headless Chromium:
//
//   npm run playground:build && npm run playground:e2e
//
// dist/ is served under /hd-lang/playground/, the GitHub Pages path, to prove
// the build only uses relative URLs. Set CHROME_PATH to pick the browser;
// otherwise a local Chrome, Edge, or Playwright Chromium is used. Set
// E2E_SCREENSHOTS to a directory to save screenshots there.

import assert from "node:assert/strict";
import { existsSync } from "node:fs";
import { readFile } from "node:fs/promises";
import { createServer } from "node:http";
import { homedir } from "node:os";
import { extname, join, normalize } from "node:path";

import { chromium, type Page } from "playwright-core";

import { classify } from "../src/highlight.ts";
import { DIST } from "./build.ts";
import { encodeBase64Url } from "./src/share.ts";

const BASE = "/hd-lang/playground/";
/** A directory for screenshots, when set. */
const SCREENSHOTS = process.env.E2E_SCREENSHOTS;
const TYPES: Record<string, string> = {
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".css": "text/css; charset=utf-8",
  ".txt": "text/plain; charset=utf-8",
};

function browserPath(): string {
  const candidates = [
    process.env.CHROME_PATH,
    "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
    "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
    "/usr/bin/google-chrome",
    "/usr/bin/chromium",
    "/usr/bin/chromium-browser",
    join(
      homedir(),
      "Library/Caches/ms-playwright/chromium-1208/chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing",
    ),
  ];
  const found = candidates.find((candidate) => candidate && existsSync(candidate));
  if (!found) throw new Error("no Chromium found; set CHROME_PATH");
  return found;
}

const server = createServer(async (request, response) => {
  const url = new URL(request.url ?? "/", "http://localhost");
  if (!url.pathname.startsWith(BASE)) {
    response.writeHead(404).end();
    return;
  }
  const relative = normalize(url.pathname.slice(BASE.length) || "index.html");
  try {
    const body = await readFile(
      join(DIST, relative.endsWith("/") ? `${relative}index.html` : relative),
    );
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
const results: string[] = [];

async function step(name: string, body: () => Promise<void>): Promise<void> {
  await body();
  results.push(`ok - ${name}`);
  console.log(`ok - ${name}`);
}

async function openPage(hash = "", colorScheme: "light" | "dark" = "light"): Promise<Page> {
  const context = await browser.newContext({ colorScheme, viewport: { width: 1280, height: 800 } });
  await context.grantPermissions(["clipboard-read", "clipboard-write"], { origin });
  const page = await context.newPage();
  page.on("pageerror", (error) => console.error("page error:", error.message));
  await page.goto(`${origin}${BASE}${hash}`);
  await page.getByText("Compiler ready").waitFor({ timeout: 60_000 });
  return page;
}

const code = (source: string): string => `#code=${encodeBase64Url(source)}`;
const outcome = (page: Page) => page.locator(".outcome").first();

try {
  await step("hello world runs and prints", async () => {
    const page = await openPage(
      code('pub fn main() -> void $ Console:\n    println("hello, world")\n'),
    );
    await page.click("#run");
    await page.locator(".outcome.passed").waitFor();
    assert.equal(await page.locator(".stdout").textContent(), "hello, world\n");
    await page.context().close();
  });

  await step("Ctrl+Enter runs; a type error jumps to its line", async () => {
    const source =
      'pub fn main() -> void $ Console:\n    println("start")\n    let count: i32 = "three"\n';
    const page = await openPage(code(source));
    await page.click(".cm-content");
    await page.keyboard.press("Control+Enter");
    await page.locator(".outcome.failed").waitFor();
    const location = page.locator(".diagnostic .location").first();
    assert.equal(await location.textContent(), "src/main.hd:3:22");
    assert.equal(await page.locator(".diagnostic .code").first().textContent(), "type-mismatch");
    await page.locator(".diagnostic .jump").first().click();
    assert.equal(
      await page.locator(".cm-activeLine").textContent(),
      '    let count: i32 = "three"',
    );
    assert.equal(await page.locator(".cm-lintRange-error").count(), 1);
    await page.click("#check");
    await page.locator(".outcome.failed").waitFor();
    await page.context().close();
  });

  await step("Stop ends an endless loop and the next run works", async () => {
    const page = await openPage(code("pub fn main() -> void:\n    while true:\n        pass\n"));
    await page.click("#run");
    await page.locator("#stop").waitFor();
    await page.waitForTimeout(300);
    await page.click("#stop");
    assert.equal(await page.locator(".outcome.failed").textContent(), "Stopped.");
    // A new #code= hash on the same page loads it (hashchange); the restarted
    // worker then runs it.
    const again = code('pub fn main() -> void $ Console:\n    println("again")\n');
    await page.evaluate((hash) => (location.hash = hash), again);
    await page.getByText("Loaded the shared project").waitFor();
    await page.click("#run");
    await page.locator(".outcome.passed").waitFor();
    assert.equal(await page.locator(".stdout").textContent(), "again\n");
    await page.context().close();
  });

  await step("a runtime panic is reported", async () => {
    const page = await openPage();
    await page.selectOption("#examples", "panic");
    await page.click("#run");
    await page.locator(".outcome.panic").waitFor();
    assert.match(
      (await outcome(page).textContent()) ?? "",
      /integer-division-by-zero: runtime panic in main/,
    );
    assert.match((await page.locator(".stdout").textContent()) ?? "", /average of 10 over 2: 5/);
    await page.context().close();
  });

  await step("the multi-file example runs; tabs add, rename, and share", async () => {
    const page = await openPage();
    await page.selectOption("#examples", "package");
    assert.deepEqual(await page.locator(".tab-name").allTextContents(), [
      "main.hd",
      "models/mod.hd",
      "models/user.hd",
    ]);
    await page.click("#run");
    await page.locator(".outcome.passed").waitFor();
    assert.equal(
      await page.locator(".stdout").textContent(),
      "hello Ada, visit 4 (regular)\nhello Grace, visit 2\n",
    );
    await page.click(".tab-add");
    const rename = page.locator(".tab-rename");
    await rename.fill("src/util/text.hd");
    await rename.press("Enter");
    assert.ok((await page.locator(".tab-name").allTextContents()).includes("util/text.hd"));
    await page.click("#share");
    await page.getByText("Link copied").waitFor();
    const hash = await page.evaluate(() => location.hash);
    assert.match(hash, /^#project=[A-Za-z0-9_-]+$/);
    const clipboard = await page.evaluate(() => navigator.clipboard.readText());
    assert.ok(clipboard.endsWith(hash));
    const reopened = await openPage(hash);
    assert.equal(await reopened.locator(".tab-name").count(), 4);
    await reopened.click("#run");
    await reopened.locator(".outcome.passed").waitFor();
    await reopened.context().close();
    await page.context().close();
  });

  await step("editor token classes match classify", async () => {
    const source = [
      'println("total ${count + 1} for $name")  # note',
      'raw := r"C:\\dir\\${x}"',
      "fn read!(path: string) -> List[Map[string, i64]] $ Files:",
      "    ok := !done && (ready || `match`) != false",
      "use pkg.a.{B as C}",
      "fn make[reified T](value: T) -> T: value",
      "big := 1_000i64 + 0xFF + 2.5e3",
    ].join("\n");
    const page = await openPage(code(source));
    const lines = await page.$$eval(".cm-line", (nodes) =>
      nodes.map((line) => {
        const runs: [string, string][] = [];
        const walk = (node: Node, className: string): void => {
          if (node.nodeType === Node.TEXT_NODE) {
            const last = runs.at(-1);
            if (last && last[1] === className) last[0] += node.textContent;
            else runs.push([node.textContent ?? "", className]);
            return;
          }
          const own = [...(node as Element).classList].find((name) => name.startsWith("hd-"));
          for (const child of node.childNodes) walk(child, own ?? className);
        };
        for (const child of line.childNodes) walk(child, "");
        return runs.filter(([text]) => text !== "");
      }),
    );
    const expected = source.split("\n").map((line) => {
      const runs: [string, string][] = [];
      for (const { text, kind } of classify(line)) {
        const className = kind === "plain" ? "" : `hd-${kind}`;
        const last = runs.at(-1);
        if (last && last[1] === className) last[0] += text;
        else runs.push([text, className]);
      }
      return runs;
    });
    assert.deepEqual(lines, expected);
    await page.context().close();
  });

  await step("dark mode and a phone-width layout", async () => {
    const page = await openPage("", "dark");
    const background = await page.evaluate(() => getComputedStyle(document.body).backgroundColor);
    assert.equal(background, "rgb(20, 23, 29)");
    await page.setViewportSize({ width: 375, height: 740 });
    const overflow = await page.evaluate(
      () => document.documentElement.scrollWidth - document.documentElement.clientWidth,
    );
    assert.equal(overflow, 0);
    if (SCREENSHOTS) await page.screenshot({ path: join(SCREENSHOTS, "phone-dark.png") });
    await page.context().close();
    const light = await openPage();
    await light.click("#run");
    await light.locator(".outcome.passed").waitFor();
    if (SCREENSHOTS) await light.screenshot({ path: join(SCREENSHOTS, "desktop-light.png") });
    await light.context().close();
  });
  console.log(`${results.length} passed`);
} finally {
  await browser.close();
  server.close();
}
