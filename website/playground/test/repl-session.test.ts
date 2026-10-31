// A REPL session in the JS host (host/core.ts `createSession`) over the
// session programs the playground's compiler builds through the REPL
// goals: each input runs after the kept ones replay from the journal, a
// panic resets the runtime state, a stop cancels a waiting input, which
// the next input's replay skips, and a rebuild from the journal repeats no
// host call and drops from the first divergence. The wasmtime side of the
// same journal format is hd_run_wasmtime's
// `a_session_runs_and_replays_the_same_on_node`.

import { instantiateCompiler } from "../src/compiler-host.ts";

import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { before, test } from "node:test";

import { createWebSession, type Session } from "../../../host/web.ts";
import { recorded } from "../../../host/journal.ts";
import { COMPILER_WASM } from "../../../scripts/build-web-compiler.js";
import {
  OUTCOME,
  STATUS,
  compileRequest,
  encodeReplInput,
  encodeReplRebuildInput,
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
  const { instance } = await instantiateCompiler(readFileSync(COMPILER_WASM));
  compiler = instance.exports as unknown as CompilerExports;
});

/** A session with its compiler session, and what each input printed. */
class Repl {
  out = "";
  err = "";
  /** The text of each input the session keeps, by number. */
  texts = new Map<number, string>();
  /** The last session program built. */
  wasm: Uint8Array | null = null;
  session: Session;
  journalCap: number;

  constructor(journalCap = 64 * 1024 * 1024) {
    this.journalCap = journalCap;
    this.session = this.fresh();
  }

  /** A new session over the same output. */
  fresh(): Session {
    return createWebSession(
      {
        out: (text) => {
          this.out += text;
        },
        err: (text) => {
          this.err += text;
        },
      },
      {},
      { journalCap: this.journalCap },
    );
  }

  /** Starts the compiler's session. */
  start(): void {
    const started = compileRequest(compiler, encodeReplStart({}));
    assert.equal(started.status, STATUS.ok, JSON.stringify(started.json));
  }

  /** Builds and runs `source`; returns its status. `during` runs while it does. */
  async input(source: string, during?: (session: Session) => void): Promise<number | null> {
    const compiled = compileRequest(compiler, encodeReplInput(source));
    assert.equal(compiled.status, STATUS.ok, JSON.stringify(compiled.json));
    const number = compiled.json.input!;
    this.wasm = compiled.wasm;
    const running = this.session.run(compiled.wasm, [...this.texts, [number, source]], number);
    during?.(this.session);
    const ran = await running;
    for (const count of [...ran.skipped, ...ran.dropped]) {
      this.texts.delete(count);
    }
    const live = ran.live!;
    assert.equal(live.error ?? null, null);
    const stopped = live.end.kind === "stopped" || live.end.kind === "cancelled";
    let outcome: number = OUTCOME.failed;
    if (live.joined) {
      outcome = OUTCOME.joined;
    } else if (stopped) {
      outcome = OUTCOME.dropped;
    }
    assert.equal(compileRequest(compiler, encodeReplOutcome(number, outcome)).status, STATUS.ok);
    if (!stopped) {
      this.texts.set(number, source);
    }
    return live.status;
  }

  /** Build one complete base for replay, preserving recorded failed inputs. */
  baseForReplay(texts = this.texts): Uint8Array {
    const history = recorded(this.session.journal()!);
    const started = compileRequest(compiler, encodeReplStart({}));
    assert.equal(started.status, STATUS.ok);
    let wasm: Uint8Array = new Uint8Array();
    for (const [number, source] of texts) {
      const built = compileRequest(compiler, encodeReplRebuildInput(source, number));
      assert.equal(built.status, STATUS.ok, JSON.stringify(built.json));
      wasm = built.wasm;
      const end = history.inputs.find((i) => i.number === number)!.end!;
      const outcome = end.kind === "exit" && end.status === 0 ? OUTCOME.joined : OUTCOME.failed;
      assert.equal(compileRequest(compiler, encodeReplOutcome(number, outcome)).status, STATUS.ok);
    }
    this.wasm = wasm;
    return wasm;
  }

  /** What was printed since the last call. */
  printed(): [string, string] {
    const both: [string, string] = [this.out, this.err];
    this.out = "";
    this.err = "";
    return both;
  }
}

const journalInputs = (session: Session) =>
  recorded(session.journal()!).inputs.map((i) => i.number);

/** What a journal records, with each input's module hash left out. */
const butModules = (journal: Uint8Array) => {
  const record = recorded(journal);
  for (const input of record.inputs) {
    input.module = 0n;
  }
  return record;
};

test("inputs see earlier ones, and a panic inside deferred cleanup leaves no state behind", async () => {
  const repl = new Repl();
  repl.start();
  assert.equal(await repl.input("rate := +3\n"), 0);
  assert.equal(await repl.input("fn price(n: i32) -> i32:\n    n * rate\n"), 0);
  assert.equal(await repl.input("price(+2)\n"), 0);
  assert.deepEqual(repl.printed(), ["6 : i32\n", ""]);
  const panics =
    "use std.ops.defer\nfn cleanup(xs: List[i32]) -> void:\n    defer(fn() -> void: _ := xs[3])\n    pass\n\ncleanup([+1])\n";
  assert.equal(await repl.input(panics), 3);
  assert.deepEqual(repl.printed(), ["", "panic: index-out-of-bounds: list index out of bounds\n"]);
  assert.equal(await repl.input('println("after ${price(+5)}")\n'), 0);
  assert.deepEqual(repl.printed(), ["after 15\n", ""]);
  assert.deepEqual(journalInputs(repl.session), [1, 2, 3, 4, 5]);
  repl.session.close();
});

test("a rebuild serves every host call from the journal and prints nothing", async () => {
  const repl = new Repl();
  repl.start();
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
  // Into the same session, as a rebuild after a stop does, and into a new
  // one, as a page that terminated its worker does.
  const base = repl.baseForReplay();
  for (const session of [repl.session, repl.fresh()]) {
    const before = session.hostCalls();
    const rebuilt = await session.run(base, repl.texts, null, journal);
    assert.deepEqual(rebuilt, {
      kept: [1, 2, 3, 4, 5],
      skipped: [],
      dropped: [],
      reason: null,
      live: null,
    });
    assert.deepEqual(repl.printed(), ["", ""]);
    assert.deepEqual(butModules(session.journal()!), butModules(journal));
    assert.equal(session.hostCalls(), before, "a host call repeated");
    repl.session = session;
  }
  assert.equal(await repl.input("started\n"), 0);
  assert.deepEqual(repl.printed(), [shown.slice("slept\n".length), ""]);
  repl.session.close();
});

test("later inputs edit an earlier input's type's facts, before and after a rebuild", async () => {
  const repl = new Repl();
  repl.start();
  // A walker that writes each member's `Column` fact, or else its name.
  const columns = `use std.structure.{Structure, Field, Variant, Walker}

struct Column:
    name: string

trait Columns:
    fn columns(self) -> string

struct Header:
    out: string

impl[S] Walker[S] for Header:
    type Error = never

    fn variant(&mut self, v: Variant[S]) -> Result[void, never]:
        .Ok(())

    fn member[F](&mut self, h: Field[S, F], value: F) -> Result[void, never]:
        name := match h.info.facts.find::[Column]():
            .Some(c) => c.name
            .None => h.info.name
        self.out = "\${self.out}\${name};"
        .Ok(())

impl[T] Columns for T by Structure:
    fn columns(self) -> string:
        let mut w = Header { out: "" }
        _ := Structure::walk(self, &mut w)
        w.out
`;
  for (const source of [
    columns,
    "struct Row:\n    id: i32\n    total: i32\n    cache: i32 = +0\n",
    // A block with no trait, then a derivation block with member lines.
    'impl Row by Structure:\n    total += [Column { name: "sum_total" }]\n',
    'impl Columns for Row by Structure:\n    id = [Column { name: "key" }]\n    cache = pass\n',
  ]) {
    assert.equal(await repl.input(source), 0, repl.err);
  }
  assert.equal(await repl.input("Row { id: +1, total: +2 }.columns()\n"), 0);
  const shown = '"key;sum_total;" : string\n';
  assert.deepEqual(repl.printed(), [shown, ""]);
  const journal = repl.session.journal()!;
  const base = repl.baseForReplay();
  repl.session = repl.fresh();
  const rebuilt = await repl.session.run(base, repl.texts, null, journal);
  assert.deepEqual(rebuilt.kept, [1, 2, 3, 4, 5]);
  assert.deepEqual(repl.printed(), ["", ""]);
  assert.equal(await repl.input("Row { id: +3, total: +4 }.columns()\n"), 0);
  assert.deepEqual(repl.printed(), [shown, ""]);
  repl.session.close();
});

test("a stop cancels a waiting input; the next replay skips it, a rebuild drops from the first divergence", async () => {
  const repl = new Repl();
  repl.start();
  assert.equal(await repl.input("use std.ops.defer\nuse std.time.{sleep, Duration}\n"), 0);
  assert.equal(await repl.input("let kept = 7\n"), 0);
  // Its deferred callback runs on the cancel; the next input's replay skips
  // it, so its change is not restored (`cli.repl.rebuild.stopped`).
  const nap =
    "fn nap!() -> void:\n    defer(fn() -> void: kept = 8)\n    sleep!(Duration.seconds(+30))\n\nnap!()\n";
  const started = performance.now();
  const status = await repl.input(nap, (session) => setTimeout(() => session.stop(), 50));
  assert.equal(status, 3);
  assert.ok(performance.now() - started < 10_000, "the stop did not cancel the sleep");
  assert.deepEqual(repl.printed(), ["", "cancelled\n"]);
  for (const source of ["1 + 1\n", "kept = kept + 1\nprintln(kept)\n", "2 + 2\n"]) {
    assert.equal(await repl.input(source), 0);
  }
  assert.deepEqual(repl.printed(), ["2 : usize\n8\n4 : usize\n", ""]);
  assert.deepEqual(journalInputs(repl.session), [1, 2, 4, 5, 6]);
  // Input 5 is no longer the text the session ran.
  const texts = new Map(repl.texts);
  texts.set(5, "kept = kept + 2\nprintln(kept)\n");
  const base = repl.baseForReplay(texts);
  const rebuilt = await repl.session.run(base, texts, null);
  assert.deepEqual(rebuilt, {
    kept: [1, 2, 4],
    skipped: [],
    dropped: [5, 6],
    reason: "input 5 is not the text the session ran",
    live: null,
  });
  assert.deepEqual(repl.printed(), ["", ""]);
  assert.deepEqual(journalInputs(repl.session), [1, 2, 4]);
  repl.session.close();
});

test("browser-provider checkpoints continue past the cap and restore without repeating effects", async () => {
  for (const cap of [1, 20_000]) {
    const repl = new Repl(cap);
    repl.start();
    assert.equal(
      await repl.input(`use std.random.fill
struct Counter:
    value: usize
counter := box(Counter { value: 0 })
alias := counter
`),
      0,
    );
    for (let count = 1; count <= 24; count++) {
      assert.equal(
        await repl.input(`_ := fill(4096).len()
alias.value += 1
if counter.value != ${count}:
    panic("checkpoint lost a counter update")
pass
`),
        0,
        repl.err,
      );
    }
    const stats = repl.session.checkpointStats();
    assert.ok(stats.archivedJournals > 0);
    assert.ok(stats.activeJournalBytes <= cap || stats.activeJournalBytes === 24);
    assert.ok(stats.snapshotBytes > 0);
    const journal = repl.session.journal()!;
    const calls = repl.session.hostCalls();
    assert.equal(calls, 24);
    const restored = await repl.session.run(repl.wasm!, repl.texts, null, journal);
    assert.deepEqual(restored.kept, [...repl.texts.keys()]);
    assert.equal(repl.session.hostCalls(), calls);
    assert.deepEqual(repl.session.journal(), journal);
    assert.deepEqual(repl.printed(), ["", ""]);
    // A fresh worker reconstructs from historical results, then appends
    // against its one initialized base. It never imports foreign storage.
    const base = repl.baseForReplay();
    repl.session.close();
    repl.session = repl.fresh();
    assert.deepEqual((await repl.session.run(base, repl.texts, null, journal)).kept, [
      ...repl.texts.keys(),
    ]);
    assert.equal(repl.session.hostCalls(), 0);
    assert.equal(await repl.input("println(counter.value)\n"), 0);
    assert.deepEqual(repl.printed(), ["24\n", ""]);
    repl.session.close();
  }
});
