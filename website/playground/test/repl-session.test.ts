// A REPL session in the JS host (host/core.mjs `createSession`, repl-plan.md
// job W4) over modules the playground's compiler builds through W3's REPL
// goals: inputs link to earlier ones through `hd.s`, a panic resets the
// runtime state, a stop cancels a waiting input, and a rebuild from the
// journal repeats no host call, skips the cancelled input and drops from
// the first divergence. The wasmtime side of the same journal format is
// hd_run_wasmtime's `a_session_runs_and_replays_the_same_on_node`.

import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { before, test } from "node:test";

import { createWebSession, type Session } from "../../../host/web.mjs";
import { recorded } from "../../../host/journal.mjs";
import { COMPILER_WASM } from "../build.ts";
import {
  OUTCOME,
  STATUS,
  compileRequest,
  encodeReplInput,
  encodeReplOutcome,
  encodeReplStart,
  type CompilerExports,
} from "../src/compiler-wire.ts";

let compiler: CompilerExports;

before(async () => {
  // The compiler wasm is a build output; build it when a fresh checkout
  // has none (as runner.test.ts does).
  if (!existsSync(COMPILER_WASM)) {
    const script = join(import.meta.dirname, "../../../scripts/build-web-compiler.js");
    execFileSync(process.execPath, [script], { stdio: "inherit" });
  }
  const { instance } = await WebAssembly.instantiate(readFileSync(COMPILER_WASM), {});
  compiler = instance.exports as unknown as CompilerExports;
});

/** A session with its compiler session, and what each input printed. */
class Repl {
  out = "";
  err = "";
  /** Each input's text and module, by number, for a rebuild. */
  built = new Map<number, { source: string; wasm: Uint8Array }>();
  session: Session;

  constructor() {
    this.session = this.fresh();
  }

  /** A new session over the same output. */
  fresh(): Session {
    return createWebSession({
      out: (text) => {
        this.out += text;
      },
      err: (text) => {
        this.err += text;
      },
    });
  }

  /** Starts the compiler's session and runs the prelude. */
  async start(): Promise<void> {
    const prelude = compileRequest(compiler, encodeReplStart({}));
    assert.equal(prelude.status, STATUS.ok, JSON.stringify(prelude.json));
    await this.run(0, "", prelude.wasm);
  }

  /** Builds and runs `source`; returns its status. `during` runs while it does. */
  async input(source: string, during?: (session: Session) => void): Promise<number | null> {
    const compiled = compileRequest(compiler, encodeReplInput(source));
    assert.equal(compiled.status, STATUS.ok, JSON.stringify(compiled.json));
    return this.run(compiled.json.input!, source, compiled.wasm, during);
  }

  async run(
    number: number,
    source: string,
    wasm: Uint8Array,
    during?: (session: Session) => void,
  ): Promise<number | null> {
    this.built.set(number, { source, wasm });
    const running = this.session.input(number, source, wasm);
    during?.(this.session);
    const ran = await running;
    assert.equal(ran.error ?? null, null);
    const outcome = ran.joined ? OUTCOME.joined : OUTCOME.dropped;
    assert.equal(compileRequest(compiler, encodeReplOutcome(number, outcome)).status, STATUS.ok);
    return ran.status;
  }

  /** What was printed since the last call. */
  printed(): [string, string] {
    const both: [string, string] = [this.out, this.err];
    this.out = "";
    this.err = "";
    return both;
  }

  rebuilder() {
    return { build: (n: number) => this.built.get(n)! };
  }
}

const journalInputs = (session: Session) =>
  recorded(session.journal()!).inputs.map((i) => i.number);

test("inputs link to earlier ones, and a panic inside defer leaves no state behind", async () => {
  const repl = new Repl();
  await repl.start();
  assert.equal(await repl.input("rate := +3\n"), 0);
  assert.equal(await repl.input("fn price(n: i32) -> i32:\n    n * rate\n"), 0);
  assert.equal(await repl.input("price(+2)\n"), 0);
  assert.deepEqual(repl.printed(), ["6 : i32\n", ""]);
  const panics =
    "fn cleanup(xs: List[i32]) -> void:\n    defer:\n        _ := xs[3]\n    pass\n\ncleanup([+1])\n";
  assert.equal(await repl.input(panics), 3);
  assert.deepEqual(repl.printed(), ["", "panic: index-out-of-bounds: list index out of bounds\n"]);
  assert.equal(await repl.input('println("after ${price(+5)}")\n'), 0);
  assert.deepEqual(repl.printed(), ["after 15\n", ""]);
  assert.deepEqual(journalInputs(repl.session), [0, 1, 2, 3, 4, 5]);
  repl.session.close();
});

test("a rebuild serves every host call from the journal and prints nothing", async () => {
  const repl = new Repl();
  await repl.start();
  for (const source of [
    "use std.time.{now, sleep, Duration}\n",
    "started := now().unix_milliseconds()\n",
    "sleep!(Duration.milliseconds(+1))\n",
    'println("slept")\n',
  ]) {
    assert.equal(await repl.input(source), 0);
  }
  assert.equal(await repl.input("started\n"), 0);
  const [shown] = repl.printed();
  assert.match(shown, /^slept\n\d+ : i64\n$/);
  const journal = repl.session.journal()!;
  const calls = repl.session.hostCalls();
  // Into the same session, as a rebuild after a stop does, and into a new
  // one, as a page that terminated its worker does.
  for (const session of [repl.session, repl.fresh()]) {
    const rebuilt = await session.rebuild(repl.rebuilder(), journal);
    assert.deepEqual(rebuilt, { kept: [0, 1, 2, 3, 4, 5], skipped: [], dropped: [], reason: null });
    assert.deepEqual(repl.printed(), ["", ""]);
    assert.deepEqual(session.journal(), journal);
    repl.session = session;
  }
  assert.equal(repl.session.hostCalls(), 0, "a host call repeated");
  assert.ok(calls > 0);
  assert.equal(await repl.input("started\n"), 0);
  assert.deepEqual(repl.printed(), [shown.slice("slept\n".length), ""]);
  repl.session.close();
});

test("a stop cancels a waiting input; a rebuild skips it and drops from the first divergence", async () => {
  const repl = new Repl();
  await repl.start();
  assert.equal(await repl.input("use std.time.{sleep, Duration}\n"), 0);
  assert.equal(await repl.input("let kept = 7\n"), 0);
  // Its `defer` suite runs on the cancel, and its change stays.
  const nap =
    "fn nap!() -> void:\n    defer:\n        kept = 8\n    sleep!(Duration.seconds(+30))\n\nnap!()\n";
  const started = performance.now();
  const status = await repl.input(nap, (session) => setTimeout(() => session.stop(), 50));
  assert.equal(status, 3);
  assert.ok(performance.now() - started < 10_000, "the stop did not cancel the sleep");
  assert.deepEqual(repl.printed(), ["", "cancelled\n"]);
  for (const source of ["1 + 1\n", "kept = kept + 1\nprintln(kept)\n", "2 + 2\n"]) {
    assert.equal(await repl.input(source), 0);
  }
  assert.deepEqual(repl.printed(), ["2 : usize\n9\n4 : usize\n", ""]);
  const rebuilt = await repl.session.rebuild(repl.rebuilder());
  assert.deepEqual(rebuilt, {
    kept: [0, 1, 2, 4],
    skipped: [3],
    dropped: [5, 6],
    reason: "input 5 called hd:Console write_line.start with other arguments than before",
  });
  assert.deepEqual(repl.printed(), ["", ""]);
  assert.deepEqual(journalInputs(repl.session), [0, 1, 2, 4]);
  assert.equal(await repl.input("kept\n"), 0);
  assert.deepEqual(repl.printed(), ["7 : usize\n", ""]);
  repl.session.close();
});
