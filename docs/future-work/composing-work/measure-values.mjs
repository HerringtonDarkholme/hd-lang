// Dev-profile allocation/call evidence for actual public std.task policy ports.
// Usage: node measure-values.mjs PATH_TO_HD WORK_PACKAGE [SOURCE_REPO] [--smoke] [--check] [--baseline JSON]
// --smoke validates rewrite/parity at zero repetitions; it establishes no workload baseline.
// Runs 0, 1, 5 and 9 repetitions of the complete asserted port sequence.
// It measures the inferred apply calls after the prerequisite compiler fix.
// Poll counts can vary with timer scheduling; both interval slopes are reported.
import { createHash } from "node:crypto";
import { execFileSync } from "node:child_process";
import { copyFileSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));
function invalid(code, detail) {
  const error = new Error(`${code}: ${detail}`);
  error.code = code;
  error.exitCode = 2;
  throw error;
}
function options(args) {
  const positional = [];
  let smoke = false,
    check = false,
    baseline;
  for (let i = 0; i < args.length; i++) {
    const arg = args[i];
    if (arg === "--smoke" && !smoke) smoke = true;
    else if (arg === "--check" && !check) check = true;
    else if (arg === "--baseline" && baseline === undefined) {
      baseline = args[++i];
      if (!baseline || baseline.startsWith("--")) invalid("usage", "--baseline needs a JSON path");
    } else if (arg.startsWith("--")) invalid("usage", `unknown or repeated option ${arg}`);
    else positional.push(arg);
  }
  if (positional.length > 3) invalid("usage", "expected HD WORK [SOURCE_REPO]");
  if (baseline !== undefined && !check) invalid("usage", "--baseline requires --check");
  if (smoke && check) invalid("incomplete-measurement", "--check cannot use --smoke");
  return { positional, smoke, check, baseline };
}
process.on("uncaughtException", (error) => {
  if (error.exitCode !== 2) throw error;
  console.error(error.message);
  process.exitCode = 2;
});
const OPTIONS = options(process.argv.slice(2));
const REPO = resolve(OPTIONS.positional[2] ?? resolve(HERE, "..", "..", ".."));
const { createHost } = await import(join(REPO, "host", "node.mjs"));
const GIVEN = 244;
const SMOKE = OPTIONS.smoke;
const RUNS = SMOKE ? [0] : [0, 1, 5, 9];
const GRANTS = { Args: true, Clock: true, Console: true };
const COUNTER_CONTRACT = {
  version: 1,
  allocation_offset: 244,
  heap_memory_index: 1,
  entry_counters: "defined-functions-i32",
  runtime_name_pattern: "^rt:|^std/rt\\.",
};
const sha256 = (bytes) => createHash("sha256").update(bytes).digest("hex");
// Three reviewed full cohorts had identical rows; ceilings are their maxima.
// Compiler/std/host hashes are provenance, never implementation equality gates.
const BUILTIN_BASELINE = {
  format: 1,
  kind: "hd-production-policy-cost-baseline",
  compatibility: {
    profile: "dev",
    engine: "node",
    node: "v24.19.0",
    probe_sha256: "5a0bc9066a80a92bfb7c08bcd7d85ba73c5f87547c81d9e760e387add3a05e18",
    instrumenter_sha256: "84e3e9f39b88e03133df6b0c4582a4d8e4578497fe66a0cf38970e002719664e",
    manifest_sha256: "464385da23c3364b9656205a74269a84c5d05592d84dde11cea057920d237b0f",
    grants: {
      Args: true,
      Clock: true,
      Console: true,
    },
    run_counts: [0, 1, 5, 9],
    counter_contract: {
      version: 1,
      allocation_offset: 244,
      heap_memory_index: 1,
      entry_counters: "defined-functions-i32",
      runtime_name_pattern: "^rt:|^std/rt\\.",
    },
  },
  reference: {
    cohorts: 3,
    measurement_sha256: [
      "e795ae30cc9d0965bcda737c00698063d9020c093c605bea67f12c6af9b8996c",
      "e795ae30cc9d0965bcda737c00698063d9020c093c605bea67f12c6af9b8996c",
      "e795ae30cc9d0965bcda737c00698063d9020c093c605bea67f12c6af9b8996c",
    ],
    compiler_sha256: "70a24761809fd6977bdf8a0b2bbd273a4459e61386dbd986baec4a52dc3a5348",
    source_inputs: {
      task: "290ee918bd4adf2e27c8dbf6d7a50f72f529148b530478f2873a7ad2074a0e2a",
      allocator: "e50affe5d8513b22e4e91b88a1f2bad7564d4d9f40f6283b708c43535405f5ed",
      allocationEmitter: "15f766cd2c94924270be2d144ebdf746520c3093084627ac72e8e91e87ad8930",
      host: "1b85e1c2fbbc4edc0e0107357ef3dae030c9baefee92c46206ba57698bb305ec",
    },
    original_sha256: "28efa22ed2d8f6ceca381d979722b7cc07a58ae7b7143ad4890f36e801d18a3d",
    instrumented_sha256: "c5a2836806c24783674397852b2fe13b681acc7bb19d60b0f86a72d8c90c4472",
    raw_rows: [
      {
        count: 0,
        allocations: 3,
        calls: 228,
        runtime: 215,
      },
      {
        count: 1,
        allocations: 302,
        calls: 28511,
        runtime: 27508,
      },
      {
        count: 5,
        allocations: 1487,
        calls: 140365,
        runtime: 135306,
      },
      {
        count: 9,
        allocations: 2672,
        calls: 252200,
        runtime: 243085,
      },
    ],
  },
  ceilings: {
    allocations: {
      startup: 3,
      first_sequence: 299,
      first: 296.25,
      second: 296.25,
      whole: 296.25,
    },
    calls: {
      startup: 228,
      first_sequence: 28283,
      first: 27963.5,
      second: 27958.75,
      whole: 27961.125,
    },
    runtime: {
      startup: 215,
      first_sequence: 27293,
      first: 26949.5,
      second: 26944.75,
      whole: 26947.125,
    },
  },
};

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

// ---- guardrail ----
function same(a, b) {
  if (a === null || b === null || typeof a !== "object" || typeof b !== "object") return a === b;
  const ak = Object.keys(a).sort(),
    bk = Object.keys(b).sort();
  return ak.length === bk.length && ak.every((key, i) => key === bk[i] && same(a[key], b[key]));
}
function validateBaseline(baseline) {
  if (baseline?.format !== 1 || baseline.kind !== "hd-production-policy-cost-baseline")
    invalid("baseline-format", "unsupported baseline");
  const reference = baseline.reference;
  if (
    !reference ||
    !Number.isInteger(reference.cohorts) ||
    reference.cohorts < 1 ||
    !Array.isArray(reference.measurement_sha256) ||
    reference.measurement_sha256.length !== reference.cohorts
  )
    invalid("baseline-format", "missing reviewed cohort provenance");
  const hashes = [
    ...reference.measurement_sha256,
    reference.compiler_sha256,
    reference.original_sha256,
    reference.instrumented_sha256,
    ...["task", "allocator", "allocationEmitter", "host"].map(
      (key) => reference.source_inputs?.[key],
    ),
  ];
  if (hashes.some((hash) => typeof hash !== "string" || !/^[a-f0-9]{64}$/.test(hash)))
    invalid("baseline-format", "invalid reference provenance hash");
  const c = baseline.compatibility;
  if (!c || !baseline.reference || !same(c.run_counts, [0, 1, 5, 9]))
    invalid("baseline-format", "missing compatibility/reference or invalid run counts");
  for (const field of ["profile", "engine", "node"])
    if (typeof c[field] !== "string" || !c[field]) invalid("baseline-format", `missing ${field}`);
  for (const field of ["probe_sha256", "instrumenter_sha256", "manifest_sha256"])
    if (!/^[a-f0-9]{64}$/.test(c[field] ?? "")) invalid("baseline-format", `invalid ${field}`);
  if (!same(c.grants, GRANTS) || !same(c.counter_contract, COUNTER_CONTRACT))
    invalid("baseline-format", "unsupported grants or counter contract");
  for (const metric of ["allocations", "calls", "runtime"]) {
    for (const interval of ["startup", "first_sequence", "first", "second", "whole"]) {
      const limit = baseline.ceilings?.[metric]?.[interval];
      if (typeof limit !== "number" || !Number.isFinite(limit) || limit < 0)
        invalid(
          "baseline-unconfigured",
          `${metric}.${interval} needs a finite nonnegative ceiling`,
        );
    }
  }
}
function compatibility(baseline, current) {
  for (const [key, expected] of Object.entries(baseline.compatibility)) {
    if (!same(current[key], expected))
      invalid(
        "measurement-incompatible",
        `${key}: expected ${JSON.stringify(expected)}, got ${JSON.stringify(current[key])}`,
      );
  }
}
function instrumenterHash() {
  if (GIVEN !== COUNTER_CONTRACT.allocation_offset)
    invalid("counter-layout-mismatch", "GIVEN differs from counter contract");
  const source = readFileSync(fileURLToPath(import.meta.url), "utf8").replaceAll("\r\n", "\n");
  const start = source.indexOf("// ---- " + "LEB128 and section helpers ----");
  const end = source.indexOf("// ---- " + "guardrail ----", start);
  if (start < 0 || end < 0)
    invalid("counter-layout-mismatch", "instrumenter source boundaries missing");
  return sha256(JSON.stringify(COUNTER_CONTRACT) + "\n" + source.slice(start, end));
}
function summaries(rows) {
  if (
    rows.length !== 4 ||
    !same(
      rows.map((r) => r.count),
      [0, 1, 5, 9],
    )
  )
    invalid("incomplete-measurement", "expected exactly counts 0,1,5,9 in order");
  for (const row of rows) {
    if (
      row.parity !== true ||
      row.status !== 0 ||
      typeof row.stdout !== "string" ||
      typeof row.stderr !== "string"
    )
      invalid("parity-mismatch", `missing successful parity at count ${row.count}`);
    for (const metric of ["allocations", "calls", "runtime"])
      if (!Number.isSafeInteger(row[metric]) || row[metric] < 0)
        invalid("invalid-counter", `${metric} at count ${row.count}`);
    if (row.runtime > row.calls)
      invalid("invalid-counter", `runtime exceeds calls at count ${row.count}`);
  }
  return Object.fromEntries(
    ["allocations", "calls", "runtime"].map((metric) => [
      metric,
      {
        startup: rows[0][metric],
        first_sequence: rows[1][metric] - rows[0][metric],
        first: (rows[2][metric] - rows[1][metric]) / 4,
        second: (rows[3][metric] - rows[2][metric]) / 4,
        whole: (rows[3][metric] - rows[1][metric]) / 8,
      },
    ]),
  );
}
function checkCosts(baseline, current) {
  compatibility(baseline, current);
  const values = summaries(current.runs);
  const comparisons = [];
  for (const metric of ["allocations", "calls", "runtime"])
    for (const interval of ["startup", "first_sequence", "first", "second", "whole"]) {
      const actual = values[metric][interval],
        maximum = baseline.ceilings[metric][interval];
      comparisons.push({ metric, interval, actual, maximum, passed: actual <= maximum });
    }
  const reference = baseline.reference;
  const implementation = {
    compiler: { before: reference.compiler_sha256, after: current.compiler.sha256 },
  };
  for (const [name, source] of Object.entries(current.source_inputs))
    implementation[name] = { before: reference.source_inputs?.[name], after: source.sha256 };
  const changes = Object.fromEntries(
    Object.entries(implementation).filter(([, pair]) => pair.before !== pair.after),
  );
  return {
    format: 1,
    status: comparisons.every((c) => c.passed) ? "passed" : "regression",
    baseline_sha256: sha256(JSON.stringify(baseline)),
    implementation_changes: changes,
    comparisons,
  };
}
// Import validation distinguishes mutable i32 globals from integer-valued f64 globals.
const COUNTER_TYPE = new WebAssembly.Module(
  Uint8Array.from([0, 97, 115, 109, 1, 0, 0, 0, 2, 8, 1, 1, 112, 1, 103, 3, 127, 1]),
);
function functionNameCoverage(info, required) {
  const missing = [];
  for (let i = 0; i < info.definedFuncs; i++) {
    const index = info.importedFuncs + i;
    const name = info.names.get(index);
    if (typeof name !== "string" || name.trim() === "") missing.push(index);
  }
  if (required && missing.length)
    invalid("missing-function-name", `defined function indices ${missing.join(", ")}`);
  return { complete: missing.length === 0, missing_defined_indices: missing };
}
function functionCounts(ex, info) {
  let calls = 0,
    runtime = 0;
  const byName = new Map();
  const byFunction = [];
  for (let i = 0; i < info.definedFuncs; i++) {
    const count = counter(ex, `probe.c${i}`);
    if (!count) continue;
    const index = info.importedFuncs + i;
    const name = info.names.get(index) || `<unnamed:${index}>`;
    calls += count;
    if (/^rt:|^std\/rt\./.test(name)) runtime += count;
    byFunction.push({ index, name, count });
    byName.set(name, (byName.get(name) ?? 0) + count);
  }
  return { calls, runtime, byName, byFunction };
}

function counter(ex, name) {
  const value = ex[name];
  if (
    !(value instanceof WebAssembly.Global) ||
    !Number.isInteger(value.value) ||
    value.value < -2147483648 ||
    value.value > 2147483647
  )
    invalid("missing-counter", `${name} must be an exported i32 global`);
  try {
    new WebAssembly.Instance(COUNTER_TYPE, { p: { g: value } });
  } catch {
    invalid("missing-counter", `${name} must be a mutable i32 global`);
  }
  return value.value >>> 0;
}

// ---- running ----
async function measure(module, info, args) {
  let stdout = "";
  let stderr = "";
  const host = createHost(
    {
      out: (t) => {
        stdout += t;
      },
      err: (t) => {
        stderr += t;
      },
    },
    { module, args, program: "policy-cost", grants: GRANTS },
  );
  try {
    const instance = await WebAssembly.instantiate(module, host.imports);
    const { status } = await host.run(instance, "hd.init", "hd.poll", null, null);
    if (status !== 0) throw new Error(`status ${status} for ${args.join(" ")}`);
    if (info === null) return { stdout, stderr, status };
    const ex = instance.exports;
    const table = counter(ex, "probe.table");
    const heap = ex[info.heapExport];
    if (
      !(heap instanceof WebAssembly.Memory) ||
      (table !== 0 && table + GIVEN + 4 > heap.buffer.byteLength)
    )
      invalid("invalid-counter-memory", "allocator metadata is outside the exported heap");
    const allocations = table === 0 ? 0 : new DataView(heap.buffer).getUint32(table + GIVEN, true);
    const { calls, runtime, byName, byFunction } = functionCounts(ex, info);
    return { stdout, stderr, status, allocations, calls, runtime, byName, byFunction };
  } finally {
    host.close();
  }
}

const hd = resolve(OPTIONS.positional[0] ?? join(REPO, "target", "debug", "hd"));
const work = resolve(OPTIONS.positional[1] ?? join(REPO, "target", "policy-values-cost"));
let selectedBaseline = BUILTIN_BASELINE;
if (OPTIONS.baseline) {
  try {
    selectedBaseline = JSON.parse(readFileSync(resolve(OPTIONS.baseline), "utf8"));
  } catch (error) {
    invalid("baseline-format", error.message);
  }
}
const measurementCompatibility = {
  profile: "dev",
  engine: "node",
  node: process.version,
  probe_sha256: sha256(readFileSync(join(HERE, "policy-values-cost.hd"))),
  instrumenter_sha256: instrumenterHash(),
  manifest_sha256: sha256(
    '[package]\nname = "policy-values-cost"\n\n[capabilities]\nArgs = true\nClock = true\nConsole = true\n',
  ),
  grants: GRANTS,
  run_counts: RUNS,
  counter_contract: COUNTER_CONTRACT,
};
if (OPTIONS.check) {
  validateBaseline(selectedBaseline);
  compatibility(selectedBaseline, measurementCompatibility);
}
mkdirSync(join(work, "src"), { recursive: true });
writeFileSync(
  join(work, "hd.toml"),
  '[package]\nname = "policy-values-cost"\n\n[capabilities]\nArgs = true\nClock = true\nConsole = true\n',
);
copyFileSync(join(HERE, "policy-values-cost.hd"), join(work, "src", "main.hd"));
execFileSync(hd, ["build", "--engine", "node"], { cwd: work, stdio: "inherit" });
const wasm = readFileSync(join(work, "build", "debug", "policy-values-cost.wasm"));
let info;
try {
  info = instrument(wasm);
} catch (error) {
  invalid("counter-layout-mismatch", error.message);
}
const functionNames = functionNameCoverage(info, OPTIONS.check);
if (!functionNames.complete)
  console.error(
    `incomplete-function-names: runtime subtotal excludes ${functionNames.missing_defined_indices.length} unnamed defined functions`,
  );
writeFileSync(join(work, "instrumented.wasm"), info.bytes);
const original = await WebAssembly.compile(wasm);
const module = await WebAssembly.compile(info.bytes);
const rows = [];
for (const count of RUNS) {
  const args = [String(count)];
  const baseline = await measure(original, null, args);
  const result = await measure(module, info, args);
  for (const key of ["status", "stdout", "stderr"]) {
    if (baseline[key] !== result[key])
      invalid("parity-mismatch", `instrumentation changed ${key} at count ${count}`);
  }
  const { byName, ...plain } = result;
  rows.push({ count, parity: true, ...plain, byName: [...byName] });
}
const slopes = {};
for (const key of SMOKE ? [] : ["allocations", "calls", "runtime"]) {
  slopes[key] = {
    first: (rows[2][key] - rows[1][key]) / 4,
    second: (rows[3][key] - rows[2][key]) / 4,
    whole: (rows[3][key] - rows[1][key]) / 8,
  };
}
const sourceInputs = Object.fromEntries(
  [
    ["task", join(REPO, "lib/std/task.hd")],
    ["allocator", join(REPO, "lib/std/rt/alloc.hd")],
    ["allocationEmitter", join(REPO, "crates/hd_wasm_linear/src/rt.rs")],
    ["host", join(REPO, "host/node.mjs")],
  ].map(([name, path]) => [name, { path, sha256: sha256(readFileSync(path)) }]),
);
const result = {
  ...measurementCompatibility,
  format: 1,
  mode: SMOKE ? "smoke-zero-repetitions" : "production-port-sequence",
  profile: "dev",
  node: process.version,
  compiler: { path: hd, sha256: sha256(readFileSync(hd)) },
  probe_sha256: sha256(readFileSync(join(work, "src/main.hd"))),
  script_sha256: sha256(readFileSync(fileURLToPath(import.meta.url))),
  manifest_sha256: sha256(readFileSync(join(work, "hd.toml"))),
  source_inputs: sourceInputs,
  grants: GRANTS,
  original_sha256: sha256(wasm),
  instrumented_sha256: sha256(info.bytes),
  scope:
    "guest allocation calls and defined Wasm entries; no latency, host allocation or live-byte claim",
  wasm_bytes: wasm.length,
  instrumented_bytes: info.bytes.length,
  defined_functions: info.definedFuncs,
  function_names: functionNames,
  runs: rows,
  slopes,
};
writeFileSync(join(work, "measurements.json"), JSON.stringify(result, null, 2) + "\n");
console.log(
  JSON.stringify(
    {
      ...result,
      runs: rows.map(
        ({ stdout: _stdout, byName: _byName, byFunction: _byFunction, ...row }) => row,
      ),
    },
    null,
    2,
  ),
);

if (OPTIONS.check) {
  const check = checkCosts(selectedBaseline, result);
  writeFileSync(join(work, "guardrail-check.json"), JSON.stringify(check, null, 2) + "\n");
  console.log(JSON.stringify(check, null, 2));
  if (check.status === "regression") {
    for (const c of check.comparisons.filter((c) => !c.passed))
      console.error(`cost-regression: ${c.metric}.${c.interval} ${c.actual} exceeds ${c.maximum}`);
    process.exitCode = 1;
  }
}
