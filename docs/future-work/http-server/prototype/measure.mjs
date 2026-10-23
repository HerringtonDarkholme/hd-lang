if (!process.argv[2]) throw new Error("pass a scratch evidence directory");
import { readFileSync, writeFileSync } from "node:fs";
import { gzipSync } from "node:zlib";
import { createCoreHost } from "../../../../host/core.mjs";
const bytes = readFileSync(new URL("./app/build/debug/muse-http-prototype.wasm", import.meta.url));
let at = 8;
const u = () => {
  let x = 0,
    s = 0,
    n;
  do {
    n = bytes[at++];
    x += (n & 127) * 2 ** s;
    s += 7;
  } while (n & 128);
  return x;
};
const enc = (n) => {
  const a = [];
  do {
    let b = n & 127;
    n = Math.floor(n / 128);
    a.push(b | (n ? 128 : 0));
  } while (n);
  return a;
};
let funcs = 0,
  codeBytes = 0,
  memories = [];
const sections = [];
while (at < bytes.length) {
  const begin = at;
  const tag = bytes[at++];
  const length = u(),
    start = at,
    end = at + length;
  if (tag === 3) funcs = u();
  if (tag === 10) codeBytes = length;
  if (tag === 5) {
    const count = u();
    for (let i = 0; i < count; i++) {
      const flags = u(),
        minimum = u(),
        maximum = flags & 1 ? u() : null;
      memories.push({ index: i, minimumBytes: minimum * 65536, maximum });
    }
  }
  if (tag === 7) {
    const count = u(),
      rest = bytes.subarray(at, end),
      name = new TextEncoder().encode("prototype.heap");
    const payload = Uint8Array.from([
      ...enc(count + 1),
      ...rest,
      ...enc(name.length),
      ...name,
      2,
      0,
    ]);
    sections.push(Uint8Array.from([7, ...enc(payload.length), ...payload]));
  } else sections.push(bytes.subarray(begin, end));
  at = end;
}
// This measurement-only module exposes existing memory 0. No code is changed.
const exposed = Uint8Array.from([...bytes.subarray(0, 8), ...sections.flatMap((x) => [...x])]);
const module = await WebAssembly.compile(exposed),
  times = [],
  sizes = [];
for (let i = 0; i < 100; i++) {
  const host = createCoreHost(
    { out() {}, err() {} },
    { args: ['{"method":"GET","url":"https://handler.test/","headers":[],"body":[]}'] },
    {
      providers: () => ({
        traits: { "hd:Http": { send: () => ({ error: "Other", text: "not used" }) } },
      }),
    },
  );
  const t = performance.now();
  const instance = new WebAssembly.Instance(module, host.imports);
  times.push(performance.now() - t);
  await host.run(instance, "hd.init", "hd.poll");
  sizes.push({
    heap: instance.exports["prototype.heap"].buffer.byteLength,
    exchange: instance.exports["hd.x"].buffer.byteLength,
  });
  host.close();
}
times.sort((a, b) => a - b);
const graph = [
  "host/core.mjs",
  "host/checked-report.mjs",
  "host/float.mjs",
  "host/gen/abi.mjs",
  "host/journal.mjs",
  "host/wire.mjs",
];
const js = graph.map((p) => readFileSync(p));
js.push(readFileSync(new URL("./fetch-control.mjs", import.meta.url)));
const concat = Buffer.concat([...js, bytes]);
const proof = {
  sample: "100 sequential fresh instances; same compiled module; measurement-only heap export",
  node: process.version,
  wasmBytes: bytes.length,
  wasmGzip: gzipSync(bytes, { level: 9 }).length,
  definedFunctions: funcs,
  codeSectionBytes: codeBytes,
  declaredMemories: memories,
  heapAfterSmallRequest: Math.max(...sizes.map((x) => x.heap)),
  exchangeAfterSmallRequest: Math.max(...sizes.map((x) => x.exchange)),
  instantiateMedianMs: times[50],
  instantiateP95Ms: times[95],
  jsBytes: js.reduce((n, b) => n + b.length, 0),
  concatBytes: concat.length,
  concatGzipProxy: gzipSync(concat, { level: 9 }).length,
  instantiateTrialsMs: times,
};
writeFileSync(`${process.argv[2]}/measurements.json`, JSON.stringify(proof, null, 2));
console.log(JSON.stringify({ ...proof, instantiateTrialsMs: undefined }, null, 2));
