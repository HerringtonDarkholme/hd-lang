// A `{ c, js }` trait's JavaScript provider: pointers arrive as typed-array
// views of the guest heap, and a `&mut` one is written in place.
export default {
  write(path: string, data: Uint8Array, at: BigUint64Array, scale: number): number {
    at[0] += 10n;
    return path.length * 100 + data.length * 10 + scale * 2;
  },
  fill(into: Int32Array, big: bigint, wide: number): boolean {
    into[0] = Number(big / 1000000000n);
    into[1] = wide > 2 ** 31 ? 1 : 0;
    return true;
  },
  async take(limit: number): Promise<{ ok: boolean; value: Uint8Array }> {
    return { ok: true, value: new Uint8Array(limit).fill(9) };
  },
};
