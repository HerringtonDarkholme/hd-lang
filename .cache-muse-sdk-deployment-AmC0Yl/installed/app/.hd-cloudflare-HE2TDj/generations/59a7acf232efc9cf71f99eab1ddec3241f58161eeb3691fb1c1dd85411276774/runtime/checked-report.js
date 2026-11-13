// Checked-only descriptor counts. Metadata stays in a custom section;
// the guest stores only u64 counters, never names or formatting strings.
export function cycleReport(module         , instance                      )                {
  if (!(module instanceof WebAssembly.Module)) {
    return null;
  }
  const sections = WebAssembly.Module.customSections(module, "hd.cycles");
  if (sections.length !== 1) {
    return null;
  }
  const bytes = new Uint8Array(sections[0]);
  if (bytes[0] !== 1 || (bytes.length - 1) % 5 !== 0) {
    return null;
  }
  const ex = instance.exports;
  if (!(ex["hd.heap"] instanceof WebAssembly.Memory)) {
    return null;
  }
  if (
    typeof ex["hd.cycles.pointer"] !== "function" ||
    typeof ex["hd.cycles.length"] !== "function"
  ) {
    return null;
  }
  const pointer = ex["hd.cycles.pointer"]() >>> 0;
  const length = ex["hd.cycles.length"]() >>> 0;
  const heap = ex["hd.heap"].buffer;
  if (pointer > heap.byteLength || length > (heap.byteLength - pointer) / 8) {
    return null;
  }
  const counts = new DataView(heap);
  const metadata = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const totals           = Array(7).fill(0n);
  const seen = new Set        ();
  for (let at = 1; at < bytes.length; at += 5) {
    const id = metadata.getUint32(at, true);
    const kind = bytes[at + 4];
    if (kind >= totals.length || id === 0 || seen.has(id)) {
      return null;
    }
    seen.add(id);
    if (id < length) {
      totals[kind] += counts.getBigUint64(pointer + id * 8, true);
    }
  }
  const labels = [
    "objects",
    "closure envs",
    "local cells",
    "frames",
    "waker/all nodes",
    "list/map buffers",
    "list/map headers",
  ];
  return `cycles freed: ${[1, 2, 3, 4, 5, 6, 0].map((k) => `${totals[k]} ${labels[k]}`).join(", ")}\n`;
}

                               
                   
               
               
                
                 
 

// Full-width counts stay decimal strings in JSON reports. The metadata and
// counter bounds are checked independently before accessing guest memory.
export function countPairRows(
  module         ,
  instance                      ,
)                        {
  if (!(module instanceof WebAssembly.Module)) {
    return null;
  }
  const sections = WebAssembly.Module.customSections(module, "hd.count.sites");
  if (sections.length !== 1) {
    return null;
  }
  const bytes = new Uint8Array(sections[0]);
  if (bytes[0] !== 1) {
    return null;
  }
  const counter = countCounterView(instance);
  if (counter === null) {
    return null;
  }
  const { counters, pointer, length } = counter;
  const metadata = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  let at = 1;
  const u32 = ()         => {
    if (at + 4 > bytes.length) {
      throw new RangeError("count site metadata");
    }
    const value = metadata.getUint32(at, true);
    at += 4;
    return value;
  };
  const text = ()         => {
    const size = u32();
    if (size > bytes.length - at) {
      throw new RangeError("count site string");
    }
    const value = new TextDecoder("utf-8", { fatal: true }).decode(bytes.subarray(at, at + size));
    at += size;
    return value;
  };
  const seen = new Set        ();
  const totals = new Map                      ();
  if (length > 0 && counters.getBigUint64(pointer, true) !== 0n) {
    totals.set("unattributed", {
      function: "<unattributed>",
      file: "<runtime>",
      line: 0,
      reason: "instrumentation gap",
      count: String(counters.getBigUint64(pointer, true)),
    });
  }
  try {
    while (at < bytes.length) {
      const id = u32();
      const fn = text();
      const file = text();
      const line = u32();
      const reason = text();
      if (id === 0 || seen.has(id)) {
        return null;
      }
      seen.add(id);
      const count = id < length ? counters.getBigUint64(pointer + id * 8, true) : 0n;
      if (count === 0n) {
        continue;
      }
      const key = JSON.stringify([fn, file, line, reason]);
      const row = totals.get(key);
      if (row) {
        row.count = String(BigInt(row.count) + count);
      } else {
        totals.set(key, { function: fn, file, line, reason, count: String(count) });
      }
    }
  } catch {
    return null;
  }
  return rankCountPairs([...totals.values()]);
}

export function rankCountPairs(rows                )                 {
  return rows.sort((left, right) => {
    const difference = BigInt(right.count) - BigInt(left.count);
    if (difference < 0n) {
      return -1;
    }
    if (difference > 0n) {
      return 1;
    }
    return (
      left.function.localeCompare(right.function) ||
      left.file.localeCompare(right.file) ||
      left.line - right.line ||
      left.reason.localeCompare(right.reason)
    );
  });
}

function countCounterView(
  instance                      ,
)                                                                 {
  const ex = instance.exports;
  if (
    !(ex["hd.heap"] instanceof WebAssembly.Memory) ||
    typeof ex["hd.count.pairs.pointer"] !== "function" ||
    typeof ex["hd.count.pairs.length"] !== "function"
  ) {
    return null;
  }
  const pointer = ex["hd.count.pairs.pointer"]() >>> 0;
  const length = ex["hd.count.pairs.length"]() >>> 0;
  const heap = ex["hd.heap"].buffer;
  if (pointer > heap.byteLength || length > (heap.byteLength - pointer) / 8) {
    return null;
  }
  return { counters: new DataView(heap), pointer, length };
}
