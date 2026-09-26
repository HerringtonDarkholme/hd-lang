// 5.5 suspension lowering probes: poll cost vs depth, code size vs site count,
// cancellation cleanup order, and driver-guard behavior.
// Run: node --experimental-strip-types audit/scripts/arch/susp-probes.ts
import { writeFileSync } from "node:fs";
import { resolve } from "node:path";

import { instantiate, type InstantiateOptions } from "../../../src/compiler.ts";
import { compilePhased, exported, functionBodies, header, median } from "./bench-lib.ts";

const root = resolve(import.meta.dirname, "../../..");
const probeDir = resolve(root, "audit/probes/arch/suspension");
const evidenceDir = resolve(root, "audit/evidence/05-requirements");
const out: string[] = [header("node --experimental-strip-types audit/scripts/arch/susp-probes.ts")];

function save(name: string, source: string): string {
  writeFileSync(resolve(probeDir, name), source);
  return source;
}

async function load(source: string, options: InstantiateOptions = {}) {
  const result = await instantiate(source, options);
  const indexOf = (name: string): number => {
    const found = result.compilation.hir.functions.find((fn) => fn.name === name);
    if (!found) throw new Error(`no function ${name}`);
    return found.suspensionIndex ?? found.index;
  };
  return { ...result, indexOf };
}

// 1. Poll cost versus suspension depth.
function depthChain(depth: number): string {
  const lines = [`# probe 5.5: ${depth}-deep suspension chain; leaf held pending by the dev runtime`];
  lines.push(`fn s${depth}!() -> i32: 1`);
  for (let level = depth - 1; level >= 1; level -= 1)
    lines.push(`fn s${level}!() -> i32: s${level + 1}!() + 1`);
  lines.push(`fn main!() -> i32: s1!()`);
  return lines.join("\n") + "\n";
}

const POLLS = 4000;
out.push("\n## Poll cost versus depth\n");
out.push(`Leaf frame held pending for ${POLLS} polls; root driven through \`__hd_poll\`. Median of 5 runs.\n`);
out.push("| depth | ns per root poll | pending-hook calls per poll | trace-hook calls per poll |");
out.push("| --- | --- | --- | --- |");
for (const depth of [1, 4, 16, 64]) {
  const source = save(`depth-${depth}.hd`, depthChain(depth));
  let leaf = -1;
  let pendingCalls = 0;
  let traceCalls = 0;
  const loaded = await load(source, {
    pending: (fn, count) => {
      pendingCalls += 1;
      return fn === leaf && count <= POLLS;
    },
    trace: () => {
      traceCalls += 1;
    },
  });
  leaf = loaded.indexOf(`s${depth}`);
  const start = exported(loaded.instance, "__hd_start");
  const poll = exported(loaded.instance, "__hd_poll");
  const cancel = exported(loaded.instance, "__hd_cancel");
  const times: number[] = [];
  let perPollPending = 0;
  let perPollTrace = 0;
  for (let run = 0; run < 6; run += 1) {
    // Fresh instance per run keeps the one-shot frame valid.
    const again = run === 0 ? loaded : await load(source, {
      pending: (fn, count) => {
        pendingCalls += 1;
        return fn === leaf && count <= POLLS;
      },
      trace: () => {
        traceCalls += 1;
      },
    });
    const s = run === 0 ? start : exported(again.instance, "__hd_start");
    const p = run === 0 ? poll : exported(again.instance, "__hd_poll");
    s();
    pendingCalls = 0;
    traceCalls = 0;
    const begin = performance.now();
    let polls = 0;
    while (p() === 0) polls += 1;
    const elapsed = performance.now() - begin;
    if (run > 0) times.push((elapsed * 1e6) / (polls + 1));
    perPollPending = pendingCalls / (polls + 1);
    perPollTrace = traceCalls / (polls + 1);
  }
  void cancel;
  out.push(`| ${depth} | ${median(times).toFixed(0)} | ${perPollPending.toFixed(2)} | ${perPollTrace.toFixed(2)} |`);
}

// 2. Code size versus number of suspension sites in one frame.
function sites(count: number): string {
  const lines = [`# probe 5.5: one frame with ${count} sequential suspension sites`, "fn step!(value: i32) -> i32: value + 1", "fn main!() -> i32:", "    v0 := 0"];
  for (let index = 1; index <= count; index += 1) lines.push(`    v${index} := step!(v${index - 1})`);
  lines.push(`    v${count}`);
  return lines.join("\n") + "\n";
}
out.push("\n## Frame code size versus suspension sites\n");
out.push("| sites | WAT bytes of main poll fn | child poll calls in main poll fn | frame spill stores in main poll fn | wasm bytes (module) | compile ms (emit / assemble) |");
out.push("| --- | --- | --- | --- | --- | --- |");
for (const count of [1, 2, 4, 8, 16, 32, 48]) {
  const source = save(`sites-${count}.hd`, sites(count));
  const compiled = compilePhased(source);
  const bodies = functionBodies(compiled.wat);
  const pollName = [...bodies.keys()].filter((name) => /^\$poll\d+$/.test(name)).at(-1)!;
  const body = bodies.get(pollName)!;
  const childPolls = [...body.matchAll(/\(call \$poll0 /g)].length;
  const spills = [...body.matchAll(/\(struct\.set \$s\d+ \$s\d+l\d+/g)].length;
  out.push(`| ${count} | ${body.length} | ${childPolls} | ${spills} | ${compiled.bytes.length} | ${compiled.times.emit.toFixed(0)} / ${compiled.times.assemble.toFixed(0)} |`);
}

out.push("\n## CFG lowering code size versus sites (`v = step!(v) + 0`)\n");
out.push("| sites | WAT bytes of main poll fn | wasm bytes (module) | compile ms (emit / assemble) |");
out.push("| --- | --- | --- | --- |");
for (const count of [1, 2, 8, 16, 32, 48]) {
  const source = sites(count).replace(/step!\((v\d+)\)\n/g, "step!($1) + 0\n");
  const compiled = compilePhased(source);
  const bodies = functionBodies(compiled.wat);
  const pollName = [...bodies.keys()].filter((name) => /^\$poll\d+$/.test(name)).at(-1)!;
  out.push(`| ${count} | ${bodies.get(pollName)!.length} | ${compiled.bytes.length} | ${compiled.times.emit.toFixed(0)} / ${compiled.times.assemble.toFixed(0)} |`);
}

// 3. Cancellation cleanup order across nested frames.
const cleanupSource = save(
  "cancel-order.hd",
  `# probe 5.5: cancellation cleanup order, 3 frames with several defer suites each
let log: i32 = 0

fn note(value: i32) -> void:
    log = log * 10 + value

fn wait!() -> void:
    pass

fn leaf!() -> void:
    defer:
        note(1)
    defer:
        note(2)
    wait!()
    defer:
        note(9)

fn middle!() -> void:
    defer:
        note(3)
    if true:
        defer:
            note(4)
        leaf!()
    defer:
        note(8)

fn main!() -> void:
    defer:
        note(5)
    defer:
        note(6)
    middle!()

pub fn order() -> i32:
    log
`,
);
out.push("\n## Cancellation cleanup order\n");
{
  let waitIndex = -1;
  const cancelled = await load(cleanupSource, { pending: (fn) => fn === waitIndex });
  waitIndex = cancelled.indexOf("wait");
  exported(cancelled.instance, "__hd_start")();
  const firstPoll = exported(cancelled.instance, "__hd_poll")();
  exported(cancelled.instance, "__hd_cancel")();
  const cancelOrder = exported(cancelled.instance, "order")();
  const normal = await load(cleanupSource);
  exported(normal.instance, "main")();
  const normalOrder = exported(normal.instance, "order")();
  out.push(`- first poll returned ${firstPoll} (0 = pending)`);
  out.push(`- cancellation log: ${cancelOrder} (expected by spec 11 Cancellation + 06 Deferred Cleanup: 214365)`);
  out.push(`- normal completion log: ${normalOrder} (expected: 92148365)`);
}

// 4. Driver guard: per instance, reentrant host callback, and panic poisoning.
out.push("\n## Driver guard\n");
const guardSource = save(
  "guard.hd",
  `# probe 5.5: driver guard behavior
let divisor: i32 = 1

pub fn set_divisor(value: i32) -> void:
    divisor = value

fn leaf!() -> i32: 10 / divisor

fn main!() -> i32: leaf!() + 1
`,
);
async function attempt(label: string, action: () => unknown): Promise<void> {
  try {
    const value = action();
    out.push(`- ${label}: returned ${String(value)}`);
  } catch (error) {
    out.push(`- ${label}: threw ${(error as Error).name}: ${(error as Error).message}`);
  }
}
{
  let leafIndex = -1;
  const a = await load(guardSource, { pending: (fn, count) => fn === leafIndex && count === 1 });
  leafIndex = a.indexOf("leaf");
  const b = await load(guardSource);
  exported(a.instance, "__hd_start")();
  await attempt("instance A started (pending); A.__hd_poll()", () => exported(a.instance, "__hd_poll")());
  await attempt("instance B.main() while A holds its driver", () => exported(b.instance, "main")());
  await attempt("instance A.main() while A's dev frame is started", () => exported(a.instance, "main")());
  let reentrant: WebAssembly.Instance | undefined;
  let hostCalled = false;
  const c = await load(guardSource, {
    pending: () => {
      if (!hostCalled && reentrant) {
        hostCalled = true;
        return Boolean(exported(reentrant, "main")());
      }
      return false;
    },
  });
  reentrant = c.instance;
  await attempt("host pending callback re-enters C.main() during C.main()", () => exported(c.instance, "main")());
  const d = await load(guardSource);
  exported(d.instance, "set_divisor")(0);
  await attempt("instance D.main() with divisor 0 (panic inside a drive)", () => exported(d.instance, "main")());
  exported(d.instance, "set_divisor")(1);
  await attempt("instance D.main() after that panic, divisor 1", () => exported(d.instance, "main")());
}

// 5. Per-call cost of a fn! call that completes immediately vs a plain call.
writeFileSync(resolve(evidenceDir, "susp-probes.md"), out.join("\n") + "\n");
console.log(out.join("\n"));
