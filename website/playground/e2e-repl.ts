// The REPL's end-to-end steps (repl-plan.md job W5), which
// `pnpm run website:e2e` runs after the playground's: the playground's REPL
// view (a value, a later input reading an earlier binding, a panic, Stop of
// a loop and of a sleep, the commands) and the site's docked panel with its
// Try in REPL buttons.

import assert from "node:assert/strict";

import type { Browser, Locator, Page } from "playwright-core";

interface ReplE2e {
  readonly browser: Browser;
  /** The origin serving the site, such as `http://127.0.0.1:4173`. */
  readonly origin: string;
  /** The site's base path, such as `/hd-lang/`. */
  readonly siteBase: string;
  /** The playground's URL path, such as `/hd-lang/playground/`. */
  readonly base: string;
  readonly step: (name: string, body: () => Promise<void>) => Promise<void>;
}

/** The first reply waits for a worker to load the compiler. */
const FIRST_REPLY_MS = 60_000;

/**
 * Types `text` into the REPL input and presses Enter until the input is
 * sent: a block takes one more Enter on its empty last line.
 */
async function enter(repl: Locator, text: string): Promise<void> {
  const input = repl.locator(".repl-input");
  await input.fill(text);
  for (let presses = 0; presses < 3 && (await input.inputValue()) !== ""; presses += 1)
    await input.press("End").then(() => input.press("Enter"));
  assert.equal(await input.inputValue(), "", `${JSON.stringify(text)} was not sent`);
}

/** Waits for a transcript line of `kind` that reads exactly `text`. */
async function shows(
  repl: Locator,
  kind: "out" | "error" | "debug" | "note",
  text: string | RegExp,
): Promise<void> {
  const escaped = typeof text === "string" ? text.replaceAll(/[.*+?^${}()|[\]\\]/g, "\\$&") : "";
  const exact = typeof text === "string" ? new RegExp(`^${escaped}$`) : text;
  try {
    await repl
      .locator(`.repl-${kind}`, { hasText: exact })
      .last()
      .waitFor({ timeout: FIRST_REPLY_MS });
  } catch (error) {
    // The transcript says what the REPL showed instead.
    const shown = await repl.locator(".repl-log").innerText();
    throw new Error(`no ${kind} line ${exact} in the transcript:\n${shown}`, { cause: error });
  }
}

/**
 * Waits until input `number` runs: Stop shows from the first input that
 * runs, and inputs entered meanwhile wait their turn.
 */
async function running(page: Page, number: number): Promise<void> {
  await page.locator("#status", { hasText: `Running repl#${number}…` }).waitFor();
  await page.locator("#repl-stop").waitFor({ state: "visible" });
}

/** Waits until the page has saved a session with at least `count` inputs. */
async function saved(page: Page, count: number): Promise<void> {
  const inputs = (): Promise<number> =>
    page.evaluate(
      () =>
        new Promise<number>((resolve) => {
          const open = indexedDB.open("hd-repl", 1);
          open.onupgradeneeded = () => open.result.createObjectStore("sessions");
          open.onerror = () => resolve(0);
          open.onsuccess = () => {
            const db = open.result;
            const all = db.transaction("sessions").objectStore("sessions").getAll();
            all.onerror = () => resolve(0);
            all.onsuccess = () => {
              db.close();
              resolve(
                Math.max(0, ...(all.result as { inputs: unknown[] }[]).map((s) => s.inputs.length)),
              );
            };
          };
        }),
    );
  const deadline = Date.now() + FIRST_REPLY_MS;
  while ((await inputs()) < count) {
    assert.ok(Date.now() < deadline, `the session with ${count} inputs was never saved`);
    await page.waitForTimeout(100);
  }
}

/** Waits until the transcript shows `count` output lines that read `text`. */
async function showsTimes(repl: Locator, text: string, count: number): Promise<void> {
  const lines = repl.locator(".repl-out", { hasText: new RegExp(`^${text}$`) });
  const deadline = Date.now() + FIRST_REPLY_MS;
  while ((await lines.count()) < count) {
    assert.ok(Date.now() < deadline, `${text} was shown ${await lines.count()} times`);
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
}

/** The text of the last transcript line of `kind`. */
async function lastLine(repl: Locator, kind: string): Promise<string> {
  return (await repl.locator(`.repl-${kind}`).last().innerText()).trim();
}

export async function replSteps(options: ReplE2e): Promise<void> {
  const { browser, origin, siteBase, base, step } = options;

  async function openPlayground(): Promise<{ page: Page; repl: Locator }> {
    const context = await browser.newContext({ viewport: { width: 1280, height: 800 } });
    const page = await context.newPage();
    page.on("pageerror", (error) => console.error("page error:", error.message));
    await page.goto(`${origin}${base}`);
    await page.locator("#view-repl").click();
    const repl = page.locator("#repl");
    await repl.locator(".repl-input").waitFor();
    return { page, repl };
  }

  const { page, repl } = await openPlayground();

  await step("the REPL view shows values; later inputs read earlier bindings", async () => {
    assert.equal(await page.locator("#repl").isVisible(), true);
    assert.equal(await page.locator("#output").isVisible(), false);
    await enter(repl, "[3, 6]");
    await shows(repl, "out", "[3, 6] : List[usize]");
    await enter(repl, "let total = 40");
    await enter(repl, "total + 2");
    await shows(repl, "out", "42 : usize");
    // Each input is echoed with its number, as diagnostics name it.
    assert.deepEqual(await repl.locator(".repl-number").allTextContents(), [
      "repl#1",
      "repl#2",
      "repl#3",
    ]);
    await enter(repl, "nope + 1");
    await shows(repl, "error", "error: repl#4:1:1: unknown-name: `nope` is not defined");
  });

  await step("a panicking input is reported and the session goes on", async () => {
    await enter(repl, "[1][3]");
    await shows(repl, "error", "panic: index-out-of-bounds: list index out of bounds");
    await enter(repl, "total");
    await shows(repl, "out", "40 : usize");
  });

  await step("Stop of a loop ends the worker and restores the session", async () => {
    await enter(repl, "while true:\n    pass");
    await running(page, 7);
    await page.locator("#repl-stop").click();
    await shows(repl, "error", /^stopped: repl#7 kept computing/);
    await shows(repl, "note", /^restored: \d+ inputs replayed$/);
    await page.locator("#repl-stop").waitFor({ state: "hidden" });
    await enter(repl, "total + 1");
    await shows(repl, "out", "41 : usize");
  });

  await step("Stop of a sleep cancels it and runs its defer suite", async () => {
    const restored = await repl.locator(".repl-note", { hasText: /^restored:/ }).count();
    await enter(repl, "use std.time.{sleep, Duration}");
    await enter(
      repl,
      'fn nap!() -> void:\n    defer:\n        dbg("woke")\n    sleep!(Duration.seconds(+30))',
    );
    await enter(repl, "nap!()");
    await running(page, 11);
    const started = Date.now();
    await page.locator("#repl-stop").click();
    await shows(repl, "debug", 'repl#10:3:9: "woke" = "woke"');
    await shows(repl, "error", "cancelled");
    assert.ok(Date.now() - started < 10_000, "the sleep was not cancelled");
    assert.equal(
      await repl.locator(".repl-note", { hasText: /^restored:/ }).count(),
      restored,
      "a cancel needs no rebuild",
    );
    await enter(repl, "total");
    await shows(repl, "out", "40 : usize");
  });

  await step(":help, :reload and :quit act on the session", async () => {
    await enter(repl, ":help");
    await shows(repl, "note", /:reload {7}rebuild the session over the editor's files/);
    await enter(repl, ":reload");
    await shows(
      repl,
      "note",
      "note: repl#11 was stopped, so the rebuild skipped it; its changes are not restored",
    );
    await shows(repl, "note", /^reloaded: \d+ inputs replayed$/);
    await enter(repl, "total * 2");
    await shows(repl, "out", "80 : usize");
    await enter(repl, ":quit");
    await shows(repl, "note", "session ended; the next input starts a new one");
    await enter(repl, "total");
    await shows(repl, "error", "error: repl#1:1:1: unknown-name: `total` is not defined");
    // The view's Clear empties the transcript.
    await page.locator("#clear").click();
    assert.equal(await repl.locator(".repl-log pre").count(), 0);
    await page.context().close();
  });

  await step("Try in REPL runs a page's snippet in the docked panel", async () => {
    const context = await browser.newContext({ viewport: { width: 1280, height: 800 } });
    const site = await context.newPage();
    site.on("pageerror", (error) => console.error("page error:", error.message));
    await site.goto(`${origin}${siteBase}guide/learn-in-10-minutes.html`);
    const snippet = site.locator(".code-block", { hasText: "fn greet(name: string)" }).first();
    await snippet.hover();
    await snippet.locator(".try-repl").click();
    const panel = site.locator("#repl-panel");
    await panel.waitFor({ state: "visible" });
    await shows(panel, "out", "hello, Ada");
    await enter(panel, 'shout("hd")');
    await shows(panel, "out", "HD");
    await site.keyboard.press("Escape");
    await panel.waitFor({ state: "hidden" });
    await context.close();
  });

  const DRAW = /^\d+ : u64$/;

  await step("a reload restores the session: its transcript and bindings", async () => {
    const context = await browser.newContext({ viewport: { width: 1280, height: 800 } });
    const first = await context.newPage();
    first.on("pageerror", (error) => console.error("page error:", error.message));
    await first.goto(`${origin}${base}`);
    await first.locator("#view-repl").click();
    const panel = first.locator("#repl");
    await enter(panel, "use std.random.rng");
    await enter(panel, "let draw = rng().next_u64()");
    await enter(panel, "draw");
    await shows(panel, "out", DRAW);
    const drawn = await lastLine(panel, "out");
    await saved(first, 3);
    await first.reload();
    await first.locator("#view-repl").click();
    await shows(panel, "note", "restored: 3 inputs replayed");
    // The earlier output is on screen, and the binding reads the same draw.
    assert.equal(await lastLine(panel, "out"), drawn);
    await enter(panel, "draw");
    await showsTimes(panel, drawn, 2);
    // Up recalls an input of the saved session.
    await panel.locator(".repl-input").press("ArrowUp");
    assert.equal(await panel.locator(".repl-input").inputValue(), "draw");
    await context.close();
  });

  await step("the site panel restores its session after a reload", async () => {
    const context = await browser.newContext({ viewport: { width: 1280, height: 800 } });
    const site = await context.newPage();
    site.on("pageerror", (error) => console.error("page error:", error.message));
    const url = `${origin}${siteBase}guide/learn-in-10-minutes.html`;
    await site.goto(url);
    await site.locator("#repl-toggle").click();
    const panel = site.locator("#repl-panel");
    await enter(panel, "let kept = 7 * 6");
    await saved(site, 1);
    await site.reload();
    await site.locator("#repl-toggle").click();
    await shows(panel, "note", "restored: 1 input replayed");
    await enter(panel, "kept");
    await shows(panel, "out", "42 : usize");
    await context.close();
  });

  await step("blocked storage starts a fresh session", async () => {
    const context = await browser.newContext({ viewport: { width: 1280, height: 800 } });
    await context.addInitScript(() => {
      Object.defineProperty(window, "indexedDB", {
        get() {
          throw new DOMException("blocked", "SecurityError");
        },
      });
    });
    const page = await context.newPage();
    page.on("pageerror", (error) => console.error("page error:", error.message));
    await page.goto(`${origin}${base}`);
    await page.locator("#view-repl").click();
    const panel = page.locator("#repl");
    await enter(panel, "let one = 1");
    await enter(panel, "one + 1");
    await shows(panel, "out", "2 : usize");
    await context.close();
  });

  await step("a share link reopens the sender's session with its outputs", async () => {
    const sender = await browser.newContext({ viewport: { width: 1280, height: 800 } });
    await sender.grantPermissions(["clipboard-read", "clipboard-write"], { origin });
    const page = await sender.newPage();
    page.on("pageerror", (error) => console.error("page error:", error.message));
    await page.goto(`${origin}${base}`);
    await page.locator("#view-repl").click();
    const panel = page.locator("#repl");
    await enter(panel, "use std.random.rng");
    await enter(panel, "let draw = rng().next_u64()");
    await enter(panel, "draw");
    await shows(panel, "out", DRAW);
    const drawn = await lastLine(panel, "out");
    await page.locator("#repl-share").click();
    await shows(panel, "note", /^link copied \(\d+ characters, inputs and outputs\)$/);
    const link = await page.evaluate(() => navigator.clipboard.readText());
    assert.match(link, /#repl=[zj]\./);

    // A fresh browser context has no saved session. A re-run would draw again.
    const recipient = await browser.newContext({ viewport: { width: 1280, height: 800 } });
    const opened = await recipient.newPage();
    opened.on("pageerror", (error) => console.error("page error:", error.message));
    await opened.goto(link);
    const shared = opened.locator("#repl");
    await shows(shared, "note", "restored from a shared link: 3 inputs replayed");
    assert.equal(await lastLine(shared, "out"), drawn);
    await enter(shared, "draw");
    await showsTimes(shared, drawn, 2);
    await recipient.close();

    // A damaged link says so.
    const damaged = await browser.newContext({ viewport: { width: 1280, height: 800 } });
    const broken = await damaged.newPage();
    await broken.goto(`${origin}${base}#repl=z.AAAA`);
    await shows(
      broken.locator("#repl"),
      "error",
      "error: the shared session link is damaged, so it was not opened",
    );
    await damaged.close();

    // Past the link limit, the link carries the inputs only.
    await enter(panel, `let blob = "${randomText(14_000)}"`);
    await page.locator("#repl-share").click();
    await shows(panel, "note", /inputs only; the session's record is too long for a link/);
    const bare = await page.evaluate(() => navigator.clipboard.readText());
    const again = await browser.newContext({ viewport: { width: 1280, height: 800 } });
    const reopened = await again.newPage();
    await reopened.goto(bare);
    const rerun = reopened.locator("#repl");
    await shows(rerun, "note", /^shared link: it carried the inputs only/);
    await shows(rerun, "out", DRAW);
    assert.notEqual(await lastLine(rerun, "out"), drawn, "the draw was recomputed");
    await again.close();
    await sender.close();
  });
}

/** `count` characters no compressor shrinks much. */
function randomText(count: number): string {
  const alphabet = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
  let out = "";
  for (let i = 0; i < count; i += 1) out += alphabet[Math.floor(Math.random() * 64)];
  return out;
}
