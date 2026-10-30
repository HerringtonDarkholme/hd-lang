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

import type { Browser, Locator, Page } from "playwright-core";

import { classify } from "../src/highlight.ts";
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

/**
 * Runs a program that fetches `pageUrl`, of the playground's own origin, and
 * another origin, which the grant refuses before any request
 * (spec/std/http.md#playground). No request leaves 127.0.0.1.
 */
async function sameOriginHttp(
  open: (source: string) => Promise<Page>,
  pageUrl: string,
): Promise<void> {
  const source = [
    "use std.http.{Http, get}",
    "",
    "fn status!(url: string) -> string $ Http:",
    "    match get!(url):",
    '        .Ok(response) => "status ${response.status.code()}"',
    '        .Err(.NotGranted(host)) => "no ${host}"',
    '        .Err(error) => "${error}"',
    "",
    "pub fn main!() -> void $ Console + Http:",
    `    println(status!("${pageUrl}"))`,
    '    println(status!("https://example.com/"))',
    "",
  ].join("\n");
  const page = await open(source);
  await page.click("#run");
  await page.locator(".outcome.passed").waitFor();
  assert.equal(await page.locator(".stdout").textContent(), "status 200\nno example.com\n");
  await page.context().close();
}

/** Opens a program in the editor and checks its token classes against `classify`. */
async function editorClassesMatch(open: (source: string) => Promise<Page>): Promise<void> {
  const source = [
    'println("total ${count + 1} for $name")  # note',
    'raw := r"C:\\dir\\${x}"',
    "fn read!(path: string) -> List[Map[string, i64]] $ Files:",
    "    ok := !done && (ready || `match`) != false",
    "use pkg.a.{B as C}",
    "fn make[T](value: T) -> T: value",
    "big := 1_000i64 + 0xFF + 2.5e3",
  ].join("\n");
  const page = await open(source);
  const lines = await page.$$eval(".cm-line", (nodes) =>
    nodes.map((line) => {
      const runs: [string, string][] = [];
      const walk = (node: Node, className: string): void => {
        if (node.nodeType === Node.TEXT_NODE) {
          const last = runs.at(-1);
          if (last && last[1] === className) {
            last[0] += node.textContent;
          } else {
            runs.push([node.textContent ?? "", className]);
          }
          return;
        }
        const own = [...(node as Element).classList].find((name) => name.startsWith("hd-"));
        for (const child of node.childNodes) {
          walk(child, own ?? className);
        }
      };
      for (const child of line.childNodes) {
        walk(child, "");
      }
      return runs.filter(([text]) => text !== "");
    }),
  );
  const expected = source.split("\n").map((line) => {
    const runs: [string, string][] = [];
    for (const { text, kind } of classify(line)) {
      const className = kind === "plain" ? "" : `hd-${kind}`;
      const last = runs.at(-1);
      if (last && last[1] === className) {
        last[0] += text;
      } else {
        runs.push([text, className]);
      }
    }
    return runs;
  });
  assert.deepEqual(lines, expected);
  await page.context().close();
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

  await step("Http fetches the playground's own origin, and refuses another", () =>
    sameOriginHttp((source) => openPage(code(source)), `${origin}${base}`),
  );

  await step("Ctrl+Enter runs; a type error jumps to its line", async () => {
    // `count` is read, so the type error is the only diagnostic.
    const source =
      'pub fn main() -> void $ Console:\n    println("start")\n    let count: i32 = "three"\n    println(count)\n';
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
    // A program runs in the run worker, so a Stop leaves the compiler's
    // worker, and its warm store, in place (#254).
    const workers: string[] = [];
    const page = await openPage(code("pub fn main() -> void:\n    while true:\n        pass\n"));
    page.on("worker", (worker) => workers.push(new URL(worker.url()).pathname.split("/").pop()!));
    const before = workers.length;
    // Stop only once the program runs: the run worker loads when the
    // compile is done, so a Stop before it would end the compiler's worker.
    const running = page.waitForEvent("worker", {
      predicate: (worker) => worker.url().endsWith("/run-worker.js"),
    });
    await page.click("#run");
    await running;
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
    assert.deepEqual(
      workers.slice(before).filter((name) => name === "worker.js"),
      [],
      "a Stop restarts no compiler worker",
    );
    assert.equal(workers.filter((name) => name === "run-worker.js").length, 2);
    await page.context().close();
  });

  await step("a runtime panic is reported", async () => {
    const page = await openPage();
    await page.selectOption("#examples", "panic");
    await page.click("#run");
    await page.locator(".outcome.panic").waitFor();
    assert.match(
      (await outcome(page).textContent()) ?? "",
      /integer-division-by-zero: division by zero/,
    );
    assert.match((await page.locator(".stdout").textContent()) ?? "", /average of 10 over 2: 5/);
    await page.context().close();
  });

  await step("Clock and Random run in the browser; FsRead stops before running", () =>
    hostCapabilities(openPage, code, outcome),
  );

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

  await step("without main, Run runs the top-level statements as a script", async () => {
    const source = 'x := 21\nprintln("start")\nprintln(x * 2)\n[x, x + 1]\n';
    const page = await openPage(code(source));
    await page.click("#run");
    await page.locator(".outcome.passed").waitFor();
    assert.equal(await page.locator(".stdout").textContent(), "start\n42\n");
    assert.match((await outcome(page).textContent()) ?? "", /exited normally/);
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
    assert.match(text, /\(func \$playground\/main\/main#0 /);
    for (const kind of ["keyword", "instruction", "type", "name", "string", "number"]) {
      assert.ok((await page.locator(`.wat-code .wat-${kind}`).count()) > 0, kind);
    }
    const main = page.locator(".wat-line", { hasText: "(func $playground/main/main#0 " }).first();
    assert.equal(await main.locator(".wat-keyword").first().textContent(), "func");
    if (screenshots) {
      await page.screenshot({ path: join(screenshots, "desktop-wat.png") });
    }
    await page.click("#wat-copy");
    await page.getByText("WAT copied").waitFor();
    const copied = await page.evaluate(() => navigator.clipboard.readText());
    assert.ok(copied.startsWith("(module"));
    // The view shows at most the first 10,000 lines; the copy has them all.
    const copiedLines = copied.replace(/\n$/, "").split("\n").length;
    assert.equal(await page.locator(".wat-line").count(), Math.min(copiedLines, 10_000));
    assert.equal(await page.locator(".wat-truncated").isVisible(), copiedLines > 10_000);
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
      await page
        .locator("#wat .diagnostic", { hasText: "type-mismatch" })
        .locator(".location")
        .textContent(),
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

  await step("the WAT view shows the module of every example", async () => {
    const page = await openPage();
    await page.click("#view-wat");
    const ids = await page
      .locator("#examples option")
      .evaluateAll((options) =>
        options
          .map((option) => (option as HTMLOptionElement).value)
          .filter((value) => value !== ""),
      );
    assert.ok(ids.length >= 15, `${ids.length} examples`);
    let previous = "";
    for (const id of ids) {
      await page.selectOption("#examples", id);
      // The view changes to the new example's module, or to the errors that
      // stop it.
      await page.waitForFunction((before) => {
        const view = document.querySelector("#wat");
        const shown = view?.querySelector(".wat-code, .outcome.failed");
        return shown !== null && shown !== undefined && shown.textContent !== before;
      }, previous);
      const failed = await page.locator("#wat .outcome.failed").count();
      if (failed > 0) {
        assert.equal(id, "missing-provider", `${id} does not compile`);
        previous = (await page.locator("#wat .outcome.failed").textContent()) ?? "";
        continue;
      }
      const text = (await page.locator(".wat-code").textContent()) ?? "";
      previous = text;
      assert.ok(text.startsWith("(module"), `${id}: ${text.slice(0, 40)}`);
      // The entry module's functions by name.
      assert.match(text, /\(func \$playground\/main\//, id);
    }
    await page.context().close();
  });

  await step("editor token classes match classify", () =>
    editorClassesMatch((source) => openPage(code(source))),
  );

  await step("dark mode and a phone-width layout", async () => {
    const page = await openPage("", "dark");
    // A first visit opens the home page's welcome example.
    assert.equal(
      await page.locator(".cm-line").first().textContent(),
      "# Effects you can review. Tests without mocks.",
    );
    const background = await page.evaluate(() => getComputedStyle(document.body).backgroundColor);
    assert.equal(background, "rgb(17, 19, 24)");
    await page.setViewportSize({ width: 375, height: 740 });
    const overflow = await page.evaluate(
      () => document.documentElement.scrollWidth - document.documentElement.clientWidth,
    );
    assert.equal(overflow, 0);
    if (screenshots) {
      await page.screenshot({ path: join(screenshots, "phone-dark.png") });
    }
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
    if (screenshots) {
      await page.screenshot({ path: join(screenshots, "phone-dark-wat.png") });
    }
    await page.context().close();
    const light = await openPage();
    await light.click("#run");
    await light.locator(".outcome.passed").waitFor();
    if (screenshots) {
      await light.screenshot({ path: join(screenshots, "desktop-light.png") });
    }
    await light.context().close();
  });
}

/**
 * The default profile's traits a browser provides, `Clock` and `Random`,
 * run; a `main` that needs another stops before it runs.
 */
async function hostCapabilities(
  openPage: (hash?: string) => Promise<Page>,
  code: (source: string) => string,
  outcome: (page: Page) => Locator,
): Promise<void> {
  const source = [
    "use std.time.{Clock, ms, now, sleep}",
    "use std.random.{Random, rng}",
    "",
    "pub fn main!() -> void $ Console + Clock + Random:",
    "    println(now().unix_milliseconds() > 0)",
    "    sleep!(ms(5))",
    "    let mut draws = rng()",
    "    println(draws.int(0..10) < 10)",
    "",
  ].join("\n");
  const page = await openPage(code(source));
  await page.click("#run");
  await page.locator(".outcome.passed").waitFor();
  assert.equal(await page.locator(".stdout").textContent(), "true\ntrue\n");
  const files = code(
    'use std.fs.{FsRead, read_text}\nuse std.path.Path\n\npub fn main!() -> void $ Console + FsRead:\n    _ := read_text!(Path("notes.txt"))\n',
  );
  await page.evaluate((hash) => (location.hash = hash), files);
  await page.getByText("Loaded the shared project").waitFor();
  await page.click("#run");
  await page.locator(".outcome.failed").waitFor();
  assert.match(
    (await outcome(page).textContent()) ?? "",
    /main needs FsRead, which the playground does not provide/,
  );
  await page.context().close();
}
