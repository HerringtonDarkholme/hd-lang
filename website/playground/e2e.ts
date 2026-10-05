// The playground's end-to-end steps, which `pnpm run website:e2e` runs in a
// headless Chromium after the website's own steps:
//
//   pnpm run website:build && pnpm run website:e2e
//
// The site is served under /hd-lang/, the GitHub Pages base, so the
// playground runs under /hd-lang/playground/. That proves its build only uses
// relative URLs.

import assert from "node:assert/strict";
import { join } from "node:path";

import type { Browser, Page } from "playwright-core";

import { classify } from "../../src/highlight.ts";
import { encodeBase64Url } from "./src/share.ts";

interface PlaygroundE2e {
  readonly browser: Browser;
  /** The origin serving the site, such as `http://127.0.0.1:4173`. */
  readonly origin: string;
  /** The playground's URL path, such as `/hd-lang/playground/`. */
  readonly base: string;
  /** Runs one named check and reports it. */
  readonly step: (name: string, body: () => Promise<void>) => Promise<void>;
  /** A directory for screenshots, when set. */
  readonly screenshots?: string;
}

/** Drives the built playground at `origin + base` through every check. */
export async function playgroundSteps(options: PlaygroundE2e): Promise<void> {
  const { browser, origin, base, step, screenshots } = options;

  async function openPage(hash = "", colorScheme: "light" | "dark" = "light"): Promise<Page> {
    const context = await browser.newContext({
      colorScheme,
      viewport: { width: 1280, height: 800 },
    });
    await context.grantPermissions(["clipboard-read", "clipboard-write"], { origin });
    const page = await context.newPage();
    page.on("pageerror", (error) => console.error("page error:", error.message));
    await page.goto(`${origin}${base}${hash}`);
    await page.getByText("Compiler ready").waitFor({ timeout: 60_000 });
    return page;
  }

  const code = (source: string): string => `#code=${encodeBase64Url(source)}`;
  const outcome = (page: Page) => page.locator(".outcome").first();

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

  await step("without main, Run prints top-level expression values", async () => {
    const source = 'x := 21\nprintln("start")\nx * 2\n[x, x + 1]\n';
    const page = await openPage(code(source));
    await page.click("#run");
    await page.locator(".outcome.passed").waitFor();
    assert.equal(
      await page.locator(".stdout").textContent(),
      "start\n42 : usize\n[21, 22] : List[usize]\n",
    );
    assert.match((await outcome(page).textContent()) ?? "", /ran 4 top-level inputs/);
    await page.context().close();
  });

  await step("an error without main points at its line; Test and exit codes report", async () => {
    const page = await openPage(code("x := 1\ny := missing\n"));
    await page.click("#run");
    await page.locator(".outcome.failed").waitFor();
    assert.equal(
      await page.locator(".diagnostic .location").first().textContent(),
      "src/main.hd:2:6",
    );
    const tests = [
      "use std.testing.assert_equal",
      "",
      "pub fn main() -> void $ Console:",
      '    println("main")',
      "",
      "tests:",
      '    it("adds"):',
      '        assert_equal(1 + 1, 2, reason="sum")',
    ].join("\n");
    await page.evaluate((hash) => (location.hash = hash), code(tests));
    await page.getByText("Loaded the shared project").waitFor();
    await page.click("#test");
    await page.locator(".outcome.passed").waitFor();
    assert.match((await outcome(page).textContent()) ?? "", /1 test passed/);
    await page.selectOption("#examples", "tests");
    await page.click("#test");
    await page.locator(".outcome.passed").waitFor();
    assert.match((await outcome(page).textContent()) ?? "", /5 tests passed/);
    await page.selectOption("#examples", "exit-code");
    await page.click("#run");
    await page.locator(".outcome.failed").waitFor();
    assert.equal(await outcome(page).textContent(), "✗ main exited with code 1");
    await page.context().close();
  });

  await step("the WAT view shows the module, copies, downloads, and reports errors", async () => {
    const page = await openPage();
    await page.selectOption("#examples", "hello");
    assert.equal(await page.locator("#wat-copy").isVisible(), false);
    await page.click("#view-wat");
    assert.equal(await page.locator("#view-wat").getAttribute("aria-selected"), "true");
    assert.equal(await page.locator("#output").isVisible(), false);
    const module = page.locator(".wat-code");
    await module.waitFor();
    const text = (await module.textContent()) ?? "";
    assert.ok(text.startsWith("(module"), text.slice(0, 40));
    assert.match(text, /\(func \(export "main"\) \(param \$provider0 externref\)/);
    for (const kind of ["keyword", "instruction", "type", "name", "string", "number"])
      assert.ok((await page.locator(`.wat-code .wat-${kind}`).count()) > 0, kind);
    const main = page.locator(".wat-line", { hasText: '(export "main")' }).first();
    assert.equal(await main.locator(".wat-keyword").first().textContent(), "func");
    if (screenshots) await page.screenshot({ path: join(screenshots, "desktop-wat.png") });
    await page.click("#wat-copy");
    await page.getByText("WAT copied").waitFor();
    const copied = await page.evaluate(() => navigator.clipboard.readText());
    assert.ok(copied.startsWith("(module"));
    // The view shows at most the first 5,000 lines; the copy has them all.
    const copiedLines = copied.replace(/\n$/, "").split("\n").length;
    assert.equal(await page.locator(".wat-line").count(), Math.min(copiedLines, 5000));
    assert.equal(await page.locator(".wat-truncated").isVisible(), copiedLines > 5000);
    const [download] = await Promise.all([
      page.waitForEvent("download"),
      page.click("#wat-download"),
    ]);
    assert.equal(download.suggestedFilename(), "main.wat");
    // A type error replaces the module with its diagnostics.
    const broken = code('pub fn main() -> void:\n    let x: i32 = "no"\n');
    await page.evaluate((hash) => (location.hash = hash), broken);
    await page.locator("#wat .outcome.failed").waitFor();
    assert.equal(await page.locator(".wat-code").count(), 0);
    assert.equal(
      await page.locator("#wat .diagnostic .location").textContent(),
      "src/main.hd:2:18",
    );
    assert.equal(await page.locator("#wat-copy").isDisabled(), true);
    // Fixing it in the editor brings the module back.
    await page.click(".cm-content");
    await page.keyboard.press("ControlOrMeta+a");
    await page.keyboard.insertText("pub fn main() -> void:\n    pass\n");
    await page.locator(".wat-code").waitFor();
    await page.click("#view-output");
    assert.equal(await page.locator("#wat").isVisible(), false);
    assert.equal(await page.locator("#clear").isVisible(), true);
    await page.context().close();
  });

  await step("without main, the WAT view shows the last module Run compiled", async () => {
    const page = await openPage();
    await page.selectOption("#examples", "top-level");
    await page.click("#view-wat");
    await page.getByText("Run the project to see the last one").waitFor();
    await page.click("#run");
    await page.locator(".wat-code").waitFor();
    assert.match(
      (await page.locator(".wat-code").textContent()) ?? "",
      /^\(module[\s\S]*\(export "main"\)/,
    );
    await page.click("#view-output");
    await page.locator(".outcome.passed").waitFor();
    assert.match((await page.locator(".stdout").textContent()) ?? "", /\[7, 12\] : List\[i32\]/);
    await page.context().close();
  });

  await step("editor token classes match classify", async () => {
    const source = [
      'println("total ${count + 1} for $name")  # note',
      'raw := r"C:\\dir\\${x}"',
      "fn read!(path: string) -> List[Map[string, i64]] $ Files:",
      "    ok := !done && (ready || `match`) != false",
      "use pkg.a.{B as C}",
      "fn make[T](value: T) -> T: value",
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
    if (screenshots) await page.screenshot({ path: join(screenshots, "phone-dark.png") });
    await page.click("#view-wat");
    await page.locator(".wat-code").waitFor();
    const watOverflow = await page.evaluate(
      () => document.documentElement.scrollWidth - document.documentElement.clientWidth,
    );
    assert.equal(watOverflow, 0);
    assert.notEqual(
      await page
        .locator(".wat-keyword")
        .first()
        .evaluate((node) => getComputedStyle(node).color),
      await page
        .locator(".wat-name")
        .first()
        .evaluate((node) => getComputedStyle(node).color),
    );
    if (screenshots) await page.screenshot({ path: join(screenshots, "phone-dark-wat.png") });
    await page.context().close();
    const light = await openPage();
    await light.click("#run");
    await light.locator(".outcome.passed").waitFor();
    if (screenshots) await light.screenshot({ path: join(screenshots, "desktop-light.png") });
    await light.context().close();
  });
}
