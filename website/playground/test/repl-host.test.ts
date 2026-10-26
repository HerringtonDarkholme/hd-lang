// The worker side of the playground's REPL (src/repl-host.ts, `f1c55020:future-work/compiler/repl-plan.md`
// job W5), driven as the page drives it: `repl-*` requests in, `repl-out`
// and `repl` messages back. A worker the page terminates is a host whose
// compiler and session are gone; the page then hands a fresh host the
// journal and the texts of the inputs it holds (`repl-rebuild`).

import { instantiateCompiler } from "../src/compiler-host.ts";

import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { before, test } from "node:test";

import { COMPILER_WASM } from "../build.ts";
import type { CompilerExports } from "../src/compiler-wire.ts";
import {
  createReplHost,
  diagnosticLine,
  type ReplAnswer,
  type ReplFiles,
  type ReplMessage,
  type ReplRan,
  type ReplRebuilt,
  type ReplRequest,
  type ReplStarted,
} from "../src/repl-host.ts";

let wasm: Buffer;

before(() => {
  // The compiler wasm is a build output; build it when a fresh checkout
  // has none (as runner.test.ts does).
  if (!existsSync(COMPILER_WASM)) {
    const script = join(import.meta.dirname, "../../../scripts/build-web-compiler.js");
    execFileSync(process.execPath, [script], { stdio: "inherit" });
  }
  wasm = readFileSync(COMPILER_WASM);
});

/** A worker: a host over its own compiler instance, and what it posted. */
class Worker {
  messages: ReplMessage[] = [];
  private nextId = 1;
  /** The page's record: each input's text, and the last journal. */
  sources = new Map<number, string>();
  journal: Uint8Array | null = null;
  private readonly host = createReplHost(
    instantiateCompiler(new Uint8Array(wasm)).then(
      ({ instance }) => instance.exports as unknown as CompilerExports,
    ),
    (message) => this.messages.push(message),
  );

  /** Sends `request` with a fresh id; resolves to its answer. */
  async send(request: { kind: string; [field: string]: unknown }): Promise<ReplAnswer> {
    const id = this.nextId++;
    await this.host.handle({ ...request, id } as ReplRequest);
    const found = this.messages.find((m) => m.kind === "repl" && m.id === id);
    assert.ok(found?.kind === "repl", `no answer to ${request.kind}`);
    if ("journal" in found.answer && found.answer.journal) this.journal = found.answer.journal;
    return found.answer;
  }

  /** What `repl-out` messages printed since the last call: stdout, stderr. */
  printed(): [string, string] {
    let out = "";
    let err = "";
    for (const m of this.messages) {
      if (m.kind !== "repl-out") continue;
      if (m.stream === "out") out += m.text;
      else err += m.text;
    }
    this.messages = [];
    return [out, err];
  }

  async start(files: ReplFiles = {}): Promise<ReplStarted> {
    return (await this.send({ kind: "repl-start", files })) as ReplStarted;
  }

  /** Runs input `number`; `during` runs while it does. */
  async input(number: number, source: string, during?: () => void): Promise<ReplRan> {
    this.sources.set(number, source);
    const answer = this.send({ kind: "repl-input", number, source });
    during?.();
    return (await answer) as ReplRan;
  }

  stop(): void {
    void this.host.handle({ kind: "repl-stop" });
  }

  /** A fresh worker's rebuild from this one's journal and texts. */
  async rebuildIn(other: Worker, files: ReplFiles = {}): Promise<ReplRebuilt> {
    other.sources = new Map(this.sources);
    return (await other.send({
      kind: "repl-rebuild",
      files,
      journal: this.journal,
      sources: [...this.sources],
    })) as ReplRebuilt;
  }
}

test("a diagnostic prints as hd prints it", () => {
  const base = { endLine: 1, endColumn: 2, severity: "error" } as const;
  assert.equal(
    diagnosticLine({
      ...base,
      path: "repl#3",
      code: "unknown-name",
      message: "m",
      line: 1,
      column: 5,
    }),
    "error: repl#3:1:5: unknown-name: m",
  );
  assert.equal(
    diagnosticLine({ ...base, path: null, code: null, message: "m", line: null, column: null }),
    "error: m",
  );
});

test("inputs run under the page's numbers; a panic leaves the session going", async () => {
  const w = new Worker();
  const started = await w.start();
  assert.equal(started.ok, true, JSON.stringify(started));
  assert.ok(started.journal, "the start answers the journal");
  const value = await w.input(1, "[3, 6]\n");
  assert.deepEqual([value.joined, value.end], [true, { kind: "exit", status: 0 }]);
  assert.deepEqual(w.printed(), ["[3, 6] : List[usize]\n", ""]);
  assert.equal((await w.input(2, "let total = 40\n")).joined, true);
  assert.equal((await w.input(3, "total + 2\n")).joined, true);
  assert.deepEqual(w.printed(), ["42 : usize\n", ""]);
  const panic = await w.input(4, "[1][3]\n");
  assert.deepEqual([panic.joined, panic.end], [false, { kind: "trapped" }]);
  assert.deepEqual(w.printed(), ["", "panic: index-out-of-bounds: list index out of bounds\n"]);
  // A number the page skipped is fine: numbers only go up.
  const unknown = await w.input(7, "nope + total\n");
  assert.equal(unknown.end, null);
  assert.deepEqual(unknown.diagnostics, ["error: repl#7:1:1: unknown-name: `nope` is not defined"]);
  const again = await w.input(8, "let total = 1\n");
  assert.match(again.diagnostics[0]!, /^error: repl#8:1:5: duplicate-binding: .*repl#2/);
  assert.equal((await w.input(9, "dbg(total)\n")).joined, true);
  assert.deepEqual(w.printed(), ["", "repl#9:1:1: total = 40\n"]);
  // A trait the browser lacks stops the input before it runs, as Run does.
  const fs = await w.input(
    10,
    'use std.fs.{read_text}\nuse std.path.Path\n_ := read_text!(Path("a.txt"))\n',
  );
  assert.deepEqual([fs.diagnostics, fs.end], [[], null]);
  assert.equal(fs.error, "repl#10 needs FsRead, which the playground does not provide");
  assert.equal((await w.input(11, "total\n")).joined, true);
  assert.deepEqual(w.printed(), ["40 : usize\n", ""]);
});

test("a stop cancels an input that waits; its defer suite runs", async () => {
  const w = new Worker();
  await w.start();
  await w.input(1, "use std.time.{sleep, Duration}\n");
  await w.input(2, "let woke = 0\n");
  await w.input(
    3,
    'fn nap!() -> void:\n    defer:\n        woke = 1\n        dbg("woke")\n    sleep!(Duration.seconds(+30))\n',
  );
  w.printed();
  const begun = performance.now();
  const nap = await w.input(4, "nap!()\n", () => setTimeout(() => w.stop(), 50));
  assert.ok(performance.now() - begun < 10_000, "the stop did not cancel the sleep");
  assert.deepEqual([nap.joined, nap.end], [false, { kind: "cancelled" }]);
  assert.deepEqual(w.printed(), [
    "",
    'repl#3:4:9: "woke" = "woke"\ncancelled\nnote: repl#4 was cancelled, so the next rebuild skips it; its changes are not restored\n',
  ]);
  await w.input(5, "woke\n");
  assert.deepEqual(w.printed(), ["0 : usize\n", ""]);
  // The next input already rebuilt without the cancelled input. A fresh
  // worker replays that same kept prefix without losing more inputs.
  const fresh = new Worker();
  const rebuilt = await w.rebuildIn(fresh);
  assert.deepEqual(
    [rebuilt.ok, rebuilt.kept, rebuilt.skipped, rebuilt.dropped],
    [true, [1, 2, 3, 5], [], []],
  );
  assert.equal(rebuilt.reason, null);
  assert.deepEqual(fresh.printed(), ["", ""], "a rebuild prints nothing");
  await fresh.input(6, "woke\n");
  assert.deepEqual(fresh.printed(), ["0 : usize\n", ""]);
});

test("a module use of an earlier input survives a rebuild", async () => {
  const w = new Worker();
  await w.start();
  await w.input(1, "use std.path\n");
  await w.input(2, "use std.cmp as c\n");
  await w.input(3, 'println(path.Path("a").join("b").parent().is_some())\n');
  const fresh = new Worker();
  const rebuilt = await w.rebuildIn(fresh);
  assert.deepEqual([rebuilt.ok, rebuilt.dropped, rebuilt.error], [true, [], null]);
  assert.deepEqual(fresh.printed(), ["", ""], "a rebuild prints nothing");
  await fresh.input(4, 'println(path.Path("a").join("b").parent().is_some())\n');
  assert.deepEqual(fresh.printed(), ["true\n", ""]);
  await fresh.input(5, "println(c.max(1, 2))\n");
  assert.deepEqual(fresh.printed(), ["2\n", ""]);
  const again = await fresh.input(6, "fn path() -> usize: 1\n");
  assert.match(again.diagnostics[0]!, /duplicate-binding: .*repl#1/);
});

test("a terminated worker's session comes back in a fresh one under the same numbers", async () => {
  const w = new Worker();
  await w.start();
  await w.input(1, "use std.time.{now}\n");
  await w.input(2, "let total = 40\n");
  await w.input(3, "started := now().unix_milliseconds()\n");
  await w.input(4, "total = total + 1\n");
  await w.input(5, "missing\n");
  await w.input(6, "fn twice(n: usize) -> usize: n * 2\n");
  const [, before] = [w.printed(), await w.input(7, "started\n")];
  assert.equal(before.joined, true);
  const [shown] = w.printed();
  // Input 8, `while true: pass`, computes; the page terminates the worker
  // with the journal it held after input 7, and input 8 is not in it.
  w.sources.set(8, "while true:\n    pass\n");
  const fresh = new Worker();
  const rebuilt = await w.rebuildIn(fresh);
  assert.deepEqual(
    [rebuilt.ok, rebuilt.kept, rebuilt.skipped, rebuilt.dropped, rebuilt.error],
    [true, [1, 2, 3, 4, 6, 7], [], [], null],
  );
  assert.ok(rebuilt.journal, "the rebuild answers its journal");
  assert.deepEqual(fresh.printed(), ["", ""]);
  await fresh.input(9, "twice(total)\n");
  await fresh.input(10, "started\n");
  assert.deepEqual(fresh.printed(), [`82 : usize\n${shown}`, ""]);
  const redeclared = await fresh.input(11, "fn twice(n: usize) -> usize: n\n");
  assert.match(redeclared.diagnostics[0]!, /^error: repl#11:1:4: duplicate-binding: .*repl#6/);
});

test("a rebuild over edited files drops the first input that no longer builds", async () => {
  const w = new Worker();
  const started = await w.start({ "src/lib.hd": "fn rate() -> usize: 3\n" });
  assert.equal(started.ok, true, JSON.stringify(started));
  await w.input(1, "let base = 10\n");
  await w.input(2, "base * rate()\n");
  await w.input(3, "base + 1\n");
  assert.deepEqual(w.printed(), ["30 : usize\n11 : usize\n", ""]);
  const rebuilt = await w.rebuildIn(w, { "src/lib.hd": "fn ratio() -> usize: 3\n" });
  assert.deepEqual([rebuilt.ok, rebuilt.kept, rebuilt.dropped], [true, [1], [2, 3]]);
  assert.equal(rebuilt.reason, "repl#2 no longer builds");
  assert.match(rebuilt.diagnostics[0]!, /^error: repl#2:1:8: unknown-name: /);
  await w.input(4, "base * ratio()\n");
  assert.deepEqual(w.printed(), ["30 : usize\n", ""]);
  // A library that does not parse fails only the inputs that name it
  // (lib.hd loads lazily, #159): input 1 stays, input 4 is dropped.
  const broken = await w.rebuildIn(w, { "src/lib.hd": "fn ratio( -> usize: 3\n" });
  assert.deepEqual([broken.ok, broken.kept, broken.dropped], [true, [1], [4]]);
  assert.match(broken.diagnostics[0]!, /^error: src\/lib\.hd:1:/);
  // Quit ends a session; a start begins a new one.
  assert.deepEqual(await w.send({ kind: "repl-quit" }), { type: "ended" });
  assert.equal((await w.start()).ok, true);
  await w.input(1, "1 + 1\n");
  assert.deepEqual(w.printed(), ["2 : usize\n", ""]);
});
