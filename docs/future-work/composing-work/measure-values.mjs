// Current dev-profile allocation/call evidence for the lean policy ports.
// Usage: node measure-values.mjs PATH_TO_HD WORK_PACKAGE
// Runs 0, 1, 5 and 9 repetitions of the complete asserted port sequence.
// It measures the inferred apply calls after the prerequisite compiler fix.
// Poll counts can vary with timer scheduling; both interval slopes are reported.
import { execFileSync } from "node:child_process";
import { copyFileSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));
const REPO = resolve(HERE, "..", "..", "..");
const { createHost } = await import(join(REPO, "host", "node.mjs"));

const GIVEN = 244;
const RUNS = [0, 1, 5, 9];

// ---- LEB128 and section helpers ----
const uleb = (n) => {
  const out = [];
  do {
    let b = n & 127;
    n >>>= 7;
    if (n) b |= 128;
    out.push(b);
  } while (n);
  return out;
};
const readU = (d, at) => {
  let value = 0,
    shift = 0,
    b;
  do {
    b = d[at++];
    value += (b & 127) * 2 ** shift;
    shift += 7;
  } while (b & 128);
  return [value, at];
};
const skipS = (d, at) => {
  while (d[at] & 128) at++;
  return at + 1;
};
const readName = (d, at) => {
  const [n, p] = readU(d, at);
  return [new TextDecoder().decode(d.subarray(p, p + n)), p + n];
};
const nameBytes = (s) => {
  const b = [...new TextEncoder().encode(s)];
  return [...uleb(b.length), ...b];
};

function sections(d) {
  const out = [];
  let at = 8;
  while (at < d.length) {
    const id = d[at];
    const [size, p] = readU(d, at + 1);
    out.push({ id, body: d.subarray(p, p + size) });
    at = p + size;
  }
  return out;
}

// ---- the rewrite ----
function instrument(bytes) {
  const d = new Uint8Array(bytes);
  const secs = sections(d);
  const find = (id) => secs.find((s) => s.id === id);
  // Imports: count functions and globals; find whether memory 1 exists.
  let importedFuncs = 0,
    importedGlobals = 0,
    importedMems = 0;
  const imp = find(2);
  if (imp) {
    let [n, at] = readU(imp.body, 0);
    for (let i = 0; i < n; i++) {
      [, at] = readName(imp.body, at);
      [, at] = readName(imp.body, at);
      const kind = imp.body[at++];
      if (kind === 0) {
        [, at] = readU(imp.body, at);
        importedFuncs++;
      } else if (kind === 1) {
        at = skipRef(imp.body, at);
        at = skipLimits(imp.body, at);
      } else if (kind === 2) {
        at = skipLimits(imp.body, at);
        importedMems++;
      } else if (kind === 3) {
        at = skipRef(imp.body, at);
        at++;
        importedGlobals++;
      } else if (kind === 4) {
        at++;
        [, at] = readU(imp.body, at);
      } else throw new Error(`import kind ${kind}`);
    }
  }
  const [definedFuncs] = readU(find(3).body, 0);
  const glob = find(6);
  const [definedGlobals, globRest] = glob ? readU(glob.body, 0) : [0, 0];
  const firstCounter = importedGlobals + definedGlobals;
  const counter = (i) => firstCounter + i;

  // The allocator's metadata global: `global.get G; global.get G;
  // i32.load align=2 memory=1 offset=244`, the increment in `alloc`.
  const code = find(10).body;
  const tables = new Set();
  for (let i = 0; i + 1 < code.length; i++) {
    if (code[i] !== 0x23) continue;
    const [g, p] = readU(code, i + 1);
    if (code[p] !== 0x23) continue;
    const [g2, q] = readU(code, p + 1);
    if (g2 !== g) continue;
    if (
      code[q] === 0x28 &&
      code[q + 1] === 0x42 &&
      code[q + 2] === 0x01 &&
      code[q + 3] === 0xf4 &&
      code[q + 4] === 0x01
    )
      tables.add(g);
  }
  if (tables.size !== 1) throw new Error(`allocator counter pattern found ${tables.size} times`);
  const table = [...tables][0];

  // New code section: a counter increment opens every body.
  let [count, at] = readU(code, 0);
  if (count !== definedFuncs) throw new Error("function and code counts differ");
  const bodies = [];
  for (let i = 0; i < count; i++) {
    const [size, p] = readU(code, at);
    const body = code.subarray(p, p + size);
    let [groups, q] = readU(body, 0);
    for (let g = 0; g < groups; g++) {
      [, q] = readU(body, q);
      q = skipRef(body, q);
    }
    const g = uleb(counter(i));
    const bump = [0x23, ...g, 0x41, 0x01, 0x6a, 0x24, ...g];
    const next = [...body.subarray(0, q), ...bump, ...body.subarray(q)];
    bodies.push([...uleb(next.length), ...next]);
    at = p + size;
  }
  const newCode = [...uleb(bodies.length)];
  for (const b of bodies) for (const x of b) newCode.push(x);

  // Globals: the old ones, then one mutable i32 per function.
  const counters = Array.from({ length: definedFuncs }, () => [0x7f, 0x01, 0x41, 0x00, 0x0b]);
  const newGlobals = [
    ...uleb(definedGlobals + definedFuncs),
    ...(glob ? glob.body.subarray(globRest) : []),
  ];
  for (const c of counters) for (const x of c) newGlobals.push(x);

  // Exports: the old ones, the counters, the allocator global and memory 1.
  const exp = find(7).body;
  let [nexp, ep] = readU(exp, 0);
  const old = exp.subarray(ep);
  let hasHeap = false;
  {
    let p = 0;
    for (let i = 0; i < nexp; i++) {
      let name;
      [name, p] = readName(old, p);
      const kind = old[p++];
      let idx;
      [idx, p] = readU(old, p);
      if (kind === 2 && idx === 1) hasHeap = name;
    }
  }
  const added = [];
  for (let i = 0; i < definedFuncs; i++)
    added.push([...nameBytes(`probe.c${i}`), 3, ...uleb(counter(i))]);
  added.push([...nameBytes("probe.table"), 3, ...uleb(table)]);
  if (!hasHeap) added.push([...nameBytes("probe.heap"), 2, ...uleb(1)]);
  const newExports = [...uleb(nexp + added.length), ...old];
  for (const a of added) for (const x of a) newExports.push(x);

  // Function names from the name section, for the report.
  const names = new Map();
  for (const s of secs) {
    if (s.id !== 0) continue;
    const [title, p0] = readName(s.body, 0);
    // A release build writes `hd.names` (index, name) pairs instead of a
    // `name` section (crates/hd_wasm_linear/src/module.rs, compact_names).
    if (title === "hd.names") {
      let [n, r] = readU(s.body, p0);
      for (let i = 0; i < n; i++) {
        let idx, nm;
        [idx, r] = readU(s.body, r);
        [nm, r] = readName(s.body, r);
        names.set(idx, nm);
      }
      continue;
    }
    if (title !== "name") continue;
    let p = p0;
    while (p < s.body.length) {
      const kind = s.body[p];
      const [size, q] = readU(s.body, p + 1);
      if (kind === 1) {
        let [n, r] = readU(s.body, q);
        for (let i = 0; i < n; i++) {
          let idx, nm;
          [idx, r] = readU(s.body, r);
          [nm, r] = readName(s.body, r);
          names.set(idx, nm);
        }
      }
      p = q + size;
    }
  }

  const parts = [d.subarray(0, 8)];
  const emit = (id, body) =>
    parts.push(Uint8Array.from([id, ...uleb(body.length)]), Uint8Array.from(body));
  let globalsDone = false;
  for (const s of secs) {
    // The global section sits after memory (5) and tag (13), before export (7).
    if (!globalsDone && !glob && s.id === 7) {
      emit(6, newGlobals);
      globalsDone = true;
    }
    if (s.id === 6) {
      emit(6, newGlobals);
      globalsDone = true;
    } else if (s.id === 7) emit(7, newExports);
    else if (s.id === 10) emit(10, newCode);
    else emit(s.id, s.body);
  }
  const total = parts.reduce((n, p) => n + p.length, 0);
  const out = new Uint8Array(total);
  let o = 0;
  for (const p of parts) {
    out.set(p, o);
    o += p.length;
  }
  return { bytes: out, importedFuncs, definedFuncs, names, heapExport: hasHeap || "probe.heap" };
}

function skipRef(d, at) {
  const b = d[at];
  if (b === 0x63 || b === 0x64) return skipS(d, at + 1);
  return at + 1;
}
function skipLimits(d, at) {
  const flag = d[at++];
  [, at] = readU(d, at);
  if (flag & 1) [, at] = readU(d, at);
  return at;
}

// ---- running ----
async function measure(module, info, args) {
  let stdout = "";
  const host = createHost(
    {
      out: (t) => {
        stdout += t;
      },
      err: (t) => process.stderr.write(t),
    },
    { module, args, program: "policy-cost", grants: {} },
  );
  try {
    const instance = await WebAssembly.instantiate(module, host.imports);
    const { status } = await host.run(instance, "hd.init", "hd.poll", null, null);
    if (status !== 0) throw new Error(`status ${status} for ${args.join(" ")}`);
    const ex = instance.exports;
    const table = ex["probe.table"].value;
    const heap = ex[info.heapExport];
    const allocations = table === 0 ? 0 : new DataView(heap.buffer).getUint32(table + GIVEN, true);
    let calls = 0,
      runtime = 0;
    const byName = new Map();
    for (let i = 0; i < info.definedFuncs; i++) {
      const c = ex[`probe.c${i}`].value >>> 0;
      if (!c) continue;
      calls += c;
      const name = info.names.get(info.importedFuncs + i) ?? `f${i}`;
      if (/^rt:|^std\/rt\./.test(name)) runtime += c;
      byName.set(name, c);
    }
    return { stdout, allocations, calls, runtime, byName };
  } finally {
    host.close();
  }
}

const hd = resolve(process.argv[2] ?? join(REPO, "target", "debug", "hd"));
const work = resolve(process.argv[3] ?? join(REPO, "target", "policy-values-cost"));
mkdirSync(join(work, "src"), { recursive: true });
writeFileSync(join(work, "hd.toml"), '[package]\nname = "policy-values-cost"\n');
copyFileSync(join(HERE, "policy-values-cost.hd"), join(work, "src", "main.hd"));
execFileSync(hd, ["build", "--engine", "node"], { cwd: work, stdio: "inherit" });
const wasm = readFileSync(join(work, "build", "debug", "policy-values-cost.wasm"));
const info = instrument(wasm);
writeFileSync(join(work, "instrumented.wasm"), info.bytes);
const module = await WebAssembly.compile(info.bytes);
const rows = [];
for (const count of RUNS) {
  const result = await measure(module, info, [String(count)]);
  const { byName, ...plain } = result;
  rows.push({ count, ...plain, byName: [...byName] });
}
const slopes = {};
for (const key of ["allocations", "calls", "runtime"]) {
  slopes[key] = {
    first: (rows[2][key] - rows[1][key]) / 4,
    second: (rows[3][key] - rows[2][key]) / 4,
    whole: (rows[3][key] - rows[1][key]) / 8,
  };
}
const result = {
  wasm_bytes: wasm.length,
  instrumented_bytes: info.bytes.length,
  defined_functions: info.definedFuncs,
  runs: rows,
  slopes,
};
writeFileSync(join(work, "measurements.json"), JSON.stringify(result, null, 2) + "\n");
console.log(
  JSON.stringify({ ...result, runs: rows.map(({ stdout, byName, ...row }) => row) }, null, 2),
);
