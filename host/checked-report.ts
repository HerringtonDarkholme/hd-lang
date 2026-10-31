// Checked-only descriptor counts. Metadata stays in a custom section;
// the guest stores only u64 counters, never names or formatting strings.
export function cycleReport(module: unknown, instance: WebAssembly.Instance): string | null {
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
  const totals: bigint[] = Array(7).fill(0n);
  const seen = new Set<number>();
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
